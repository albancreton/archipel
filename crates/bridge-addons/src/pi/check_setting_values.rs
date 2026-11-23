//! `pi.check-setting-values`: a setting can only be given a value it offers.
//!
//! `pi-acp` accepts any value for a setting and reports it back as current, so an app that
//! sends a wrong effort level is told it worked (`docs/research/23-conformance-first-run.md`).
//! ACP asks for an invalid-params error instead, which is what Claude's adapter gives. This
//! addon checks the value against the choices the session last reported and refuses the rest.

use bridge_addon_api::{Addon, Agent, Info, Kind, Request, Verdict};
use serde_json::Value;

pub struct CheckSettingValues;

impl Addon for CheckSettingValues {
    fn info(&self) -> Info {
        Info {
            id: "pi.check-setting-values",
            kind: Kind::Fix,
            summary: "Refuses a value that a setting does not offer.",
            because: "pi-acp accepts any value for a setting and reports it as current (seen with \
                      the effort level, November 2026). ACP asks for error -32602.",
            until: "pi-acp refuses a value it does not offer.",
            scenario: Some("settings.rejects_unknown"),
        }
    }

    fn applies(&self, agent: &Agent<'_>) -> bool {
        agent.harness == Some("pi")
    }

    fn request(&mut self, request: &mut Request<'_>) -> Verdict {
        if request.method != "session/set_config_option" {
            return Verdict::Pass;
        }
        let (Some(id), Some(value)) = (
            request.params.get("configId").and_then(Value::as_str),
            request.params.get("value").and_then(Value::as_str),
        ) else {
            return Verdict::Pass;
        };
        let reported = request
            .history
            .and_then(|history| history.last_update("config_option_update"));
        let offered = reported
            .as_ref()
            .and_then(|update| update.get("configOptions")?.as_array())
            .and_then(|options| options.iter().find(|option| option["configId"] == id))
            .and_then(|option| option.get("options")?.as_array())
            .map(|choices| values(choices));
        match offered {
            // Without a list to check against, the adapter decides.
            Some(offered) if !offered.contains(&value) => Verdict::Refuse {
                code: -32602,
                message: format!("`{value}` is not a value that `{id}` offers"),
            },
            _ => Verdict::Pass,
        }
    }
}

/// The values of a list of choices, which may be grouped.
fn values(choices: &[Value]) -> Vec<&str> {
    choices
        .iter()
        .flat_map(
            |choice| match choice.get("options").and_then(Value::as_array) {
                Some(group) => values(group),
                None => choice
                    .get("value")
                    .and_then(Value::as_str)
                    .into_iter()
                    .collect(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_addon_api::History;
    use serde_json::json;

    struct Log(Option<Value>);

    impl History for Log {
        fn last_update(&self, _kind: &str) -> Option<Value> {
            self.0.clone()
        }
    }

    fn verdict(log: &Log, method: &str, id: &str, value: Value) -> Verdict {
        let mut params = json!({"sessionId": "s", "configId": id, "value": value});
        let agent = Agent {
            id: "pi",
            harness: Some("pi"),
            adapter: None,
            version: None,
            capabilities: None,
            os: "macos",
        };
        CheckSettingValues.request(&mut Request {
            agent,
            method,
            params: &mut params,
            history: Some(log),
        })
    }

    #[test]
    fn only_a_value_the_setting_does_not_offer_is_refused() {
        let log = Log(Some(
            json!({"sessionUpdate": "config_option_update", "configOptions": [
                {"configId": "thinking", "type": "select", "currentValue": "low",
                 "options": [{"value": "low"}, {"value": "high"}]},
                {"configId": "model", "type": "select", "currentValue": "a/one",
                 "options": [{"groupId": "a", "options": [{"value": "a/one"}, {"value": "a/two"}]}]},
            ]}),
        ));
        let set = "session/set_config_option";
        assert_eq!(verdict(&log, set, "thinking", json!("high")), Verdict::Pass);
        assert_eq!(
            verdict(&log, set, "model", json!("a/two")),
            Verdict::Pass,
            "in a group"
        );
        assert_eq!(
            verdict(&log, set, "thinking", json!("enormous")),
            Verdict::Refuse {
                code: -32602,
                message: "`enormous` is not a value that `thinking` offers".to_owned()
            }
        );
        // What it cannot judge is left to the adapter.
        assert_eq!(verdict(&log, set, "unheard-of", json!("x")), Verdict::Pass);
        assert_eq!(verdict(&log, set, "thinking", json!(true)), Verdict::Pass);
        assert_eq!(
            verdict(&log, "session/other", "thinking", json!("enormous")),
            Verdict::Pass
        );
        assert_eq!(
            verdict(&Log(None), set, "thinking", json!("enormous")),
            Verdict::Pass
        );
    }

    #[test]
    fn it_is_for_pi_only() {
        let agent = |harness| Agent {
            id: "x",
            harness,
            adapter: None,
            version: None,
            capabilities: None,
            os: "linux",
        };
        assert!(CheckSettingValues.applies(&agent(Some("pi"))));
        assert!(!CheckSettingValues.applies(&agent(Some("claude"))));
        assert!(!CheckSettingValues.applies(&agent(None)));
    }
}
