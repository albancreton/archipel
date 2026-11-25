//! Regenerates `schema/state-rules.json` (and the client SDKs' copies of it) and
//! `schema/archipel.schema.json`. Run through `just gen`.

use archipel_schema::{ext::ArchipelExtensions, rules::RuleSet};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");

    let acp: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_dir.join("acp-v2.schema.json"))?)?;
    let rules = RuleSet::extract(&acp, "acp-v2.schema.json (x-state annotations)");
    let rules = serde_json::to_value(&rules)?;
    write(&schema_dir.join("state-rules.json"), &rules)?;
    // The client SDKs carry their own copy, so each package is complete by itself.
    for copy in [
        "../../clients/ts/src/generated/state-rules.json",
        "../../clients/swift/Sources/ArchipelClient/Resources/state-rules.json",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(copy);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write(&path, &rules)?;
    }

    let mut ext = serde_json::to_value(schemars::schema_for!(ArchipelExtensions))?;
    if let Some(root) = ext.as_object_mut() {
        root.insert(
            "title".into(),
            "Archipel extensions to ACP v2 (draft)".into(),
        );
    }
    write(&schema_dir.join("archipel.schema.json"), &ext)?;
    Ok(())
}

fn write(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    fs::write(path, text)
}
