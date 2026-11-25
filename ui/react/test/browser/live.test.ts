// The whole way in a real browser: the demo page, the daemon, and a fake agent that replays a
// recorded session of Claude. Nothing here costs anything.

import assert from "node:assert/strict";
import path from "node:path";
import test, { after, before } from "node:test";
import type { Page } from "playwright";
import { closeBrowser, openPage, servePage, startDaemon, type Daemon } from "./support.ts";

let site: Awaited<ReturnType<typeof servePage>>;
before(async () => { site = await servePage(path.join(import.meta.dirname, "../../demo/live.tsx")); });
after(async () => { await closeBrowser(); await site.close(); });

/** The demo page on a daemon whose agent replays `recording`. */
async function withAgent(recording: string, work: (page: Page, daemon: Daemon) => Promise<void>) {
  const daemon = await startDaemon(`claude-v2/${recording}`, site.url);
  try {
    const page = await openPage(`${site.url}/?ws=${encodeURIComponent(daemon.ws)}&cwd=/work#${daemon.token}`);
    await work(page, daemon);
    await page.close();
  } finally {
    await daemon.close();
  }
}

/** Starts a session, sets its mode as the recording did, and sends a message. */
async function begin(page: Page, message: string) {
  await page.getByRole("button", { name: "New session" }).click();
  const mode = page.locator(".arc-setting-mode select");
  await mode.selectOption({ index: 1 });
  await page.waitForFunction(() => !document.querySelector<HTMLSelectElement>(".arc-setting-mode select")!.disabled);
  const field = page.locator(".arc-composer textarea");
  await field.fill(message);
  await field.press("Enter");
  // The message shown is the recorded one: the agent says what it was sent, and this one replays.
  await page.locator(".arc-user").waitFor();
  assert.equal(await field.inputValue(), "");
}

test("a turn with an approval: the user allows the command and the agent finishes", async () => {
  await withAgent("approval_command", async (page) => {
    // The agent's own history has the recorded session; the daemon has not seen it yet.
    await page.getByText("from claude").waitFor();
    await begin(page, "write the probe file");

    const approval = page.locator(".arc-pending");
    await approval.waitFor();
    assert.match((await approval.locator(".arc-request-title").textContent())!, /printf 'approval fixture'/);
    await page.locator(".arc-status .arc-badge", { hasText: "requires action" }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Stop" }).count(), 1);

    await approval.getByRole("button", { name: "Yes", exact: true }).click();
    await page.locator(".arc-request .arc-answer", { hasText: "Yes" }).waitFor();
    await page.locator(".arc-status .arc-badge", { hasText: "idle" }).waitFor();
    assert.equal(await page.locator(".arc-pending").count(), 0);
    assert.match((await page.locator(".arc-assistant").last().textContent())!, /done/i);
    assert.equal(await page.getByRole("button", { name: "Stop" }).count(), 0);
    // The new session is in the list, with the state the daemon reported.
    assert.equal(await page.locator("li.arc-session.arc-selected").count(), 1);
  });
});

test("a question: the user answers with the form and the agent goes on", async () => {
  await withAgent("question", async (page) => {
    await begin(page, "ask me");
    const question = page.locator(".arc-pending");
    await question.waitFor();
    await question.getByLabel("Colour").selectOption("Red");
    await question.getByRole("button", { name: "Answer" }).click();
    await page.locator(".arc-request .arc-answer", { hasText: "Red" }).waitFor();
    await page.locator(".arc-status .arc-badge", { hasText: "idle" }).waitFor();
    assert.ok((await page.locator(".arc-assistant").count()) > 0);
  });
});

test("Stop ends a running turn", async () => {
  await withAgent("interrupt", async (page) => {
    await begin(page, "run something slow");
    await page.locator(".arc-tool").first().waitFor();
    await page.getByRole("button", { name: "Stop" }).click();
    await page.locator(".arc-status .arc-badge", { hasText: "idle" }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Stop" }).count(), 0);
  });
});

test("a session from the agent's own history is opened with its history", async () => {
  await withAgent("multi_turn", async (page) => {
    const row = page.locator("li.arc-session", { hasText: "from claude" });
    await row.locator(".arc-session-open").click();
    await page.locator(".arc-user").nth(1).waitFor();
    assert.ok((await page.locator(".arc-assistant").count()) >= 2, "both recorded turns are shown");
    // Opening it put it in the daemon's log, and the list says so.
    await page.locator("li.arc-session.arc-selected").waitFor();
  });
});

test("the daemon goes away and comes back: the page says so, then carries on", async () => {
  await withAgent("approval_command", async (page, daemon) => {
    await begin(page, "write the probe file");
    await page.locator(".arc-pending").waitFor();

    await daemon.stop();
    await page.getByText("Connection lost. Reconnecting…").waitFor();
    await daemon.start();
    await page.getByText("Connection lost. Reconnecting…").waitFor({ state: "detached", timeout: 30_000 });
    // What was on the page is still there, read from the daemon's log.
    await page.locator(".arc-user").waitFor();
    await page.locator(".arc-request-title", { hasText: "printf 'approval fixture'" }).waitFor();
  });
});
