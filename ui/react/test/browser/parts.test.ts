// The kit's parts in a real browser, clicked and typed into. Each test loads one part of
// parts.page.tsx and checks what the user sees and what the app is asked to do.

import assert from "node:assert/strict";
import path from "node:path";
import test, { after, before } from "node:test";
import type { Page } from "playwright";
import { closeBrowser, openPage, servePage } from "./support.ts";

let site: Awaited<ReturnType<typeof servePage>>;
before(async () => { site = await servePage(path.join(import.meta.dirname, "parts.page.tsx")); });
after(async () => { await closeBrowser(); await site.close(); });

const part = (name: string, query = "") => openPage(`${site.url}/${query}#${name}`);
const calls = (page: Page) => page.evaluate(() => window.calls);
const refuse = (page: Page, why: string | undefined) => page.evaluate((reason) => { window.refuse = reason; }, why);
/** Waits until the page has asked its app `count` times. A call is made a moment after the click. */
const called = async (page: Page, count: number) => {
  await page.waitForFunction((wanted) => window.calls.length >= wanted, count);
  return calls(page);
};
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64");

test("a session is opened, renamed and deleted from its row", async () => {
  const page = await part("sessions");
  assert.deepEqual(await page.locator("h3").allTextContents(), ["alpha", "beta"]);
  assert.equal(await page.locator('[aria-current="true"]').textContent().then((text) => text?.includes("Write the docs")), true);

  await page.getByRole("button", { name: /Fix the parser/ }).click();
  assert.deepEqual(await called(page, 1), [["open", "a"]]);

  // Rename: the field starts with the title; Escape leaves it as it was.
  const row = page.locator("li.arc-session").first();
  await row.getByRole("button", { name: "Rename" }).click();
  const field = page.getByLabel("Session title");
  assert.equal(await field.inputValue(), "Fix the parser");
  assert.equal(await field.evaluate((input) => input === document.activeElement), true, "the field has the focus");
  await field.fill("Something else");
  await field.press("Escape");
  await row.getByRole("button", { name: /Fix the parser/ }).waitFor();
  assert.equal((await calls(page)).length, 1);

  await row.getByRole("button", { name: "Rename" }).click();
  await field.fill("  Fix the lexer ");
  await field.press("Enter");
  assert.deepEqual((await called(page, 2))[1], ["rename", "a", "Fix the lexer"]);

  // Delete asks first, says what else is removed, and Cancel does nothing.
  await row.getByRole("button", { name: "Delete" }).click();
  const dialog = page.getByRole("alertdialog");
  assert.match((await dialog.textContent())!, /removed from claude’s own history too/);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await dialog.waitFor({ state: "detached" });
  assert.equal((await calls(page)).length, 2);

  await row.getByRole("button", { name: "Delete" }).click();
  await dialog.getByRole("button", { name: "Delete" }).click();
  assert.deepEqual((await called(page, 3))[2], ["delete", "a"]);

  // A refusal is shown on the row.
  await refuse(page, "needs the admin scope");
  await row.getByRole("button", { name: "Delete" }).click();
  await dialog.getByRole("button", { name: "Delete" }).click();
  assert.equal(await row.getByRole("alert").textContent(), "Not deleted: needs the admin scope");
  await refuse(page, undefined);

  await page.getByRole("button", { name: "Show more" }).click();
  assert.deepEqual((await called(page, 5))[4], ["more"]);
  await page.close();
});

