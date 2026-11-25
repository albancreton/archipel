// A chat view assembled from the item components, and the hook that keeps it current.

import { RpcError, type Conversation, type Json, type JsonObject, type Session, type SessionState } from "@harness-bridge/client";
import { useState, useSyncExternalStore, type ComponentType, type ReactNode } from "react";
import {
  Approval, AssistantMessage, Compaction, Fallback, Plan, Question, SessionMessage, Subagent, Terminal, Thought, ToolCall, UserMessage,
} from "./items.tsx";

/** What the user can do. Leave one out and its control is not drawn or does nothing. */
export interface Actions {
  /** Answer an approval with one of its options. */
  approve?: (requestId: string, optionId: string) => void;
  /** Answer a question: the form's content, or null to skip it. */
  answer?: (requestId: string, content: JsonObject | null) => void;
  /** Send a message. If it returns a promise that fails, the composer gives the text back. */
  prompt?: (text: string) => void | Promise<unknown>;
  cancel?: () => void;
  /**
   * Continue on a copy of the session, when another process has this one open. The app shows
   * the copy once the promise resolves; if it fails, the notice says why.
   */
  fork?: () => void | Promise<unknown>;
}

/** Another process of the harness that has the session open, as the daemon reports it. */
export interface Holder {
  pid?: number;
  /** What it says it is doing, such as `busy` or `idle`. */
  status?: string | null;
}

/** Components for kinds of item that this package does not know, by target, e.g. `x.acme.checkpoint`. */
export type ItemComponents = Record<string, ComponentType<{ item: Json }>>;

export interface TimelineProps {
  /** Every session of the conversation, to find a subagent's. */
  conversation: Conversation;
  /** The session to draw. */
  state: SessionState;
  actions?: Actions;
  components?: ItemComponents;
}

/** The items of one session, in order. A subagent's session is drawn inside its item. */
export function Timeline({ conversation, state, actions, components }: TimelineProps) {
  // A terminal that a tool call shows is not drawn a second time.
  const inline = new Set<string>();
  for (const tool of state.collections.get("toolCall")?.values() ?? []) {
    const content = (tool as JsonObject)["content"];
    for (const block of Array.isArray(content) ? content : []) {
      const id = (block as JsonObject | null)?.["terminalId"];
      if (typeof id === "string") inline.add(id);
    }
  }
  const terminal = (id: string) => state.collections.get("terminal")?.get(id);

  const draw = (target: string, id: string): ReactNode => {
    if (target === "request") {
      const request = state.requests.get(id);
      if (!request) return null;
      return request.method === "session/request_permission" ? (
        <Approval request={request} onChoose={(optionId) => actions?.approve?.(id, optionId)} />
      ) : (
        <Question request={request} onAnswer={(content) => actions?.answer?.(id, content)} />
      );
    }
    const item = state.collections.get(target)?.get(id);
    if (item === undefined) return null;
    const Custom = components?.[target];
    if (Custom) return <Custom item={item} />;
    switch (target) {
      case "userMessage": return <UserMessage item={item} />;
      case "agentMessage": return <AssistantMessage item={item} />;
      case "agentThought": return <Thought item={item} />;
      case "toolCall": return <ToolCall item={item} terminal={terminal} />;
      case "terminal": return inline.has(id) ? null : <div className="hb-item"><Terminal item={item} /></div>;
      case "plan": return <Plan item={item} />;
      case "sessionMessage": return <SessionMessage item={item} />;
      case "compaction": return <Compaction item={item} />;
      case "subagent": {
        const child = conversation.sessions.get(id);
        return (
          <Subagent item={item}>
            {child && <Timeline conversation={conversation} state={child} actions={actions} components={components} />}
          </Subagent>
        );
      }
      // A cursor marks a place in the history; it is not content.
      case "cursor": return null;
      default: return <Fallback kind={target} item={item} />;
    }
  };

  return (
    <div className="hb-timeline">
      {state.timeline.map((ref) => {
        const drawn = draw(ref.target, ref.id);
        return drawn && <div key={`${ref.target}:${ref.id}`} className="hb-row">{drawn}</div>;
      })}
    </div>
  );
}

/** The session's title, turn state and context use. */
export function StatusBar({ state }: { state: SessionState }) {
  const turn = state.values.get("state") as JsonObject | undefined;
  const info = state.values.get("sessionInfo") as JsonObject | undefined;
  const usage = state.values.get("usage") as JsonObject | undefined;
  const turnState = typeof turn?.["state"] === "string" ? turn["state"] : "";
  return (
    <div className="hb-status">
      <span className="hb-status-title">{typeof info?.["title"] === "string" ? info["title"] : "New session"}</span>
      {typeof usage?.["used"] === "number" && typeof usage["size"] === "number" && (
        <span className="hb-status-usage">{Math.round((usage["used"] / usage["size"]) * 100)}% of context</span>
      )}
      {turnState && (
        <span className={`hb-badge hb-${turnState === "idle" ? "ok" : "running"}`}>{turnState.replace("_", " ")}</span>
      )}
    </div>
  );
}

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Says that another process has the session open, so messages from here are refused, and
 * offers to continue on a copy. Without `onFork` it only says so.
 */
