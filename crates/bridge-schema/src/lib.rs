//! Schema files and the types generated from them. No IO.
//!
//! - [`acp`]: the ACP v2 types, from the official schema crate.
//! - [`rules`]: how each ACP message changes client state, read from the schema.
//! - [`ext`]: the daemon's own extensions (`_bridge/` methods, `_meta.bridge`).

pub mod ext;
pub mod rules;

/// ACP v2 protocol types.
pub use agent_client_protocol_schema::v2 as acp;

/// The pinned ACP v2 schema, as vendored in `schema/acp-v2.schema.json`.
pub const ACP_V2_SCHEMA: &str = include_str!("../../../schema/acp-v2.schema.json");

/// The state rules extracted from [`ACP_V2_SCHEMA`], as checked in at `schema/state-rules.json`.
pub const STATE_RULES: &str = include_str!("../../../schema/state-rules.json");
