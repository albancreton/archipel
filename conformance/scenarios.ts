// The scenarios. Each one does something with the real harness through the daemon and notes
// what it saw. The wording of a model's answer varies, so nothing here depends on it beyond a
// marker word the prompt asks for.

import { base64Text, RpcError, type Json, type JsonObject, type Session } from "@harness-bridge/client";
import { count, current, RpcFailure, said, settingOf, settings, settingsOf, turn, within, type Run } from "./lib.ts";

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
