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

    let shown = bridge(&socket, &["show", session]).await;
    let vector = fs::read_to_string(testdata.join("vectors/claude-v2/simple.txt")).unwrap();
    assert_eq!(shown, vector.split_once("\n\n").unwrap().1);

    let listed = bridge(&socket, &["sessions"]).await;
    assert!(
        listed.contains(session) && listed.contains("idle"),
        "{listed}"
    );
    server.shutdown().await;
    fs::remove_dir_all(&dir).unwrap();
}
