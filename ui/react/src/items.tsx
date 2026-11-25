// One component per kind of item. Each takes the item and, where the user can act, a callback.
// Nothing here knows about connections; give the components state and they draw it.

import { base64Text, type Json, type JsonObject, type Request } from "@harness-bridge/client";
import { useContext, useState, type ReactNode } from "react";
import { RenderText } from "./markdown.tsx";

const isObject = (value: Json | undefined): value is JsonObject =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: Json | undefined, key: string): string => {
  const found = isObject(value) ? value[key] : undefined;
  return typeof found === "string" ? found : "";
};
const list = (value: Json | undefined, key: string): Json[] => {
  const found = isObject(value) ? value[key] : undefined;
  return Array.isArray(found) ? found : [];
};

/** The text of a content field: a list of blocks, one block, or a string. */
export function contentText(content: Json | undefined): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(contentText).join("");
  if (isObject(content)) {
    if (content["type"] === "text") return text(content, "text");
    if (content["type"] === "content") return contentText(content["content"]);
    return `[${text(content, "type") || "content"}]`;
  }
  return "";
}

export function UserMessage({ item }: { item: Json }) {
  return <div className="hb-item hb-user">{contentText(isObject(item) ? item["content"] : undefined)}</div>;
}

/** What the agent said, drawn as Markdown unless `RenderText` says otherwise. */
export function AssistantMessage({ item }: { item: Json }) {
  const render = useContext(RenderText);
  return <div className="hb-item hb-assistant">{render(contentText(isObject(item) ? item["content"] : undefined))}</div>;
}

/** Reasoning, folded by default. */
export function Thought({ item }: { item: Json }) {
  const render = useContext(RenderText);
  const body = contentText(isObject(item) ? item["content"] : undefined).trim();
  return (
    <details className="hb-item hb-thought">
      <summary>Thinking</summary>
      <div>{render(body)}</div>
    </details>
  );
}

/** The output of a command, with the command line and how it ended. */
export function Terminal({ item }: { item: Json }) {
  const output = isObject(item) && isObject(item["output"]) ? base64Text(text(item["output"], "data")) : "";
  const exit = isObject(item) ? item["exitStatus"] : undefined;
  const ended = isObject(exit)
    ? typeof exit["exitCode"] === "number" ? `exit ${exit["exitCode"]}` : "ended"
    : "running";
  return (
    <div className="hb-terminal">
      <div className="hb-terminal-command">
        <code>$ {text(item, "command")}</code>
        <span className={`hb-badge hb-${ended === "running" ? "running" : ended === "exit 0" ? "ok" : "failed"}`}>{ended}</span>
      </div>
      {output && <pre>{output}</pre>}
    </div>
  );
}

/** A diff: which files changed, and the patch. */
export function Diff({ block }: { block: Json }) {
  const patch = isObject(block) && isObject(block["patch"]) ? text(block["patch"], "text") : "";
  return (
    <div className="hb-diff">
      {list(block, "changes").map((change, at) => (
        <div key={at} className="hb-diff-file">
          {text(change, "operation")} <code>{text(change, "path")}</code>
        </div>
      ))}
      {patch && (
        <pre>
          {patch.split("\n").map((line, at) => (
            <span key={at} className={line.startsWith("+") ? "hb-added" : line.startsWith("-") ? "hb-removed" : undefined}>
              {line + "\n"}
            </span>
          ))}
        </pre>
      )}
    </div>
  );
}

/**
 * A tool call: its title and status, and what it produced. `terminal` finds a terminal by id,
 * for calls whose content points at one.
 */
export function ToolCall({ item, terminal }: { item: Json; terminal?: (id: string) => Json | undefined }) {
  const status = text(item, "status") || "pending";
  return (
    <div className={`hb-item hb-tool hb-tool-${status}`}>
      <div className="hb-tool-head">
        <span className="hb-tool-kind">{text(item, "kind") || text(item, "name") || "tool"}</span>
        <span className="hb-tool-title">{text(item, "title") || text(item, "name")}</span>
        <span className={`hb-badge hb-${status === "completed" ? "ok" : status === "failed" || status === "cancelled" ? "failed" : "running"}`}>
          {status.replace("_", " ")}
        </span>
      </div>
      {list(item, "content").map((block, at) => {
        const type = text(block, "type");
        if (type === "terminal") {
          const found = terminal?.(text(block, "terminalId"));
          return found ? <Terminal key={at} item={found} /> : null;
        }
        if (type === "diff") return <Diff key={at} block={block} />;
        const body = contentText(block);
        return body ? <pre key={at} className="hb-tool-output">{body}</pre> : null;
      })}
    </div>
  );
}

