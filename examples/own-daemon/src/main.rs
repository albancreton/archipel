//! A daemon with an addon of one's own.
//!
//! It is `archipeld` in every way (same command line, same files) plus one addon written here.
//! This is how to add a feature or a quick fix without forking the library and without
//! waiting for a release: write the addon in your own crate and build your own daemon.
//!
//! ```sh
//! cargo run -p own-daemon -- addons      # lists the library's addons and this one
//! cargo run -p own-daemon -- --home DIR  # runs the daemon
//! ```
//!
//! The addon below uses one hook. There are more: an addon can also change answers, answer
//! approvals in the user's place, send requests of its own, add updates to a session and
//! serve methods. `docs/design/addons.md` lists them, and
//! `crates/archipel-server/tests/addons.rs` has a short test for each.
//!
//! The addon below is a house rule, not a fix: sessions may not be switched to the mode that
//! skips every approval. It is a `Feature`, so it is off until `config.json` says
//! `{"addons": {"example.no-bypass": true}}`.

use archipel_addon_api::{Addon, Agent, Cx, Info, Kind, Request, Verdict};
use serde_json::Value;

struct NoBypass;

impl Addon for NoBypass {
    fn info(&self) -> Info {
        Info {
            id: "example.no-bypass",
            kind: Kind::Feature,
            summary: "Refuses to switch a session to the mode that skips every approval.",
            because: "A house rule: no harness offers a way to forbid this mode.",
            until: "",
            scenario: None,
        }
    }

    /// An addon picks its agents here: by harness, by adapter version, by operating system.
    fn applies(&self, agent: &Agent<'_>) -> bool {
        agent.harness == Some("claude")
    }

    fn request(&mut self, _cx: &mut Cx<'_>, request: &mut Request<'_>) -> Verdict {
        let sets_mode =
            |entry: &Value| entry["configId"] == "mode" && entry["value"] == "bypassPermissions";
        let asked = match request.method {
            "session/set_config_option" => sets_mode(request.params),
            // Settings may also come with the request that creates or opens a session.
            "session/new" | "session/resume" | "session/fork" => request.params["configOptions"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(sets_mode)),
            _ => false,
        };
        if asked {
            return Verdict::Refuse {
                code: -32602,
                message: "this daemon does not allow the mode that skips approvals".to_owned(),
            };
        }
        Verdict::Pass
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut addons = archipel_addons::all();
    addons.push(Box::new(NoBypass));
    archipeld::main(addons).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use archipel_addon_api::Sessions;
    use serde_json::json;

    struct NoLog;

    impl Sessions for NoLog {
        fn last_update(&self, _session_id: &str, _kind: &str) -> Option<Value> {
            None
        }
    }

    fn verdict(method: &str, mut params: Value) -> Verdict {
        let agent = Agent {
            id: "claude",
            harness: Some("claude"),
            adapter: None,
            version: None,
            capabilities: None,
            os: "macos",
        };
        let mut request = Request {
            id: 1,
            agent,
            from: archipel_addon_api::Origin::App { name: None },
            method,
            params: &mut params,
        };
        NoBypass.request(&mut Cx::new(&NoLog, 1), &mut request)
    }

    #[test]
    fn the_mode_is_refused_however_it_is_asked_for_and_other_modes_pass() {
        let set =
            |mode: &str| json!({"sessionId": "s", "configId": "mode", "type": "id", "value": mode});
        assert_eq!(
            verdict("session/set_config_option", set("plan")),
            Verdict::Pass
        );
        assert!(matches!(
            verdict("session/set_config_option", set("bypassPermissions")),
            Verdict::Refuse { .. }
        ));
        let with = json!({"cwd": "/w", "configOptions": [{"configId": "mode", "type": "id", "value": "bypassPermissions"}]});
        assert!(matches!(
            verdict("session/new", with),
            Verdict::Refuse { .. }
        ));
        assert_eq!(verdict("session/new", json!({"cwd": "/w"})), Verdict::Pass);
    }
}
