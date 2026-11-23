//! Runs the `bridge` program against a daemon on a local socket, with a fake adapter.

use bridge_agents::{AgentSpec, Fake};
use bridge_core::transcript::Transcript;
use bridge_server::Server;
use bridge_transport::local::LocalServer;
use std::{fs, path::Path, sync::Arc};
use tokio::process::Command;

async fn bridge(socket: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_bridge"))
        .arg("--socket")
        .arg(socket)
        .args(args)
        .output()
        .await
        .expect("bridge runs");
    assert!(
        output.status.success(),
        "bridge {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("text")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_command_line_client_creates_prompts_watches_and_shows_a_session() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let recorded =
        fs::read_to_string(testdata.join("transcripts/claude-v2/simple.ndjson")).unwrap();
    let transcript = Transcript::parse(&recorded).unwrap();
    let prompt = transcript.header.prompts[0].clone();

    let dir = std::env::temp_dir().join(format!("bridge-cli-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("daemon.sock");
    let listener = LocalServer::bind(&socket).unwrap();
    let server = Server::builder()
        .agent(AgentSpec {
            id: "claude".into(),
            name: "Fake".into(),
            launcher: Arc::new(Fake {
                transcript,
                speed: 0.0,
            }),
        })
        .start();
    let accepting = server.clone();
    tokio::spawn(async move {
        while let Ok(link) = listener.accept().await {
            accepting.connect(link);
        }
    });

    // The demo extension, as its own process.
    let mut extension = Command::new(env!("CARGO_BIN_EXE_bridge-demo-checkpoints"))
        .arg("--socket")
        .arg(&socket)
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the extension starts");
    let mut ready = String::new();
    tokio::io::AsyncBufReadExt::read_line(
        &mut tokio::io::BufReader::new(extension.stderr.take().unwrap()),
        &mut ready,
    )
    .await
    .unwrap();
    assert!(ready.contains("registered"), "{ready}");

    assert!(bridge(&socket, &["agents"]).await.starts_with("claude"));
    let session = bridge(&socket, &["new", "--cwd", "/"]).await;
    let session = session.trim();
    // The first characters of the id are enough.
    assert!(
        bridge(&socket, &["prompt", &session[..8], &prompt])
            .await
            .starts_with("sent ")
    );

    let watched = bridge(&socket, &["watch", session, "--until-idle"]).await;
    assert!(watched.contains("[agent_message] "), "{watched}");
    assert!(watched.trim_end().ends_with("-- idle"), "{watched}");

    // The session as recorded, plus the checkpoint the extension added when the turn ended.
    let checkpoint = format!("{}-1", &session[..8]);
    let listed = loop {
        let params = format!(r#"{{"sessionId":"{session}"}}"#);
        let listed = bridge(&socket, &["call", "_x/demo/checkpoints/list", &params]).await;
        if listed.contains(&checkpoint) {
            break listed;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert!(listed.contains("after turn 1"), "{listed}");
    let shown = bridge(&socket, &["show", session]).await;
    let vector = fs::read_to_string(testdata.join("vectors/claude-v2/simple.txt")).unwrap();
    let recorded: Vec<&str> = vector.split_once("\n\n").unwrap().1.lines().collect();
    let shown_lines: Vec<&str> = shown.lines().collect();
    let extra: Vec<&&str> = shown_lines
        .iter()
        .filter(|line| !recorded.contains(line))
        .collect();
    assert_eq!(extra.len(), 1, "{shown}");
    assert!(extra[0].starts_with("[x.demo.checkpoint] ") && extra[0].contains(&checkpoint));

    // Restoring changes that item; it does not add another.
    let params = format!(r#"{{"sessionId":"{session}","checkpointId":"{checkpoint}"}}"#);
    let restored = bridge(&socket, &["call", "_x/demo/checkpoint/restore", &params]).await;
    assert!(restored.contains(&checkpoint), "{restored}");
    let shown = bridge(&socket, &["show", session]).await;
    assert_eq!(shown.matches("[x.demo.checkpoint]").count(), 1, "{shown}");
    assert!(shown.contains(r#""restored":true"#), "{shown}");

    let listed = bridge(&socket, &["sessions"]).await;
    assert!(
        listed.contains(session) && listed.contains("idle"),
        "{listed}"
    );

    // The same through a command that carries the connection, as SSH would:
    // `bridge --via "ssh host bridge connect --stdio" …`. Here the "remote" side is local.
    let carrier = format!(
        "{} --socket {} connect --stdio",
        env!("CARGO_BIN_EXE_bridge"),
        socket.display()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_bridge"))
        .args(["--via", &carrier, "show", session])
        .output()
        .await
        .expect("bridge runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), shown);
    server.shutdown().await;
    fs::remove_dir_all(&dir).unwrap();
}
