// The agent's settings (model, effort, permission mode, switches), drawn from what the agent
// reports. Nothing here knows a model's or a mode's name.

import type { Json, JsonObject } from "@harness-bridge/client";
import { useState } from "react";

/** One value a setting can take. */
export interface Choice {
  value: string;
  name: string;
  description?: string;
}

/** A setting as the kit draws it. `settingsOf` makes these from the agent's `configOptions`. */
export interface Setting {
  id: string;
  name: string;
  description?: string;
  /** What the setting is about, such as `model`, `thought_level` or `mode`, when the agent says. */
  category?: string;
  kind: "choice" | "switch";
  /** The value now: a choice's `value`, or on/off. */
  current: string | boolean;
  /** For a choice: its values, under a heading when the agent groups them. */
  groups: { name?: string; choices: Choice[] }[];
}

const text = (value: Json | undefined) => (typeof value === "string" ? value : undefined);

function choice(option: Json): Choice | null {
  const from = option as JsonObject | null;
  const value = text(from?.["value"]);
  if (value === undefined) return null;
  return { value, name: text(from?.["name"]) ?? value, description: text(from?.["description"]) };
}

/**
 * Reads the agent's `configOptions`: a session's (`state.values.get("configOptions")`) or
 * those of `client.sessionOptions()` before a session exists. A setting of a kind this kit
 * does not know is left out.
 */
export function settingsOf(configOptions: Json | undefined): Setting[] {
  const settings: Setting[] = [];
  for (const option of Array.isArray(configOptions) ? configOptions : []) {
    const from = option as JsonObject | null;
    const id = text(from?.["configId"]);
    const current = from?.["currentValue"];
    if (id === undefined) continue;
    const common = { id, name: text(from?.["name"]) ?? id, description: text(from?.["description"]), category: text(from?.["category"]) };
    if (from?.["type"] === "boolean" && typeof current === "boolean") {
      settings.push({ ...common, kind: "switch", current, groups: [] });
    } else if (from?.["type"] === "select" && typeof current === "string" && Array.isArray(from["options"])) {
      const flat: Choice[] = [];
      const groups: Setting["groups"] = [];
      for (const entry of from["options"]) {
        const inner = (entry as JsonObject | null)?.["options"];
        if (Array.isArray(inner)) {
          groups.push({ name: text((entry as JsonObject)["name"]), choices: inner.map(choice).filter((found) => found !== null) });
        } else {
          const found = choice(entry);
          if (found) flat.push(found);
        }
      }
      if (flat.length > 0) groups.unshift({ choices: flat });
      settings.push({ ...common, kind: "choice", current, groups });
    }
  }
  return settings;
}

export interface SettingsBarProps {
  settings: Setting[];
  /**
   * The user changed a setting. If it returns a promise, the control shows the new value while
   * it runs, and the bar says why if it fails. Without it the settings are shown and cannot be changed.
   */
  onChange?: (id: string, value: string | boolean) => void | Promise<unknown>;
  disabled?: boolean;
}

/** One control per setting: a menu for a choice, a checkbox for a switch. */
export function SettingsBar({ settings, onChange, disabled }: SettingsBarProps) {
  // What the user chose and the agent has not confirmed yet.
  const [asked, setAsked] = useState<{ id: string; value: string | boolean } | null>(null);
  const [problem, setProblem] = useState("");
  if (settings.length === 0) return null;
  const change = (setting: Setting, value: string | boolean) => {
    setProblem("");
    const pending = { id: setting.id, value };
    setAsked(pending);
    Promise.resolve(onChange?.(setting.id, value))
      .catch((error: unknown) => setProblem(`${setting.name}: ${error instanceof Error ? error.message : String(error)}`))
      .finally(() => setAsked((now) => (now === pending ? null : now)));
  };
  const locked = disabled || !onChange || asked !== null;
  return (
    <div className="hb-settings">
      {settings.map((setting) => {
        const shown = asked?.id === setting.id ? asked.value : setting.current;
        const category = setting.category ? ` hb-setting-${setting.category}` : "";
        return setting.kind === "switch" ? (
          <label key={setting.id} className={`hb-setting hb-setting-switch${category}`} title={setting.description}>
            <input type="checkbox" checked={shown === true} disabled={locked} onChange={(event) => change(setting, event.target.checked)} />
            {setting.name}
          </label>
        ) : (
          <label key={setting.id} className={`hb-setting${category}`} title={setting.description}>
            <span className="hb-label">{setting.name}</span>
            <select value={String(shown)} disabled={locked} onChange={(event) => change(setting, event.target.value)}>
              {setting.groups.map((group, index) => {
                const options = group.choices.map((option) => (
                  <option key={option.value} value={option.value} title={option.description}>{option.name}</option>
                ));
                return group.name === undefined ? options : <optgroup key={index} label={group.name}>{options}</optgroup>;
              })}
            </select>
          </label>
        );
      })}
      {problem && <div className="hb-problem" role="alert">Not changed: {problem}</div>}
    </div>
  );
}

/**
 * Settings chosen before a session exists. Give it the `configOptions` of
 * `client.sessionOptions()`: `settings` and `onChange` go to `SettingsBar`, and `initial` goes
 * to `client.newSession({ configOptions: initial })`.
 */
export function useDraftSettings(configOptions: Json | undefined): {
  settings: Setting[];
  onChange: (id: string, value: string | boolean) => void;
  initial: { configId: string; value: string | boolean }[];
} {
  const [chosen, setChosen] = useState<Record<string, string | boolean>>({});
  const settings = settingsOf(configOptions).map((setting) => ({ ...setting, current: chosen[setting.id] ?? setting.current }));
  return {
    settings,
    onChange: (id, value) => setChosen((before) => ({ ...before, [id]: value })),
    initial: Object.entries(chosen).map(([configId, value]) => ({ configId, value })),
  };
}