export function OpenElsewhere({ holder, onFork }: { holder: Holder; onFork?: () => void | Promise<unknown> }) {
  const [forking, setForking] = useState(false);
  const [problem, setProblem] = useState("");
  const fork = async () => {
    setForking(true);
    setProblem("");
    try {
      await onFork?.();
    } catch (error) {
      setProblem(reason(error));
    }
    setForking(false);
  };
  const who = [holder.pid ? `pid ${holder.pid}` : "", holder.status ?? ""].filter(Boolean).join(", ");
  return (
    <div className="hb-elsewhere" role="status">
      <div className="hb-elsewhere-text">
        <strong>This session is open in another window of the agent{who && ` (${who})`}.</strong>{" "}
        Messages from here are refused so the two do not overwrite each other. Close it there and send again
        {onFork ? ", or continue here on a copy." : "."}
        {problem && <div className="hb-problem">Could not fork: {problem}</div>}
      </div>
      {onFork && (
        <button type="button" disabled={forking} onClick={() => void fork()}>
          {forking ? "Forking…" : "Fork and continue here"}
        </button>
      )}
    </div>
  );
}

/** Where the user types. While a turn runs it also offers to stop it. */
export function Composer({ running, actions }: { running: boolean; actions?: Actions }) {
  const [draft, setDraft] = useState("");
  const [problem, setProblem] = useState("");
  const send = () => {
    const text = draft;
    if (!text.trim()) return;
    setDraft("");
    setProblem("");
    // A refused message goes back into the field, unless the user has typed something since.
    Promise.resolve(actions?.prompt?.(text)).catch((error: unknown) => {
      setDraft((now) => now || text);
      // The notice above the composer already says it when the session is open elsewhere.
      if (!(error instanceof RpcError && error.openElsewhere)) setProblem(reason(error));
    });
  };
  return (
    <form className="hb-composer" onSubmit={(event) => { event.preventDefault(); send(); }}>
      <textarea
        value={draft}
        placeholder={running ? "Add to the running turn…" : "Message the agent…"}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            send();
          }
        }}
      />
      <button type="submit">Send</button>
      {running && <button type="button" onClick={() => actions?.cancel?.()}>Stop</button>}
      {problem && <div className="hb-problem">Not sent: {problem}</div>}
    </form>
  );
}

export interface ChatViewProps {
  conversation: Conversation;
  /** The session the user talks to. The default is the conversation's first. */
  sessionId?: string;
  actions?: Actions;
  components?: ItemComponents;
  /** Set while another process has the session open: the chat says so and offers `actions.fork`. */
  openElsewhere?: Holder | null;
  /** Something the daemon reported outside any request, such as a log it cannot write. Shown above the composer. */
  error?: string | null;
}

/** A simple chat: status, the items, and a composer. Assemble your own from the parts if it does not fit. */
export function ChatView({ conversation, sessionId, actions, components, openElsewhere, error }: ChatViewProps) {
  const state = sessionId ? conversation.sessions.get(sessionId) : conversation.root()?.[1];
  if (!state) return <div className="hb-chat hb-empty">No session</div>;
  const turn = state.values.get("state") as JsonObject | undefined;
  const running = turn !== undefined && turn["state"] !== "idle";
  return (
    <div className="hb-chat">
      <StatusBar state={state} />
      <Timeline conversation={conversation} state={state} actions={actions} components={components} />
      {openElsewhere && <OpenElsewhere holder={openElsewhere} onFork={actions?.fork} />}
      {error && <div className="hb-problem hb-session-error" role="alert">{error}</div>}
      {actions?.prompt && <Composer running={running} actions={actions} />}
    </div>
  );
}

/**
 * Follows a session of the client SDK: the component draws again after every change.
 * Returns what `ChatView` takes: the conversation, the actions wired to the session, and who
 * else has the session open. Give `onFork` to offer the fork: it receives the copy to show.
 */
export function useSession(
  session: Session,
  options: { onFork?: (copy: Session) => void } = {},
): { conversation: Conversation; actions: Actions; openElsewhere: Holder | null; error: string | null } {
  // The session changes in place, so a counter is the snapshot React compares.
  const [store] = useState(() => {
    let version = 0;
    return {
      subscribe: (changed: () => void) => session.subscribe(() => { version += 1; changed(); }),
      snapshot: () => version,
    };
  });
  useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot);
  const { onFork } = options;
  return {
    conversation: session.conversation,
    openElsewhere: session.openElsewhere,
    error: session.error?.message ?? null,
    actions: {
      approve: (requestId, optionId) => session.approve(requestId, optionId),
      answer: (requestId, content) =>
        session.answer(requestId, content === null ? { action: "decline" } : { action: "accept", content }),
      prompt: (text) => session.prompt(text),
      cancel: () => session.cancel(),
      fork: onFork && (async () => onFork(await session.fork())),
    },
  };
}
