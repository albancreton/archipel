// Where the user types: text, attachments, the agent's commands after `/`, and files after `@`.

import { RpcError, type Json, type JsonObject } from "@archipel/client";
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent } from "react";

/** A slash command the agent offers (`state.values.get("availableCommands")`). */
export interface Command {
  name: string;
  description?: string;
  /** What to type after the command, when the agent says. */
  hint?: string;
}

/** Reads the agent's `availableCommands`. */
export function commandsOf(availableCommands: Json | undefined): Command[] {
  const commands: Command[] = [];
  for (const entry of Array.isArray(availableCommands) ? availableCommands : []) {
    const from = entry as JsonObject | null;
    const name = from?.["name"];
    if (typeof name !== "string") continue;
    const hint = (from?.["input"] as JsonObject | null | undefined)?.["hint"];
    commands.push({
      name,
      description: typeof from?.["description"] === "string" ? from["description"] : undefined,
      hint: typeof hint === "string" ? hint : undefined,
    });
  }
  return commands;
}

/** Something the user can mention with `@`: a file, usually. The app supplies them. */
export interface Mention {
  /** Where it is, such as `file:///work/app/src/main.ts`. Sent to the agent as a link. */
  uri: string;
  /** What is written into the text after `@`, such as `src/main.ts`. No spaces. */
  name: string;
  /** A second line in the menu. */
  detail?: string;
}

/** A file the user attached. */
export interface Attachment {
  name: string;
  mimeType: string;
  /** The file's bytes, base64. */
  data: string;
}

/** What the agent takes in a message besides text (`capabilities.session.prompt` of `client.agents()`). */
export interface Accepts {
  /** Pictures. */
  image?: boolean;
  /** Other files, sent with their content. */
  files?: boolean;
}

/** What an agent said it takes, from one entry of `client.agents()`. Nothing until the agent has started. */
export function acceptsOf(agent: { capabilities?: unknown } | undefined): Accepts {
  const prompt = (agent?.capabilities as { session?: { prompt?: Record<string, unknown> } } | null | undefined)?.session?.prompt;
  return { image: prompt?.["image"] != null, files: prompt?.["embeddedContext"] != null };
}

/** The command the user is typing: the draft is `/` and a name with nothing after it yet. */
export function commandQuery(draft: string): string | null {
  return /^\/[^\s/]*$/.test(draft) ? draft.slice(1) : null;
}

/** The commands for what was typed after `/`: those whose name starts with it first, then those that contain it. */
export function matchCommands(commands: Command[], query: string): Command[] {
  const wanted = query.toLowerCase();
  const starts = commands.filter((command) => command.name.toLowerCase().startsWith(wanted));
  return [...starts, ...commands.filter((command) => !starts.includes(command) && command.name.toLowerCase().includes(wanted))];
}

/** The mention the user is typing at the caret: `@` at the start of a word, and what follows it. */
export function mentionQuery(draft: string, caret: number): { start: number; query: string } | null {
  const found = /(^|\s)@([^\s@]*)$/.exec(draft.slice(0, caret));
  return found ? { start: caret - found[2]!.length - 1, query: found[2]! } : null;
}

const isText = (mimeType: string) => mimeType.startsWith("text/") || /json|xml|javascript|typescript|yaml|toml|x-sh/.test(mimeType);

function utf8(base64: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(base64), (char) => char.charCodeAt(0)));
}

/**
 * What goes to the agent: the text alone when there is nothing else, otherwise content blocks.
 * A picture is an `image` block, another file a `resource` with its content, and a mention
 * whose `@name` is still in the text a `resource_link`.
 */