test("the composer sends on Enter, offers commands after / and files after @", async () => {
  const page = await part("composer");
  const field = page.getByPlaceholder("Message the agent…");

  // Shift+Enter is a new line; Enter sends and empties the field.
  await field.pressSequentially("one");
  await field.press("Shift+Enter");
  await field.pressSequentially("two");
  assert.equal((await calls(page)).length, 0);
  await field.press("Enter");
  assert.deepEqual(await called(page, 1), [["send", "one\ntwo"]]);
  assert.equal(await field.inputValue(), "");
  await field.press("Enter");
  await page.getByRole("button", { name: "Send" }).click();
  assert.equal((await calls(page)).length, 1, "nothing is sent from an empty field");

  // Commands: names that start with what was typed come first; arrows move, Enter takes.
  await field.pressSequentially("/re");
  const menu = page.getByRole("listbox", { name: "Commands" });
  assert.deepEqual(await menu.getByRole("option").locator(".arc-menu-label").allTextContents(), ["/review", "/rewind", "/deep-research"]);
  assert.equal(await menu.locator('[aria-selected="true"]').textContent(), "/reviewReview the changes");
  await field.press("ArrowDown");
  assert.match((await menu.locator('[aria-selected="true"]').textContent())!, /^\/rewind/);
  await field.press("ArrowUp");
  await field.press("ArrowUp");
  assert.match((await menu.locator('[aria-selected="true"]').textContent())!, /^\/deep-research/);
  await field.press("Enter");
  assert.equal(await field.inputValue(), "/deep-research ");
  assert.equal((await calls(page)).length, 1, "Enter took the command and sent nothing");
  await field.pressSequentially("tides");
  await field.press("Enter");
  assert.deepEqual((await called(page, 2))[1], ["send", "/deep-research tides"]);

  // Escape closes the menu until the text changes; a click takes an entry and keeps the focus.
  await field.pressSequentially("/r");
  await menu.waitFor();
  await field.press("Escape");
  await menu.waitFor({ state: "detached" });
  await field.pressSequentially("e");
  await menu.getByRole("option", { name: /rewind/ }).click();
  assert.equal(await field.inputValue(), "/rewind ");
  assert.equal(await field.evaluate((input) => input === document.activeElement), true);
  await field.fill("");

  // Mentions: the file is named in the text and sent as a link.
  await field.pressSequentially("look at @ma");
  const mentions = page.getByRole("listbox", { name: "Mentions" });
  assert.deepEqual(await mentions.getByRole("option").locator(".arc-menu-label").allTextContents(), ["@main.rs", "@Makefile"]);
  await field.press("Tab");
  assert.equal(await field.inputValue(), "look at @main.rs ");
  await field.pressSequentially("please");
  await field.press("Enter");
  assert.deepEqual((await called(page, 3))[2], ["send", [
    { type: "text", text: "look at @main.rs please" },
    { type: "resource_link", uri: "file:///work/main.rs", name: "main.rs" },
  ]]);
  await page.close();
});

test("files are attached by choosing, pasting and dropping, and a refused message comes back", async () => {
  const page = await part("composer");
  const field = page.getByPlaceholder("Message the agent…");
  const chips = page.locator(".arc-attachments li");

  // The Attach button opens the file chooser.
  const choosing = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Attach" }).click();
  await (await choosing).setFiles([
    { name: "dot.png", mimeType: "image/png", buffer: png },
    { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("remember the tides") },
    { name: "big.txt", mimeType: "text/plain", buffer: Buffer.alloc(2001, "x") },
  ]);
  await chips.nth(1).waitFor();
  assert.deepEqual(await chips.locator("span").allTextContents(), ["dot.png", "notes.txt"]);
  assert.equal(await chips.first().locator("img").evaluate((image: HTMLImageElement) => image.naturalWidth), 1, "the picture is shown");
  assert.equal(await page.getByRole("alert").textContent(), "big.txt: larger than 0 MB");

  await page.getByRole("button", { name: "Remove notes.txt" }).click();
  assert.deepEqual(await chips.locator("span").allTextContents(), ["dot.png"]);

  // Pasted and dropped files arrive the same way.
  const give = (event: "paste" | "drop", name: string) => field.evaluate((target, [kind, fileName, bytes]) => {
    const data = new DataTransfer();
    data.items.add(new File([Uint8Array.from(atob(bytes!), (char) => char.charCodeAt(0))], fileName!, { type: "image/png" }));
    target.dispatchEvent(kind === "paste"
      ? new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true })
      : new DragEvent("drop", { dataTransfer: data, bubbles: true, cancelable: true }));
  }, [event, name, png.toString("base64")]);
  await give("paste", "pasted.png");
  await chips.nth(1).waitFor();
  await give("drop", "dropped.png");
  await chips.nth(2).waitFor();
  assert.deepEqual(await chips.locator("span").allTextContents(), ["dot.png", "pasted.png", "dropped.png"]);
  await page.getByRole("button", { name: "Remove pasted.png" }).click();
  await page.getByRole("button", { name: "Remove dropped.png" }).click();

  // The daemon says no: the text and the picture are back, with the reason.
  await refuse(page, "the session log cannot be written");
  await field.fill("what is this?");
  await field.press("Enter");
  await page.getByRole("alert").filter({ hasText: "Not sent: the session log cannot be written" }).waitFor();
  assert.equal(await field.inputValue(), "what is this?");
  assert.equal(await chips.count(), 1);

  await refuse(page, undefined);
  await page.getByRole("button", { name: "Send" }).click();
  assert.deepEqual((await called(page, 2))[1], ["send", [
    { type: "text", text: "what is this?" },
    { type: "image", mimeType: "image/png", data: png.toString("base64") },
  ]]);
  assert.equal(await chips.count(), 0);
  assert.equal(await page.getByRole("alert").count(), 0);
  await page.close();

  // An agent that only takes pictures: a text file is refused with the reason.
  const picturesOnly = await part("composer", "?pictures-only");
  await picturesOnly.locator('input[type="file"]').setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("x") });
  assert.equal(await picturesOnly.getByRole("alert").textContent(), "notes.txt: the agent does not take files");
  await picturesOnly.close();

  // While a turn runs, the composer says so and offers to stop it.
  const running = await part("composer", "?running");
  await running.getByPlaceholder("Add to the running turn…").waitFor();
  await running.getByRole("button", { name: "Stop" }).click();
  assert.deepEqual(await called(running, 1), [["cancel"]]);
  await running.close();
});

