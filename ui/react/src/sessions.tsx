// The list of sessions: grouped by folder, with each session's state, and rename and delete.

import type { Archipel, SessionSummary } from "@archipel/client";
import { useState, type FormEvent } from "react";

/** The list after the daemon said a session appeared or changed: that session first, once. */
export function withSession(sessions: SessionSummary[], changed: SessionSummary): SessionSummary[] {
  return [changed, ...sessions.filter((other) => other.sessionId !== changed.sessionId)];
}

/** The list after the daemon said a session was deleted. */
export function withoutSession(sessions: SessionSummary[], sessionId: string): SessionSummary[] {
  return sessions.filter((other) => other.sessionId !== sessionId);
}

/** The sessions by working folder, in the order the folders first appear. */
export function byFolder(sessions: SessionSummary[]): { cwd: string; sessions: SessionSummary[] }[] {
  const folders = new Map<string, SessionSummary[]>();
  for (const session of sessions) {
    const folder = folders.get(session.cwd) ?? [];
    folder.push(session);
    folders.set(session.cwd, folder);
  }
  return [...folders].map(([cwd, inside]) => ({ cwd, sessions: inside }));
}

/** How long ago, in a few characters: `now`, `5m`, `3h`, `2d`, then the date. */
export function ago(updatedAt: string | null | undefined, now: Date): string {
  const then = updatedAt ? Date.parse(updatedAt) : Number.NaN;
  if (Number.isNaN(then)) return "";
  const minutes = Math.floor((now.getTime() - then) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`;
  if (minutes < 60 * 24 * 30) return `${Math.floor(minutes / (60 * 24))}d`;
  return new Date(then).toISOString().slice(0, 10);
}

export interface SessionListProps {
  sessions: SessionSummary[];
  /** The session that is open, to mark it. */
  selected?: string;
  onOpen?: (sessionId: string) => void;
  /** Rename a session (`client.setTitle`). Without it, or for an agent that cannot, no rename is offered. */
  onRename?: (sessionId: string, title: string) => void | Promise<unknown>;
  /**
   * Delete a session (`client.deleteSession`). The agent removes it from its own history too
   * and nobody can undo that, so the list asks first. The daemon wants the `admin` scope for it.
   */
  onDelete?: (sessionId: string) => void | Promise<unknown>;
  /**
   * `client.agents()`: an agent that does not say it can rename or delete gets no such control.
   * An agent says so once it has been started. Without this, every session gets the controls.
   */
  agents?: Archipel.AgentInfo[];
  /** One list under folder headings (the default), or one flat list. */
  groupBy?: "folder" | "none";
  /** There are more sessions (`nextCursor` of `client.sessionsPage`): draws "Show more". */
  onMore?: () => void | Promise<unknown>;
  /** The time "5m ago" is counted from. The default is the time of drawing. */
  now?: Date;
}

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));
const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

/** Sessions to choose from. Assemble your own from `byFolder` and `SessionRow` if it does not fit. */
export function SessionList({ sessions, selected, onOpen, onRename, onDelete, agents, groupBy = "folder", onMore, now = new Date() }: SessionListProps) {
  const can = (session: SessionSummary, what: "title" | "delete") => {
    if (agents === undefined) return true;
    const said = agents.find((agent) => agent.agentId === session.agentId)?.capabilities as { session?: Record<string, unknown> } | null | undefined;
    return said?.session?.[what] != null;
  };
  const row = (session: SessionSummary) => (
    <SessionRow
      key={session.sessionId}
      session={session}
      selected={session.sessionId === selected}
      now={now}
      onOpen={onOpen}
      onRename={onRename && can(session, "title") ? onRename : undefined}
      onDelete={onDelete && can(session, "delete") ? onDelete : undefined}
    />
  );
  return (
    <div className="arc-sessions">
      {sessions.length === 0 && <div className="arc-empty">No sessions</div>}
      {groupBy === "none" ? (
        <ul>{sessions.map(row)}</ul>
      ) : (
        byFolder(sessions).map((folder) => (
          <section key={folder.cwd}>
            <h3 className="arc-label" title={folder.cwd}>{folderName(folder.cwd)}</h3>
            <ul>{folder.sessions.map(row)}</ul>
          </section>
        ))
      )}
      {onMore && <button type="button" className="arc-more" onClick={() => void onMore()}>Show more</button>}
    </div>
  );
}

export interface SessionRowProps {
  session: SessionSummary;
  selected?: boolean;
  now?: Date;
  onOpen?: (sessionId: string) => void;
  onRename?: (sessionId: string, title: string) => void | Promise<unknown>;
  onDelete?: (sessionId: string) => void | Promise<unknown>;
}

/** One session: its title, what it is doing, and where else it lives. */
export function SessionRow({ session, selected, now = new Date(), onOpen, onRename, onDelete }: SessionRowProps) {
  const [mode, setMode] = useState<"shown" | "renaming" | "deleting" | "busy">("shown");
  const [draft, setDraft] = useState("");
  const [problem, setProblem] = useState("");
  const title = session.title ?? "Untitled";
  const run = (what: string, work: () => void | Promise<unknown>) => {
    setMode("busy");
    setProblem("");
    Promise.resolve()
      .then(work)
      .catch((error: unknown) => setProblem(`${what}: ${reason(error)}`))
      .finally(() => setMode("shown"));
  };
  const rename = (event: FormEvent) => {
    event.preventDefault();
    const wanted = draft.trim();
    if (!wanted || wanted === session.title) return setMode("shown");
    run("Not renamed", () => onRename?.(session.sessionId, wanted));
  };
  // What needs the user comes first; a session nobody is running says nothing.
  const state = session.state === "requires_action" ? "needs you" : session.state === "running" ? "running" : "";
  return (
    <li className={`arc-session${selected ? " arc-selected" : ""}`}>
      {mode === "renaming" ? (
        <form className="arc-session-rename" onSubmit={rename}>
          <input
            autoFocus
            value={draft}
            aria-label="Session title"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => event.key === "Escape" && setMode("shown")}
          />
          <button type="submit">Save</button>
          <button type="button" onClick={() => setMode("shown")}>Cancel</button>
        </form>
      ) : (
        <div className="arc-session-line">
          <button type="button" className="arc-session-open" aria-current={selected ? "true" : undefined} onClick={() => onOpen?.(session.sessionId)}>
            <span className="arc-session-title">{title}</span>
            <span className="arc-session-meta">
              {state && <span className={`arc-badge arc-${state === "running" ? "running" : "attn"}`}>{state}</span>}
              {session.openElsewhere && <span className="arc-badge arc-attn" title="Another window of the agent has this session open. Fork it to continue here.">open elsewhere</span>}
              {!session.logged && <span className="arc-session-harness" title="Started outside this app. Opening it loads its history from the agent.">from {session.agentId}</span>}
              <span className="arc-session-agent">{session.logged ? session.agentId : ""}</span>
              <time dateTime={session.updatedAt ?? undefined}>{ago(session.updatedAt, now)}</time>
            </span>
          </button>
          <span className="arc-session-actions">
            {onRename && (
              <button type="button" className="arc-session-action" title="Rename" disabled={mode === "busy"} onClick={() => { setDraft(session.title ?? ""); setMode("renaming"); }}>
                <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10.5 2.5l3 3L5.5 13.5H2.5v-3z" /></svg><span className="arc-hidden">Rename</span>
              </button>
            )}
            {onDelete && (
              <button type="button" className="arc-session-action" title="Delete" disabled={mode === "busy"} onClick={() => setMode("deleting")}>
                <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 4h11M6 4V2.5h4V4M4 4l.75 9.5h6.5L12 4M6.75 6.5v4.5M9.25 6.5v4.5" /></svg><span className="arc-hidden">Delete</span>
              </button>
            )}
          </span>
        </div>
      )}
      {mode === "deleting" && (
        <div className="arc-session-confirm" role="alertdialog" aria-label={`Delete ${title}`}>
          <span>Delete “{title}”? It is removed from {session.agentId}’s own history too. This cannot be undone.</span>
          <button type="button" className="arc-danger" onClick={() => run("Not deleted", () => onDelete?.(session.sessionId))}>Delete</button>
          <button type="button" onClick={() => setMode("shown")}>Cancel</button>
        </div>
      )}
      {problem && <div className="arc-problem" role="alert">{problem}</div>}
    </li>
  );
}