export function promptContent(text: string, attachments: Attachment[] = [], mentions: Mention[] = []): string | Json[] {
  const kept = mentions.filter((mention, at) =>
    new RegExp(`(^|\\s)@${mention.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(\\s|$)`).test(text)
    && mentions.findIndex((other) => other.uri === mention.uri) === at);
  if (attachments.length === 0 && kept.length === 0) return text;
  const blocks: Json[] = text.trim() ? [{ type: "text", text }] : [];
  for (const mention of kept) blocks.push({ type: "resource_link", uri: mention.uri, name: mention.name });
  for (const file of attachments) {
    if (file.mimeType.startsWith("image/")) {
      blocks.push({ type: "image", mimeType: file.mimeType, data: file.data });
    } else {
      const uri = `file:///${encodeURIComponent(file.name)}`;
      const resource: JsonObject = isText(file.mimeType) ? { uri, mimeType: file.mimeType, text: utf8(file.data) } : { uri, mimeType: file.mimeType, blob: file.data };
      blocks.push({ type: "resource", resource });
    }
  }
  return blocks;
}

function read(file: File): Promise<Attachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("could not read the file"));
    reader.onload = () => {
      const url = String(reader.result);
      resolve({ name: file.name || "pasted", mimeType: file.type || "application/octet-stream", data: url.slice(url.indexOf(",") + 1) });
    };
    reader.readAsDataURL(file);
  });
}

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

export interface ComposerProps {
  /** A turn is running: the composer also offers to stop it. */
  running: boolean;
  /** Send a message. If it returns a promise that fails, the composer gives everything back. */
  onSend?: (content: string | Json[]) => void | Promise<unknown>;
  onCancel?: () => void;
  /** The agent's commands: typing `/` offers them. */
  commands?: Command[];
  /**
   * Finds what the user may mention, for what they typed after `@`: files of the project,
   * usually. Without it `@` is only a character.
   */
  mentions?: (query: string) => Mention[] | Promise<Mention[]>;
  /** What the agent takes besides text. With neither, no attaching is offered. */
  accepts?: Accepts;
  /** The largest file that can be attached, in bytes. Default 5 MB. */
  maxBytes?: number;
}