test("a setting is changed from its menu, and a refusal puts it back", async () => {
  const page = await part("settings");
  const model = page.getByLabel("Model");
  assert.equal(await model.inputValue(), "haiku");
  const other = await model.locator("option").evaluateAll((options) => (options as HTMLOptionElement[]).map((option) => option.value).find((value) => value !== "haiku")!);
  await model.selectOption(other);
  assert.deepEqual(await called(page, 1), [["setOption", "model", other]]);
  // The agent has not said the model changed (the page's state is fixed), so the old one shows again.
  await page.waitForFunction(() => !document.querySelector("select")!.disabled);
  assert.equal(await model.inputValue(), "haiku");

  await refuse(page, "no such model");
  await model.selectOption(other);
  assert.equal(await page.getByRole("alert").textContent(), "Not changed: Model: no such model");
  assert.equal(await model.inputValue(), "haiku");

  await refuse(page, undefined);
  await page.getByRole("checkbox").click();
  const last = (await called(page, 3))[2]!;
  assert.deepEqual([last[0], last[2]], ["setOption", true]);
  await page.close();
});

test("an approval is answered with one click, a question with its form", async () => {
  const approval = await part("approval");
  assert.deepEqual(await approval.locator(".arc-pending button").allTextContents().then((all) => [all[0], all.at(-1)]), ["Yes", "No"]);
  await approval.getByRole("button", { name: "No", exact: true }).click();
  const [, , option] = (await called(approval, 1))[0]!;
  assert.equal(option, "reject");
  await approval.close();

  const question = await part("question");
  await question.locator(".arc-pending .arc-request-title", { hasText: "Which colour do you prefer?" }).waitFor();
  await question.getByLabel("Colour").selectOption("Blue");
  await question.getByLabel("Other").fill("or teal");
  await question.getByRole("button", { name: "Answer" }).click();
  assert.deepEqual((await called(question, 1))[0]!.slice(2), [{ question_0: "Blue", question_0_custom: "or teal" }]);
  await question.getByRole("button", { name: "Skip" }).click();
  assert.deepEqual((await called(question, 2))[1]!.slice(2), [null]);
  await question.close();
});

test("a long session shows its end first and earlier messages on request, keeping the place", async () => {
  const page = await part("long");
  const rows = page.locator(".arc-row");
  assert.equal(await rows.count(), 200);
  assert.equal(await rows.first().textContent(), "message 251");
  const earlier = page.getByRole("button", { name: "Show earlier (250 more)" });
  await earlier.click();
  await page.getByRole("button", { name: "Show earlier (50 more)" }).click();
  assert.equal(await rows.count(), 450);
  assert.equal(await rows.first().textContent(), "message 1");
  assert.equal(await page.getByRole("button", { name: /Show earlier/ }).count(), 0);
  await page.close();
});

test("folded parts open: thinking, unchanged lines of a diff", async () => {
  const thinking = await part("thinking");
  const thought = thinking.locator("details.arc-thought").first();
  assert.equal(await thought.evaluate((details: HTMLDetailsElement) => details.open), false);
  await thought.locator("summary").click();
  assert.equal(await thought.evaluate((details: HTMLDetailsElement) => details.open), true);
  assert.ok((await thought.innerText()).length > "Thinking".length + 10, "the thought's text is shown");
  await thinking.close();

  const diff = await part("diff");
  assert.equal(await diff.getByText("line 20", { exact: true }).count(), 0);
  await diff.getByRole("button", { name: /Show \d+ unchanged lines/ }).click();
  await diff.getByText("line 20", { exact: true }).waitFor();
  assert.equal(await diff.getByRole("button", { name: /unchanged lines/ }).count(), 0);
  await diff.close();
});

test("the fork button works once and says why when it fails; a missing agent can be tried again", async () => {
  const page = await part("fork");
  await refuse(page, "the agent cannot fork");
  const button = page.getByRole("button", { name: "Fork and continue here" });
  await button.click();
  await page.getByText("Could not fork: the agent cannot fork").waitFor();
  await refuse(page, undefined);
  await button.click();
  await page.getByText("On the copy").waitFor();
  assert.deepEqual(await calls(page), [["fork"], ["fork"]]);
  await page.close();

  const agents = await part("agents");
  assert.match((await agents.getByRole("alert").textContent())!, /Pi is not installed on the daemon's machine/);
  await agents.getByRole("button", { name: "Try again" }).click();
  assert.deepEqual(await called(agents, 1), [["retry"]]);
  await agents.close();
});
