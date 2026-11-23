//! `bridge`: a command-line client for the daemon.

use bridge_client::{Connection, Endpoint, Error};
use bridge_core::{
    mirror::{Mirror, seq_of},
    render,
};
use bridge_schema::{ext, rules::RuleSet};
use serde_json::{Value, json};
use std::{io::Write as _, path::PathBuf, process::ExitCode, time::Duration};

const USAGE: &str = "\
bridge: talk to the harness-bridge daemon

  bridge agents                          the agents the daemon can start
  bridge sessions [--logged]             all sessions, newest first. `+` marks those only the
                                         harness knows; opening one loads its history.
                                         --logged leaves those out and starts no agent
  bridge new [--cwd DIR] [--agent ID] [--mode MODE]
                                         create a session and print its id
  bridge prompt SESSION TEXT... [--watch]
                                         send a message to the agent; with --watch, print what
                                         happens until the turn ends or the agent asks something
  bridge watch SESSION [--until-idle]    print the session as it happens; reconnects by itself
  bridge show SESSION                    print the session as it stands and exit
  bridge answer SESSION OPTION           answer the oldest open approval with one of its options
  bridge answer SESSION --json JSON      answer the oldest open request with a raw result
  bridge cancel SESSION                  interrupt the running turn
  bridge set SESSION OPTION VALUE        change a setting, e.g. `set SESSION mode plan`

SESSION may be the first characters of a session id.

Where the daemon is:
  --home DIR       the daemon's directory (default: $BRIDGE_HOME or ~/.harness-bridge)
  --socket PATH    its local socket (default: HOME/daemon.sock)
  --ws URL         a WebSocket address instead, with --token TOKEN or --token-file PATH
";

type Failure = Box<dyn std::error::Error>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bridge: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Removes `--name VALUE` from the arguments and returns the value.
fn option(args: &mut Vec<String>, name: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == name)?;
    args.remove(at);
    (at < args.len()).then(|| args.remove(at))
}

/// Removes `--name` from the arguments and says whether it was there.
fn switch(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != name);
    args.len() != before
}

fn endpoint(args: &mut Vec<String>) -> Result<Endpoint, Failure> {
    let home = option(args, "--home")
        .or_else(|| std::env::var("BRIDGE_HOME").ok())
        .map_or_else(
            || PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".harness-bridge"),
            PathBuf::from,
        );
    let socket = option(args, "--socket");
    let token = option(args, "--token");
    let token_file = option(args, "--token-file");
    Ok(match option(args, "--ws") {
        Some(url) => {
            let token = match (token, token_file) {
                (Some(token), _) => token,
                (None, file) => {
                    let path = file.map_or_else(|| home.join("ws-token"), PathBuf::from);
                    std::fs::read_to_string(&path)
                        .map_err(|error| format!("{}: {error}", path.display()))?
                        .trim()
                        .to_owned()
                }
            };
            Endpoint::Ws { url, token }
        }
        None => Endpoint::Local(socket.map_or_else(|| home.join("daemon.sock"), PathBuf::from)),
    })
}