/** Where the user types. */
export function Composer({ running, onSend, onCancel, commands = [], mentions, accepts = {}, maxBytes = 5_000_000 }: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [caret, setCaret] = useState(0);
  const [attached, setAttached] = useState<Attachment[]>([]);
  const [mentioned, setMentioned] = useState<Mention[]>([]);
  const [found, setFound] = useState<Mention[]>([]);
  const [chosen, setChosen] = useState(0);
  const [closed, setClosed] = useState(false);
  const [problem, setProblem] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const canAttach = accepts.image || accepts.files;

  const command = commandQuery(draft);
  const mention = mentions ? mentionQuery(draft, caret) : null;
  const query = mention?.query;
  // Asks the app for what matches; an answer to an older query is dropped.
  useEffect(() => {
    if (query === undefined || !mentions) return setFound([]);
    let current = true;
    Promise.resolve(mentions(query)).then((list) => current && setFound(list.slice(0, 20)), () => current && setFound([]));
    return () => { current = false; };
  }, [query, mentions]);

  const matching = command === null ? [] : matchCommands(commands, command).slice(0, 20);
  const menu: { key: string; label: string; detail?: string; pick: () => void }[] = closed ? [] : command !== null
    ? matching.map((entry) => ({
      key: entry.name, label: `/${entry.name}`, detail: entry.hint ? `${entry.hint} · ${entry.description ?? ""}` : entry.description,
      pick: () => write(`/${entry.name} `, entry.name.length + 2),
    }))
    : mention ? found.map((entry) => ({
      key: entry.uri, label: `@${entry.name}`, detail: entry.detail,
      pick: () => {
        setMentioned((before) => [...before, entry]);
        const head = `${draft.slice(0, mention.start)}@${entry.name} `;
        write(head + draft.slice(caret), head.length);
      },
    })) : [];
  const at = Math.min(chosen, Math.max(menu.length - 1, 0));

  function write(text: string, position: number) {
    setDraft(text);
    setCaret(position);
    setChosen(0);
    // After React has put the text in the field.
    queueMicrotask(() => { field.current?.focus(); field.current?.setSelectionRange(position, position); });
  }

  const attach = async (files: ArrayLike<File>) => {
    setProblem("");
    for (const file of Array.from(files)) {
      const image = file.type.startsWith("image/");
      if (image ? !accepts.image : !accepts.files) { setProblem(`${file.name}: the agent does not take ${image ? "pictures" : "files"}`); continue; }
      if (file.size > maxBytes) { setProblem(`${file.name}: larger than ${Math.round(maxBytes / 1_000_000)} MB`); continue; }
      try {
        const loaded = await read(file);
        setAttached((before) => [...before, loaded]);
      } catch (error) {
        setProblem(`${file.name}: ${reason(error)}`);
      }
    }
  };

  const send = () => {
    const text = draft, files = attached, links = mentioned;
    if (!text.trim() && files.length === 0) return;
    setDraft(""); setAttached([]); setMentioned([]); setProblem("");
    // A refused message goes back into the field, unless the user has typed something since.
    Promise.resolve(onSend?.(promptContent(text, files, links))).catch((error: unknown) => {
      setDraft((now) => now || text);
      setAttached((now) => (now.length ? now : files));
      setMentioned((now) => (now.length ? now : links));
      // The notice above the composer already says it when the session is open elsewhere.
      if (!(error instanceof RpcError && error.openElsewhere)) setProblem(`Not sent: ${reason(error)}`);
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (menu.length > 0) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        return setChosen((at + (event.key === "ArrowDown" ? 1 : menu.length - 1)) % menu.length);
      }
      if (event.key === "Enter" || event.key === "Tab") { event.preventDefault(); return menu[at]!.pick(); }
      if (event.key === "Escape") { event.preventDefault(); return setClosed(true); }
    }
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); send(); }
  };
  const onPaste = (event: ClipboardEvent) => {
    const files = Array.from(event.clipboardData.files);
    if (files.length === 0 || !canAttach) return;
    event.preventDefault();
    void attach(files);
  };
  const onDrop = (event: DragEvent) => {
    if (!canAttach || event.dataTransfer.files.length === 0) return;
    event.preventDefault();
    void attach(event.dataTransfer.files);
  };

  return (
    <form className="arc-composer" onSubmit={(event) => { event.preventDefault(); send(); }} onDrop={onDrop} onDragOver={(event) => canAttach && event.preventDefault()}>
      {menu.length > 0 && (
        <ul className="arc-menu" role="listbox" aria-label={command !== null ? "Commands" : "Mentions"}>
          {menu.map((entry, index) => (
            <li key={entry.key} role="option" aria-selected={index === at} className={index === at ? "arc-chosen" : undefined}
              onMouseDown={(event) => { event.preventDefault(); entry.pick(); }}>
              <span className="arc-menu-label">{entry.label}</span>
              {entry.detail && <span className="arc-menu-detail">{entry.detail}</span>}
            </li>
          ))}
        </ul>
      )}
      {attached.length > 0 && (
        <ul className="arc-attachments">
          {attached.map((file, index) => (
            <li key={index}>
              {file.mimeType.startsWith("image/") && <img src={`data:${file.mimeType};base64,${file.data}`} alt="" />}
              <span>{file.name}</span>
              <button type="button" aria-label={`Remove ${file.name}`} onClick={() => setAttached(attached.filter((_, other) => other !== index))}>×</button>
            </li>
          ))}
        </ul>
      )}
      <textarea
        ref={field}
        value={draft}
        placeholder={running ? "Add to the running turn…" : "Message the agent…"}
        onChange={(event) => { setDraft(event.target.value); setCaret(event.target.selectionStart); setClosed(false); setChosen(0); }}
        onSelect={(event) => setCaret(event.currentTarget.selectionStart)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
      />
      {canAttach && (
        <>
          <input ref={picker} type="file" multiple hidden accept={accepts.files ? undefined : "image/*"}
            onChange={(event) => { void attach(event.target.files ?? []); event.target.value = ""; }} />
          <button type="button" className="arc-attach" onClick={() => picker.current?.click()}>Attach</button>
        </>
      )}
      <button type="submit">Send</button>
      {running && <button type="button" onClick={() => onCancel?.()}>Stop</button>}
      {problem && <div className="arc-problem" role="alert">{problem}</div>}
    </form>
  );
}
