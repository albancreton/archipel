// The scenarios. Each one does something with the real harness through the daemon and notes
// what it saw. The wording of a model's answer varies, so nothing here depends on it beyond a
// marker word the prompt asks for.

import { base64Text, RpcError, type Json, type JsonObject, type Session } from "@harness-bridge/client";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { count, current, here, RpcFailure, said, settingOf, settings, settingsOf, solidPng, turn, within, type Run } from "./lib.ts";

export interface Scenario {
  name: string;
  /** What question it answers, for the report. */
  about: string;
  agents: string[];
  /** `turn`: it spends at least one model turn. */
  cost: "free" | "turn";
  run(t: Run, shared: Map<string, Json>): Promise<void>;
}

const OK = "Reply with exactly: ok";
const failure = (error: unknown): string =>
  error instanceof RpcError || error instanceof RpcFailure ? `error ${error.code}` : `error: ${error instanceof Error ? error.message : String(error)}`;
/** The result of a call, as a fact: `ok`, or the error's code. */
const attempt = async (work: Promise<unknown>): Promise<string> => work.then(() => "ok", failure);
/** A value of the setting other than the current one, `preferred` if it is there. */
const another = (values: string[], now: Json, preferred: string): string | undefined =>
  values.includes(preferred) && now !== preferred ? preferred : values.find((value) => value !== now);

/**
 * Sets a model, an effort and a mode, restarts the daemon and looks at what the sessions
 * continue with. `addons` switches addons for the restarted daemon.
 */
async function reopen(t: Run, shared: Map<string, unknown>, addons: Record<string, boolean>): Promise<void> {
    const app = await t.app();
    // Three sessions, so that each choice is judged alone. `untouched` tells "the choice is
    // lost" from "the default is lost too". Plan mode is left out: it can change the model.
    const sessions = { untouched: await t.session(app), model: await t.session(app), effortAndMode: await t.session(app) };
    const chosen: Record<keyof typeof sessions, Record<string, string>> = { untouched: {}, model: {}, effortAndMode: {} };
    const choose = async (label: keyof typeof sessions, id: string, value: string) => {
      await sessions[label].setConfigOption(id, value);
      chosen[label][id] = value;
    };
    if (t.agent.otherCheapModel && settingOf(sessions.model, "model")?.values.includes(t.agent.otherCheapModel)) {
      await choose("model", "model", t.agent.otherCheapModel);
    }
    const effort = settingOf(sessions.effortAndMode, "thought_level");
    if (effort) await choose("effortAndMode", effort.id, another(effort.values, effort.current, "low")!);
    const mode = settingOf(sessions.effortAndMode, "mode");
    if (mode?.values.includes("acceptEdits")) await choose("effortAndMode", mode.id, "acceptEdits");

    const ids = [...settings(sessions.untouched).values()].filter((setting) => setting.type === "select").map((setting) => setting.id);
    const snapshot = (of: Session) => Object.fromEntries(ids.map((id) => [id, current(of, id)]));
    const labels = (Object.keys(sessions) as (keyof typeof sessions)[]).filter((label) => label === "untouched" || Object.keys(chosen[label]).length > 0);
    const before: Record<string, { shown: Json; used: Json }> = {};
    for (const label of labels) {
      const first = await turn(sessions[label], OK);
      before[label] = { shown: snapshot(sessions[label]), used: first.model };
      t.must(`the ${label} session shows its choices`, Object.entries(chosen[label]).every(([id, value]) => current(sessions[label], id) === value));
    }

    await t.restartDaemon(addons);
    const reopened = await t.app();
    for (const label of labels) {
      const again = await reopened.session(sessions[label].id);
      const shownAfterRestart = snapshot(again);
      const second = await turn(again, OK);
      t.fact(label, {
        chosen: chosen[label],
        shownBefore: before[label]!.shown,
        shownAfterRestart,
        shownAfterWake: snapshot(again),
        kept: Object.fromEntries(Object.entries(chosen[label]).map(([id, value]) => [id, current(again, id) === value])),
        modelUsedBefore: before[label]!.used,
        modelUsedAfter: second.model,
      });
      t.info(`${label}FirstTurnAfterReopenMs`, second.ms);
      t.must(`the ${label} session answers after the restart`, count(again, "agentMessage") >= 2);
    }
    shared.set("reopened", { sessionId: sessions.effortAndMode.id, chosen: chosen.effortAndMode });
}