async fn run(mut args: Vec<String>) -> Result<(), Failure> {
    if args.is_empty() || switch(&mut args, "--help") || switch(&mut args, "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let endpoint = endpoint(&mut args)?;
    let command = args.remove(0);
    if command == "watch" {
        return watch(&endpoint, args).await;
    }
    let (mut daemon, _) = Connection::open(&endpoint, "bridge-cli").await?;
    match command.as_str() {
        "agents" => {
            let listed = daemon.call(ext::METHOD_AGENTS_LIST, json!({})).await?;
            for agent in listed["agents"].as_array().into_iter().flatten() {
                println!(
                    "{:<10} {:<14} {:<8} {} {}",
                    text(agent, "agentId"),
                    text(agent, "name"),
                    text(agent, "readiness"),
                    text(agent, "version"),
                    text(agent, "detail"),
                );
            }
        }
        "sessions" => {
            let params = json!({"loggedOnly": switch(&mut args, "--logged")});
            let listed = daemon.call(ext::METHOD_SESSIONS_LIST, params).await?;
            for session in listed["sessions"].as_array().into_iter().flatten() {
                println!(
                    "{}  {:<8} {:<5} {:>5}  {}  {}",
                    text(session, "sessionId"),
                    text(session, "agentId"),
                    if session["live"] == json!(true) {
                        "live"
                    } else if session["logged"] == json!(false) {
                        "+"
                    } else {
                        "-"
                    },
                    session["headSeq"],
                    text(session, "state"),
                    text(session, "title"),
                );
            }
        }
        "new" => {
            let cwd = match option(&mut args, "--cwd") {
                Some(cwd) => std::fs::canonicalize(cwd)?,
                None => std::env::current_dir()?,
            };
            let mut params = json!({"cwd": cwd, "mcpServers": []});
            if let Some(agent) = option(&mut args, "--agent") {
                params["_meta"] = json!({ext::META_KEY: {"agentId": agent}});
            }
            let created = daemon.call("session/new", params).await?;
            let session = text(&created, "sessionId").to_owned();
            if let Some(mode) = option(&mut args, "--mode") {
                set(&mut daemon, &session, "mode", &mode).await?;
            }
            println!("{session}");
        }
        "prompt" => {
            let follow = switch(&mut args, "--watch");
            let session = session(&mut daemon, &mut args).await?;
            let mut mirror = Mirror::default();
            if follow {
                attach(&mut daemon, &session, &mut mirror, |_, _| {}).await?;
            }
            let prompt = json!([{"type": "text", "text": args.join(" ")}]);
            let params = json!({"sessionId": session, "prompt": prompt});
            let sent = daemon.call("session/prompt", params).await?;
            if !follow {
                println!("sent {}", text(&sent, "messageId"));
                return Ok(());
            }
            // Print what happens until the turn ends or the agent waits for an answer.
            let rules = RuleSet::pinned();
            let mut printer = Printer::default();
            // A session that had to be woken first reports `idle` before the turn starts, so
            // only an `idle` that follows another state ends the watch.
            let mut started = false;
            while let Some(message) = daemon.next().await {
                mirror.received(&rules, &message)?;
                printer.print(&message);
                let update = &message["params"]["update"];
                if message.get("id").is_some() && message.get("method").is_some() {
                    printer.note("waiting for an answer");
                    break;
                }
                if update["sessionUpdate"] == json!("state_update") {
                    if update["state"] != json!("idle") {
                        started = true;
                    } else if started {
                        printer.note("idle");
                        break;
                    }
                }
            }
        }
        "show" => {
            let session = session(&mut daemon, &mut args).await?;
            let mut mirror = Mirror::default();
            attach(&mut daemon, &session, &mut mirror, |_, _| {}).await?;
            print!("{}", render::conversation(&mirror.conversation));
        }
        "answer" => {
            let session = session(&mut daemon, &mut args).await?;
            let mut mirror = Mirror::default();
            attach(&mut daemon, &session, &mut mirror, |_, _| {}).await?;
            let open = mirror
                .conversation
                .sessions
                .values()
                .flat_map(|session| session.pending_requests())
                .next()
                .map(|(id, _)| id.clone())
                .ok_or("nothing is waiting for an answer")?;
            let result = match option(&mut args, "--json") {
                Some(raw) => serde_json::from_str(&raw)?,
                None => {
                    let choice = args.first().ok_or("which option?")?;
                    json!({"outcome": {"outcome": "selected", "optionId": choice}})
                }
            };
            daemon.answer(&json!(open), result).await?;
            // The daemon confirms once the answer is recorded.
            while let Some(message) = daemon.next().await {
                if message["method"] == ext::NOTIFICATION_REQUEST_RESOLVED
                    && message["params"]["requestId"] == json!(open)
                {
                    println!("answered {}", message["params"]["outcome"]);
                    return Ok(());
                }
            }
            return Err(Error::Closed.into());
        }
        "cancel" => {
            let session = session(&mut daemon, &mut args).await?;
            daemon
                .notify("session/cancel", json!({"sessionId": session}))
                .await?;
            // Make sure the notification was read before the connection goes away.
            daemon.call(ext::METHOD_SESSIONS_LIST, json!({})).await?;
        }
        "set" => {
            let session = session(&mut daemon, &mut args).await?;
            let [option, value] = args.as_slice() else {
                return Err("usage: bridge set SESSION OPTION VALUE".into());
            };
            set(&mut daemon, &session, option, value).await?;
        }
        other => return Err(format!("unknown command `{other}`; try --help").into()),
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

async fn set(
    daemon: &mut Connection,
    session: &str,
    option: &str,
    value: &str,
) -> Result<(), Error> {
    let params = json!({"sessionId": session, "configId": option, "type": "id", "value": value});
    daemon
        .call("session/set_config_option", params)
        .await
        .map(drop)
}

/// Takes the session argument and completes it to a full id.
async fn session(daemon: &mut Connection, args: &mut Vec<String>) -> Result<String, Failure> {
    if args.is_empty() {
        return Err("which session? see `bridge sessions`".into());
    }
    let prefix = args.remove(0);
    let listed = daemon.call(ext::METHOD_SESSIONS_LIST, json!({})).await?;
    let matches: Vec<&str> = listed["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|session| text(session, "sessionId"))
        .filter(|id| id.starts_with(&prefix))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).to_owned()),
        [] => Err(format!("no session starts with `{prefix}`").into()),
        _ => Err(format!("several sessions start with `{prefix}`").into()),
    }
}

/// Attaches to a session from the mirror's position and reads until the mirror is up to date.
/// `seen` gets every message that was applied, with the mirror after applying it.
async fn attach(
    daemon: &mut Connection,
    session: &str,
    mirror: &mut Mirror,
    mut seen: impl FnMut(&Value, &Mirror),
) -> Result<Value, Failure> {
    let rules = RuleSet::pinned();
    let after = (mirror.seq > 0).then_some(mirror.seq);
    let params = json!({"sessionId": session, "afterSeq": after});
    let attached = daemon.call(ext::METHOD_SESSION_ATTACH, params).await?;
    let head = attached["headSeq"].as_u64().unwrap_or_default();
    let replay = attached["catchUp"] == json!("replay");
    if replay {
        mirror.reset();
    }
    while replay || mirror.seq < head {
        let message = daemon.next().await.ok_or(Error::Closed)?;
        mirror.received(&rules, &message)?;
        if message["params"]["update"]["sessionUpdate"] == json!("replay_complete") {
            break;
        }
        seen(&message, mirror);
    }
    Ok(attached)
}

/// Prints a session as it happens. When the connection drops, connects again and continues
/// from the last sequence number seen.
async fn watch(endpoint: &Endpoint, mut args: Vec<String>) -> Result<(), Failure> {
    let until_idle = switch(&mut args, "--until-idle");
    let rules = RuleSet::pinned();
    let mut mirror = Mirror::default();
    let mut printer = Printer::default();
    let mut session_id: Option<String> = None;
    let mut first = true;
    loop {
        let Ok((mut daemon, _)) = Connection::open(endpoint, "bridge-cli").await else {
            if first {
                Connection::open(endpoint, "bridge-cli").await?;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        let session_id = match &session_id {
            Some(known) => known.clone(),
            None => session_id
                .insert(session(&mut daemon, &mut args).await?)
                .clone(),
        };
        let before = mirror.seq;
        let attached = match attach(&mut daemon, &session_id, &mut mirror, |message, _| {
            printer.print(message);
        })
        .await
        {
            Ok(attached) => attached,
            Err(error) if first => return Err(error),
            Err(_) => continue,
        };
        if !first {
            printer.note(&format!(
                "reconnected: {} from {before} to {}",
                text(&attached, "catchUp"),
                mirror.seq
            ));
        }
        first = false;
        loop {
            if until_idle && is_idle(&mirror) {
                printer.note("idle");
                return Ok(());
            }
            let Some(message) = daemon.next().await else {
                printer.note(&format!("connection lost at {}; retrying", mirror.seq));
                break;
            };
            mirror.received(&rules, &message)?;
            printer.print(&message);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The user's session has finished its turn and nothing waits for an answer.
fn is_idle(mirror: &Mirror) -> bool {
    mirror.conversation.root().is_some_and(|(_, root)| {
        root.values.get("state").map(|state| text(state, "state")) == Some("idle")
            && root.pending_requests().next().is_none()
    })
}

/// Prints messages as lines. Streamed text is printed as it arrives, under one heading per item.
#[derive(Default)]
struct Printer {
    /// The item whose text is being streamed: session, kind and id.
    streaming: Option<(String, String, String)>,
}

impl Printer {
    fn line(&mut self, line: &str) {
        if self.streaming.take().is_some() {
            println!();
        }
        println!("{line}");
    }

    fn note(&mut self, note: &str) {
        self.line(&format!("-- {note}"));
    }

    fn print(&mut self, message: &Value) {
        let params = &message["params"];
        let seq = seq_of(message).unwrap_or_default();
        let child = if params["_meta"][ext::META_KEY]["rootSessionId"].is_string() {
            "  (subagent) "
        } else {
            " "
        };
        match message["method"].as_str() {
            Some("session/update") => {
                let update = &params["update"];
                let kind = text(update, "sessionUpdate");
                let streamed = update["content"]["text"]
                    .as_str()
                    .or_else(|| update["data"].as_str().filter(|_| kind.ends_with("_chunk")));
                if let (true, Some(piece)) = (kind.ends_with("_chunk"), streamed) {
                    let id = ["messageId", "terminalId", "compactionId"]
                        .iter()
                        .find_map(|key| update[key].as_str())
                        .unwrap_or_default();
                    let item = (
                        text(params, "sessionId").to_owned(),
                        kind.to_owned(),
                        id.to_owned(),
                    );
                    if self.streaming.as_ref() != Some(&item) {
                        if self.streaming.is_some() {
                            println!();
                        }
                        print!("{seq:>5}{child}[{}] ", kind.trim_end_matches("_chunk"));
                        self.streaming = Some(item);
                    }
                    print!("{piece}");
                    let _ = std::io::stdout().flush();
                    return;
                }
                let details: Vec<&str> = ["title", "status", "state", "stopReason"]
                    .iter()
                    .filter_map(|key| update[key].as_str())
                    .collect();
                self.line(&format!("{seq:>5}{child}[{kind}] {}", details.join(" · ")));
            }
            Some(ext::NOTIFICATION_REQUEST_RESOLVED) => {
                let by = text(params, "resolvedBy");
                self.line(&format!(
                    "{seq:>5}{child}[answered] {}{}{}",
                    params["outcome"],
                    if by.is_empty() { "" } else { " by " },
                    by
                ));
            }
            Some(method) if message.get("id").is_some() => {
                let title = ["title", "message"]
                    .iter()
                    .find_map(|key| params[key].as_str())
                    .unwrap_or_default();
                self.line(&format!("{seq:>5}{child}[{method}] {title}"));
                for choice in params["options"].as_array().into_iter().flatten() {
                    self.line(&format!(
                        "        option `{}`: {}",
                        text(choice, "optionId"),
                        text(choice, "name")
                    ));
                }
                self.line("        answer with: bridge answer SESSION OPTION");
            }
            _ => {}
        }
    }
}