/** A plan: a document, or a list of entries with their status (which is also how todos arrive). */
export function Plan({ item }: { item: Json }) {
  const entries = list(item, "entries");
  return (
    <div className="hb-item hb-plan">
      <div className="hb-label">Plan</div>
      {entries.length > 0 ? (
        <ul>
          {entries.map((entry, at) => (
            <li key={at} className={`hb-plan-${text(entry, "status")}`}>
              <span className="hb-plan-mark">{text(entry, "status") === "completed" ? "✓" : text(entry, "status") === "in_progress" ? "…" : "○"}</span>
              {text(entry, "content")}
            </li>
          ))}
        </ul>
      ) : (
        <pre>{contentText(isObject(item) ? item["content"] : undefined)}</pre>
      )}
    </div>
  );
}

/** A subagent: its title and state, and its own session drawn inside by `children`. */
export function Subagent({ item, children }: { item: Json; children?: ReactNode }) {
  const state = isObject(item) && isObject(item["state"]) ? text(item["state"], "state") : "";
  return (
    <details className="hb-item hb-subagent" open>
      <summary>
        <span className="hb-label">Subagent</span> {text(item, "title")}
        <span className={`hb-badge hb-${state === "idle" ? "ok" : "running"}`}>{state}</span>
      </summary>
      <div className="hb-subagent-body">{children}</div>
    </details>
  );
}

/** A message between a session and its subagent: the task given, or the result returned. */
export function SessionMessage({ item }: { item: Json }) {
  return (
    <div className="hb-item hb-session-message">
      <div className="hb-label">Between agents</div>
      {contentText(isObject(item) ? item["content"] : undefined)}
    </div>
  );
}

export function Compaction({ item }: { item: Json }) {
  return (
    <details className="hb-item hb-compaction">
      <summary>Conversation compacted ({text(item, "status")})</summary>
      <div>{contentText(isObject(item) ? item["summary"] : undefined)}</div>
    </details>
  );
}

/** An approval: the question and its options. Once answered it shows which option was taken. */
export function Approval({ request, onChoose }: { request: Request; onChoose?: (optionId: string) => void }) {
  const options = list(request.params, "options");
  const outcome = isObject(request.outcome) ? request.outcome["outcome"] : undefined;
  const chosen = isObject(outcome) ? text(outcome, "optionId") : "";
  const cancelled = isObject(outcome) && outcome["outcome"] === "cancelled";
  return (
    <div className={`hb-item hb-request ${request.outcome === null ? "hb-pending" : ""}`}>
      <div className="hb-label">Permission</div>
      <div className="hb-request-title">{text(request.params, "title")}</div>
      {request.outcome === null ? (
        <div className="hb-options">
          {options.map((option) => (
            <button key={text(option, "optionId")} type="button" onClick={() => onChoose?.(text(option, "optionId"))}>
              {text(option, "name")}
            </button>
          ))}
        </div>
      ) : (
        <div className="hb-answer">
          {cancelled ? "Cancelled" : (text(options.find((option) => text(option, "optionId") === chosen), "name") || chosen)}
        </div>
      )}
    </div>
  );
}

/** A question from the agent: a small form built from the schema it sent. */
export function Question({ request, onAnswer }: { request: Request; onAnswer?: (content: JsonObject | null) => void }) {
  const schema = isObject(request.params) ? request.params["requestedSchema"] : undefined;
  const fields = Object.entries(isObject(schema) && isObject(schema["properties"]) ? schema["properties"] : {});
  const [values, setValues] = useState<JsonObject>({});
  const answered = isObject(request.outcome) ? request.outcome : undefined;
  return (
    <div className={`hb-item hb-request ${request.outcome === null ? "hb-pending" : ""}`}>
      <div className="hb-label">Question</div>
      <div className="hb-request-title">{text(request.params, "message")}</div>
      {answered ? (
        <div className="hb-answer">
          {answered["action"] === "accept"
            ? Object.values(isObject(answered["content"]) ? answered["content"] : {}).map(String).join(", ")
            : "No answer"}
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onAnswer?.(values);
          }}
        >
          {fields.map(([key, field]) => {
            const choices = list(field, "oneOf");
            return (
              <label key={key} className="hb-field">
                <span>{text(field, "title") || key}</span>
                {choices.length > 0 ? (
                  <select value={String(values[key] ?? "")} onChange={(event) => setValues({ ...values, [key]: event.target.value })}>
                    <option value="" />
                    {choices.map((choice) => (
                      <option key={text(choice, "const")} value={text(choice, "const")}>
                        {text(choice, "title") || text(choice, "const")}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input value={String(values[key] ?? "")} onChange={(event) => setValues({ ...values, [key]: event.target.value })} />
                )}
              </label>
            );
          })}
          <div className="hb-options">
            <button type="submit">Answer</button>
            <button type="button" onClick={() => onAnswer?.(null)}>Skip</button>
          </div>
        </form>
      )}
    </div>
  );
}

/** Any item there is no component for, such as an extension's: its kind and its data. */
export function Fallback({ kind, item }: { kind: string; item: Json }) {
  return (
    <details className="hb-item hb-fallback">
      <summary>{kind}</summary>
      <pre>{JSON.stringify(item, null, 2)}</pre>
    </details>
  );
}
