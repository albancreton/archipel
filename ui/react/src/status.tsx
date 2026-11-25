// What is going on around the conversation: the agent, the connection, usage, and work that
// goes on in the background.

import type { Archipel, ArchipelClient, JsonObject, SessionState } from "@archipel/client";
import { useSyncExternalStore } from "react";

/** What to tell the user about an agent that cannot be used as it is, or null when it can. */
export function agentProblem(agent: Archipel.AgentInfo): { tone: "wait" | "act" | "bad"; text: string } | null {
  const detail = agent.detail ? ` (${agent.detail})` : "";
  switch (agent.readiness) {
    case "starting": return { tone: "wait", text: `${agent.name} is starting…` };
    case "needs_login": return { tone: "act", text: `${agent.name} is not signed in. Sign in with ${agent.name}'s own program, then try again.` };
    case "not_installed": return { tone: "bad", text: `${agent.name} is not installed on the daemon's machine${detail}.` };
    case "error": return { tone: "bad", text: `${agent.name} could not be started${detail}.` };
    // Ready, or not started yet: nothing is wrong as far as anyone knows.
    default: return null;
  }
}

/**
 * Says which agents cannot be used and why: not signed in, not installed, failed to start, or
 * still starting. Draws nothing when all is well. `agents` is `client.agents()`; ask again
 * after a session was created or listed, since an agent is only known once it was started.
 */
export function AgentStatus({ agents, onRetry }: { agents: Archipel.AgentInfo[]; onRetry?: () => void }) {
  const problems = agents.flatMap((agent) => {
    const problem = agentProblem(agent);
    return problem ? [{ id: agent.agentId, ...problem }] : [];
  });
  if (problems.length === 0) return null;
  return (
    <div className="arc-chat arc-notices">
      {problems.map((problem) => (
        <div key={problem.id} className={`arc-notice arc-notice-${problem.tone}`} role={problem.tone === "wait" ? "status" : "alert"}>
          <span>{problem.text}</span>
          {onRetry && problem.tone !== "wait" && <button type="button" onClick={() => onRetry()}>Try again</button>}
        </div>
      ))}
    </div>
  );
}

/** Whether the client is connected, kept current. */
export function useConnected(client: ArchipelClient): boolean {
  return useSyncExternalStore((changed) => client.onConnectionChange(changed), () => client.connected, () => client.connected);
}

/** Says that the connection to the daemon is lost and being retried. Draws nothing while connected. */
export function ConnectionStatus({ connected }: { connected: boolean }) {
  if (connected) return null;
  return (
    <div className="arc-chat arc-notices">
      <div className="arc-notice arc-notice-wait" role="status">Connection lost. Reconnecting… What you see may be out of date.</div>
    </div>
  );
}

const compact = (count: number) => (count >= 1_000_000 ? `${(count / 1_000_000).toFixed(1)}M` : count >= 1000 ? `${Math.round(count / 1000)}k` : String(count));

/** What the session has used, as far as the agent says: `usage` of its values, and the last turn's tokens. */
export function usageOf(state: SessionState): { used?: number; size?: number; percent?: number; cost?: string; turnTokens?: number } {
  const usage = state.values.get("usage") as JsonObject | undefined;
  const turn = (state.values.get("state") as JsonObject | undefined)?.["usage"] as JsonObject | undefined;
  const cost = usage?.["cost"] as JsonObject | undefined;
  const used = typeof usage?.["used"] === "number" ? usage["used"] : undefined;
  const size = typeof usage?.["size"] === "number" && usage["size"] > 0 ? usage["size"] : undefined;
  const amount = typeof cost?.["amount"] === "number" ? cost["amount"] : undefined;
  const currency = typeof cost?.["currency"] === "string" ? cost["currency"] : "";
  return {
    used, size,
    percent: used !== undefined && size !== undefined ? Math.round((used / size) * 100) : undefined,
    // Small amounts keep their cents' cents: a turn often costs less than a cent.
    cost: amount === undefined ? undefined : `${currency === "USD" ? "$" : ""}${amount < 1 ? amount.toFixed(amount < 0.01 ? 4 : 2) : amount.toFixed(2)}${currency && currency !== "USD" ? ` ${currency}` : ""}`,
    turnTokens: typeof turn?.["totalTokens"] === "number" ? turn["totalTokens"] : undefined,
  };
}

