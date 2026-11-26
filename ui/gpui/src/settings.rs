//! The agent's settings: model, effort, permission mode, switches. Drawn from what the agent
//! reports; nothing here knows a model or a mode by name.

use crate::items::text;
use crate::memory;
use crate::parts::{MenuChoice, MenuGroup, checkbox, menu, problem, tip};
use crate::status::Outcome;
use crate::theme::theme;
use gpui::{App, Div, Window, div, prelude::*, px};
use serde_json::Value;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
    Choice(String),
    On(bool),
}

/// A setting of the agent as the kit draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// What the setting is about, such as `model`, `thought_level` or `mode`, when the agent says.
    pub category: Option<String>,
    pub current: SettingValue,
    /// For a choice: its values, under headings when the agent groups them.
    pub groups: Vec<MenuGroup>,
}

impl Setting {
    /// Reads the agent's `configOptions`: a session's (`state.values["configOptions"]`) or those
    /// the daemon gives before a session exists. A setting of a kind this kit does not know is
    /// left out.
    #[must_use]
    pub fn all(config_options: Option<&Value>) -> Vec<Self> {
        let optional = |value: &Value, field: &str| {
            Some(text(value, field))
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let choice = |option: &Value| {
            let value = optional(option, "value")?;
            Some(MenuChoice {
                name: optional(option, "name").unwrap_or_else(|| value.clone()),
                value,
                description: optional(option, "description"),
            })
        };
        let options = config_options
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        options
            .iter()
            .filter_map(|option| {
                let id = optional(option, "configId")?;
                let (current, groups) = match (text(option, "type"), option.get("currentValue")) {
                    ("boolean", Some(Value::Bool(on))) => (SettingValue::On(*on), Vec::new()),
                    ("select", Some(Value::String(value))) => {
                        let (mut groups, mut flat) = (Vec::new(), Vec::new());
                        for entry in option
                            .get("options")
                            .and_then(Value::as_array)
                            .map_or(&[][..], Vec::as_slice)
                        {
                            match entry.get("options").and_then(Value::as_array) {
                                Some(inner) => groups.push(MenuGroup {
                                    name: optional(entry, "name"),
                                    choices: inner.iter().filter_map(choice).collect(),
                                }),
                                None => flat.extend(choice(entry)),
                            }
                        }
                        if !flat.is_empty() {
                            groups.insert(
                                0,
                                MenuGroup {
                                    name: None,
                                    choices: flat,
                                },
                            );
                        }
                        (SettingValue::Choice(value.clone()), groups)
                    }
                    _ => return None,
                };
                Some(Self {
                    name: optional(option, "name").unwrap_or_else(|| id.clone()),
                    id,
                    description: optional(option, "description"),
                    category: optional(option, "category"),
                    current,
                    groups,
                })
            })
            .collect()
    }
}

/// Called when the user changes a setting: its id and the new value.
pub type Change = Rc<dyn Fn(&str, SettingValue, &mut Window, &mut App) -> Outcome>;

fn encode(id: &str, value: &SettingValue) -> String {
    match value {
        SettingValue::Choice(choice) => format!("{id}\u{1}c{choice}"),
        SettingValue::On(on) => format!("{id}\u{1}{}", if *on { "1" } else { "0" }),
    }
}

fn decode(asked: &str) -> Option<(&str, SettingValue)> {
    let (id, value) = asked.split_once('\u{1}')?;
    Some((
        id,
        match value.strip_prefix('c') {
            Some(choice) => SettingValue::Choice(choice.to_owned()),
            None => SettingValue::On(value == "1"),
        },
    ))
}

/// One control per setting: a menu for a choice, a box to tick for a switch, the description
/// on hover.
///
/// `change` is called when the user changes one. Until its outcome arrives the control shows
/// the new value and all are locked; if it fails, the bar says why and shows the old value
/// again. Without `change` the settings are shown and cannot be changed. `None` without settings.
pub fn settings_bar(
    key: &str,
    settings: &[Setting],
    change: Option<Change>,
    cx: &App,
) -> Option<Div> {
    if settings.is_empty() {
        return None;
    }
    let theme = theme(cx);
    let (asked_key, problem_key) = (format!("{key}/asked"), format!("{key}/problem"));
    let asked_text = memory::text(cx, &asked_key);
    let asked = asked_text.as_deref().and_then(decode);
    let failed = memory::text(cx, &problem_key);
    // What the user chose and the agent has not confirmed yet locks every control.
    let change = change.filter(|_| asked.is_none());
    let set = |setting: &Setting| {
        let change = change.clone()?;
        let (id, name, current) = (
            setting.id.clone(),
            setting.name.clone(),
            setting.current.clone(),
        );
        let (asked_key, problem_key) = (asked_key.clone(), problem_key.clone());
        Some(Rc::new(
            move |value: SettingValue, window: &mut Window, cx: &mut App| {
                if value == current {
                    return;
                }
                memory::set_text(cx, &asked_key, Some(encode(&id, &value)));
                memory::set_text(cx, &problem_key, None);
                let work = change(&id, value, window, cx);
                let (asked_key, problem_key, name) =
                    (asked_key.clone(), problem_key.clone(), name.clone());
                cx.spawn(async move |cx| {
                    let result = work.await;
                    cx.update(|cx| {
                        memory::set_text(cx, &asked_key, None);
                        memory::set_text(
                            cx,
                            &problem_key,
                            result.err().map(|why| format!("{name}: {why}")),
                        );
                    })
                    .ok();
                })
                .detach();
            },
        ))
    };
    // Wraps onto more lines when the controls do not fit beside each other.
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(14.0))
        .gap_y(px(8.0))
        .w_full();
    for setting in settings {
        let shown = match &asked {
            Some((id, value)) if *id == setting.id => value,
            _ => &setting.current,
        };
        let set = set(setting);
        let control_key = format!("{key}/{}", setting.id);
        let control = match shown {
            SettingValue::On(on) => {
                let on = *on;
                let mut control = checkbox(control_key.clone(), &setting.name, on, cx);
                match set {
                    Some(set) => {
                        control = control
                            .on_click(move |_, window, cx| set(SettingValue::On(!on), window, cx))
                    }
                    None => control = control.opacity(0.6).cursor_default(),
                }
                div().child(control)
            }
            SettingValue::Choice(value) => menu(
                &control_key,
                Some(&setting.name),
                value,
                &setting.groups,
                set.map(|set| {
                    Rc::new(move |value: &str, window: &mut Window, cx: &mut App| {
                        set(SettingValue::Choice(value.to_owned()), window, cx)
                    }) as Rc<dyn Fn(&str, &mut Window, &mut App)>
                }),
                cx,
            ),
        };
        row = row.child(match &setting.description {
            Some(description) => div()
                .id(gpui::SharedString::from(format!("{control_key}/about")))
                .child(control)
                .tooltip(tip(description))
                .into_any_element(),
            None => control.into_any_element(),
        });
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .text_color(theme.colors.text)
            .child(row)
            .when_some(failed, |column, why| {
                column.child(problem(&format!("Not changed: {why}"), cx))
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_are_read_from_what_the_agent_reports() {
        let options = json!([
            { "configId": "model", "name": "Model", "type": "select", "currentValue": "a", "category": "model",
              "options": [{ "value": "a", "name": "A" }, { "name": "Others", "options": [{ "value": "b" }] }] },
            { "configId": "fast", "name": "Fast", "type": "boolean", "currentValue": true },
            { "configId": "odd", "type": "slider", "currentValue": 3 },
        ]);
        let settings = Setting::all(Some(&options));
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].current, SettingValue::Choice("a".to_owned()));
        assert_eq!(settings[0].groups.len(), 2);
        assert_eq!(settings[0].groups[1].name.as_deref(), Some("Others"));
        assert_eq!(settings[0].groups[1].choices[0].name, "b");
        assert_eq!(settings[1].current, SettingValue::On(true));
    }

    #[test]
    fn what_was_asked_is_kept_as_text() {
        for value in [
            SettingValue::Choice("opus".to_owned()),
            SettingValue::On(true),
            SettingValue::On(false),
        ] {
            assert_eq!(decode(&encode("model", &value)), Some(("model", value)));
        }
    }
}
