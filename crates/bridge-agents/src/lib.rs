//! Starting adapters and talking to them.
//!
//! An agent is described by an [`AgentSpec`]. Its [`Launcher`] starts the adapter and returns a
//! [`Link`] of JSON-RPC messages: [`Process`] runs a program and uses its stdin and stdout,
//! [`Fake`] replays a recorded session without any program. [`pi::Pi`] is the Pi profile: a
//! translator in front of one `pi-acp` process per session.

pub mod fake;
pub mod pi;

use bridge_core::transcript::Transcript;
use bridge_transport::{Link, lines};
use serde_json::{Value, json};
use std::{any::Any, collections::BTreeMap, io, process::Stdio, sync::Arc};

/// A started adapter.
pub struct Running {
    pub link: Link,
    /// Whatever keeps the adapter alive. Dropping it stops the adapter.
    pub guard: Option<Box<dyn Any + Send>>,
}

/// Starts an adapter.
pub trait Launcher: Send + Sync {
    /// Starts the adapter. Called inside a tokio runtime.
    ///
    /// # Errors
    /// When the adapter cannot be started.
    fn launch(&self) -> io::Result<Running>;
}

/// One agent the daemon can start.
#[derive(Clone)]
pub struct AgentSpec {
    /// Stable id used by clients, such as `claude`.
    pub id: String,
    /// Name shown to users.
    pub name: String,
    pub launcher: Arc<dyn Launcher>,
}

/// An adapter that is a program speaking ACP on its stdin and stdout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    pub command: String,
    pub args: Vec<String>,
    /// Added to the daemon's own environment.
    pub env: BTreeMap<String, String>,
    /// Show what the adapter prints on stderr instead of discarding it.
    pub show_stderr: bool,
}

impl Launcher for Process {
    fn launch(&self) -> io::Result<Running> {
        let mut child = tokio::process::Command::new(&self.command)
            .args(&self.args)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if self.show_stderr {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", self.command)))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(io::Error::other("the adapter has no stdin or stdout"));
        };
        Ok(Running {
            link: lines(stdout, stdin),
            guard: Some(Box::new(child)),
        })
    }
}

/// An adapter that replays a recorded session. See [`fake::replay`].
#[derive(Debug, Clone)]
pub struct Fake {
    pub transcript: Transcript,
    /// 1.0 replays at the recorded pace, 10.0 ten times faster, 0.0 without waiting.
    pub speed: f64,
}

impl Launcher for Fake {
    fn launch(&self) -> io::Result<Running> {
        let (ours, theirs) = Link::pair();
        tokio::spawn(fake::replay(self.transcript.clone(), self.speed, theirs));
        Ok(Running {
            link: ours,
            guard: None,
        })
    }
}

/// The Claude Code profile: `claude-agent-acp`, driven over ACP v2.
///
/// `process` starts the adapter, for example `claude-agent-acp` with no arguments, or `node`
/// with the path of its `dist/index.js`.
#[must_use]
pub fn claude(mut process: Process) -> AgentSpec {
    // Without this the adapter sends no reasoning (docs/research/11).
    process
        .env
        .entry("CLAUDE_CODE_THINKING_DISPLAY".to_owned())
        .or_insert_with(|| "summarized".to_owned());
    AgentSpec {
        id: "claude".to_owned(),
        name: "Claude Code".to_owned(),
        launcher: Arc::new(process),
    }
}

/// What the daemon sends as `initialize` params to an adapter.
#[must_use]
pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": 2,
        "info": {"name": "harness-bridge", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {"elicitation": {"form": {}, "url": {}}},
    })
}
