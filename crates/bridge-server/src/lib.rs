//! The daemon as a library: sessions that outlive the apps showing them.
//!
//! ```text
//! apps ── Link ──┐                       ┌── Link ── adapter (claude-agent-acp, ...)
//! apps ── Link ──┼── one task, one state ┤
//!                └── Store (the log)     └── Link ── adapter
//! ```
//!
//! Everything runs on one task that owns all state and handles one event at a time (a message
//! from an app, a message from an adapter, a connection closing). Nothing is shared or locked,
//! so the order of events is the only thing to reason about. See [`daemon`].
//!
//! ```no_run
//! # async fn example(link: bridge_transport::Link, spec: bridge_agents::AgentSpec) {
//! let server = bridge_server::Server::builder().agent(spec).start();
//! server.connect(link); // one call per app connection, from any transport
//! # }
//! ```

mod daemon;
mod wire;

use bridge_agents::AgentSpec;
use bridge_core::store::{MemoryStore, Store};
use bridge_transport::Link;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{mpsc, oneshot};

pub use bridge_store_sqlite::SqliteStore;

/// Entry point: `Server::builder()`.
pub struct Server;

impl Server {
    #[must_use]
    pub fn builder() -> Builder {
        Builder {
            agents: Vec::new(),
            store: Box::new(MemoryStore::default()),
        }
    }
}

/// Collects what a daemon needs before it starts.
pub struct Builder {
    agents: Vec<AgentSpec>,
    store: Box<dyn Store>,
}

impl Builder {
    /// Adds an agent the daemon can start. The first one added is the default.
    #[must_use]
    pub fn agent(mut self, spec: AgentSpec) -> Self {
        self.agents.push(spec);
        self
    }

    /// Where sessions and logs are kept. Without this they are kept in memory.
    #[must_use]
    pub fn store(mut self, store: impl Store + 'static) -> Self {
        self.store = Box::new(store);
        self
    }

    /// Starts the daemon on the current tokio runtime.
    #[must_use]
    pub fn start(self) -> Handle {
        let (events, inbox) = mpsc::channel(bridge_transport::QUEUE);
        tokio::spawn(daemon::Daemon::new(self.agents, self.store, events.clone()).run(inbox));
        Handle {
            events,
            next_client: Arc::new(AtomicU64::new(1)),
        }
    }
}

/// A running daemon. Cloning gives another handle to the same daemon.
#[derive(Clone)]
pub struct Handle {
    events: mpsc::Sender<daemon::Event>,
    next_client: Arc<AtomicU64>,
}

impl Handle {
    /// Serves one app connection until it closes.
    pub fn connect(&self, link: Link) {
        let client = self.next_client.fetch_add(1, Ordering::Relaxed);
        let events = self.events.clone();
        let Link { tx, mut rx } = link;
        tokio::spawn(async move {
            if events
                .send(daemon::Event::ClientOpened { client, tx })
                .await
                .is_err()
            {
                return;
            }
            while let Some(message) = rx.recv().await {
                if events
                    .send(daemon::Event::FromClient { client, message })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            let _ = events.send(daemon::Event::ClientClosed { client }).await;
        });
    }

    /// Stops the daemon and its adapters. Returns once it has stopped.
    pub async fn shutdown(&self) {
        let (done, stopped) = oneshot::channel();
        if self
            .events
            .send(daemon::Event::Shutdown(done))
            .await
            .is_ok()
        {
            let _ = stopped.await;
        }
    }
}
