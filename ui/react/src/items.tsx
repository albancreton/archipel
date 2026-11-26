// One component per kind of item. Each takes the item and, where the user can act, a callback.
// Nothing here knows about connections; give the components state and they draw it.

import { base64Text, type Json, type JsonObject, type Request } from "@archipel/client";
import { useContext, useState, type ReactNode } from "react";
import { ansiPieces, type AnsiStyle } from "./ansi.ts";
import { foldContext, parsePatch, type DiffRow } from "./diff.ts";
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

/** What the user sent: the text as written, and what was attached or mentioned. */
export function UserMessage({ item }: { item: Json }) {
  const content = isObject(item) ? item["content"] : undefined;
  const blocks = (Array.isArray(content) ? content : [content]).filter(isObject);
  const extras = blocks.filter((block) => block["type"] !== "text");
  return (
    <div className="arc-item arc-user">
      {blocks.filter((block) => block["type"] === "text").map((block) => text(block, "text")).join("") || (typeof content === "string" ? content : "")}
      {extras.length > 0 && (
        <span className="arc-user-attached">
          {extras.map((block, at) => {
            // Only a picture the user sent themselves is shown, and only from its own bytes.
            if (block["type"] === "image" && text(block, "data") && /^image\/[\w.+-]+$/.test(text(block, "mimeType"))) {
              return <img key={at} src={`data:${text(block, "mimeType")};base64,${text(block, "data")}`} alt="attached picture" />;
            }
            const resource = isObject(block["resource"]) ? block["resource"] : block;
            const name = text(block, "name") || decodeURIComponent(text(resource, "uri").split("/").pop() ?? "") || text(block, "type");
            return <span key={at} className="arc-chip">{name}</span>;
          })}
        </span>
      )}
    </div>
  );
}

/** What the agent said, drawn as Markdown unless `RenderText` says otherwise. */
export function AssistantMessage({ item }: { item: Json }) {
  const render = useContext(RenderText);
  return <div className="arc-item arc-assistant">{render(contentText(isObject(item) ? item["content"] : undefined))}</div>;
}

/** Reasoning, folded by default. */
export function Thought({ item }: { item: Json }) {
  const render = useContext(RenderText);
  const body = contentText(isObject(item) ? item["content"] : undefined).trim();
  return (
    <details className="arc-item arc-thought">
      <summary>Thinking</summary>
      <div>{render(body)}</div>
    </details>
  );
}

function ansiProps(style: AnsiStyle): { className?: string; style?: { color?: string; backgroundColor?: string } } {
  // Inverse swaps the two colours; without colours it is the terminal's own pair, swapped.
  const [fg, bg] = style.inverse ? [style.bg ?? "bg", style.fg ?? "fg"] : [style.fg, style.bg];
  const classes = [
    typeof fg === "number" ? `arc-ansi-fg-${fg}` : fg === "bg" ? "arc-ansi-fg-bg" : "",
    typeof bg === "number" ? `arc-ansi-bg-${bg}` : bg === "fg" ? "arc-ansi-bg-fg" : "",
    style.bold ? "arc-ansi-bold" : "", style.dim ? "arc-ansi-dim" : "", style.italic ? "arc-ansi-italic" : "", style.underline ? "arc-ansi-underline" : "",
  ].filter(Boolean).join(" ");
  const css = {
    ...(typeof fg === "string" && fg !== "bg" ? { color: fg } : {}),
    ...(typeof bg === "string" && bg !== "fg" ? { backgroundColor: bg } : {}),
  };
  return { ...(classes ? { className: classes } : {}), ...(Object.keys(css).length ? { style: css } : {}) };
}

/** Terminal output with its colours. */
export function AnsiText({ text: output }: { text: string }) {
  return <>{ansiPieces(output).map((piece, at) => {
    const props = ansiProps(piece.style);
    return props.className || props.style ? <span key={at} {...props}>{piece.text}</span> : piece.text;
  })}</>;
}

/** The output of a command, with the command line and how it ended. */
export function Terminal({ item }: { item: Json }) {
  const output = isObject(item) && isObject(item["output"]) ? base64Text(text(item["output"], "data")) : "";
  const exit = isObject(item) ? item["exitStatus"] : undefined;
  const ended = isObject(exit)
    ? typeof exit["exitCode"] === "number" ? `exit ${exit["exitCode"]}` : "ended"
    : "running";
  return (
    <div className="arc-terminal">
      <div className="arc-terminal-command">
        <code>$ {text(item, "command")}</code>
        <span className={`arc-badge arc-${ended === "running" ? "running" : ended === "exit 0" ? "ok" : "failed"}`}>{ended}</span>
      </div>
      {output && <pre className="arc-terminal-output"><AnsiText text={output} /></pre>}
    </div>
  );
}

/** One line of a diff, or a fold of unchanged lines that opens when clicked. */
function DiffLine({ row }: { row: DiffRow }) {
  const [open, setOpen] = useState(false);
  if (row.kind === "fold") {
    if (open) return <>{row.rows.map((inner, at) => <DiffLine key={at} row={inner} />)}</>;
    return (
      <tr className="arc-diff-fold">
        <td colSpan={3}><button type="button" onClick={() => setOpen(true)}>Show {row.rows.length} unchanged lines</button></td>
      </tr>
    );
  }
  if (row.kind === "hunk") return <tr className="arc-diff-hunk"><td colSpan={3}>{row.text}</td></tr>;
  return (
    <tr className={row.kind === "add" ? "arc-diff-add" : row.kind === "remove" ? "arc-diff-remove" : undefined}>
      <td className="arc-diff-number">{row.old ?? ""}</td>
      <td className="arc-diff-number">{row.new ?? ""}</td>
      <td className="arc-diff-text"><span className="arc-diff-sign">{row.kind === "add" ? "+" : row.kind === "remove" ? "-" : " "}</span>{row.text}</td>
    </tr>
  );
}

