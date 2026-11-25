// The smallest useful chat: one message to the agent, everything it does printed as it
// happens, approvals answered.
//
//   node chat.ts [--home DIR] [--cwd DIR] [--mode MODE] [--approve OPTION] "your message"
//
// Free, against a fake agent that replays a recorded session (after `cargo build`):
//
//   target/debug/archipeld --home /tmp/chat-home --fake testdata/transcripts/claude-v2/approval_command.ndjson &
//   node examples/node/chat.ts --home /tmp/chat-home --mode default --approve allow-once "anything"
//
// The parts an app needs are all here: connect, make or open a session, follow its items,
// send a message, answer what the agent asks. A UI replaces `print` with drawing: the React
// and SwiftUI kits do exactly that (`ui/react/demo/live.tsx`, `ui/swift/Demo`).

import { ArchipelClient, unixSocketTransport, type Json, type Session } from "@archipel/client";
import path from "node:path";
import { fileURLToPath } from "node:url";

export interface ChatOptions {
  /** The daemon's local socket. */
  socket: string;
  /** The folder the agent works in. */
  cwd: string;
  message: string;
  /** A permission mode to set first, such as `default`. Without it the harness's own. */
  mode?: string;
  /** The option to answer approvals with, such as `allow-once`. Without it they are refused. */
  approve?: string;
  print?: (line: string) => void;
}

/** The text of a message item: its content blocks joined. */
function textOf(item: Json): string {
  const content = (item as { content?: { type: string; text?: string }[] } | null)?.content ?? [];
  return content.map((block) => block.text ?? `[${block.type}]`).join("");
}

/** One line for an item, by its kind. Kinds this does not know are named, not dropped. */
function describe(target: string, item: Json): string {
  const fields = (item ?? {}) as Record<string, Json>;
  switch (target) {
    case "userMessage": return `you: ${textOf(item)}`;
    case "agentMessage": return `agent: ${textOf(item)}`;
    case "agentThought": return `(thinking) ${textOf(item).trim()}`;
    case "toolCall": return `tool ${String(fields["title"] ?? fields["name"])}: ${String(fields["status"])}`;
    default: return `[${target}]`;
  }
}

/** Runs one turn and resolves with the session when the agent is done. */
export async function chat(options: ChatOptions): Promise<Session> {
  const print = options.print ?? console.log;
  const client = await ArchipelClient.connect({
    open: () => unixSocketTransport(options.socket),
    name: "example-chat",
  });
  const session = await client.newSession({ cwd: options.cwd });
  if (options.mode) await session.setConfigOption("mode", options.mode);

  // Items change while they stream: a message grows, a tool call goes from pending to
  // completed. So this prints an item when its line changes, not once per update.
  const printed = new Map<string, string>();
  const answered = new Set<string>();
  const stop = session.subscribe(() => {
    for (const { ref, item } of session.items()) {
      if (ref.target === "request" || ref.target === "terminal") continue;
      const line = describe(ref.target, item as Json);
      // Text that is still streaming is printed when the turn ends, below.
      const streaming = ref.target === "agentMessage" || ref.target === "agentThought";
      if (!streaming && printed.get(`${ref.target}:${ref.id}`) !== line) {
        printed.set(`${ref.target}:${ref.id}`, line);
        print(line);
      }
    }
    // What the agent asks. Any app attached to the session may answer; the first answer counts.
    for (const { requestId, request } of session.pendingRequests()) {
      if (answered.has(requestId)) continue;
      answered.add(requestId);
      const title = String((request.params as { title?: string } | null)?.title ?? request.method);
      if (request.method === "session/request_permission" && options.approve) {
        print(`approval: ${title} -> ${options.approve}`);
        session.approve(requestId, options.approve);
      } else {
        print(`refused: ${title}`);
        session.answer(requestId, { outcome: { outcome: "cancelled" } });
      }
    }
  });

  // `prompt` resolves when the agent has taken the message. The turn is over when the
  // state is back to idle.
  await session.prompt(options.message);
  await session.until(() => session.turnState === "idle");
  stop();
  for (const { ref, item } of session.items()) {
    if (ref.target === "agentMessage" || ref.target === "agentThought") print(describe(ref.target, item as Json));
  }
  client.close();
  return session;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name: string): string | undefined => {
    const at = args.indexOf(name);
    return at < 0 ? undefined : args.splice(at, 2)[1];
  };
  const home = option("--home") ?? path.join(process.env["HOME"] ?? "", ".archipel");
  const cwd = option("--cwd") ?? process.cwd();
  const [mode, approve] = [option("--mode"), option("--approve")];
  const message = args.join(" ");
  if (!message) {
    console.error('usage: node chat.ts [--home DIR] [--cwd DIR] [--mode MODE] [--approve OPTION] "your message"');
    process.exit(2);
  }
  await chat({ socket: path.join(home, "daemon.sock"), cwd, message, mode, approve });
}
