//! Starting adapters and talking to them.
//!
//! An agent is described by an [`AgentSpec`]. Its [`Launcher`] starts the adapter and returns a
//! [`Link`] of JSON-RPC messages: [`Process`] runs a program and uses its stdin and stdout,
//! [`Fake`] replays a recorded session without any program. [`pi::Pi`] is the Pi profile: a
//! translator in front of one `pi-acp` process per session.

pub mod fake;
pub mod pi;

use archipel_core::transcript::Transcript;
use archipel_transport::{Link, lines};
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

    /// Another process of the harness, not started by this daemon, that has the session open
    /// right now. The default says there is none, for harnesses that offer no way to know.
    fn elsewhere(&self, _session_id: &str) -> Option<Elsewhere> {
        None
    }
}

/// A process outside the daemon that has a session open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elsewhere {
    pub pid: u32,
    /// What the process says it is doing, such as `busy` or `idle`.
    pub status: Option<String>,
    /// How the process was started, such as `cli`.
    pub client: Option<String>,
}

/// One agent the daemon can start.
#[derive(Clone)]
pub struct AgentSpec {
    /// Stable id used by clients, such as `claude`.
    pub id: String,
    /// Name shown to users.
    pub name: String,
    /// Which harness this is, such as `claude` or `pi`, for addons that are about one harness.
    /// `None` for a plain ACP adapter.
    pub harness: Option<String>,
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
        harness: Some("claude".to_owned()),
        launcher: Arc::new(Claude {
            process,
            registry: Claude::default_registry(),
        }),
    }
}

/// Claude Code's adapter, plus a look at Claude Code's own record of its running processes.
#[derive(Debug, Clone)]
pub struct Claude {
    pub process: Process,
    /// The directory where every Claude Code process writes `<pid>.json` with the session it
    /// has open. This is Claude Code's own layout (seen in 2.1.330), not a documented
    /// interface: if it is missing or changes, the answer is "nobody else", not an error.
    pub registry: std::path::PathBuf,
}

impl Claude {
    /// `$CLAUDE_CONFIG_DIR/sessions`, or `~/.claude/sessions`.
    #[must_use]
    pub fn default_registry() -> std::path::PathBuf {
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".claude")
            })
            .join("sessions")
    }
}

impl Launcher for Claude {
    fn launch(&self) -> io::Result<Running> {
        self.process.launch()
    }

    fn elsewhere(&self, session_id: &str) -> Option<Elsewhere> {
        let entries = std::fs::read_dir(&self.registry).ok()?;
        entries.flatten().find_map(|file| {
            let path = file.path();
            if path.extension()? != "json" {
                return None;
            }
            let entry: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            if entry.get("sessionId")?.as_str()? != session_id {
                return None;
            }
            let pid = u32::try_from(entry.get("pid")?.as_u64()?).ok()?;
            // The adapter's own Claude processes are in the registry too. Those are ours.
            (!started_by_this_process(pid)?).then(|| Elsewhere {
                pid,
                status: entry
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                client: entry
                    .get("entrypoint")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
    }
}

/// Whether `pid` is this process or one it started, directly or not. `None` when `pid` is not
/// running, which is how a registry entry left behind by a dead process is told apart.
fn started_by_this_process(pid: u32) -> Option<bool> {
    let me = std::process::id();
    let mut current = pid;
    for step in 0..32 {
        if current == me {
            return Some(true);
        }
        let parent = std::process::Command::new("ps")
            .args(["-o", "ppid=", "-p", &current.to_string()])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        match parent {
            Some(parent) if parent > 1 => current = parent,
            Some(_) => return Some(false),
            // Not running. For the pid we were asked about that means "gone".
            None if step == 0 => return None,
            None => return Some(false),
        }
    }
    Some(false)
}

/// What the daemon sends as `initialize` params to an adapter.
#[must_use]
pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": 2,
        "info": {"name": "archipel", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {"elicitation": {"form": {}, "url": {}}},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_open_in_a_process_we_did_not_start_is_seen_and_our_own_is_not() {
        let registry =
            std::env::temp_dir().join(format!("archipel-registry-{}", std::process::id()));
        std::fs::create_dir_all(&registry).unwrap();
        let write = |pid: u32, session: &str| {
            let entry =
                json!({"pid": pid, "sessionId": session, "status": "busy", "entrypoint": "cli"});
            std::fs::write(registry.join(format!("{pid}.json")), entry.to_string()).unwrap();
        };
        // A child of this test process, kept alive for the check, stands in for the adapter's own.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        write(std::process::id(), "mine");
        write(child.id(), "my-child");
        write(1, "someone-else"); // pid 1 always runs and is nobody's child
        write(4_000_000, "left-behind"); // no such process
        std::fs::write(registry.join("notes.txt"), "not a registry entry").unwrap();
        let claude = Claude {
            process: Process::default(),
            registry: registry.clone(),
        };

        assert_eq!(
            claude.elsewhere("someone-else"),
            Some(Elsewhere {
                pid: 1,
                status: Some("busy".into()),
                client: Some("cli".into())
            })
        );
        assert_eq!(claude.elsewhere("mine"), None);
        assert_eq!(claude.elsewhere("my-child"), None);
        assert_eq!(claude.elsewhere("left-behind"), None, "its process is gone");
        assert_eq!(claude.elsewhere("unknown"), None);
        let missing = Claude {
            process: Process::default(),
            registry: registry.join("nope"),
        };
        assert_eq!(
            missing.elsewhere("someone-else"),
            None,
            "no registry means nobody else"
        );

        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(&registry).unwrap();
    }
}
