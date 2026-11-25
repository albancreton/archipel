// Spike: run T3's Pi adapter with no orchestrator, no server, no database.
import * as NodeFS from "node:fs";
import * as NodeServices from "@effect/platform-node/NodeServices";
import {
  ProviderReplayEntry,
  ProviderInstanceId,
  ProviderSessionId,
  ThreadId,
  RunId,
  RunAttemptId,
  NodeId,
  type ProviderReplayEntry as ProviderReplayEntryType,
} from "@t3tools/contracts";
import * as Cause from "effect/Cause";
import * as DateTime from "effect/DateTime";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Queue from "effect/Queue";
import * as Schema from "effect/Schema";
import * as Sink from "effect/Sink";
import * as Stream from "effect/Stream";
import { ChildProcess, ChildProcessSpawner } from "effect/process";
import * as TestProviderHost from "@t3tools/provider-testing/TestProviderHost";
import * as IdAllocator from "@t3tools/provider-core/server/IdAllocator";
import * as McpProviderSessions from "@t3tools/provider-core/server/McpProviderSessions";
import * as ProviderAdapter from "@t3tools/provider-core/server/ProviderAdapter";
import { ProviderAdapterDriverCreateError } from "@t3tools/provider-core/server/adapterDriver";
import { PI_PROVIDER, PiAdapterV2Driver } from "/work/t3code-minitel/packages/provider-pi/src/server.ts";
export const PI_RPC_REPLAY_PROTOCOL = "pi.rpc-jsonl";
export const PI_REPLAY_ANY = "<any>";

const PiRpcReplayTranscript = Schema.Struct({
  provider: Schema.Literal(PI_PROVIDER),
  protocol: Schema.Literal(PI_RPC_REPLAY_PROTOCOL),
  version: Schema.String,
  scenario: Schema.String,
  metadata: Schema.optional(Schema.Record(Schema.String, Schema.Unknown)),
  entries: Schema.Array(ProviderReplayEntry),
});
type PiRpcReplayTranscript = typeof PiRpcReplayTranscript.Type;
const decodePiRpcReplayTranscript = Schema.decodeUnknownEffect(PiRpcReplayTranscript);

