import * as NodeFS from "node:fs";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Stream from "effect/Stream";
import * as Deferred from "effect/Deferred";
import * as DateTime from "effect/DateTime";
import { NodeId, ProviderInstanceId, ProviderSessionId, RunAttemptId, RunId, ThreadId } from "@t3tools/contracts";
import * as McpProviderSessions from "@t3tools/provider-core/server/McpProviderSessions";
import * as ProviderAdapter from "@t3tools/provider-core/server/ProviderAdapter";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as NodeCrypto from "@effect/platform-node/NodeCrypto";
import * as Schema from "effect/Schema";
import { ClaudeSettings } from "@t3tools/contracts";
import * as IdAllocator from "@t3tools/provider-core/server/IdAllocator";
import * as ProviderEventLoggers from "@t3tools/provider-core/server/ProviderEventLoggers";
import * as ClaudeAdapterV2 from "/work/t3code-minitel/apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts";
import * as ServerConfig from "/work/t3code-minitel/apps/server/src/config.ts";
import * as PortScanner from "/work/t3code-minitel/apps/server/src/preview/PortScanner.ts";
import * as ProviderInstallationGate from "/work/t3code-minitel/apps/server/src/provider/ProviderInstallationGate.ts";

const [fixturePath, mode, policyName, ...prompts] = process.argv.slice(2);
const raw = { scenario: "live" };
const meta = { model: process.env.SPIKE_MODEL ?? "haiku" } as Record<string, any>;
const turnPrompts: string[] = prompts;
const T0 = Date.now();

const INSTANCE = ProviderInstanceId.make("claudeAgent");
const THREAD = ThreadId.make("thread-spike");
const selection = { instanceId: INSTANCE, model: meta.model ?? "claude-sonnet-4-6" };
const POLICIES: Record<string, object> = {
  full: {},
  readonly_on_request: { approvalPolicy: "on-request", sandboxPolicy: { type: "readOnly", access: { type: "fullAccess" }, networkAccess: false } },
};
const runtimePolicy = ProviderAdapter.ProviderAdapterV2RuntimePolicy.make({
  runtimeMode: "full-access", interactionMode: "default", cwd: fixturePath!, ...POLICIES[policyName ?? "full"],
} as any);

const clip = (v: unknown, n = 110) => {
  const s = typeof v === "string" ? v : JSON.stringify(v);
  return s === undefined ? "" : s.length > n ? s.slice(0, n).replace(/\n/g, "⏎") + "…" : s.replace(/\n/g, "⏎");
};
const counts: Record<string, number> = {};
const itemTypes = new Set<string>();
function describe(event: any): string {
  switch (event.type) {
    case "turn_item.updated": {
      const i = event.turnItem ?? {};
      itemTypes.add(i.type);
      const body = i.text ?? i.command ?? i.title ?? i.input ?? i.output ?? i.detail ?? "";
      const sub = i.threadId !== THREAD ? " (child thread)" : "";
      return `item ${i.type} [${i.status}]${sub} ${clip(body, 80)}`;
    }
    case "message.updated": return `message ${event.message?.role} ${clip(event.message?.text, 80)}`;
    case "provider_turn.updated": return `turn #${event.providerTurn?.ordinal} [${event.providerTurn?.status}]`;
    case "provider_thread.updated": return `thread [${event.providerThread?.status}] native=${clip(event.providerThread?.nativeThreadRef?.nativeId, 40)}`;
    case "provider_session.updated": return `session [${event.providerSession?.status}]`;
    case "node.updated": return `node ${event.node?.kind} [${event.node?.status}]`;
    case "runtime_request.updated": { const r = event.runtimeRequest ?? event.request ?? {}; return `REQUEST ${r.kind} [${r.status}] cap=${r.responseCapability} ${clip(r.title ?? r.detail ?? r.payload, 70)}`; }
    case "subagent.updated": { const s = event.subagent ?? {}; return `SUBAGENT [${s.status}] ${clip(s.title ?? s.name ?? s.description ?? s.prompt, 70)}`; }
    case "turn.terminal": return `TERMINAL ${event.status} disposition=${event.threadDisposition}`;
    default: return clip(Object.keys(event).filter((k) => k !== "type"));
  }
}

