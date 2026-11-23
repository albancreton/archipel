//! Items, reducer, session state, sequencing, and the profile and store traits. No IO.
//!
//! - [`state`]: folds ACP v2 messages into what a client shows, by following the schema's state rules.
//! - [`log`]: the numbered entries of a session and their condensed replay.
//! - [`store`]: where sessions and logs are kept, with an in-memory implementation.
//! - [`mirror`]: a client's copy of a session, fed by the daemon's messages.
//! - [`transcript`]: reads recorded sessions, for tests and for replaying against a fake adapter.
//! - [`render`]: a plain-text view of a conversation, for humans checking test results.

pub mod log;
pub mod mirror;
pub mod render;
pub mod state;
pub mod store;
pub mod transcript;

mod json;
