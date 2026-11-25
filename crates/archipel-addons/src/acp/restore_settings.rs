//! `acp.restore-settings`: a session that is woken keeps the model, effort and mode it had.
//!
//! A harness does not reliably remember what was chosen in a session. Claude's adapter starts
//! a resumed session on the configured default model and effort
//! (`docs/research/23-conformance-first-run.md`), so the user would see one model and get
//! another. ACP v2 leaves this to the client: a `session/resume` may carry `configOptions`,
//! which the adapter sets before anything else. That part of ACP is marked unstable, which is
//! why this is an addon and not the daemon's own behaviour.
//!
//! The daemon's log has every settings update the session ever sent, so the last one is what
//! the user last saw. This addon adds it to the `session/resume` that wakes the session.

use archipel_addon_api::{Addon, Agent, Cx, Info, Kind, Request, Verdict};
use serde_json::{Value, json};

pub struct RestoreSettings;

impl Addon for RestoreSettings {
    fn info(&self) -> Info {
        Info {
            id: "acp.restore-settings",
            kind: Kind::Fix,
            summary: "A session that is woken keeps the model, effort and mode it had.",
            because: "A resumed session starts on the harness's defaults unless the client sends its \
                      settings again (claude-agent-acp 0.109.1). ACP v2 has `configOptions` on \
                      `session/resume` for this, marked unstable.",
            until: "`configOptions` on `session/resume` is stable in ACP (then this moves into the \
                    daemon), or adapters restore a session's settings themselves.",
            scenario: Some("reopen.keeps_settings"),
        }
    }

    /// ACP: a client must only send `configOptions` to an agent that advertises this.
    fn applies(&self, agent: &Agent<'_>) -> bool {
        agent
            .capabilities
            .is_some_and(|capabilities| capabilities.pointer("/session/options").is_some())
    }

    fn request(&mut self, cx: &mut Cx<'_>, request: &mut Request<'_>) -> Verdict {
        if request.method != "session/resume"
            // Whoever sent settings of their own knows better.
            || request.params.get("configOptions").is_some_and(|sent| !sent.is_null())
        {
            return Verdict::Pass;
        }
        let logged = request
            .params
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|session| cx.last_update(session, "config_option_update"));
        let entries = entries(
            logged
                .as_ref()
                .and_then(|update| update.get("configOptions")),
        );
        if !entries.is_empty() {
            request.params["configOptions"] = Value::Array(entries);
        }
        Verdict::Pass
    }
}

/// The current values of a list of config options, as entries for a setup request.
///
/// The adapter sets them in this order, and a model decides which effort levels and modes
/// exist, so the model goes first. An entry the adapter cannot apply any more is skipped by
/// the adapter, as ACP asks.
fn entries(options: Option<&Value>) -> Vec<Value> {
    let mut entries: Vec<(bool, Value)> = options
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = option.get("configId")?.as_str()?;
            let entry = match option.get("currentValue")? {
                Value::String(value) => json!({"configId": id, "type": "id", "value": value}),
                Value::Bool(value) => json!({"configId": id, "type": "boolean", "value": value}),
                _ => return None,
            };
            let is_model = option.get("category").and_then(Value::as_str) == Some("model");
            Some((!is_model, entry))
        })
        .collect();
    entries.sort_by_key(|(after_model, _)| *after_model); // stable: the rest keeps its order
    entries.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use archipel_addon_api::Sessions;

    struct Log(Option<Value>);

    impl Sessions for Log {
        fn last_update(&self, session_id: &str, kind: &str) -> Option<Value> {
            assert_eq!((session_id, kind), ("s1", "config_option_update"));
            self.0.clone()
        }
    }

    fn agent(capabilities: &Value) -> Agent<'_> {
        Agent {
            id: "claude",
            harness: Some("claude"),
            adapter: None,
            version: None,
            capabilities: Some(capabilities),
            os: "macos",
        }
    }

    /// What the addon makes of a request, given the session's last logged settings.
    fn after(method: &str, mut params: Value, logged: Option<Value>) -> Value {
        let capabilities = json!({"session": {"options": {}}});
        let log = Log(logged.map(
            |options| json!({"sessionUpdate": "config_option_update", "configOptions": options}),
        ));
        let mut request = Request {
            id: 1,
            agent: agent(&capabilities),
            from: archipel_addon_api::Origin::App { name: None },
            method,
            params: &mut params,
        };
        let mut cx = Cx::new(&log, 1);
        assert_eq!(
            RestoreSettings.request(&mut cx, &mut request),
            Verdict::Pass
        );
        assert_eq!(cx.effects(), [], "it only edits the request");
        params
    }

    fn logged() -> Value {
        json!([
            {"configId": "mode", "category": "mode", "type": "select", "currentValue": "acceptEdits", "options": []},
            {"configId": "effort", "category": "thought_level", "type": "select", "currentValue": "low", "options": []},
            {"configId": "model", "category": "model", "type": "select", "currentValue": "haiku", "options": []},
            {"configId": "fast", "type": "boolean", "currentValue": true},
            {"configId": "odd", "type": "number", "currentValue": 3},
        ])
    }

    #[test]
    fn a_resume_carries_the_logged_settings_with_the_model_first() {
        let params = after(
            "session/resume",
            json!({"sessionId": "s1", "cwd": "/w"}),
            Some(logged()),
        );
        assert_eq!(
            params["configOptions"],
            json!([
                {"configId": "model", "type": "id", "value": "haiku"},
                {"configId": "mode", "type": "id", "value": "acceptEdits"},
                {"configId": "effort", "type": "id", "value": "low"},
                {"configId": "fast", "type": "boolean", "value": true},
            ]),
            "a value of a type ACP cannot set is left out"
        );
    }

    #[test]
    fn nothing_is_added_without_logged_settings_or_to_another_request() {
        let resume = json!({"sessionId": "s1", "cwd": "/w"});
        assert_eq!(after("session/resume", resume.clone(), None), resume);
        assert_eq!(
            after("session/resume", resume.clone(), Some(json!([]))),
            resume
        );
        let prompt = json!({"sessionId": "s1", "prompt": []});
        assert_eq!(
            after("session/prompt", prompt.clone(), Some(logged())),
            prompt
        );
    }

    #[test]
    fn settings_the_request_already_carries_are_left_alone() {
        let own = json!({"sessionId": "s1", "configOptions": [{"configId": "model", "type": "id", "value": "other"}]});
        assert_eq!(after("session/resume", own.clone(), Some(logged())), own);
    }

    #[test]
    fn it_applies_only_to_adapters_that_take_settings_on_setup_requests() {
        assert!(RestoreSettings.applies(&agent(&json!({"session": {"options": {}}}))));
        assert!(!RestoreSettings.applies(&agent(&json!({"session": {"fork": {}}}))));
        let unknown = Agent {
            capabilities: None,
            ..agent(&Value::Null)
        };
        assert!(!RestoreSettings.applies(&unknown));
    }
}
