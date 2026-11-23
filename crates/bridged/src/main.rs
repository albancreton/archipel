//! `bridged`: the daemon. One per user per machine.

use bridge_agents::{AgentSpec, Fake, Process, claude};
use bridge_core::transcript::Transcript;
use bridge_server::{Server, SqliteStore};
use bridge_transport::{local::LocalServer, ws};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};

const USAGE: &str = "\
bridged: the harness-bridge daemon

  bridged [--home DIR] [--ws ADDR] [--fake TRANSCRIPT [--fake-speed N]] [--agent-stderr]

  --home DIR         where the daemon keeps its files (default: $BRIDGE_HOME or ~/.harness-bridge)
                       daemon.sock   the local socket apps connect to
                       log.sqlite    the session log
                       config.json   the agents (optional)
                       ws-token      the WebSocket token (written when --ws is used)
  --ws ADDR          also listen for WebSocket clients, e.g. 127.0.0.1:7890. Off by default.
  --fake TRANSCRIPT  use a fake agent that replays a recorded session instead of the config
  --fake-speed N     pace of the fake agent: 1 as recorded (default), 0 without pauses
  --agent-stderr     show what adapters print on stderr

config.json:
  {\"agents\": [{\"id\": \"claude\", \"profile\": \"claude\", \"command\": \"claude-agent-acp\",
               \"args\": [], \"env\": {}}]}
Without it, one agent `claude` is started as `claude-agent-acp` from PATH.
";

type Failure = Box<dyn std::error::Error>;

#[derive(Debug, Deserialize)]
struct Config {
    agents: Vec<AgentConfig>,
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

fn agents(home: &Path, args: &mut Vec<String>) -> Result<Vec<AgentSpec>, Failure> {
    let show_stderr = switch(args, "--agent-stderr");
    let speed = option(args, "--fake-speed").map_or(Ok(1.0), |text| text.parse::<f64>())?;
    if let Some(path) = option(args, "--fake") {
        let transcript = Transcript::parse(&fs::read_to_string(&path)?)?;
        return Ok(vec![AgentSpec {
            id: transcript.header.agent.clone(),
            name: format!("Fake ({})", transcript.header.scenario),
            launcher: Arc::new(Fake { transcript, speed }),
        }]);
    }
    let path = home.join("config.json");
    let config = if path.exists() {
        serde_json::from_str::<Config>(&fs::read_to_string(&path)?)
            .map_err(|error| format!("{}: {error}", path.display()))?
            .agents
    } else {
        vec![AgentConfig {
            id: "claude".into(),
            name: None,
            profile: Some("claude".into()),
            command: "claude-agent-acp".into(),
            args: Vec::new(),
            env: BTreeMap::new(),
        }]
    };
    config
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
        .collect()
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
    let agents = agents(&home, &mut args)?;
    if let Some(unknown) = args.first() {
        return Err(format!("unknown argument `{unknown}`; try --help").into());
    }

    let socket = home.join("daemon.sock");
    let local = LocalServer::bind(&socket)?;
    let mut builder = Server::builder().store(SqliteStore::open(&home.join("log.sqlite"))?);
    for agent in agents {
        eprintln!("bridged: agent `{}` ({})", agent.id, agent.name);
        builder = builder.agent(agent);
    }
    let server = builder.start();
    eprintln!("bridged: listening on {}", socket.display());

    let (links, mut accepted) = mpsc::channel(16);
    if let Some(addr) = ws_addr {
        let token_path = home.join("ws-token");
        let token = match fs::read_to_string(&token_path) {
            Ok(token) => token.trim().to_owned(),
            Err(_) => {
                let token = ws::new_token()?;
                fs::write(&token_path, &token)?;
                fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600))?;
                token
            }
        };
        let ws_server = ws::WsServer::bind(addr.parse()?, token).await?;
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
            Some(link) = accepted.recv() => server.connect(link),
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
        }
    }
    eprintln!("bridged: stopping");
    server.shutdown().await;
    Ok(())
}