class PiReplayTranscriptDecodeError extends Schema.TaggedError<PiReplayTranscriptDecodeError>()(
  "PiReplayTranscriptDecodeError",
  {
    driver: Schema.optional(Schema.String),
    protocol: Schema.optional(Schema.String),
    scenario: Schema.optional(Schema.String),
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to decode Pi RPC replay transcript for scenario ${this.scenario ?? "<unknown>"}.`;
  }
}

class PiReplayMismatchError extends Schema.TaggedError<PiReplayMismatchError>()(
  "PiReplayMismatchError",
  {
    scenario: Schema.String,
    cursor: Schema.Number,
    process: Schema.Number,
    label: Schema.optional(Schema.String),
    actualType: Schema.optional(Schema.String),
    expected: Schema.Unknown,
    actual: Schema.Unknown,
  },
) {
  override get message(): string {
    return `Pi replay frame mismatch at cursor ${this.cursor} (expected ${this.label ?? "<none>"}, received ${this.actualType ?? "<untyped>"} from process ${this.process}) in scenario ${this.scenario}.`;
  }
}

class PiReplayIncompleteError extends Schema.TaggedError<PiReplayIncompleteError>()(
  "PiReplayIncompleteError",
  {
    scenario: Schema.String,
    cursor: Schema.Number,
    remaining: Schema.Number,
    nextLabel: Schema.optional(Schema.String),
  },
) {
  override get message(): string {
    return `Pi replay ended with ${this.remaining} unconsumed entries at cursor ${this.cursor} (next ${this.nextLabel ?? "<none>"}) in scenario ${this.scenario}.`;
  }
}

function entryLabel(entry: ProviderReplayEntryType | undefined): string | undefined {
  if (entry === undefined) return undefined;
  return entry.type === "runtime_exit" ? "runtime_exit" : (entry.label ?? entry.type);
}

const PiOrchestratorReplayHarnessError = Schema.Union([
  PiReplayTranscriptDecodeError,
  ProviderAdapterDriverCreateError,
]);
type PiOrchestratorReplayHarnessError = typeof PiOrchestratorReplayHarnessError.Type;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function replayValueMatches(expected: unknown, actual: unknown): boolean {
  if (expected === PI_REPLAY_ANY) return true;
  if (Array.isArray(expected)) {
    return (
      Array.isArray(actual) &&
      expected.length === actual.length &&
      expected.every((entry, index) => replayValueMatches(entry, actual[index]))
    );
  }
  if (isRecord(expected)) {
    if (!isRecord(actual)) return false;
    const keys = new Set([...Object.keys(expected), ...Object.keys(actual)]);
    return [...keys].every((key) => replayValueMatches(expected[key], actual[key]));
  }
  return Object.is(expected, actual);
}

const ADAPTER_REQUEST_ID = /^t3-(?:prompt-)?\d+$/u;

function adapterRequestId(frame: unknown): string | undefined {
  const id = isRecord(frame) ? frame.id : undefined;
  return typeof id === "string" && ADAPTER_REQUEST_ID.test(id) ? id : undefined;
}

/** Rebind adapter request ids without pinning counters; provider ids that responses must echo stay exact. */
function withRequestId(expected: unknown, actual: unknown): unknown {
  const actualId = adapterRequestId(actual);
  return isRecord(expected) &&
    actualId !== undefined &&
    (adapterRequestId(expected) !== undefined ||
      (expected.type === "prompt" && expected.id === undefined))
    ? { ...expected, id: actualId }
    : expected;
}

function startsTurn(entry: ProviderReplayEntryType): boolean {
  if (entry.type !== "expect_outbound") return false;
  const type = isRecord(entry.frame) ? entry.frame.type : undefined;
  return type === "prompt" || type === "compact" || type === "process_start";
}

function processStartFrame(command: ChildProcess.Command) {
  const args = ChildProcess.isStandardCommand(command) ? command.args : [];
  return { type: "process_start", args: [...args] };
}

function entryProcess(entry: ProviderReplayEntryType): number {
  const label = entry.type === "runtime_exit" ? undefined : entry.label;
  const match = label === undefined ? null : /@p(\d+)$/u.exec(label);
  return match === null ? 1 : Number(match[1]);
}

function recordLabel(frame: unknown, process: number): string | undefined {
  const base = !isRecord(frame)
    ? undefined
    : frame.type === "response" && typeof frame.command === "string"
      ? `response:${frame.command}`
      : typeof frame.type === "string"
        ? frame.type
        : undefined;
  if (base === undefined || process === 1) return base;
  return `${base}@p${process}`;
}

const decodeJsonLine = Schema.decodeUnknownSync(Schema.fromJsonString(Schema.Unknown));
const encodeJsonLine = Schema.encodeSync(Schema.fromJsonString(Schema.Unknown));

function makeLineSplitter() {
  let buffer = "";
  const decoder = new TextDecoder();
  return (chunk: Uint8Array): ReadonlyArray<string> => {
    buffer += decoder.decode(chunk, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    return lines.map((line) => line.replace(/\r$/u, "")).filter((line) => line.length > 0);
  };
}

interface PiReplayProcess {
  readonly ordinal: number;
  readonly stdout: Queue.Queue<Uint8Array, Cause.Done>;
  readonly ids: Map<string, string>;
  open: boolean;
}

/** Adapter fibers can reorder stdin writes; hold matches within the turn until reached, and fail unmatched writes. */
class PiReplayController {
  private cursor = 0;
  private failure: unknown = null;
  private readonly consumed = new Set<number>();
  private readonly held: Array<{ readonly process: PiReplayProcess; readonly actual: unknown }> =
    [];
  private readonly processes: Array<PiReplayProcess> = [];
  private readonly transcript: PiRpcReplayTranscript;

  constructor(transcript: PiRpcReplayTranscript) {
    this.transcript = transcript;
  }

  spawn(
    command: ChildProcess.Command,
    stdout: Queue.Queue<Uint8Array, Cause.Done>,
  ): PiReplayProcess {
    const process: PiReplayProcess = {
      ordinal: this.processes.length + 1,
      stdout,
      ids: new Map(),
      open: true,
    };
    this.processes.push(process);
    this.receive(process, processStartFrame(command));
    return process;
  }

  receive(process: PiReplayProcess, actual: unknown): void {
    if (this.failure !== null) return;
    const entries = this.transcript.entries;
    let possible = false;
    for (let index = this.cursor; index < entries.length && !possible; index += 1) {
      const entry = entries[index]!;
      const pending = entry.type === "expect_outbound" && !this.consumed.has(index);
      possible =
        pending &&
        entryProcess(entry) === process.ordinal &&
        replayValueMatches(withRequestId(entry.frame, actual), actual);
      if (!possible && pending && startsTurn(entry)) break;
    }
    if (!possible) {
      const expected = entries[this.cursor];
      const label = entryLabel(expected);
      const actualType = isRecord(actual) ? actual.type : undefined;
      this.fail(
        new PiReplayMismatchError({
          scenario: this.transcript.scenario,
          cursor: this.cursor,
          process: process.ordinal,
          ...(label === undefined ? {} : { label }),
          ...(typeof actualType === "string" ? { actualType } : {}),
          expected: expected ?? null,
          actual,
        }),
      );
      return;
    }
    this.held.push({ process, actual });
    this.advance();
  }

  close(process: PiReplayProcess): void {
    process.open = false;
    Queue.endUnsafe(process.stdout);
  }

  assertComplete(): void {
    if (this.failure !== null) throw this.failure;
    if (this.cursor !== this.transcript.entries.length || this.held.length > 0) {
      const nextLabel = entryLabel(this.transcript.entries[this.cursor]);
      throw new PiReplayIncompleteError({
        scenario: this.transcript.scenario,
        cursor: this.cursor,
        remaining: this.transcript.entries.length - this.cursor,
        ...(nextLabel === undefined ? {} : { nextLabel }),
      });
    }
  }

  private advance(): void {
    let progressed = true;
    while (progressed && this.failure === null) {
      progressed = this.drainInbound();
      for (const [heldIndex, write] of this.held.entries()) {
        if (this.consumePending(write.process, write.actual)) {
          this.held.splice(heldIndex, 1);
          progressed = true;
          break;
        }
      }
    }
  }

  private consumePending(process: PiReplayProcess, actual: unknown): boolean {
    const entries = this.transcript.entries;
    for (let index = this.cursor; index < entries.length; index += 1) {
      const entry = entries[index]!;
      if (entry.type !== "expect_outbound") return false;
      if (this.consumed.has(index) || entryProcess(entry) !== process.ordinal) continue;
      if (!replayValueMatches(withRequestId(entry.frame, actual), actual)) continue;
      const recordedId = adapterRequestId(entry.frame);
      const actualId = adapterRequestId(actual);
      if (recordedId !== undefined && actualId !== undefined) {
        process.ids.set(recordedId, actualId);
      }
      this.consumed.add(index);
      while (this.consumed.has(this.cursor)) this.cursor += 1;
      return true;
    }
    return false;
  }

  private drainInbound(): boolean {
    let emitted = false;
    while (this.failure === null) {
      const entry = this.transcript.entries[this.cursor];
      if (entry?.type === "runtime_exit") {
        this.cursor += 1;
        emitted = true;
        const process = this.processes.at(-1);
        if (process !== undefined) this.close(process);
        continue;
      }
      if (entry?.type !== "emit_inbound") return emitted;
      const process = this.processes[entryProcess(entry) - 1];
      if (process === undefined) return emitted;
      this.cursor += 1;
      emitted = true;
      if (!process.open) continue;
      const frame = entry.frame;
      const recordedId = isRecord(frame) ? frame.id : undefined;
      const reboundId = typeof recordedId === "string" ? process.ids.get(recordedId) : undefined;
      const rebound =
        isRecord(frame) && reboundId !== undefined ? { ...frame, id: reboundId } : frame;
      Queue.offerUnsafe(process.stdout, new TextEncoder().encode(`${encodeJsonLine(rebound)}\n`));
    }
    return emitted;
  }

  private fail(cause: unknown): void {
    if (this.failure !== null) return;
    this.failure = cause;
    for (const process of this.processes) Queue.endUnsafe(process.stdout);
  }
}

/** Deliberately outside the valid pid range so PiRpc's group kill never lands. */
const REPLAY_PID = 999_999_999;

function makePiReplaySpawner(
  controller: PiReplayController,
): ChildProcessSpawner.ChildProcessSpawner["Service"] {
  return ChildProcessSpawner.make((command) =>
    Effect.gen(function* () {
      const process = controller.spawn(command, yield* Queue.unbounded<Uint8Array, Cause.Done>());
      yield* Effect.addFinalizer(() => Effect.sync(() => controller.close(process)));
      const split = makeLineSplitter();
      return ChildProcessSpawner.makeHandle({
        pid: ChildProcessSpawner.ProcessId(REPLAY_PID),
        exitCode: Effect.never,
        isRunning: Effect.succeed(true),
        kill: () => Effect.void,
        unref: Effect.succeed(Effect.void),
        stdin: Sink.forEach((chunk: Uint8Array) =>
          Effect.sync(() => {
            for (const line of split(chunk)) controller.receive(process, decodeJsonLine(line));
          }),
        ),
        stdout: Stream.fromQueue(process.stdout),
        stderr: Stream.empty,
        all: Stream.empty,
        getInputFd: () => Sink.drain,
        getOutputFd: () => Stream.empty,
      });
    }),
  );
}

// ---- spike driver ----
const [fixturePath, mode, ...prompts] = process.argv.slice(2);
const lines = NodeFS.readFileSync(fixturePath!, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const [header, ...entries] = lines;
const transcript = { ...header, entries } as PiRpcReplayTranscript;

const INSTANCE = ProviderInstanceId.make("pi");
const THREAD = ThreadId.make("thread-spike");
const selection = { instanceId: INSTANCE, model: metadataString(transcript, "model") || "default" };
const runtimePolicy = ProviderAdapter.ProviderAdapterV2RuntimePolicy.make({
  runtimeMode: "full-access",
  interactionMode: "default",
  cwd: null,
});

const clip = (v: unknown, n = 110) => {
  const s = typeof v === "string" ? v : JSON.stringify(v);
  return s === undefined ? "" : s.length > n ? s.slice(0, n) + "…" : s;
};
const counts: Record<string, number> = {};
function describe(event: any): string {
  switch (event.type) {
    case "turn_item.updated": {
      const i = event.turnItem ?? event.item ?? {};
      const body = i.text ?? i.command ?? i.title ?? i.output ?? i.detail ?? "";
      return `item ${i.type} [${i.status}] ${clip(body, 90)}`;
    }
    case "message.updated": return `message ${event.message?.role} ${clip(event.message?.text, 80)}`;
    case "provider_turn.updated": return `turn #${event.providerTurn?.ordinal} [${event.providerTurn?.status}] native=${clip(event.providerTurn?.nativeTurnRef?.nativeId, 40)}`;
    case "provider_thread.updated": return `thread [${event.providerThread?.status}] native=${clip(event.providerThread?.nativeThreadRef?.nativeId, 60)}`;
    case "provider_session.updated": return `session [${event.providerSession?.status}] model=${event.providerSession?.model}`;
    case "node.updated": return `node ${event.node?.kind} [${event.node?.status}]`;
    case "turn.terminal": return `TERMINAL ${event.status} disposition=${event.threadDisposition}`;
    default: return clip(Object.keys(event).filter((k) => k !== "type"));
  }
}

