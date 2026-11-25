//! Runs the Pi profile against recorded `pi-acp` sessions (ACP v1) and checks what it tells the
//! daemon (ACP v2).
//!
//! The v2 conversations it produces are stored in `testdata/transcripts/pi-v2`, where the core's
//! tests treat them like any other recording. Set `UPDATE_VECTORS=1` to rewrite them.

use archipel_agents::{Fake, Launcher, Running, pi::Pi};
use archipel_core::{state::Direction, transcript::Transcript};
use archipel_transport::Link;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn recordings() -> Vec<(String, Transcript)> {
    let mut found: Vec<(String, Transcript)> = fs::read_dir(testdata().join("adapters/pi-v1"))
        .expect("recordings exist")
        .flatten()
        .filter(|file| file.path().extension().is_some_and(|ext| ext == "ndjson"))
        .map(|file| {
            let name = file
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(file.path()).unwrap();
            (name, Transcript::parse(&text).expect("parses"))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(found.len() >= 12);
    found
}

/// Counts how many `pi-acp` processes were started.
struct Counting {
    fake: Fake,
    started: Arc<AtomicUsize>,
}

impl Launcher for Counting {
    fn launch(&self) -> std::io::Result<Running> {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.fake.launch()
    }
}

fn profile(recording: &Transcript, idle: Duration) -> (Pi, Arc<AtomicUsize>) {
    let started = Arc::new(AtomicUsize::new(0));
    let counter = AtomicUsize::new(0);
    let pi = Pi {
        adapter: Arc::new(Counting {
            fake: Fake {
                transcript: recording.clone(),
                speed: 0.0,
            },
            started: started.clone(),
        }),
        model: Some(recording.header.model.clone()),
        idle,
        // Predictable ids, so the stored conversations do not change from run to run.
        ids: Arc::new(move || format!("m{}", counter.fetch_add(1, Ordering::SeqCst) + 1)),
    };
    (pi, started)
}

/// What identifies an update on both sides of the translation, to know how far a turn has got.
fn mark(update: &Value) -> Option<String> {
    let kind = update["sessionUpdate"].as_str()?;
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            let text = update["content"]["text"].as_str()?;
            let notice = text.starts_with("Queued message") || text.starts_with("Starting queued");
            (!notice).then(|| format!("{kind}:{text}"))
        }
        "tool_call" | "tool_call_update" => Some(format!(
            "tool:{}:{}",
            update["toolCallId"], update["status"]
        )),
        "usage_update" => Some(format!("usage:{}", update["used"])),
        _ => None,
    }
}

/// The daemon's side of a conversation with the profile.
struct Daemon {
    link: Link,
    next_id: u64,
    /// Everything received, in order.
    received: Vec<Value>,
    /// Messages sent, each with how many had been received by then.
    sent: Vec<(usize, Value)>,
    seen: HashMap<String, usize>,
    idles: usize,
}