/**
 * A diff: per file, how much changed and the changed lines with their numbers. Long unchanged
 * parts are folded. A patch this cannot read is shown as it is.
 */
export function Diff({ block }: { block: Json }) {
  const patch = isObject(block) && isObject(block["patch"]) ? text(block["patch"], "text") : "";
  const files = parsePatch(patch);
  const operations = new Map(list(block, "changes").map((change) => [text(change, "path"), text(change, "operation")]));
  return (
    <div className="arc-diff">
      {files.length === 0 && list(block, "changes").map((change, at) => (
        <div key={at} className="arc-diff-file">{text(change, "operation")} <code>{text(change, "path")}</code></div>
      ))}
      {files.length === 0 && patch && <pre>{patch}</pre>}
      {files.map((file, at) => (
        <div key={at} className="arc-diff-one">
          <div className="arc-diff-file">
            {operations.get(file.path) ?? ""} <code>{file.path}</code>
            <span className="arc-added"> +{file.added}</span> <span className="arc-removed">−{file.removed}</span>
          </div>
          <div className="arc-diff-rows">
            <table><tbody>{foldContext(file.rows).map((row, line) => <DiffLine key={line} row={row} />)}</tbody></table>
          </div>
        </div>
      ))}
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
    <div className={`arc-item arc-tool arc-tool-${status}`}>
      <div className="arc-tool-head">
        <span className="arc-tool-kind">{text(item, "kind") || text(item, "name") || "tool"}</span>
        <span className="arc-tool-title">{text(item, "title") || text(item, "name")}</span>
        <span className={`arc-badge arc-${status === "completed" ? "ok" : status === "failed" || status === "cancelled" ? "failed" : "running"}`}>
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
        return body ? <pre key={at} className="arc-tool-output">{body}</pre> : null;
      })}
    </div>
  );
}

/** The mark before a plan entry or a todo: a tick when done, a filled dot while in progress, a ring before. */
export function PlanMark({ status }: { status: string }) {
  return (
    <svg className="arc-plan-mark" viewBox="0 0 16 16" aria-label={status.replace(/_/g, " ")} role="img">
      {status === "completed" ? <path d="M3.5 8.5l3 3 6-7" /> : status === "in_progress" ? <circle cx="8" cy="8" r="3.5" className="arc-plan-dot" /> : <circle cx="8" cy="8" r="4.5" />}
    </svg>
  );
}

/** A plan: a document, or a list of entries with their status (which is also how todos arrive). */
export function Plan({ item }: { item: Json }) {
  const entries = list(item, "entries");
  return (
    <div className="arc-item arc-plan">
      <div className="arc-label">Plan</div>
      {entries.length > 0 ? (
        <ul>
          {entries.map((entry, at) => (
            <li key={at} className={`arc-plan-${text(entry, "status")}`}>
              <PlanMark status={text(entry, "status")} />
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
    <details className="arc-item arc-subagent" open>
      <summary>
        <span className="arc-label">Subagent</span> {text(item, "title")}
        <span className={`arc-badge arc-${state === "idle" ? "ok" : "running"}`}>{state}</span>
      </summary>
      <div className="arc-subagent-body">{children}</div>
    </details>
  );
}

/** A message between a session and its subagent: the task given, or the result returned. */
export function SessionMessage({ item }: { item: Json }) {
  return (
    <div className="arc-item arc-session-message">
      <span className="arc-label">Between agents</span> {contentText(isObject(item) ? item["content"] : undefined)}
    </div>
  );
}

export function Compaction({ item }: { item: Json }) {
  return (
    <details className="arc-item arc-compaction">
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
    <div className={`arc-item arc-request ${request.outcome === null ? "arc-pending" : ""}`}>
      <div className="arc-request-head">
        <span className="arc-label">Permission</span> <span className="arc-request-title">{text(request.params, "title")}</span>
      </div>
      {request.outcome === null ? (
        <div className="arc-options">
          {options.map((option) => (
            <button key={text(option, "optionId")} type="button" onClick={() => onChoose?.(text(option, "optionId"))}>
              {text(option, "name")}
            </button>
          ))}
        </div>
      ) : (
        <div className="arc-answer">
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
    <div className={`arc-item arc-request ${request.outcome === null ? "arc-pending" : ""}`}>
      <div className="arc-request-head">
        <span className="arc-label">Question</span> <span className="arc-request-title">{text(request.params, "message")}</span>
      </div>
      {answered ? (
        <div className="arc-answer">
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
              <label key={key} className="arc-field">
                <span>{text(field, "title") || key}</span>
                {choices.length > 0 ? (
                  <select value={String(values[key] ?? "")} onChange={(event) => setValues({ ...values, [key]: event.target.value })}>
                    <option value="" disabled>Choose…</option>
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
          <div className="arc-options">
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
    <details className="arc-item arc-fallback">
      <summary>{kind}</summary>
      <pre>{JSON.stringify(item, null, 2)}</pre>
    </details>
  );
}
