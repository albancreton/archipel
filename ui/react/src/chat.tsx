// A chat view assembled from the item components, and the hook that keeps it current.

import { type Conversation, type Json, type JsonObject, type Session, type SessionState } from "@harness-bridge/client";
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
  prompt?: (text: string) => void;
  cancel?: () => void;
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

/** Where the user types. While a turn runs it also offers to stop it. */
export function Composer({ running, actions }: { running: boolean; actions?: Actions }) {
  const [draft, setDraft] = useState("");
  const send = () => {
    if (!draft.trim()) return;
    actions?.prompt?.(draft);
    setDraft("");
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
    </form>
  );
}

export interface ChatViewProps {
  conversation: Conversation;
  /** The session the user talks to. The default is the conversation's first. */
  sessionId?: string;
  actions?: Actions;
  components?: ItemComponents;
}

/** A simple chat: status, the items, and a composer. Assemble your own from the parts if it does not fit. */
export function ChatView({ conversation, sessionId, actions, components }: ChatViewProps) {
  const state = sessionId ? conversation.sessions.get(sessionId) : conversation.root()?.[1];
  if (!state) return <div className="hb-chat hb-empty">No session</div>;
  const turn = state.values.get("state") as JsonObject | undefined;
  const running = turn !== undefined && turn["state"] !== "idle";
  return (
    <div className="hb-chat">
      <StatusBar state={state} />
      <Timeline conversation={conversation} state={state} actions={actions} components={components} />
      {actions?.prompt && <Composer running={running} actions={actions} />}
    </div>
  );
}

/**
 * Follows a session of the client SDK: the component draws again after every change.
 * Returns the conversation to give to `ChatView`, and the actions wired to the session.
 */
export function useSession(session: Session): { conversation: Conversation; actions: Actions } {
  // The session changes in place, so a counter is the snapshot React compares.
  const [store] = useState(() => {
    let version = 0;
    return {
      subscribe: (changed: () => void) => session.subscribe(() => { version += 1; changed(); }),
      snapshot: () => version,
    };
  });
  useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot);
  return {
    conversation: session.conversation,
    actions: {
      approve: (requestId, optionId) => session.approve(requestId, optionId),
      answer: (requestId, content) =>
        session.answer(requestId, content === null ? { action: "decline" } : { action: "accept", content }),
      prompt: (text) => void session.prompt(text),
      cancel: () => session.cancel(),
    },
  };
}
