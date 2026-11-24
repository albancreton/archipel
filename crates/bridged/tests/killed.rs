//! The daemon program when it is killed without warning, and when its log file is damaged.

use bridge_transport::{Link, local};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

const BRIDGED: &str = env!("CARGO_BIN_EXE_bridged");

fn recording() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/transcripts/claude-v2/approval_command.ndjson")
}

fn home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("bridged-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    home
}

/// Starts the daemon with the recorded agent and waits until it listens.
async fn start(home: &Path) -> Child {
    let mut daemon = Command::new(BRIDGED)
        .args([
            "--home",
            home.to_str().unwrap(),
            "--fake-speed",
            "0",
            "--fake",
        ])
        .arg(recording())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the daemon starts");
    let mut lines = BufReader::new(daemon.stderr.take().unwrap()).lines();
    while let Some(line) = lines.next_line().await.unwrap() {
        if line.contains("listening on") {
            return daemon;
        }
    }
    panic!("the daemon did not say where it listens");
}

async fn next(link: &mut Link) -> Value {
    tokio::time::timeout(Duration::from_secs(10), link.rx.recv())
        .await
        .expect("a message within 10 seconds")
        .expect("the connection is open")
}

/// Reads until `wanted` matches. Returns that message and everything before it.
async fn until(link: &mut Link, wanted: impl Fn(&Value) -> bool) -> (Value, Vec<Value>) {
    let mut before = Vec::new();
    loop {
        let message = next(link).await;
        if wanted(&message) {
            return (message, before);
        }
        before.push(message);
    }
}

#[tokio::test]
async fn a_daemon_killed_in_the_middle_of_a_turn_shows_the_turn_as_stopped_when_it_is_back() {
    let home = home("killed");
    let socket = home.join("daemon.sock");
    let mut daemon = start(&home).await;
    let mut app = local::connect(&socket).await.unwrap();

    // What the recorded app sent, up to the prompt. The turn then runs until it needs an approval.
    let recorded = fs::read_to_string(recording()).unwrap();
    let mut session = String::new();
    for frame in recorded
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
    {
        if frame["dir"] != "out" || frame["m"]["method"].is_null() {
            continue;
        }
        let mut message = frame["m"].clone();
        let method = message["method"].as_str().unwrap().to_owned();
        if message["params"].get("sessionId").is_some() {
            message["params"]["sessionId"] = json!(session);
        }
        let id = message["id"].clone();
        app.tx.send(message).await.unwrap();
        if method == "session/prompt" {
            break;
        }
        let (answered, _) = until(&mut app, |message| {
            message["method"].is_null() && message["id"] == id
        })
        .await;
        if method == "session/new" {
            session = answered["result"]["sessionId"].as_str().unwrap().to_owned();
        }
    }
    until(&mut app, |message| {
        message["method"] == "session/request_permission"
    })
    .await;

    // No warning, no clean-up: the log ends with a running turn and an open approval.
    daemon.kill().await.unwrap();
    drop(app);
    assert!(
        socket.exists(),
        "the dead daemon's socket file is still there"
    );

    // The next daemon starts over the stale socket file and reads the same log.
    let mut daemon = start(&home).await;
    let mut app = local::connect(&socket).await.unwrap();
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "_bridge/sessions/list", "params": {"loggedOnly": true}});
    app.tx.send(list).await.unwrap();
    let (listed, _) = until(&mut app, |message| message["id"] == 1).await;
    let summary = &listed["result"]["sessions"][0];
    assert_eq!(summary["sessionId"], json!(session));
    assert_eq!(summary["live"], false);
    assert_ne!(summary["state"], "running", "{summary}");
    assert_ne!(summary["state"], "requires_action", "{summary}");

    let attach = json!({"jsonrpc": "2.0", "id": 2, "method": "_bridge/session/attach", "params": {"sessionId": session}});
    app.tx.send(attach).await.unwrap();
    let (_, replay) = until(&mut app, |message| {
        message["params"]["update"]["sessionUpdate"] == "replay_complete"
    })
    .await;
    let states: Vec<&Value> = replay
        .iter()
        .map(|message| &message["params"]["update"])
        .filter(|update| update["sessionUpdate"] == "state_update")
        .collect();
    let last = states.last().expect("the turn's states are in the log");
    assert_eq!(
        last["state"], "idle",
        "the turn is not left running for ever"
    );
    assert_eq!(last["stopReason"], "cancelled");
    let resolved = replay
        .iter()
        .find(|message| message["method"] == "_bridge/request/resolved")
        .expect("the approval is closed");
    assert_eq!(
        resolved["params"]["outcome"]["outcome"]["outcome"], "cancelled",
        "{resolved}"
    );

    daemon.kill().await.unwrap();
    let _ = fs::remove_dir_all(&home);
}

#[tokio::test]
async fn a_damaged_log_file_stops_the_daemon_with_a_message_that_names_it() {
    let home = home("damaged");
    fs::create_dir_all(&home).unwrap();
    let log = home.join("log.sqlite");
    fs::write(
        &log,
        "this is not a database, and it is long enough to be read as one: ".repeat(40),
    )
    .unwrap();

    let output = Command::new(BRIDGED)
        .args(["--home", home.to_str().unwrap(), "--fake"])
        .arg(recording())
        .output();
    let output = tokio::time::timeout(Duration::from_secs(10), output)
        .await
        .expect("it does not hang")
        .unwrap();
    assert!(
        !output.status.success(),
        "it does not run on a log it cannot read"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("log.sqlite"),
        "the message names the file: {said}"
    );
    assert_eq!(
        fs::read(&log).unwrap().len(),
        fs::metadata(&log).unwrap().len() as usize,
        "the file is left as it was, for the user to look at"
    );
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .starts_with("this is not a database")
    );
    let _ = fs::remove_dir_all(&home);
}
