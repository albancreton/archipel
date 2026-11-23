// State rules: how each ACP message changes what a client holds.
//
// ACP v2 annotates its schema with `x-state`. The daemon's repository extracts those into
// `state-rules.json`; this package carries a copy. A client that follows the rules needs no
// code per message kind.

import pinned from "./generated/state-rules.json" with { type: "json" };

export type Action = "append" | "upsert" | "replace" | "remove";

/** One state change. Paths are dotted and relative to the message body. */
export interface StateRule {
  action: Action;
  /** The kind of thing changed, such as `agentMessage`, `toolCall` or `state`. */
  target: string;
  /** Where the entry's id is in the message. Absent when the target is a single value. */
  id?: string;
  /** The field of the target that is changed. Absent means the whole entry. */
  field?: string;
  /** Where the new data is in the message. Absent means the whole message body. */
  value?: string;
}

export interface Rules {
  session?: StateRule[];
  agent?: StateRule[];
}

export interface RuleSet {
  source: string;
  updates: Record<string, Rules>;
  requests: Record<string, Rules>;
  responses: Record<string, Rules>;
  notifications: Record<string, Rules>;
}

/** The rules of the ACP v2 revision this package was built for. */
export const RULES = pinned as unknown as RuleSet;