const program = Effect.gen(function* () {
  const adapter = yield* ClaudeAdapterV2.createClaudeAdapterV2({
    instanceId: INSTANCE, displayName: undefined, environment: [], enabled: true, config: Schema.decodeSync(ClaudeSettings)({}),
  }, {});
  const caps = (yield* adapter.getCapabilities()) as any;
  console.log("adapter created; capability groups:", Object.keys(caps).length);

  const runtime = yield* adapter.openSession({
    threadId: THREAD, providerSessionId: ProviderSessionId.make("session-spike"), modelSelection: selection, runtimePolicy,
  });
  let terminal = yield* Deferred.make<void>();
  const sawCommand = yield* Deferred.make<any>();
  const samples: Record<string, any> = {};
  yield* runtime.events.pipe(
    Stream.runForEach((event: any) =>
      Effect.gen(function* () {
        counts[event.type] = (counts[event.type] ?? 0) + 1;
        const key = event.type === "turn_item.updated" ? `turn_item:${event.turnItem?.type}` : event.type;
        samples[key] = event;
        if (mode === "raw") console.log(JSON.stringify(event));
        else console.log(`  +${String(Date.now() - T0).padStart(5)}ms ${event.type.padEnd(24)} ${describe(event)}`);
        const item = event.turnItem;
        if (event.type === "turn_item.updated" && item?.type === "command_execution" && item.status === "running") yield* Deferred.succeed(sawCommand, event);
        if (event.type === "runtime_request.updated") {
          const r = event.runtimeRequest ?? event.request;
          if (r?.status === "pending") {
            console.log("\n> user: APPROVE");
            yield* runtime.respondToRuntimeRequest({ requestId: r.id, decision: "accept" }).pipe(Effect.forkScoped);
          }
        }
        if (event.type === "turn.terminal") yield* Deferred.succeed(terminal, undefined);
      }),
    ),
    Effect.forkScoped,
  );

  const title = `Replay fixture: ${raw.scenario}`;
  const providerThread = yield* runtime.ensureThread({ threadId: THREAD, title, modelSelection: selection, runtimePolicy });
  const now = yield* DateTime.now;
  const appThread: any = {
    createdBy: "user", creationSource: "web", id: THREAD, projectId: "project:spike", title,
    providerInstanceId: INSTANCE, modelSelection: selection, runtimeMode: "full-access", interactionMode: "default",
    branch: null, worktreePath: null, activeProviderThreadId: null,
    lineage: { parentThreadId: null, relationshipToParent: null, rootThreadId: THREAD },
    forkedFrom: null, createdAt: now, updatedAt: now, archivedAt: null, settledOverride: null, settledAt: null,
    lastVisitedAt: null, deletedAt: null,
  };
  console.log("session opened, thread ensured:", clip(providerThread.nativeThreadRef, 120));
  let ordinal = 0;
  for (const text of turnPrompts) {
    ordinal += 1;
    console.log(`\n> user: ${clip(text, 140)}`);
    const runId = RunId.make(`run:${THREAD}:${ordinal}`);
    yield* runtime.startTurn({
      appThread, threadId: THREAD, runId, runOrdinal: ordinal, providerTurnOrdinal: ordinal,
      attemptId: RunAttemptId.make(`attempt:${runId}:1`), rootNodeId: NodeId.make(`node:${runId}:root`),
      providerThread,
      message: { messageId: `message:${THREAD}:${ordinal}` as never, text, attachments: [], createdBy: "user", creationSource: "web" },
      modelSelection: selection, runtimePolicy,
    });
    if (mode === "interrupt") {
      const ev = yield* Deferred.await(sawCommand);
      console.log("\n> user: STOP");
      yield* runtime.interruptTurn({ providerThread, providerTurnId: ev.turnItem.providerTurnId });
    }
    yield* Deferred.await(terminal);
    terminal = yield* Deferred.make<void>();
  }
  console.log("\nevent counts:", JSON.stringify(counts));
  console.log("item types:", [...itemTypes].join(", "));
  if (mode === "samples") for (const [k, v] of Object.entries(samples)) console.log(`\n### ${k}\n${JSON.stringify(v, null, 1)}`);
});

const layers = Layer.mergeAll(
  ClaudeAdapterV2.layerQueryRunner.pipe(
    Layer.provide(PortScanner.layerTest),
    Layer.provide(Layer.succeed(ProviderEventLoggers.ProviderEventLoggers, { native: undefined, canonical: undefined } as any)),
  ),
  IdAllocator.layer, ProviderInstallationGate.layer, McpProviderSessions.layer,
  ServerConfig.layerTest(fixturePath!, { prefix: "claude-spike-" }),
).pipe(Layer.provideMerge(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer)));
Effect.runPromise(program.pipe(Effect.scoped, Effect.provide(layers as any), Effect.timeout("120 seconds")) as any).then(
  () => process.exit(0),
  (e) => { console.error("FAILED:", clip(e?.message ?? e, 600), clip(e, 2500)); process.exit(1); },
);
