//! `bridged`: the daemon. One per user per machine.

use bridge_agents::{AgentSpec, Fake, Process, claude};
use bridge_core::transcript::Transcript;
use bridge_server::{Scope, Server, SqliteStore};
use bridge_transport::{local::LocalServer, ws};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

const USAGE: &str = "\
bridged: the harness-bridge daemon

  bridged [--home DIR] [--ws ADDR] [--fake TRANSCRIPT [--fake-speed N]] [--agent-stderr]
  bridged [--home DIR] token add NAME [--scope observe|interact|admin]
  bridged [--home DIR] token list
  bridged [--home DIR] token remove NAME

  --home DIR         where the daemon keeps its files (default: $BRIDGE_HOME or ~/.harness-bridge)
                       daemon.sock   the local socket apps connect to
                       log.sqlite    the session log
                       config.json   the agents (optional)
                       ws-token      the owner's WebSocket token (written when --ws is used)
                       tokens.json   further WebSocket tokens, made with `bridged token add`
  --ws ADDR          also listen for WebSocket clients, e.g. 127.0.0.1:7890. Off by default.
  --fake TRANSCRIPT  use a fake agent that replays a recorded session instead of the config
  --fake-speed N     pace of the fake agent: 1 as recorded (default), 0 without pauses
  --agent-stderr     show what adapters print on stderr

Tokens: a WebSocket client presents a token. `ws-token` may do everything. A token made with
`token add` may do what its scope says: observe (list and read; the default), interact (also
prompt, answer, create), admin. Tokens can be added and removed while the daemon runs.

config.json:
  {\"agents\": [{\"id\": \"claude\", \"profile\": \"claude\", \"command\": \"claude-agent-acp\",
               \"args\": [], \"env\": {}}],
   \"retentionDays\": 30}
Without it, one agent `claude` is started as `claude-agent-acp` from PATH.
retentionDays: remove sessions from the log when unused for that long (default: never). The
harness keeps its own copy; a removed session stays listed and is loaded again when opened.
";

type Failure = Box<dyn std::error::Error>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    agents: Vec<AgentConfig>,
    retention_days: Option<u64>,
}

/// One entry of `tokens.json`.
#[derive(Debug, Serialize, Deserialize)]
struct Token {
    name: String,
    scope: Scope,
    token: String,
}

fn read_tokens(home: &Path) -> Vec<Token> {
    fs::read_to_string(home.join("tokens.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// What a presented WebSocket token allows. Reads the files each time, so changes apply at once.
fn grant(home: &Path, presented: &str) -> Option<Scope> {
    if let Ok(owner) = fs::read_to_string(home.join("ws-token"))
        && ws::same(presented, owner.trim())
    {
        return Some(Scope::Admin);
    }
    read_tokens(home)
        .into_iter()
        .find_map(|token| ws::same(presented, &token.token).then_some(token.scope))
}

/// `bridged token …`: manage `tokens.json`.
fn token_command(home: &Path, mut args: Vec<String>) -> Result<(), Failure> {
    let scope = option(&mut args, "--scope").unwrap_or_else(|| "observe".to_owned());
    let mut tokens = read_tokens(home);
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
            let scope: Scope = serde_json::from_value(scope.clone().into())
                .map_err(|_| format!("unknown scope `{scope}`"))?;
            let token = ws::new_token()?;
            println!("{token}");
            tokens.push(Token {
                name: (*name).to_owned(),
                scope,
                token,
            });
        }
        ["list"] => {
            for token in &tokens {
                println!(
                    "{:<20} {}",
                    token.name,
                    serde_json::to_value(token.scope)?.as_str().unwrap_or("")
                );
            }
            return Ok(());
        }
        ["remove", name] => {
            let before = tokens.len();
            tokens.retain(|token| token.name != *name);
            if tokens.len() == before {
                return Err(format!("no token named `{name}`").into());
            }
        }
        _ => return Err("usage: bridged token add NAME [--scope S] | list | remove NAME".into()),
    }
    write_private(
        &home.join("tokens.json"),
        &serde_json::to_string_pretty(&tokens)?,
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentConfig {
    id: String,
    /// Shown to users. Defaults to the id.
    name: Option<String>,
    /// `claude` applies the Claude Code settings. Absent: a plain ACP v2 adapter.
    profile: Option<String>,
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

fn agents(home: &Path, args: &mut Vec<String>) -> Result<(Vec<AgentSpec>, Option<u64>), Failure> {
    let show_stderr = switch(args, "--agent-stderr");
    let speed = option(args, "--fake-speed").map_or(Ok(1.0), |text| text.parse::<f64>())?;
    if let Some(path) = option(args, "--fake") {
        let transcript = Transcript::parse(&fs::read_to_string(&path)?)?;
        let fake = AgentSpec {
            id: transcript.header.agent.clone(),
            name: format!("Fake ({})", transcript.header.scenario),
            launcher: Arc::new(Fake { transcript, speed }),
        };
        return Ok((vec![fake], None));
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
                command: "claude-agent-acp".into(),
                args: Vec::new(),
                env: BTreeMap::new(),
            }],
            retention_days: None,
        }
    };
    let retention_days = config.retention_days;
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
    Ok((specs?, retention_days))
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
    if args.first().map(String::as_str) == Some("token") {
        return token_command(&home, args.split_off(1));
    }
    let (agents, retention_days) = agents(&home, &mut args)?;
    if let Some(unknown) = args.first() {
        return Err(format!("unknown argument `{unknown}`; try --help").into());
    }

    let socket = home.join("daemon.sock");
    let local = LocalServer::bind(&socket)?;
    let mut builder = Server::builder().store(SqliteStore::open(&home.join("log.sqlite"))?);
    if let Some(days) = retention_days {
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
        let authorize: ws::Authorize<Scope> =
            Arc::new(move |presented| grant(&tokens_home, presented));
        let ws_server = ws::WsServer::bind(addr.parse()?, authorize).await?;
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
            Some((link, scope)) = accepted.recv() => server.connect_as(link, scope),
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
        }
    }
    eprintln!("bridged: stopping");
    server.shutdown().await;
    Ok(())
}
