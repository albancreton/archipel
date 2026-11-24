//! A Rust program that hosts the daemon in its own process.
//!
//! Use this when your product is a Rust program and you want one binary: no `bridged` to
//! install, no service. The server runs on your program's tokio runtime. Your own code talks
//! to it over an in-process link; other apps can still reach it through a local socket, with
//! the same SDKs as for `bridged`.
//!
//! ```sh
//! # With the real Claude adapter (`claude-agent-acp` on PATH):
//! cargo run -p embedded-daemon -- /tmp/embedded-home
//! # Free, with a fake agent that replays a recorded session:
//! cargo run -p embedded-daemon -- /tmp/embedded-home testdata/transcripts/claude-v2/simple.ndjson
//! ```
//!
//! What you choose yourself, compared with `bridged`:
//!
//! - which agents exist ([`agents`]), instead of `config.json`;
//! - where the log is kept ([`start`]): a file, or memory for a program that keeps nothing;
//! - which connections are let in, and with which scope ([`serve`]);
//! - which addons run (`Server::builder().addon(…)`, see `examples/own-daemon`).
//!
//! What you give up: sessions stop when your program stops. With `bridged` as a service they
//! go on while apps come and go.

use bridge_agents::{AgentSpec, Fake, Process, claude};
use bridge_core::transcript::Transcript;
use bridge_server::{Handle, Scope, Server, SqliteStore};
use bridge_transport::{Link, local::LocalServer};
use serde_json::{Value, json};
use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc};

/// The agents this program offers. A product would build this from its own settings.
fn agents(recording: Option<&str>) -> Result<Vec<AgentSpec>, Box<dyn Error>> {
    let Some(path) = recording else {
        // The Claude Code profile. The adapter is an npm package that needs Node:
        // `npm install -g @agentclientprotocol/claude-agent-acp`.
        let adapter = Process {
            command: "claude-agent-acp".to_owned(),
            ..Process::default()
        };
        return Ok(vec![claude(adapter)]);
    };
    let transcript = Transcript::parse(&fs::read_to_string(path)?)?;
    Ok(vec![AgentSpec {
        id: "fake".to_owned(),
        name: "Fake agent".to_owned(),
        harness: None,
        launcher: Arc::new(Fake {
            transcript,
            speed: 0.0,
        }),
    }])
}

/// Starts the server on the current tokio runtime. `log` is where sessions are kept; `None`
/// keeps them in memory, so they are gone when the program stops.
fn start(agents: Vec<AgentSpec>, log: Option<&Path>) -> Result<Handle, Box<dyn Error>> {
    let store = match log {
        Some(path) => SqliteStore::open(path)?,
        None => SqliteStore::in_memory()?,
    };
    let mut builder = Server::builder().store(store);
    for agent in agents {
        builder = builder.agent(agent);
    }
    Ok(builder.start())
}

/// A connection from this program's own code. No socket is involved.
fn connect(server: &Handle) -> Link {
    let (ours, theirs) = Link::pair();
    server.connect_as(theirs, Scope::Interact);
    ours
}

/// Sends a request over a link and waits for its answer, skipping what else arrives. A real
/// program would use a client that keeps those other messages: `bridge-client` in Rust, or
/// one of the SDKs over the socket.
async fn call(link: &mut Link, id: u64, method: &str, params: Value) -> Option<Value> {
    let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    link.tx.send(request).await.ok()?;
    loop {
        let message = link.rx.recv().await?;
        if message.get("method").is_none() && message["id"] == json!(id) {
            return Some(message);
        }
    }
}

/// Lets other apps in through a local socket, until the program is interrupted.
async fn serve(server: &Handle, socket: &Path) -> Result<(), Box<dyn Error>> {
    let local = LocalServer::bind(socket)?;
    println!("other apps can connect to {}", socket.display());
    loop {
        tokio::select! {
            // Everything that reaches the socket may do everything. Use `connect_as` with a
            // narrower scope if that is not what you want.
            link = local.accept() => server.connect(link?),
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let home = args
        .next()
        .ok_or("usage: embedded-daemon HOME [RECORDING]")?;
    let recording = args.next();
    let home = Path::new(&home);
    fs::create_dir_all(home)?;
    // Only the owner may enter: the socket gives the power to run commands as them.
    fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;

    let server = start(
        agents(recording.as_deref())?,
        Some(&home.join("log.sqlite")),
    )?;
    let mut own = connect(&server);
    let listed = call(&mut own, 1, "_bridge/agents/list", json!({})).await;
    println!(
        "agents: {}",
        listed.map_or(Value::Null, |answer| answer["result"]["agents"].clone())
    );

    serve(&server, &home.join("daemon.sock")).await?;
    server.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_hosted_server_answers_the_programs_own_code_and_apps_on_the_socket() {
        let recording = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/transcripts/claude-v2/simple.ndjson"
        );
        let server = start(agents(Some(recording)).unwrap(), None).unwrap();

        // The program's own code, in-process.
        let mut own = connect(&server);
        let listed = call(&mut own, 1, "_bridge/agents/list", json!({}))
            .await
            .unwrap();
        assert_eq!(listed["result"]["agents"][0]["agentId"], "fake");
        let hello = json!({"protocolVersion": 2, "info": {"name": "example"}});
        call(&mut own, 2, "initialize", hello).await.unwrap();
        let made = call(
            &mut own,
            3,
            "session/new",
            json!({"cwd": "/work", "mcpServers": []}),
        )
        .await
        .unwrap();
        let session = made["result"]["sessionId"].as_str().expect("a session");

        // Another app, through the socket, sees the same session.
        let home = std::env::temp_dir().join(format!("embedded-{}", std::process::id()));
        fs::create_dir_all(&home).unwrap();
        let socket = home.join("daemon.sock");
        let serving = {
            let (server, socket) = (server.clone(), socket.clone());
            tokio::spawn(async move { serve(&server, &socket).await.map_err(|e| e.to_string()) })
        };
        let mut app = loop {
            match bridge_transport::local::connect(&socket).await {
                Ok(link) => break link,
                Err(_) => tokio::task::yield_now().await,
            }
        };
        let listed = call(
            &mut app,
            1,
            "_bridge/sessions/list",
            json!({"loggedOnly": true}),
        )
        .await
        .unwrap();
        assert_eq!(listed["result"]["sessions"][0]["sessionId"], session);

        serving.abort();
        server.shutdown().await;
        fs::remove_dir_all(&home).unwrap();
    }
}
