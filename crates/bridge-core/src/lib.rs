//! Items, reducer, session state, sequencing, and the profile and store traits. No IO.
//!
//! - [`state`]: folds ACP v2 messages into what a client shows, by following the schema's state rules.
//! - [`log`]: numbers a session's updates and produces a condensed replay.
//! - [`transcript`]: reads recorded sessions, for tests and for replaying against a fake adapter.
//! - [`render`]: a plain-text view of a conversation, for humans checking test results.

pub mod log;
pub mod render;
pub mod state;
pub mod transcript;

mod json;
