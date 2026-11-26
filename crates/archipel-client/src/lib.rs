//! A client's connection to the daemon.
//!
//! [`Connection`] sends requests and hands out what the daemon sends. What a client holds for a
//! session, and how the daemon's messages change it, is [`archipel_core::mirror::Mirror`].

use archipel_transport::{Link, local, ws};
use serde_json::{Value, json};
use std::{collections::VecDeque, fmt, io, path::PathBuf};

/// Where a daemon listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// The local socket of a daemon on this machine.
    Local(PathBuf),
    /// A WebSocket address such as `ws://127.0.0.1:7890`, with the daemon's token.
    Ws { url: String, token: String },
    /// A command whose stdin and stdout carry the messages, such as
    /// `ssh host archipel connect --stdio`. It is run through `sh -c`.
    Command(String),
}

impl Endpoint {
    /// Reads where the daemon is from command-line arguments, removing those it uses:
    /// `--home DIR`, `--socket PATH`, `--ws URL` with `--token TOKEN` or `--token-file PATH`,
    /// or `--via COMMAND`.
    /// The default is the socket in `$ARCHIPEL_HOME` or `~/.archipel`.
    ///
    /// # Errors
    /// When a token file cannot be read.
    pub fn from_args(args: &mut Vec<String>) -> io::Result<Self> {
        let home = option(args, "--home")
            .or_else(|| std::env::var("ARCHIPEL_HOME").ok())
            .map_or_else(
                || PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".archipel"),
                PathBuf::from,
            );
        if let Some(command) = option(args, "--via") {
            return Ok(Self::Command(command));
        }
        let socket = option(args, "--socket");
        let token = option(args, "--token");
        let token_file = option(args, "--token-file");
        Ok(match option(args, "--ws") {
            Some(url) => {
                let token = match (token, token_file) {
                    (Some(token), _) => token,
                    (None, file) => {
                        let path = file.map_or_else(|| home.join("ws-token"), PathBuf::from);
                        std::fs::read_to_string(&path)
                            .map_err(|error| {
                                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                            })?
                            .trim()
                            .to_owned()
                    }
                };
                Self::Ws { url, token }
            }
            None => Self::Local(socket.map_or_else(|| home.join("daemon.sock"), PathBuf::from)),
        })
    }
}

/// Removes `--name VALUE` from the arguments and returns the value.
pub fn option(args: &mut Vec<String>, name: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == name)?;
    args.remove(at);
    (at < args.len()).then(|| args.remove(at))
}

/// Removes `--name` from the arguments and says whether it was there.
pub fn switch(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != name);
    args.len() != before
}

#[derive(Debug)]
pub enum Error {
    /// The connection could not be opened.
    Connect(io::Error),
    /// The daemon closed the connection.
    Closed,
    /// The daemon or the agent refused the request.
    Refused { code: i64, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(error) => write!(f, "cannot reach the daemon: {error}"),
            Self::Closed => write!(f, "the daemon closed the connection"),
            Self::Refused { code, message } => write!(f, "{message} ({code})"),
        }
    }
}

impl std::error::Error for Error {}

/// An open connection to the daemon.
#[derive(Debug)]
pub struct Connection {
    link: Link,
    next_id: u64,
    /// Messages that arrived while waiting for a response, oldest first.
    backlog: VecDeque<Value>,
    /// The command that carries the connection, if any. Stopped when this is dropped.
    _carrier: Option<tokio::process::Child>,
}

impl Connection {
    /// Connects and introduces the client under `name`. Returns the daemon's `initialize` result.
    ///
    /// # Errors
    /// When the daemon cannot be reached or refuses the connection.
    pub async fn open(endpoint: &Endpoint, name: &str) -> Result<(Self, Value), Error> {
        let mut carrier = None;
        let link = match endpoint {
            Endpoint::Local(path) => local::connect(path).await,
            Endpoint::Ws { url, token } => ws::connect(url, token).await,
            Endpoint::Command(command) => {
                let spawned = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .kill_on_drop(true)
                    .spawn();
                spawned.and_then(|mut child| {
                    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take())
                    else {
                        return Err(io::Error::other("the command has no stdin or stdout"));
                    };
                    carrier = Some(child);
                    Ok(archipel_transport::lines(stdout, stdin))
                })
            }
        }
        .map_err(Error::Connect)?;
        let mut connection = Self {
            link,
            next_id: 1,
            backlog: VecDeque::new(),
            _carrier: carrier,
        };
        let hello = json!({
            "protocolVersion": 2,
            "info": {"name": name, "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"elicitation": {"form": {}, "url": {}}},
        });
        let welcome = connection.call("initialize", hello).await?;
        Ok((connection, welcome))
    }

    /// Sends a request and waits for its result. Other messages that arrive meanwhile are kept
    /// for [`next`](Self::next).
    ///
    /// # Errors
    /// When the request is refused or the connection closes.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let message = self.link.rx.recv().await.ok_or(Error::Closed)?;
            if message.get("method").is_some() || message.get("id") != Some(&json!(id)) {
                self.backlog.push_back(message);
                continue;
            }
            return match message.get("error") {
                Some(error) => Err(Error::Refused {
                    code: error["code"].as_i64().unwrap_or_default(),
                    message: error["message"].as_str().unwrap_or("refused").to_owned(),
                }),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
        }
    }

    /// The next message from the daemon, or `None` when the connection is closed.
    pub async fn next(&mut self) -> Option<Value> {
        match self.backlog.pop_front() {
            Some(message) => Some(message),
            None => self.link.rx.recv().await,
        }
    }

    /// Answers a request from the agent, such as an approval.
    ///
    /// # Errors
    /// When the connection is closed.
    pub async fn answer(&mut self, request_id: &Value, result: Value) -> Result<(), Error> {
        self.send(json!({"jsonrpc": "2.0", "id": request_id, "result": result}))
            .await
    }

    /// Refuses a request from the daemon, such as a call to a method an extension lacks.
    ///
    /// # Errors
    /// When the connection is closed.
    pub async fn refuse(&mut self, request_id: &Value, message: &str) -> Result<(), Error> {
        let error = json!({"code": -32601, "message": message});
        self.send(json!({"jsonrpc": "2.0", "id": request_id, "error": error}))
            .await
    }

    /// Sends a notification, such as `session/cancel`.
    ///
    /// # Errors
    /// When the connection is closed.
    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), Error> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    /// Sends a message as it is. For a client that sends several requests at once and matches
    /// the responses itself, as they come out of [`next`](Self::next): give such requests ids
    /// that [`call`](Self::call) does not use, which counts up from 1.
    ///
    /// # Errors
    /// When the connection is closed.
    pub async fn send(&mut self, message: Value) -> Result<(), Error> {
        self.link.tx.send(message).await.map_err(|_| Error::Closed)
    }
}