/** How full the context is, and what the session has cost when the agent says. */
export function Usage({ state }: { state: SessionState }) {
  const usage = usageOf(state);
  if (usage.percent === undefined && usage.cost === undefined) return null;
  const detail = [
    usage.used !== undefined && usage.size !== undefined ? `${compact(usage.used)} of ${compact(usage.size)} tokens of context` : "",
    usage.turnTokens !== undefined ? `${compact(usage.turnTokens)} tokens in the last turn` : "",
  ].filter(Boolean).join(" · ");
  return (
    <span className="arc-status-usage" title={detail || undefined}>
      {usage.percent !== undefined && (
        <>
          <span className={`arc-meter${usage.percent >= 80 ? " arc-meter-high" : ""}`} role="meter" aria-valuenow={usage.percent} aria-valuemin={0} aria-valuemax={100} aria-label="Context used">
            <span style={{ width: `${Math.min(usage.percent, 100)}%` }} />
          </span>
          {usage.percent}% of context
        </>
      )}
      {usage.cost !== undefined && <span className="arc-cost">{usage.cost}</span>}
    </span>
  );
}

/** What still runs in the background after a turn ended: commands, tool calls, subagents. */
export function backgroundOf(state: SessionState): { kind: string; id: string; label: string }[] {
  const background = (state.values.get("state") as JsonObject | undefined)?.["background"];
  const named = (target: string, id: string, key: string) => {
    const label = (state.collections.get(target)?.get(id) as JsonObject | undefined)?.[key];
    return typeof label === "string" && label ? label : id;
  };
  return (Array.isArray(background) ? background : []).flatMap((entry) => {
    const work = entry as JsonObject | null;
    if (typeof work?.["terminalId"] === "string") return [{ kind: "command", id: work["terminalId"], label: named("terminal", work["terminalId"], "command") }];
    if (typeof work?.["toolCallId"] === "string") return [{ kind: "tool", id: work["toolCallId"], label: named("toolCall", work["toolCallId"], "title") }];
    if (typeof work?.["sessionId"] === "string") return [{ kind: "subagent", id: work["sessionId"], label: named("subagent", work["sessionId"], "title") }];
    return [];
  });
}

/** The newest todo list the agent keeps (a plan with entries), or null. */
export function todosOf(state: SessionState): { content: string; status: string }[] | null {
  const plans = [...(state.collections.get("plan")?.values() ?? [])].reverse();
  for (const plan of plans) {
    const entries = (plan as JsonObject | null)?.["entries"];
    if (Array.isArray(entries) && entries.length > 0) {
      return entries.map((entry) => ({ content: String((entry as JsonObject)["content"] ?? ""), status: String((entry as JsonObject)["status"] ?? "pending") }));
    }
  }
  return null;
}

/**
 * What goes on beside the conversation, kept in view above the composer: the agent's todo
 * list with its progress, and what still runs in the background. Draws nothing without either.
 */
export function Activity({ state }: { state: SessionState }) {
  const todos = todosOf(state);
  const background = backgroundOf(state);
  if (!todos && background.length === 0) return null;
  const done = todos?.filter((todo) => todo.status === "completed").length ?? 0;
  return (
    <div className="arc-activity">
      {todos && (
        <details className="arc-todos">
          <summary>
            <span className="arc-label">Tasks</span> {done} of {todos.length} done
            {todos.find((todo) => todo.status === "in_progress") && <span className="arc-todo-now"> · {todos.find((todo) => todo.status === "in_progress")!.content}</span>}
          </summary>
          <ul>
            {todos.map((todo, at) => (
              <li key={at} className={`arc-plan-${todo.status}`}>
                <span className="arc-plan-mark">{todo.status === "completed" ? "✓" : todo.status === "in_progress" ? "…" : "○"}</span>{todo.content}
              </li>
            ))}
          </ul>
        </details>
      )}
      {background.length > 0 && (
        <div className="arc-background">
          <span className="arc-label">Running in the background</span>
          {background.map((work) => <span key={work.id} className="arc-chip" title={work.kind}>{work.label}</span>)}
        </div>
      )}
    </div>
  );
}