impl Daemon {
    fn new(link: Link) -> Self {
        Self {
            link,
            next_id: 1,
            received: Vec::new(),
            sent: Vec::new(),
            seen: HashMap::new(),
            idles: 0,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(message).await;
        id
    }

    async fn send(&mut self, message: Value) {
        self.sent.push((self.received.len(), message.clone()));
        self.link.tx.send(message).await.unwrap();
    }

    async fn next(&mut self) -> Value {
        let message = tokio::time::timeout(Duration::from_secs(10), self.link.rx.recv())
            .await
            .expect("the profile says something within 10 seconds")
            .expect("the profile is running");
        let update = &message["params"]["update"];
        if let Some(mark) = mark(update) {
            *self.seen.entry(mark).or_default() += 1;
        }
        if update["sessionUpdate"] == "state_update" && update["state"] == "idle" {
            self.idles += 1;
        }
        self.received.push(message.clone());
        message
    }

    async fn response(&mut self, id: u64) -> Value {
        loop {
            let message = self.next().await;
            if message.get("method").is_none() && message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// The conversation as a transcript: every message where it was sent or received.
    fn transcript(&self, recording: &Transcript) -> String {
        let mut header = recording.header.clone();
        header.protocol = 2;
        header.adapter = format!("{} through the archipel Pi profile", header.adapter);
        let mut lines = vec![serde_json::to_string(&header).unwrap()];
        let mut sent = self.sent.iter().peekable();
        for at in 0..=self.received.len() {
            while let Some((_, message)) = sent.next_if(|(before, _)| *before == at) {
                lines.push(json!({"dir": "out", "t": at, "m": message}).to_string());
            }
            if let Some(message) = self.received.get(at) {
                lines.push(json!({"dir": "in", "t": at, "m": message}).to_string());
            }
        }
        lines.join("\n") + "\n"
    }
}

/// Plays the client's part of a v1 recording in v2 terms.
async fn play(daemon: &mut Daemon, recording: &Transcript) {
    let mut expected: HashMap<String, usize> = HashMap::new();
    let mut reached: Option<String> = None;
    // `idle_at`: how many `idle` states must have arrived for the running turn to be over.
    let (mut prompts, mut turns_over, mut idle_at) = (0usize, 0usize, 0usize);
    let mut prompt_ids = Vec::new();
    for frame in &recording.frames {
        let method = frame.m["method"].as_str();
        let params = &frame.m["params"];
        if frame.dir == Direction::In {
            if let Some(mark) = mark(&params["update"]) {
                *expected.entry(mark.clone()).or_default() += 1;
                reached = Some(mark);
            }
            if method.is_none() && prompt_ids.contains(&frame.m["id"]) {
                turns_over += 1;
            }
            continue;
        }
        // Before saying anything, let the profile get as far as the recording was.
        if prompts > 0 && turns_over == prompts {
            while daemon.idles < idle_at {
                daemon.next().await;
            }
            prompts = 0;
            turns_over = 0;
        }
        if prompts > 0
            && let Some(mark) = &reached
        {
            while daemon.seen.get(mark).copied().unwrap_or_default() < expected[mark] {
                daemon.next().await;
            }
        }
        match method {
            Some("initialize") => {
                let id = daemon
                    .request("initialize", json!({"protocolVersion": 2}))
                    .await;
                let welcome = daemon.response(id).await;
                assert_eq!(welcome["result"]["protocolVersion"], json!(2));
                assert_eq!(welcome["result"]["info"]["name"], json!("pi-acp"));
            }
            Some("session/new") => {
                let id = daemon.request("session/new", params.clone()).await;
                let created = daemon.response(id).await;
                assert!(created["result"]["sessionId"].is_string(), "{created}");
                let model = &created["result"]["configOptions"][0];
                assert_eq!(
                    model["configId"],
                    json!("model"),
                    "v2 names the key configId"
                );
                assert_eq!(model["currentValue"], json!(recording.header.model));
            }
            // The profile selects the configured model itself.
            Some("session/set_config_option") => {}
            Some("session/load") => {
                let mut params = params.clone();
                params["replayFrom"] = json!({"type": "start"});
                let id = daemon.request("session/resume", params).await;
                let resumed = daemon.response(id).await;
                assert!(resumed.get("result").is_some(), "{resumed}");
            }
            Some("session/prompt") => {
                prompt_ids.push(frame.m["id"].clone());
                if prompts == 0 {
                    idle_at = daemon.idles + 1;
                }
                prompts += 1;
                reached = None;
                let id = daemon.request("session/prompt", params.clone()).await;
                let sent = daemon.response(id).await;
                assert!(
                    sent["result"]["messageId"].is_string(),
                    "answers at once: {sent}"
                );
            }
            Some(method) => {
                let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
                daemon.send(message).await;
            }
            None => {}
        }
    }
    while prompts > 0 && daemon.idles < idle_at {
        daemon.next().await;
    }
}

#[tokio::test]
async fn recorded_pi_sessions_come_out_as_acp_v2() {
    let update = std::env::var_os("UPDATE_VECTORS").is_some();
    let mut stale = Vec::new();
    for (name, recording) in recordings() {
        let (pi, _) = profile(&recording, Duration::from_secs(3600));
        let mut daemon = Daemon::new(pi.launch().unwrap().link);
        play(&mut daemon, &recording).await;

        let kinds: Vec<&str> = daemon
            .received
            .iter()
            .filter_map(|message| message["params"]["update"]["sessionUpdate"].as_str())
            .collect();
        for dropped in ["tool_call", "current_mode_update"] {
            assert!(
                !kinds.contains(&dropped),
                "{name}: a v1 `{dropped}` got through"
            );
        }
        for message in &daemon.received {
            let update = &message["params"]["update"];
            let text = update["content"]["text"].as_str().unwrap_or_default();
            assert!(
                !text.starts_with("pi v"),
                "{name}: the startup banner got through"
            );
            assert!(
                !text.starts_with("Queued message"),
                "{name}: a queue notice got through"
            );
            if update["sessionUpdate"]
                .as_str()
                .is_some_and(|kind| kind.ends_with("_chunk"))
                && update.get("terminalId").is_none()
            {
                assert!(
                    update["messageId"].is_string(),
                    "{name}: a chunk without an id: {update}"
                );
            }
            assert!(update.pointer("/_meta/terminal_output").is_none(), "{name}");
            assert!(update.pointer("/_meta/piAcp").is_none(), "{name}");
        }

        let path = testdata().join(format!("transcripts/pi-v2/{name}.ndjson"));
        let produced = daemon.transcript(&recording);
        if update {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &produced).unwrap();
        } else if fs::read_to_string(&path).ok().as_deref() != Some(produced.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "the translation of {stale:?} differs from testdata/transcripts/pi-v2; run with UPDATE_VECTORS=1 and review the diff"
    );
}

#[tokio::test]
async fn an_idle_session_loses_its_process_and_gets_one_back_when_used() {
    let recording = recordings()
        .into_iter()
        .find(|(name, _)| name == "simple")
        .unwrap()
        .1;
    let (pi, started) = profile(&recording, Duration::from_millis(40));
    let mut daemon = Daemon::new(pi.launch().unwrap().link);
    play(&mut daemon, &recording).await;
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "one process for one session"
    );
    let session = daemon
        .received
        .iter()
        .find_map(|message| message["result"]["sessionId"].as_str())
        .unwrap()
        .to_owned();

    // Nothing happens for a while: the process is stopped. The daemon is not told anything.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = daemon.received.len();

    // The next prompt starts a process, reloads the session in it, and only then prompts.
    let params = json!({"sessionId": session, "prompt": [{"type": "text", "text": "again"}]});
    let id = daemon.request("session/prompt", params).await;
    let sent = daemon.response(id).await;
    assert!(sent["result"]["messageId"].is_string(), "{sent}");
    assert_eq!(
        started.load(Ordering::SeqCst),
        2,
        "a second process was started"
    );
    let wanted = daemon.idles + 1;
    while daemon.idles < wanted {
        daemon.next().await;
    }
    // The reload replays the session; none of that is passed on a second time.
    let later: Vec<&str> = daemon.received[before..]
        .iter()
        .filter_map(|message| message["params"]["update"]["sessionUpdate"].as_str())
        .collect();
    assert_eq!(later[..2], ["user_message", "state_update"], "{later:?}");
    assert!(!later.contains(&"agent_message_chunk"), "{later:?}");
    // The fake's recording has no second prompt, which shows as a failed prompt.
    assert!(later.contains(&"notice"), "{later:?}");
}