export const scenarios: Scenario[] = [
  {
    name: "settings.listed",
    about: "Which settings does a new session report, before any message?",
    agents: ["claude", "pi"],
    cost: "free",
    async run(t) {
      const session = await t.session(await t.app());
      const all = [...settings(session).values()];
      t.fact("settings", all.map(({ id, category, type }) => ({ id, category, type })));
      t.must("a new session reports a model setting with choices", (settingOf(session, "model")?.values.length ?? 0) > 0);
      t.fact("effortValues", settingOf(session, "thought_level")?.values ?? null);
      t.fact("modeValues", settingOf(session, "mode")?.values ?? null);
      t.info("models", settingOf(session, "model")?.values ?? null);
      t.info("commands", ((session.state.values.get("availableCommands") as Json[] | undefined) ?? []).length);
    },
  },
  {
    name: "settings.switch",
    about: "Does a changed setting show in the session, and do the effort choices depend on the model?",
    agents: ["claude", "pi"],
    cost: "free",
    async run(t) {
      const session = await t.session(await t.app());
      const effort = settingOf(session, "thought_level");
      if (effort) {
        const value = another(effort.values, effort.current, "low")!;
        await session.setConfigOption(effort.id, value);
        t.fact("effortChangeShows", current(session, effort.id) === value);
      }
      const mode = settingOf(session, "mode");
      if (mode?.values.includes("plan")) {
        await session.setConfigOption(mode.id, "plan");
        t.fact("modeChangeShows", current(session, mode.id) === "plan");
        // Claude Code may run plan mode on another model than the one selected.
        t.fact("modelShownInPlanMode", current(session, "model") === t.agent.cheapModel ? "the selected one" : current(session, "model"));
        await session.setConfigOption(mode.id, String(mode.current));
      }
      // Only where a second model can be selected without spending anything.
      if (t.agent.otherCheapModel && effort) {
        const byModel: Record<string, Json> = {};
        for (const model of [t.agent.cheapModel!, t.agent.otherCheapModel, "sonnet", "opus"]) {
          if (!settingOf(session, "model")?.values.includes(model)) continue;
          await session.setConfigOption("model", model);
          t.must(`the model setting shows ${model}`, current(session, "model") === model);
          byModel[model] = settingOf(session, "thought_level")?.values ?? null;
        }
        t.fact("effortValuesByModel", byModel);
      }
    },
  },
  {
    name: "settings.rejects_unknown",
    about: "What happens when an app sets a value the agent does not offer?",
    agents: ["claude", "pi"],
    cost: "free",
    async run(t) {
      const session = await t.session(await t.app());
      const effort = settingOf(session, "thought_level");
      if (effort) t.fact("unknownEffort", await attempt(session.setConfigOption(effort.id, "no-such-value")));
      const mode = settingOf(session, "mode");
      if (mode) {
        // A mode Claude Code has in its terminal and the adapter does not list.
        t.fact("modeDontAsk", await attempt(session.setConfigOption(mode.id, "dontAsk")));
        t.fact("modeAfterwards", current(session, mode.id) === mode.current ? "unchanged" : current(session, mode.id));
      }
      t.fact("unknownSetting", await attempt(session.setConfigOption("no-such-setting", "x")));
      // The harness by itself, without the addon `pi.check-setting-values`. When this starts
      // to be an error for Pi, the addon can go.
      if (effort) {
        try {
          await t.restartDaemon({ "pi.check-setting-values": false });
          const unaided = await t.session(await t.app());
          t.fact("unknownEffortUnaided", await attempt(unaided.setConfigOption(effort.id, "no-such-value")));
        } finally {
          await t.restartDaemon();
        }
      }
    },
  },
  {
    name: "reopen.keeps_settings",
    about: "After the daemon restarts, does a session continue with the model, effort and mode the user chose?",
    agents: ["claude", "pi"],
    cost: "turn",
    run: (t, shared) => reopen(t, shared, {}),
  },
  {
    name: "reopen.keeps_settings.unaided",
    about: "The same without the addon `acp.restore-settings`: what the harness does by itself. When this starts to keep the settings, the addon can go.",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t, shared) {
      try {
        await reopen(t, new Map(shared), { "acp.restore-settings": false });
      } finally {
        await t.restartDaemon();
      }
    },
  },
  {
    name: "session.empty",
    about: "Does a session that never got a message leave an entry in the harness's own list?",
    agents: ["claude"],
    cost: "free",
    async run(t) {
      const session = await t.session(await t.app());
      const adapter = await t.adapter();
      const listed = (await adapter.call("session/list", { cwd: t.work })) as { sessions?: { sessionId: string }[] };
      t.fact("listedByTheHarness", (listed.sessions ?? []).some((other) => other.sessionId === session.id));
    },
  },
  {
    name: "session.options",
    about: "Can the settings be read without a session, and for an existing session without opening it?",
    agents: ["claude"],
    cost: "free",
    async run(t, shared) {
      const adapter = await t.adapter();
      t.fact("advertised", (adapter.welcome as JsonObject | null)?.["capabilities"] !== undefined
        && ((adapter.welcome as { capabilities: { session?: JsonObject } }).capabilities.session?.["options"] !== undefined));
      let started = Date.now();
      const fresh = await adapter.call("session/options", { cwd: t.work }).catch(failure);
      t.info("withoutSessionMs", Date.now() - started);
      if (typeof fresh === "string") {
        t.fact("withoutSession", fresh);
      } else {
        t.fact("withoutSession", [...settingsOf((fresh as JsonObject)["configOptions"] ?? null).keys()]);
        t.fact("withoutSessionHasCommands", (((fresh as JsonObject)["availableCommands"] as Json[] | undefined) ?? []).length > 0);
      }
      const reopened = shared.get("reopened") as { sessionId: string; chosen: Record<string, string> } | undefined;
      // Needs the session `reopen.keeps_settings` leaves behind; left out of a partial run.
      if (reopened) {
        started = Date.now();
        const existing = await adapter.call("session/options", { cwd: t.work, sessionId: reopened.sessionId }).catch(failure);
        t.info("forAnExistingSessionMs", Date.now() - started);
        if (typeof existing === "string") {
          t.fact("forAnExistingSession", existing);
        } else {
          const reported = settingsOf((existing as JsonObject)["configOptions"] ?? null);
          t.fact("forAnExistingSession", Object.fromEntries(
            Object.entries(reopened.chosen).map(([id, value]) => [id, reported.get(id)?.current === value ? "the user's choice" : `other: ${String(reported.get(id)?.current)}`]),
          ));
        }
      }
      // What the daemon does with the call today.
      const app = await t.app();
      t.fact("throughTheDaemon", await attempt(app.sessionOptions({ cwd: t.work, agentId: t.id })));
      if (reopened) t.fact("throughTheDaemonForAnExistingSession", await attempt(app.sessionOptions({ cwd: t.work, sessionId: reopened.sessionId })));
    },
  },
  {
    name: "session.initial_settings",
    about: "Does a new session start with the settings chosen before it existed (`configOptions` of `session/new`)?",
    agents: ["claude", "pi"],
    cost: "free",
    async run(t) {
      const app = await t.app();
      const offered = await app.sessionOptions({ cwd: t.work, agentId: t.id }).catch(failure);
      t.fact("settingsBeforeASession", typeof offered === "string" ? offered : "ok");
      // An agent that cannot say its settings beforehand: a first session shows what they are.
      const known = typeof offered === "string" ? (await t.session(app)).state.values.get("configOptions") : (offered as JsonObject)["configOptions"];
      const effort = [...settingsOf(known ?? null).values()].find((setting) => setting.category === "thought_level");
      const other = effort?.values.find((value) => value !== effort.current);
      if (!effort || other === undefined) return t.fact("startsWithTheChosenEffort", "no effort setting with a second value");
      const session = await app.newSession({ cwd: t.work, agentId: t.id, configOptions: [{ configId: effort.id, value: other }] });
      t.made.add(session.id);
      const started = settingsOf(session.state.values.get("configOptions") ?? null).get(effort.id)?.current;
      t.fact("startsWithTheChosenEffort", started === other);
    },
  },
  {
    name: "session.rename_delete",
    about: "Can a session be renamed and deleted, at the adapter and through the daemon?",
    agents: ["claude"],
    cost: "free",
    async run(t) {
      const adapter = await t.adapter();
      const made = (await adapter.call("session/new", { cwd: t.work, mcpServers: [] })) as { sessionId: string };
      t.made.add(made.sessionId);
      const list = async () => ((await adapter.call("session/list", { cwd: t.work })) as { sessions?: { sessionId: string; title?: string }[] }).sessions ?? [];
      t.fact("rename", await attempt(adapter.call("session/set_title", { sessionId: made.sessionId, title: "conformance renamed" })));
      t.fact("renameShowsInTheList", (await list()).find((other) => other.sessionId === made.sessionId)?.title === "conformance renamed");
      t.fact("delete", await attempt(adapter.call("session/delete", { sessionId: made.sessionId })));
      t.fact("goneFromTheList", !(await list()).some((other) => other.sessionId === made.sessionId));

      const app = await t.app();
      const session = await t.session(app);
      t.fact("renameThroughTheDaemon", await attempt(app.call("session/set_title", { sessionId: session.id, title: "conformance renamed" })));
      t.fact("deleteThroughTheDaemon", await attempt(app.call("session/delete", { sessionId: session.id })));
      const still = (await app.listSessions({ loggedOnly: true })).some((other) => other.sessionId === session.id);
      t.fact("stillInTheDaemonsListAfterDelete", still);
    },
  },
  {
    name: "session.rename_used",
    about: "Renaming a session that has a turn: who learns the new title, and does it last?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const adapter = await t.adapter();
      const advertised = (adapter.welcome as { capabilities?: { session?: JsonObject } } | null)?.capabilities?.session?.["title"] !== undefined;
      t.fact("advertised", advertised);
      const app = await t.app();
      const session = await t.session(app);
      // The agent has started by now, so the daemon knows what it can do.
      const known = (await app.agents()).find((agent) => agent.agentId === t.id)?.capabilities as { session?: JsonObject } | undefined;
      t.fact("theDaemonSaysTheSame", (known?.session?.["title"] !== undefined) === advertised);
      await turn(session, OK);
      const title = (): Json => (session.state.values.get("sessionInfo") as JsonObject | undefined)?.["title"] ?? null;
      const listed = async (): Promise<Json> => (await app.listSessions({ loggedOnly: true })).find((other) => other.sessionId === session.id)?.title ?? null;
      const atTheHarness = async (): Promise<Json> =>
        (((await adapter.call("session/list", { cwd: t.work })) as { sessions?: { sessionId: string; title?: string }[] }).sessions ?? [])
          .find((other) => other.sessionId === session.id)?.title ?? null;
      t.info("titleBefore", title());

      const first = "conformance renamed";
      const stored = await app.setTitle(session.id, first).then((now) => (now === first ? "ok, title as sent" : `ok, other: ${now}`), failure);
      t.fact("rename", stored);
      await within(5_000, "the title in the session", session.until(() => title() === first)).catch(() => {});
      t.fact("theSessionSaysSo", title() === first);
      t.fact("theDaemonsListSaysSo", (await listed()) === first);
      t.fact("theHarnessListSaysSo", (await atTheHarness()) === first);

      await turn(session, OK);
      t.fact("keptAfterAnotherTurn", title() === first && (await atTheHarness()) === first);

      // Renamed while no process holds the session.
      await t.restartDaemon();
      const later = await t.app();
      const second = "conformance renamed again";
      t.fact("renameWhileAsleep", await attempt(later.setTitle(session.id, second)));
      t.fact("theDaemonsListSaysSoAsleep", (await later.listSessions({ loggedOnly: true })).find((other) => other.sessionId === session.id)?.title === second);
      t.fact("theHarnessListSaysSoAsleep", (await atTheHarness()) === second);
      const reopened = await later.session(session.id);
      t.fact("theReopenedSessionSaysSo", ((reopened.state.values.get("sessionInfo") as JsonObject | undefined)?.["title"] ?? null) === second);
    },
  },
  {
    name: "prompt.attachments",
    about: "What does the agent do with a picture, a linked file and a file sent with its content?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const app = await t.app();
      const session = await t.session(app);
      const takes = ((await app.agents()).find((agent) => agent.agentId === t.id)?.capabilities as { session?: { prompt?: JsonObject } } | undefined)?.session?.prompt ?? {};
      t.fact("advertised", Object.keys(takes).sort());
      const answerTo = async (content: Json[]) => {
        const before = said(session).length;
        await turn(session, content);
        return said(session).slice(before).toLowerCase();
      };
      const picture = { type: "image", mimeType: "image/png", data: solidPng(255, 0, 0) };
      t.fact("pictureSeen", (await answerTo([{ type: "text", text: "What colour is this picture? Answer with one word." }, picture])).includes("red"));

      writeFileSync(path.join(t.work, "mention-note.txt"), "The code word is zebra4417.\n");
      const link = { type: "resource_link", uri: `file://${path.join(t.work, "mention-note.txt")}`, name: "mention-note.txt" };
      t.fact("linkedFileRead", (await answerTo([{ type: "text", text: "What is the code word in @mention-note.txt? Answer with the word only." }, link])).includes("zebra4417"));

      const embedded = { type: "resource", resource: { uri: "file:///not-on-disk/embedded-note.txt", mimeType: "text/plain", text: "The code word is otter2290." } };
      const answer = await answerTo([{ type: "text", text: "What is the code word in the attached embedded-note.txt? Answer with the word only." }, embedded]).catch(failure);
      t.fact("embeddedFileRead", answer.startsWith("error") ? answer : answer.includes("otter2290"));

      // What the log keeps of the user's own messages: an app draws them from this.
      const kept = [...(session.state.collections.get("userMessage")?.values() ?? [])].map((item) =>
        ((item as JsonObject)["content"] as JsonObject[] | undefined ?? []).map((block) => `${String(block["type"])}${block["type"] === "image" && typeof block["data"] === "string" && block["data"].length > 0 ? " with its bytes" : ""}`));
      t.fact("userMessagesKeep", kept);
    },
  },
  {
    name: "todos.listed",
    about: "When the agent keeps a todo list, what reaches an app: a list with entries, or only tool calls?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      await turn(session, "Create three tasks with your task tools (TaskCreate, or TodoWrite if that is what you have; load them first if they are deferred): alpha, beta, gamma. Then mark alpha completed (TaskUpdate). Reply with exactly: done.");
      const plans = [...(session.state.collections.get("plan")?.values() ?? [])] as JsonObject[];
      t.fact("plans", plans.map((plan) => ({
        type: plan["type"] ?? null,
        entries: ((plan["entries"] as JsonObject[] | undefined) ?? []).map((entry) => String(entry["status"])).sort(),
      })));
      const tools = [...(session.state.collections.get("toolCall")?.values() ?? [])] as JsonObject[];
      t.fact("toolCallKinds", [...new Set(tools.map((tool) => String(tool["kind"] ?? "")))].sort());
      t.info("toolCallTitles", tools.map((tool) => String(tool["title"] ?? "")));
      t.info("said", said(session).slice(0, 300));
    },
  },
  {
    name: "approval.answered",
    about: "An approval is refused, then allowed: what happens to the command, and what does the session show?",
    agents: ["claude"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      const mode = settingOf(session, "mode");
      if (mode?.values.includes("default")) await session.setConfigOption(mode.id, "default");
      const file = (name: string) => path.join(t.work, name);
      for (const name of ["approval-no.txt", "approval-yes.txt"]) rmSync(file(name), { force: true });
      const ask = (name: string) => `Run this exact shell command: printf approval > ${name}  Then reply with exactly: done.`;
      const tools = () => [...(session.state.collections.get("toolCall")?.values() ?? [])] as JsonObject[];
      const requests = () => [...session.state.requests.values()];

      const refused = await turn(session, ask("approval-no.txt"), undefined, "reject");
      t.must("an approval was asked", refused.approvals > 0);
      const options = ((requests()[0]?.params as JsonObject | undefined)?.["options"] as JsonObject[] | undefined) ?? [];
      t.fact("optionKinds", options.map((option) => String(option["kind"])).sort());
      t.fact("refused", {
        commandRan: existsSync(file("approval-no.txt")),
        toolStatus: tools().map((tool) => String(tool["status"])),
        stopReason: refused.stopReason,
        answerShown: ((requests()[0]?.outcome as JsonObject | null)?.["outcome"] as JsonObject | undefined)?.["outcome"] ?? null,
      });

      const allowed = await turn(session, ask("approval-yes.txt"));
      t.fact("allowed", {
        commandRan: existsSync(file("approval-yes.txt")),
        lastToolStatus: String(tools().at(-1)?.["status"]),
        stopReason: allowed.stopReason,
      });
      t.must("every approval shows its answer", requests().every((request) => request.outcome !== null));
      for (const name of ["approval-no.txt", "approval-yes.txt"]) rmSync(file(name), { force: true });
    },
  },
  {
    name: "approval.two_apps",
    about: "Two apps answer the same approval at the same moment, one yes and one no: does one answer win for both?",
    agents: ["claude"],
    cost: "turn",
    async run(t) {
      const [first, second] = [await t.app(), await t.app()];
      const mine = await t.session(first);
      const mode = settingOf(mine, "mode");
      if (mode?.values.includes("default")) await mine.setConfigOption(mode.id, "default");
      const theirs = await second.session(mine.id);
      const file = path.join(t.work, "approval-race.txt");
      rmSync(file, { force: true });

      const done = turn(mine, "Run this exact shell command: printf race > approval-race.txt  Then reply with exactly: done.", undefined, "leave");
      const asked = (session: Session) => session.pendingRequests().some(({ request }) => request.method === "session/request_permission");
      await within(120_000, "both apps see the approval", Promise.all([mine.until(() => asked(mine)), theirs.until(() => asked(theirs))]));
      const { requestId, request } = mine.pendingRequests()[0]!;
      const options = ((request.params as JsonObject)["options"] as JsonObject[]) ?? [];
      const option = (kind: string) => String(options.find((entry) => String(entry["kind"]).startsWith(kind))?.["optionId"]);
      // The refusal is sent first, so a daemon that took the last answer would run the command.
      theirs.approve(requestId, option("reject"));
      mine.approve(requestId, option("allow"));
      await done;
      await theirs.until(() => theirs.turnState === "idle");

      const outcome = (session: Session) => JSON.stringify(session.state.requests.get(requestId)?.outcome ?? null);
      t.must("both apps show the same answer", outcome(mine) === outcome(theirs));
      const won = outcome(mine).includes(option("allow")) ? "allow" : outcome(mine).includes(option("reject")) ? "reject" : "neither";
      t.must("one of the two answers won", won !== "neither");
      t.must("the command ran exactly when the winning answer allowed it", existsSync(file) === (won === "allow"));
      t.fact("firstAnswerWon", won === "reject");
      t.must("both apps end with the same session", JSON.stringify(mine.toJSON()) === JSON.stringify(theirs.toJSON()));
      rmSync(file, { force: true });
    },
  },
  {
    name: "sessions.concurrent",
    about: "Three sessions run a turn at the same time: does each get its own answer?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const app = await t.app();
      const markers = ["FINCH-11", "OTTER-22", "LYNX-33"];
      const sessions = [];
      for (const _ of markers) sessions.push(await t.session(app));
      const started = Date.now();
      const turns = await Promise.all(sessions.map((session, at) => turn(session, `Reply with exactly: ${markers[at]}`).then(() => "ok", failure)));
      t.fact("turns", turns);
      t.fact("eachGotItsOwnAnswer", sessions.every((session, at) => said(session).includes(markers[at]!)));
      t.fact("answersCrossed", sessions.some((session, at) => markers.some((marker, other) => other !== at && said(session).includes(marker))));
      t.fact("userMessagesPerSession", sessions.map((session) => count(session, "userMessage")));
      t.info("ms", Date.now() - started);
    },
  },
  {
    name: "session.mcp_server",
    about: "An MCP server given when the session is created: can the agent call its tool?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const app = await t.app();
      const server = { type: "stdio", name: "marker", command: process.execPath, args: [path.join(here, "mcp-marker.mjs")], env: [] };
      const created = await app.newSession({ cwd: t.work, agentId: t.id, mcpServers: [server] }).then((session) => session, failure);
      t.fact("created", typeof created === "string" ? created : "ok");
      if (typeof created === "string") return;
      t.made.add(created.id);
      if (t.agent.cheapModel && current(created, "model") !== t.agent.cheapModel) await created.setConfigOption("model", t.agent.cheapModel);
      const result = await turn(created, "Call the tool conformance_marker of the MCP server named marker (load it first if it is deferred). Then reply with exactly the text it returned, nothing else. If you have no such tool, reply with exactly: NO TOOL.");
      const tools = [...(created.state.collections.get("toolCall")?.values() ?? [])] as JsonObject[];
      // By its title: a search for the tool also has its name, in what was searched for.
      t.fact("toolCalled", tools.some((tool) => String(tool["title"] ?? "").includes("conformance_marker")));
      t.fact("answerHasTheToolsText", said(created).includes("OKAPI-902"));
      t.fact("approvalAsked", result.approvals > 0);
      t.info("toolCallTitles", tools.map((tool) => String(tool["title"] ?? "")));
      t.info("said", said(created).slice(0, 200));
    },
  },
  {
    name: "session.fork",
    about: "A copy of a session: does it know the history, and is the original left alone?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const app = await t.app();
      const original = await t.session(app);
      await turn(original, "Remember the codeword HERON-55. Reply with exactly: ok");
      // The original's values may change on their own (Claude names a session a moment after its
      // first turn). What the copy does must not show up in the original's items.
      const items = () => JSON.stringify([original.state.timeline, [...original.state.collections].map(([kind, all]) => [kind, [...all]])]);
      const before = items();
      const copy = await app.forkSession(original.id).then((session) => session, failure);
      t.fact("fork", typeof copy === "string" ? copy : "ok");
      if (typeof copy === "string") return;
      t.made.add(copy.id);
      t.must("the copy has its own id", copy.id !== original.id);
      t.fact("copyShowsTheHistoryBeforeAnyTurn", count(copy, "userMessage") >= 1);
      await turn(copy, "What is the codeword? Reply with it only.");
      t.fact("copyKnowsTheHistory", said(copy).includes("HERON-55"));
      t.fact("originalItemsUnchanged", items() === before);
      const listed = await app.listSessions({ agentId: t.id, loggedOnly: true });
      t.fact("bothListed", [original.id, copy.id].every((id) => listed.some((summary) => summary.sessionId === id)));
    },
  },
  {
    name: "background.listed",
    about: "A command left running after the turn: does the session list it, and stop listing it when it ends?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      const background = () => {
        const list = (session.state.values.get("state") as JsonObject | undefined)?.["background"];
        return (Array.isArray(list) ? list : []) as JsonObject[];
      };
      let during = 0;
      await turn(session, "Start this shell command in the background and do not wait for it to finish: sleep 20. Then reply with exactly: started.", () => {
        during = Math.max(during, background().length);
      });
      t.fact("listedAfterTheTurn", background().length > 0);
      t.fact("kinds", [...new Set(background().flatMap((entry) => Object.keys(entry).filter((key) => key.endsWith("Id"))))].sort());
      t.info("listedDuringTheTurn", during);
      t.info("toolCallTitles", [...(session.state.collections.get("toolCall")?.values() ?? [])].map((tool) => String((tool as JsonObject)["title"] ?? "")));
      const listed = background().length > 0;
      const cleared = await within(60_000, "the list empties", session.until(() => background().length === 0)).then(() => true, () => false);
      t.fact("clearedWhenTheCommandEnded", listed ? cleared : null);
      // Claude answers the command's end with a turn of its own; let it finish before the session is deleted.
      await within(60_000, "the session is idle", session.until(() => session.turnState === "idle")).catch(() => {});
      t.info("agentMessages", count(session, "agentMessage"));
    },
  },
  {
    name: "attach.late",
    about: "An app that attaches during a turn, and one that attaches after it: do they end with what the first app has?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const [first, second, third] = [await t.app(), await t.app(), await t.app()];
      const live = await t.session(first);
      let joining: Promise<Session> | null = null;
      await turn(live, "Run this exact shell command: echo late-attach  Then count from 1 to 20, one number per line.", (now) => {
        if (!joining && count(now, "toolCall") + count(now, "agentMessage") > 0) joining = second.session(live.id);
      });
      t.must("the second app attached while the turn ran", joining !== null);
      const during = await joining!;
      const after = await third.session(live.id);
      await during.until(() => during.turnState === "idle");
      const same = (other: Session) => JSON.stringify(other.toJSON()) === JSON.stringify(live.toJSON());
      // The last messages of the turn may still be on their way to the other connections.
      await within(10_000, "the apps agree", during.until(() => same(during))).catch(() => {});
      t.fact("attachedDuringTheTurn", same(during));
      t.fact("attachedAfterTheTurn", same(after));
      t.info("items", live.state.timeline.length);
    },
  },
  {
    name: "compact.command",
    about: "The user asks to compact the conversation: does an item say so, and does the session still know what was said?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      await turn(session, "Remember the codeword IBIS-77. Reply with exactly: ok");
      const result = await turn(session, "/compact").then((done) => done.stopReason, failure);
      t.fact("compactTurn", result);
      t.fact("compactionItems", count(session, "compaction") > 0);
      t.info("agentMessagesAfterCompact", count(session, "agentMessage"));
      await turn(session, "What is the codeword? Reply with it only.");
      t.fact("knowsWhatWasSaidBefore", said(session).includes("IBIS-77"));
    },
  },
  {
    name: "turn.interrupt",
    about: "Does stopping a running turn end it, and how does the session say so?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      let cancelled = false;
      const result = await turn(session, "Write a 500-word story about a lighthouse keeper.", (now) => {
        if (!cancelled && count(now, "agentMessage") + count(now, "agentThought") > 0) {
          cancelled = true;
          now.cancel();
        }
      });
      t.must("the turn was running when it was stopped", cancelled);
      t.fact("stopReason", result.stopReason);
      t.info("ms", result.ms);
      // The session is usable afterwards.
      const next = await turn(session, OK);
      t.fact("nextTurnStopReason", next.stopReason);
    },
  },
  {
    name: "turn.message_while_busy",
    about: "What happens to a message sent while a turn runs?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      let second: Promise<string> | null = null;
      await turn(session, "Count from 1 to 60, one number per line, nothing else.", (now) => {
        if (!second && count(now, "agentMessage") > 0) {
          second = attempt(now.prompt("When you are done counting, reply with the single word MARMOT."));
        }
      });
      t.must("the second message was sent while the first turn ran", second !== null);
      t.fact("secondMessageAccepted", second ? await second : null);
      await within(120_000, "the reply to the second message", session.until(() => session.turnState === "idle" && /MARMOT/i.test(said(session)))).catch(() => {});
      t.fact("secondMessageAnswered", /MARMOT/i.test(said(session)));
      t.fact("userMessagesShown", count(session, "userMessage"));
    },
  },
  {
    name: "shell.output_streams",
    about: "Does a running command's output arrive before the command ends? Is an approval asked first?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      const mode = settingOf(session, "mode");
      if (mode?.values.includes("default")) await session.setConfigOption(mode.id, "default");
      let early = false;
      const ticks = (now: typeof session) =>
        [...(now.state.collections.get("terminal")?.values() ?? [])].map((item) => {
          const terminal = item as JsonObject;
          const data = (terminal["output"] as JsonObject | undefined)?.["data"];
          return { text: typeof data === "string" ? base64Text(data) : "", ended: terminal["exitStatus"] !== undefined };
        });
      const result = await turn(
        session,
        "Run this exact shell command, then reply with the single word done: for i in 1 2 3 4 5 6 7 8; do echo tick $i; sleep 1; done",
        (now) => {
          if (ticks(now).some((terminal) => terminal.text.includes("tick") && !terminal.ended)) early = true;
        },
      );
      t.fact("approvalAsked", result.approvals > 0);
      t.fact("outputBeforeTheCommandEnded", early);
      t.fact("allOutputAtTheEnd", ticks(session).some((terminal) => terminal.text.includes("tick 8")));
      t.fact("terminalItems", count(session, "terminal"));
    },
  },
  {
    name: "reasoning.arrives",
    about: "Does the model's reasoning arrive as items?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      await turn(session, "Think it through step by step first: what is 17 * 23 + 9? Then give only the number.");
      t.fact("reasoningItems", count(session, "agentThought") > 0);
      t.info("answeredCorrectly", said(session).includes("400"));
    },
  },
  {
    name: "memory.project_file",
    about: "Does the harness load the project's instruction file (CLAUDE.md, AGENTS.md)?",
    agents: ["claude", "pi"],
    cost: "turn",
    async run(t) {
      const session = await t.session(await t.app());
      await turn(session, "What is the conformance marker? Reply with the marker only.");
      t.fact("knowsTheMarker", said(session).includes("ZEBRA-417"));
    },
  },
];
