//! A client's connection to the daemon.
//!
//! [`Connection`] sends requests and hands out what the daemon sends. What a client holds for a
//! session, and how the daemon's messages change it, is [`bridge_core::mirror::Mirror`].

use bridge_transport::{Link, local, ws};
use serde_json::{Value, json};
use std::{collections::VecDeque, fmt, io, path::PathBuf};

/// Where a daemon listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// The local socket of a daemon on this machine.
    Local(PathBuf),
    /// A WebSocket address such as `ws://127.0.0.1:7890`, with the daemon's token.
    Ws { url: String, token: String },
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
}

impl Connection {
    /// Connects and introduces the client under `name`. Returns the daemon's `initialize` result.
    ///
    /// # Errors
    /// When the daemon cannot be reached or refuses the connection.
    pub async fn open(endpoint: &Endpoint, name: &str) -> Result<(Self, Value), Error> {
        let link = match endpoint {
            Endpoint::Local(path) => local::connect(path).await,
            Endpoint::Ws { url, token } => ws::connect(url, token).await,
        }
        .map_err(Error::Connect)?;
        let mut connection = Self {
            link,
            next_id: 1,
            backlog: VecDeque::new(),
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

    /// Sends a notification, such as `session/cancel`.
    ///
    /// # Errors
    /// When the connection is closed.
    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), Error> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn send(&mut self, message: Value) -> Result<(), Error> {
        self.link.tx.send(message).await.map_err(|_| Error::Closed)
    }
}
