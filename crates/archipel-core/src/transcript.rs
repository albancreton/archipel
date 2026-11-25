//! Recorded sessions: every JSON-RPC message between a client and an adapter, with timestamps.
//!
//! A transcript is a text file with one JSON object per line. The first line is the [`Header`];
//! each following line is a [`Frame`]. See `testdata/README.md`.

use crate::state::{ApplyError, Conversation, Direction};
use archipel_schema::rules::RuleSet;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The first line of a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// Version of the transcript format.
    pub transcript: u32,
    pub scenario: String,
    /// The harness family, such as `claude` or `pi`.
    pub agent: String,
    /// The harness and its version.
    pub harness: String,
    /// The ACP adapter and its version.
    pub adapter: String,
    /// The ACP protocol version that was negotiated.
    pub protocol: u32,
    pub model: String,
    /// The user prompts, in order.
    pub prompts: Vec<String>,
    /// The day of the recording, as `YYYY-MM-DD`.
    pub recorded_at: String,
}

/// One recorded message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Which way the message travelled, seen from the client.
    pub dir: Direction,
    /// Milliseconds since the recording started.
    pub t: u64,
    /// The JSON-RPC message.
    pub m: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub header: Header,
    pub frames: Vec<Frame>,
}

impl Transcript {
    /// Reads a transcript from its text.
    ///
    /// # Errors
    /// When a line is not valid JSON of the expected shape.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        let mut lines = text.lines().filter(|line| !line.trim().is_empty());
        let header = serde_json::from_str(lines.next().unwrap_or("null"))?;
        let frames = lines.map(serde_json::from_str).collect::<Result<_, _>>()?;
        Ok(Self { header, frames })
    }

    /// The `session/update` bodies sent by the agent, with their session id.
    pub fn updates(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.frames
            .iter()
            .filter(|frame| frame.dir == Direction::In)
            .filter_map(|frame| {
                let params = frame.m.get("params")?;
                (frame.m.get("method")?.as_str()? == "session/update")
                    .then(|| Some((params.get("sessionId")?.as_str()?, params.get("update")?)))
                    .flatten()
            })
    }

    /// Applies every message, giving what a client would hold at the end.
    ///
    /// # Errors
    /// When a message cannot be applied.
    pub fn conversation(&self, rules: &RuleSet) -> Result<Conversation, ApplyError> {
        let mut conversation = Conversation::default();
        for frame in &self.frames {
            conversation.apply_message(rules, frame.dir, &frame.m)?;
        }
        Ok(conversation)
    }
}
