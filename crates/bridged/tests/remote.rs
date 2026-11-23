//! Starts the daemon program and checks what a remote client goes through: pairing with a
//! one-time code, a token with a scope, and revoking it.

use bridge_transport::ws;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

const BRIDGED: &str = env!("CARGO_BIN_EXE_bridged");

async fn start(home: &Path) -> (Child, String) {
    let recording = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/transcripts/claude-v2/simple.ndjson");
    let mut daemon = Command::new(BRIDGED)
        .args([
            "--home",
            home.to_str().unwrap(),
            "--ws",
            "127.0.0.1:0",
            "--fake",
        ])
        .arg(recording)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the daemon starts");
    let mut lines = BufReader::new(daemon.stderr.take().unwrap()).lines();
    while let Some(line) = lines.next_line().await.unwrap() {
        if let Some(rest) = line.split("listening on ws://").nth(1) {
            let address = rest.split(' ').next().unwrap();
            return (daemon, format!("ws://{address}"));
        }
    }
    panic!("the daemon did not say where it listens");
}

/// Runs a `bridged` subcommand on the same home and returns what it printed.
async fn bridged(home: &Path, args: &[&str]) -> String {
    let output = Command::new(BRIDGED)
        .args(["--home", home.to_str().unwrap()])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

async fn call(link: &mut bridge_transport::Link, method: &str, params: Value) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    link.tx.send(request).await.unwrap();
    link.rx.recv().await.expect("an answer")
}

fn home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("bridged-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    home
}

#[tokio::test]
async fn an_app_pairs_with_a_one_time_code_and_gets_a_token_of_its_own() {
    let home = home("pair");
    let (_daemon, url) = start(&home).await;

    let code = bridged(&home, &["pair", "--scope", "observe", "--name", "phone"]).await;
    assert_eq!(
        code.len(),
        11,
        "five characters, a dash, five characters: {code}"
    );

    // A wrong code gets nothing. The right one, typed in lower case without the dash, works.
    assert!(ws::connect(&url, "pair:AAAAA-AAAAA").await.is_err());
    let typed = code.replace('-', "").to_lowercase();
    let mut pairing = ws::connect(&url, &format!("pair:{typed}"))
        .await
        .expect("accepted");
    // The pairing connection can do one thing only.
    let refused = call(&mut pairing, "_bridge/sessions/list", json!({})).await;
    assert_eq!(refused["error"]["code"], json!(-32003));
    let paired = call(&mut pairing, "_bridge/pair", json!({})).await;
    let token = paired["result"]["token"]
        .as_str()
        .expect("a token")
        .to_owned();
    assert_eq!(paired["result"]["scope"], json!("observe"));
    assert_eq!(paired["result"]["name"], json!("phone"));

    // The code is spent.
    assert!(ws::connect(&url, &format!("pair:{code}")).await.is_err());

    // The token works, with the scope the code was made for.
    let mut app = ws::connect(&url, &token)
        .await
        .expect("the token is accepted");
    let welcome = call(&mut app, "initialize", json!({"protocolVersion": 2})).await;
    assert_eq!(
        welcome["result"]["_meta"]["bridge"]["scope"],
        json!("observe")
    );
    let refused = call(
        &mut app,
        "session/new",
        json!({"cwd": "/", "mcpServers": []}),
    )
    .await;
    assert_eq!(refused["error"]["code"], json!(-32003));

    // The daemon keeps the token's hash, not the token.
    let stored = fs::read_to_string(home.join("tokens.json")).unwrap();
    assert!(
        !stored.contains(&token) && stored.contains("\"hash\""),
        "{stored}"
    );
    assert_eq!(
        bridged(&home, &["token", "list"]).await,
        format!("{:<20} observe", "phone")
    );

    // Revoking it takes effect at once, without restarting the daemon.
    bridged(&home, &["token", "remove", "phone"]).await;
    assert!(ws::connect(&url, &token).await.is_err());
    fs::remove_dir_all(&home).unwrap();
}

#[tokio::test]
async fn a_code_expires() {
    let home = home("expire");
    let (_daemon, url) = start(&home).await;
    let code = bridged(&home, &["pair", "--minutes", "0"]).await;
    assert!(ws::connect(&url, &format!("pair:{code}")).await.is_err());
    fs::remove_dir_all(&home).unwrap();
}
