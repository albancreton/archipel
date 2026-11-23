//! `bridged`: the daemon. One per user per machine.

use bridge_agents::{AgentSpec, Fake, Process, claude, pi::Pi};
use bridge_core::transcript::Transcript;
use bridge_server::{Scope, Server, SqliteStore};
use bridge_transport::{local::LocalServer, ws};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

const USAGE: &str = "\
bridged: the harness-bridge daemon

  bridged [--home DIR] [--ws ADDR] [--fake TRANSCRIPT [--fake-speed N]] [--agent-stderr]
  bridged [--home DIR] pair [--scope SCOPE] [--name NAME] [--minutes N]
  bridged [--home DIR] token add NAME [--scope observe|interact|extend|admin]
  bridged [--home DIR] token list
  bridged [--home DIR] token remove NAME

  --home DIR         where the daemon keeps its files (default: $BRIDGE_HOME or ~/.harness-bridge)
                       daemon.sock   the local socket apps connect to
                       log.sqlite    the session log
                       config.json   the agents (optional)
                       ws-token      the owner's WebSocket token (written when --ws is used)
                       tokens.json   further WebSocket tokens, made with `bridged token add`
  --ws ADDR          also listen for WebSocket clients, e.g. 127.0.0.1:7890. Off by default.
  --ws-origin ORIGIN let pages served from this web origin connect, e.g. http://localhost:5173.
                     May be given several times. Without it no browser page can connect.
  --fake TRANSCRIPT  use a fake agent that replays a recorded session instead of the config
  --fake-speed N     pace of the fake agent: 1 as recorded (default), 0 without pauses
  --agent-stderr     show what adapters print on stderr

Tokens: a WebSocket client presents a token. `ws-token` is the owner's and may do everything.
Other tokens may do what their scope says: observe (list and read), interact (also prompt,
answer, create), extend (also act as an extension), admin. Only their hashes are stored.
  pair         prints a one-time code. The app exchanges it for its own token (`bridge pair`).
  token add    prints a token to copy by hand.
  token remove revokes one. All of these work while the daemon runs.

config.json:
  {\"agents\": [{\"id\": \"claude\", \"profile\": \"claude\", \"command\": \"claude-agent-acp\",
               \"args\": [], \"env\": {}}],
   \"retentionDays\": 30}
Without it, one agent `claude` is started as `claude-agent-acp` from PATH.
Pi: {\"id\": \"pi\", \"profile\": \"pi\", \"command\": \"pi-acp\", \"model\": \"provider/model\",
     \"idleSeconds\": 600}. One pi process runs per session and is stopped when idle.
whenOpenElsewhere: \"block\" (default) refuses to act on a session that another process of the
harness has open, such as its own terminal UI; the app can fork it. \"allow\" acts anyway.
retentionDays: remove sessions from the log when unused for that long (default: never). The
harness keeps its own copy; a removed session stays listed and is loaded again when opened.
";

type Failure = Box<dyn std::error::Error>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    agents: Vec<AgentConfig>,
    retention_days: Option<u64>,
    /// `block` (the default) or `allow`: what to do with a session that another process of
    /// the harness has open.
    when_open_elsewhere: Option<String>,
}

/// One entry of `tokens.json`. The token itself is not kept, only its SHA-256.
#[derive(Debug, Serialize, Deserialize)]
struct Token {
    name: String,
    scope: Scope,
    hash: String,
}

/// One entry of `pairings.json`: a one-time code that can be exchanged for a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pairing {
    code_hash: String,
    scope: Scope,
    name: String,
    /// Milliseconds since the Unix epoch.
    expires_at: u64,
}

