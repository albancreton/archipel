//! Regenerates `schema/state-rules.json` and `schema/bridge.schema.json`. Run through `just gen`.

use bridge_schema::{ext::BridgeExtensions, rules::RuleSet};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");

    let acp: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_dir.join("acp-v2.schema.json"))?)?;
    let rules = RuleSet::extract(&acp, "acp-v2.schema.json (x-state annotations)");
    write(
        &schema_dir.join("state-rules.json"),
        &serde_json::to_value(&rules)?,
    )?;

    let mut ext = serde_json::to_value(schemars::schema_for!(BridgeExtensions))?;
    if let Some(root) = ext.as_object_mut() {
        root.insert("title".into(), "Bridge extensions to ACP v2 (draft)".into());
    }
    write(&schema_dir.join("bridge.schema.json"), &ext)?;
    Ok(())
}

fn write(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    fs::write(path, text)
}