const program = Effect.gen(function* () {
  const controller = new PiReplayController(transcript);
  const adapter = yield* PiAdapterV2Driver.create({
    instanceId: INSTANCE,
    displayName: undefined,
    enabled: true,
    environment: [],
    config: { ...PiAdapterV2Driver.defaultConfig(), binaryPath: "pi", launchArgs: metadataString(transcript, "launchArgs") },
  }).pipe(Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, makePiReplaySpawner(controller)));

  const caps = (yield* adapter.getCapabilities()) as any;
  console.log("capabilities groups:", Object.keys(caps).join(", "));

  const runtime = yield* adapter.openSession({
    threadId: THREAD,
    providerSessionId: ProviderSessionId.make("session-spike"),
    modelSelection: selection,
    runtimePolicy,
  });
  let terminal = yield* Deferred.make<void>();
  let sawCommand = yield* Deferred.make<any>();
  yield* runtime.events.pipe(
    Stream.runForEach((event: any) =>
      Effect.gen(function* () {
        counts[event.type] = (counts[event.type] ?? 0) + 1;
        if (mode === "raw") console.log(JSON.stringify(event));
        else console.log(`  ${event.type.padEnd(26)} ${describe(event)}`);
        const item = event.turnItem ?? event.item;
        if (event.type === "turn_item.updated" && item?.type === "command_execution") yield* Deferred.succeed(sawCommand, event);
        if (event.type === "turn.terminal") yield* Deferred.succeed(terminal, undefined);
      }),
    ),
    Effect.forkScoped,
  );

  const providerThread = yield* runtime.ensureThread({ threadId: THREAD, title: `Replay fixture: ${transcript.scenario}`, modelSelection: selection, runtimePolicy });
  const now = yield* DateTime.now;
  const appThread: any = {
    createdBy: "user", creationSource: "web", id: THREAD, projectId: "project:spike", title: `Replay fixture: ${transcript.scenario}`,
    providerInstanceId: INSTANCE, modelSelection: selection, runtimeMode: "full-access", interactionMode: "default",
    branch: null, worktreePath: null, activeProviderThreadId: null,
    lineage: { parentThreadId: null, relationshipToParent: null, rootThreadId: THREAD },
    forkedFrom: null, createdAt: now, updatedAt: now, archivedAt: null, settledOverride: null, settledAt: null,
    lastVisitedAt: null, deletedAt: null,
  };
  let ordinal = 0;
  for (const text of prompts) {
    ordinal += 1;
    console.log(`\n> user: ${text}`);
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
      const turnId = (ev.turnItem ?? ev.item).providerTurnId;
      yield* runtime.interruptTurn({ providerThread, providerTurnId: turnId });
    }
    yield* Deferred.await(terminal);
    terminal = yield* Deferred.make<void>();
  }
  console.log("\nevent counts:", JSON.stringify(counts));
  try { controller.assertComplete(); console.log("replay: transcript fully consumed"); }
  catch (e: any) { console.log("replay:", e.message); }
});

const layers = Layer.mergeAll(
  NodeServices.layer,
  IdAllocator.layer,
  McpProviderSessions.layer,
  TestProviderHost.layer().pipe(Layer.provide(NodeServices.layer)),
);
Effect.runPromise(program.pipe(Effect.scoped, Effect.provide(layers), Effect.timeout("30 seconds")) as any).then(
  () => process.exit(0),
  (e) => { console.error("FAILED:", e?.message ?? e, clip(e, 1500)); process.exit(1); },
);
function metadataString(t: PiRpcReplayTranscript, key: string): string {
  const v = t.metadata?.[key];
  return typeof v === "string" ? v : "";
}