/// What a presented WebSocket credential allows.
enum Grant {
    /// A token: the connection is served with this scope.
    Scope(Scope),
    /// A pairing code: the connection may only exchange it for a token.
    Pair(Pairing),
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn read_list<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn write_list<T: Serialize>(path: &Path, list: &[T]) -> Result<(), Failure> {
    Ok(write_private(path, &serde_json::to_string_pretty(list)?)?)
}

/// A pairing code as typed: upper case, without the dash or spaces.
fn normalised(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// What a presented credential allows. Reads the files each time, so changes apply at once.
fn grant(home: &Path, presented: &str) -> Option<Grant> {
    if let Ok(owner) = fs::read_to_string(home.join("ws-token"))
        && ws::same(presented, owner.trim())
    {
        return Some(Grant::Scope(Scope::Admin));
    }
    if let Some(code) = presented.strip_prefix("pair:") {
        let hash = digest(&normalised(code));
        return read_list::<Pairing>(&home.join("pairings.json"))
            .into_iter()
            .find(|pairing| pairing.expires_at > now() && ws::same(&pairing.code_hash, &hash))
            .map(Grant::Pair);
    }
    let hash = digest(presented);
    read_list::<Token>(&home.join("tokens.json"))
        .into_iter()
        .find_map(|token| ws::same(&token.hash, &hash).then_some(Grant::Scope(token.scope)))
}

fn parse_scope(text: &str) -> Result<Scope, Failure> {
    serde_json::from_value(text.into()).map_err(|_| format!("unknown scope `{text}`").into())
}

/// Adds a token under a name that is not taken yet, and returns the token and the name used.
fn add_token(home: &Path, wanted: &str, scope: Scope) -> Result<(String, String), Failure> {
    let path = home.join("tokens.json");
    let mut tokens: Vec<Token> = read_list(&path);
    let mut name = wanted.to_owned();
    let mut n = 1;
    while tokens.iter().any(|token| token.name == name) {
        n += 1;
        name = format!("{wanted}-{n}");
    }
    let token = ws::new_token()?;
    tokens.push(Token {
        name: name.clone(),
        scope,
        hash: digest(&token),
    });
    write_list(&path, &tokens)?;
    Ok((token, name))
}

/// `bridged token …`: manage `tokens.json`.
fn token_command(home: &Path, mut args: Vec<String>) -> Result<(), Failure> {
    let scope = option(&mut args, "--scope").unwrap_or_else(|| "observe".to_owned());
    let path = home.join("tokens.json");
    let mut tokens: Vec<Token> = read_list(&path);
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["add", name] => {
            if tokens.iter().any(|token| token.name == *name) {
                return Err(format!("a token named `{name}` exists").into());
            }
            let (token, _) = add_token(home, name, parse_scope(&scope)?)?;
            // Shown once. Only its hash is kept.
            println!("{token}");
        }
        ["list"] => {
            for token in &tokens {
                println!(
                    "{:<20} {}",
                    token.name,
                    serde_json::to_value(token.scope)?.as_str().unwrap_or("")
                );
            }
        }
        ["remove", name] => {
            let before = tokens.len();
            tokens.retain(|token| token.name != *name);
            if tokens.len() == before {
                return Err(format!("no token named `{name}`").into());
            }
            write_list(&path, &tokens)?;
        }
        _ => return Err("usage: bridged token add NAME [--scope S] | list | remove NAME".into()),
    }
    Ok(())
}

/// `bridged pair`: make a one-time code that an app exchanges for its own token.
fn pair_command(home: &Path, mut args: Vec<String>) -> Result<(), Failure> {
    let scope =
        parse_scope(&option(&mut args, "--scope").unwrap_or_else(|| "interact".to_owned()))?;
    let name = option(&mut args, "--name").unwrap_or_else(|| "paired".to_owned());
    let minutes: u64 = option(&mut args, "--minutes").map_or(Ok(10), |text| text.parse())?;
    if let Some(unknown) = args.first() {
        return Err(format!("unknown argument `{unknown}`").into());
    }
    // Ten characters from an alphabet without look-alikes: 50 bits, used once, short-lived.
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut random = [0u8; 10];
    getrandom::fill(&mut random)?;
    let code: String = random
        .iter()
        .map(|byte| ALPHABET[usize::from(byte % 32)] as char)
        .collect();
    let path = home.join("pairings.json");
    let mut pairings: Vec<Pairing> = read_list(&path);
    pairings.retain(|pairing| pairing.expires_at > now());
    pairings.push(Pairing {
        code_hash: digest(&code),
        scope,
        name,
        expires_at: now() + minutes * 60_000,
    });
    write_list(&path, &pairings)?;
    println!("{}-{}", &code[..5], &code[5..]);
    eprintln!("Valid once, for {minutes} minutes. In the app, or: bridge pair WS_URL CODE");
    Ok(())
}

/// Serves a connection that came with a pairing code: one request, `_bridge/pair`, answered
/// with a new token. The code stops working the moment it is used.
async fn serve_pairing(home: PathBuf, mut link: bridge_transport::Link, pairing: Pairing) {
    while let Some(message) = link.rx.recv().await {
        let Some(id) = message.get("id") else {
            continue;
        };
        let answer = if message["method"] == "_bridge/pair" {
            let path = home.join("pairings.json");
            let mut pairings: Vec<Pairing> = read_list(&path);
            let before = pairings.len();
            pairings.retain(|other| other.code_hash != pairing.code_hash);
            let unused = pairings.len() < before;
            let name = message["params"]["name"].as_str().unwrap_or(&pairing.name);
            match (unused, write_list(&path, &pairings)) {
                (true, Ok(())) => match add_token(&home, name, pairing.scope) {
                    Ok((token, name)) => {
                        json!({"result": {"token": token, "scope": pairing.scope, "name": name}})
                    }
                    Err(error) => json!({"error": {"code": -32603, "message": error.to_string()}}),
                },
                _ => json!({"error": {"code": -32003, "message": "this code was already used"}}),
            }
        } else {
            json!({"error": {"code": -32003, "message": "this connection can only call `_bridge/pair`"}})
        };
        let mut reply = answer;
        reply["jsonrpc"] = "2.0".into();
        reply["id"] = id.clone();
        let done = reply.get("result").is_some();
        let _ = link.tx.send(reply).await;
        if done {
            return;
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentConfig {
    id: String,
    /// Shown to users. Defaults to the id.
    name: Option<String>,
    /// `claude` applies the Claude Code settings; `pi` puts the Pi translator in front of
    /// `pi-acp`. Absent: a plain ACP v2 adapter.
    profile: Option<String>,
    /// Pi only: the model to select in every new session.
    model: Option<String>,
    /// Pi only: stop a session's process after this many seconds without a turn (default 600).
    idle_seconds: Option<u64>,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Removes `--name VALUE` from the arguments and returns the value.
fn option(args: &mut Vec<String>, name: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == name)?;
    args.remove(at);
    (at < args.len()).then(|| args.remove(at))
}

/// Removes `--name` from the arguments and says whether it was there.
fn switch(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != name);
    args.len() != before
}

/// What `config.json` says besides the agents.
#[derive(Default)]
struct Settings {
    retention_days: Option<u64>,
    share: bool,
}

fn agents(home: &Path, args: &mut Vec<String>) -> Result<(Vec<AgentSpec>, Settings), Failure> {
    let show_stderr = switch(args, "--agent-stderr");
    let speed = option(args, "--fake-speed").map_or(Ok(1.0), |text| text.parse::<f64>())?;
    if let Some(path) = option(args, "--fake") {
        let transcript = Transcript::parse(&fs::read_to_string(&path)?)?;
        let fake = AgentSpec {
            id: transcript.header.agent.clone(),
            name: format!("Fake ({})", transcript.header.scenario),
            launcher: Arc::new(Fake { transcript, speed }),
        };
        return Ok((vec![fake], Settings::default()));
    }
    let path = home.join("config.json");
    let config = if path.exists() {
        serde_json::from_str::<Config>(&fs::read_to_string(&path)?)
            .map_err(|error| format!("{}: {error}", path.display()))?
    } else {
        Config {
            agents: vec![AgentConfig {
                id: "claude".into(),
                name: None,
                profile: Some("claude".into()),
                model: None,
                idle_seconds: None,
                command: "claude-agent-acp".into(),
                args: Vec::new(),
                env: BTreeMap::new(),
            }],
            retention_days: None,
            when_open_elsewhere: None,
        }
    };
    let settings = Settings {
        retention_days: config.retention_days,
        share: match config.when_open_elsewhere.as_deref() {
            None | Some("block") => false,
            Some("allow") => true,
            Some(other) => {
                return Err(format!("whenOpenElsewhere: `{other}` is not block or allow").into());
            }
        },
    };
    let specs: Result<Vec<AgentSpec>, Failure> = config
        .agents
        .into_iter()
        .map(|agent| {
            let process = Process {
                command: agent.command,
                args: agent.args,
                env: agent.env,
                show_stderr,
            };
            let mut spec = match agent.profile.as_deref() {
                Some("claude") => claude(process),
                Some("pi") => {
                    let mut pi = Pi::new(Arc::new(process));
                    pi.model = agent.model;
                    if let Some(seconds) = agent.idle_seconds {
                        pi.idle = Duration::from_secs(seconds);
                    }
                    pi.spec()
                }
                None => AgentSpec {
                    id: String::new(),
                    name: agent.id.clone(),
                    launcher: Arc::new(process),
                },
                Some(other) => return Err(format!("unknown profile `{other}`").into()),
            };
            if let Some(name) = agent.name {
                spec.name = name;
            }
            spec.id = agent.id;
            Ok(spec)
        })
        .collect();
    Ok((specs?, settings))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bridged: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(mut args: Vec<String>) -> Result<(), Failure> {
    if switch(&mut args, "--help") || switch(&mut args, "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let home = option(&mut args, "--home")
        .or_else(|| std::env::var("BRIDGE_HOME").ok())
        .map_or_else(
            || PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".harness-bridge"),
            PathBuf::from,
        );
    let ws_addr = option(&mut args, "--ws");
    fs::create_dir_all(&home)?;
    // Only the owner may enter: the socket inside gives the power to run commands as them.
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
    match args.first().map(String::as_str) {
        Some("token") => return token_command(&home, args.split_off(1)),
        Some("pair") => return pair_command(&home, args.split_off(1)),
        _ => {}
    }
    let mut origins = Vec::new();
    while let Some(origin) = option(&mut args, "--ws-origin") {
        origins.push(origin);
    }
    let (agents, settings) = agents(&home, &mut args)?;
    if let Some(unknown) = args.first() {
        return Err(format!("unknown argument `{unknown}`; try --help").into());
    }

    let socket = home.join("daemon.sock");
    let local = LocalServer::bind(&socket)?;
    let mut builder = Server::builder().store(SqliteStore::open(&home.join("log.sqlite"))?);
    builder = builder.act_on_sessions_open_elsewhere(settings.share);
    if let Some(days) = settings.retention_days {
        builder = builder.retention(Duration::from_secs(days * 86_400));
    }
    for agent in agents {
        eprintln!("bridged: agent `{}` ({})", agent.id, agent.name);
        builder = builder.agent(agent);
    }
    let server = builder.start();
    eprintln!("bridged: listening on {}", socket.display());

    let (links, mut accepted) = mpsc::channel(16);
    if let Some(addr) = ws_addr {
        let token_path = home.join("ws-token");
        if !token_path.exists() {
            write_private(&token_path, &ws::new_token()?)?;
        }
        let tokens_home = home.clone();
        let authorize: ws::Authorize<Grant> =
            Arc::new(move |presented| grant(&tokens_home, presented));
        let ws_server = ws::WsServer::bind(addr.parse()?, authorize)
            .await?
            .allow_origins(origins);
        eprintln!(
            "bridged: listening on ws://{} (token in {})",
            ws_server.local_addr()?,
            token_path.display()
        );
        tokio::spawn(ws_server.run(links));
    }

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            link = local.accept() => server.connect(link?),
            Some((link, grant)) = accepted.recv() => match grant {
                Grant::Scope(scope) => server.connect_as(link, scope),
                Grant::Pair(pairing) => {
                    tokio::spawn(serve_pairing(home.clone(), link, pairing));
                }
            },
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
        }
    }
    eprintln!("bridged: stopping");
    server.shutdown().await;
    Ok(())
}
