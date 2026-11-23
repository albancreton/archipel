//! Starts a real adapter process (the fake one) and speaks ACP with it over stdin and stdout.

use bridge_agents::{Launcher, Process, initialize_params};
use serde_json::json;
use std::path::Path;

#[tokio::test]
async fn a_spawned_adapter_answers_and_stops_when_dropped() {
    let transcript = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/transcripts/claude-v2/simple.ndjson");
    let process = Process {
        command: env!("CARGO_BIN_EXE_fake-adapter").to_owned(),
        args: vec![transcript.display().to_string()],
        ..Process::default()
    };
    let mut running = process.launch().expect("starts");
    running
        .link
        .tx
        .send(json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": initialize_params()}))
        .await
        .unwrap();
    let answer = running.link.rx.recv().await.expect("an answer");
    assert_eq!(answer["id"], json!(7), "the answer carries our id");
    assert_eq!(answer["result"]["protocolVersion"], json!(2));

    drop(running.guard);
    assert_eq!(running.link.rx.recv().await, None, "the process is gone");

    let missing = Process {
        command: "/nonexistent/adapter".into(),
        ..Process::default()
    };
    assert!(missing.launch().is_err());
}
