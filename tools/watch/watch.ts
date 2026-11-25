// Looks for new versions of what the library depends on and writes down what changed.
//
//   node watch.ts [source ...] [--report] [--live | --live=all] [--agent="command args"]
//
// Without flags it costs nothing: it asks the registries for the newest versions, downloads
// what is new next to what we use, and writes the differences to target/watch/out/<date>/.
// It never changes the repository's own files; an update stays a decision.
//
// --report     also start an agent on every source that changed. It reads the differences and
//              writes docs/updates/<date>-<source>.md: what touches us and the steps to take.
// --live       also run the conformance scenarios that spend no model turn against a new
//              adapter. --live=all runs every scenario (a few cents for Claude).
// --agent=…    the agent to start instead of the one in sources.json.
//
// Sources, and the versions we are on, are in sources.json. See README.md.

import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { Adapter, type AgentConfig } from "../../conformance/lib.ts";
import { changelogBetween, differences, matches, newer, noSchemaChanges, schemaChanges, type Json, type SchemaChanges } from "./lib.ts";

interface Source {
  id: string;
  registry: "npm" | "crates";
  package: string;
  /** The version the library is built and tested with. */
  current: string;
  why: string;
  /** The files of the package that matter to us, as patterns. */
  files: string[];
  /** Where the changelog is, when it is not one of the package's files. */
  changelog?: string;
  /** The ACP schema inside the package, to compare by what we use of it. */
  schema?: string;
  /** The agent in conformance/config.json that runs this adapter: its `initialize` answer is compared. */
  probe?: string;
}
interface Config {
  agent: { command: string; args: string[] };
  sources: Source[];
}

const here = import.meta.dirname;
const root = path.join(here, "../..");
const args = process.argv.slice(2);
const flag = (name: string) => args.find((arg) => arg === `--${name}` || arg.startsWith(`--${name}=`));
const wanted = args.filter((arg) => !arg.startsWith("--"));
const config = JSON.parse(readFileSync(path.join(here, "sources.json"), "utf8")) as Config;
// The date here, not in Greenwich.
const today = new Date(Date.now() - new Date().getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
const cache = path.join(root, "target/watch/cache");
const out = path.join(root, "target/watch/out", today);
const relative = (file: string) => path.relative(root, file);

// A generic name: registries ask automated clients to say who they are, not who runs them.
const headers = { "user-agent": "harness-bridge-watch" };
async function fetchJson(url: string): Promise<Json> {
  const response = await fetch(url, { headers });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return (await response.json()) as Json;
}

/** The newest released version of a package. */
async function latest(source: Source): Promise<string> {
  if (source.registry === "crates") {
    const answer = (await fetchJson(`https://crates.io/api/v1/crates/${source.package}`)) as { crate: { max_stable_version: string } };
    return answer.crate.max_stable_version;
  }
  const answer = (await fetchJson(`https://registry.npmjs.org/${source.package.replace("/", "%2f")}/latest`)) as { version: string };
  return answer.version;
}

/** Downloads one version of a package and unpacks it. Returns the folder. Kept between runs. */
async function download(source: Source, version: string): Promise<string> {
  const folder = path.join(cache, source.id, version);
  if (existsSync(path.join(folder, ".complete"))) return folder;
  rmSync(folder, { recursive: true, force: true });
  mkdirSync(folder, { recursive: true });
  const url = source.registry === "crates"
    ? `https://static.crates.io/crates/${source.package}/${source.package}-${version}.crate`
    : ((await fetchJson(`https://registry.npmjs.org/${source.package.replace("/", "%2f")}/${version}`)) as { dist: { tarball: string } }).dist.tarball;
  const response = await fetch(url, { headers });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  const archive = path.join(folder, "..", `${version}.tgz`);
  writeFileSync(archive, Buffer.from(await response.arrayBuffer()));
  // Both registries pack everything under one top folder. Nothing in the package is run.
  const unpacked = spawnSync("tar", ["-xzf", archive, "-C", folder, "--strip-components=1"], { encoding: "utf8" });
  rmSync(archive, { force: true });
  if (unpacked.status !== 0) throw new Error(`could not unpack ${source.package}@${version}: ${unpacked.stderr}`);
  writeFileSync(path.join(folder, ".complete"), "");
  return folder;
}

function filesIn(folder: string, inside = ""): string[] {
  return readdirSync(path.join(folder, inside)).flatMap((name) => {
    const file = inside ? `${inside}/${name}` : name;
    return statSync(path.join(folder, file)).isDirectory() ? filesIn(folder, file) : [file];
  });
}

/** Copies the files that matter to a folder of their own, so a diff shows only those. */
function keep(source: Source, version: string, folder: string): string {
  const kept = path.join(cache, source.id, `${version}.kept`);
  rmSync(kept, { recursive: true, force: true });
  for (const file of filesIn(folder)) {
    if (!source.files.some((pattern) => matches(file, pattern))) continue;
    mkdirSync(path.dirname(path.join(kept, file)), { recursive: true });
    cpSync(path.join(folder, file), path.join(kept, file));
  }
  mkdirSync(kept, { recursive: true });
  return kept;
}

function diff(before: string, after: string, ...options: string[]): string {
  // Exit code 1 means "they differ", which is the expected case.
  // Run next to the two folders and name them by their own names, so no path of this machine is in the output.
  const result = spawnSync("git", ["diff", "--no-index", "--no-color", ...options, path.basename(before), path.basename(after)], { encoding: "utf8", maxBuffer: 1 << 28, cwd: path.dirname(before) });
  return result.stdout;
}

/** Installs a version of an adapter with what it needs to run, without running its scripts. */
function install(source: Source, version: string): string | null {
  const folder = path.join(cache, source.id, `${version}.installed`);
  const manifest = path.join(folder, "node_modules", source.package, "package.json");
  if (!existsSync(manifest)) {
    mkdirSync(folder, { recursive: true });
    const done = spawnSync("npm", ["install", "--prefix", folder, "--ignore-scripts", "--no-audit", "--no-fund", "--silent", `${source.package}@${version}`], { encoding: "utf8" });
    if (done.status !== 0 || !existsSync(manifest)) return null;
  }
  const bin = (JSON.parse(readFileSync(manifest, "utf8")) as { bin?: string | Record<string, string> }).bin;
  const entry = typeof bin === "string" ? bin : Object.values(bin ?? {})[0];
  return entry ? path.join(path.dirname(manifest), entry) : null;
}

const conformance = (): { agents: Record<string, AgentConfig> } | null => {
  const file = path.join(root, "conformance/config.json");
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
};

/** The conformance agent with its adapter replaced by the program at `entry`. */
const withAdapter = (agent: AgentConfig, entry: string): AgentConfig => ({ ...agent, daemon: { ...agent.daemon, command: "node", args: [entry] } });

/** What the adapter says about itself, without any model call. */
async function welcome(agent: AgentConfig): Promise<Json> {
  const adapter = await Adapter.start(agent);
  const said = adapter.welcome as Json;
  await adapter.stop();
  return said;
}

interface Finding {
  source: Source;
  latest: string;
  /** Lines for the summary. */
  notes: string[];
  /** Files written for this source, for the agent to read. */
  written: string[];
  /** The new adapter's program, when it was installed. */
  entry?: string;
}

async function look(source: Source): Promise<Finding | null> {
  const newest = await latest(source);
  if (!newer(source.current, newest)) {
    console.log(`same      ${source.id}  ${source.current}`);
    return null;
  }
  console.log(`NEW       ${source.id}  ${source.current} -> ${newest}`);
  const finding: Finding = { source, latest: newest, notes: [], written: [] };
  const write = (name: string, text: string) => {
    const file = path.join(out, `${source.id}.${name}`);
    writeFileSync(file, text);
    finding.written.push(relative(file));
    return relative(file);
  };
  const [oldFolder, newFolder] = [await download(source, source.current), await download(source, newest)];
  const [oldKept, newKept] = [keep(source, source.current, oldFolder), keep(source, newest, newFolder)];

  const stat = diff(oldKept, newKept, "--stat", "--stat-width=160").trim();
  if (stat) {
    const file = write("diff", diff(oldKept, newKept));
    finding.notes.push(`Files that matter to us changed (\`${file}\`):`, "", "```", stat, "```");
  } else {
    finding.notes.push(`None of the files that matter to us changed (${source.files.join(", ")}).`);
  }

  const inPackage = ["CHANGELOG.md", "changelog.md"].map((name) => path.join(newFolder, name)).find((file) => existsSync(file));
  const changelog = source.changelog
    ? await fetch(source.changelog, { headers }).then((response) => (response.ok ? response.text() : ""), () => "")
    : inPackage ? readFileSync(inPackage, "utf8") : "";
  const between = changelogBetween(changelog, source.current, newest);
  if (between) finding.notes.push("", `Its changelog for these versions is in \`${write("changelog.md", `${between}\n`)}\` (${between.split("\n").length} lines).`);
  else finding.notes.push("", changelog ? "Its changelog has no section for these versions." : "No changelog was found.");

  if (source.schema) {
    const read = (folder: string) => JSON.parse(readFileSync(path.join(folder, source.schema!), "utf8")) as Json;
    const pinned = JSON.parse(readFileSync(path.join(root, "schema/acp-v2.schema.json"), "utf8")) as Json;
    if (!noSchemaChanges(schemaChanges(pinned, read(oldFolder)))) {
      finding.notes.push("", `**The pinned schema is not the one of ${source.current}.** Either sources.json or schema/README.md is out of date.`);
    }
    const changes = schemaChanges(read(oldFolder), read(newFolder));
    write("schema-changes.json", `${JSON.stringify(changes, null, 2)}\n`);
    finding.notes.push("", ...schemaNotes(changes));
  }

  const agent = source.probe ? conformance()?.agents[source.probe] : undefined;
  if (source.probe && !agent) finding.notes.push("", `The adapter was not started: conformance/config.json has no agent \`${source.probe}\`.`);
  if (agent) {
    const entry = install(source, newest);
    if (!entry) {
      finding.notes.push("", "The new adapter could not be installed, so it was not started.");
    } else {
      finding.entry = entry;
      try {
        const [was, is] = [await welcome(agent), await welcome(withAdapter(agent, entry))];
        const changed = differences(was, is);
        write("initialize.json", `${JSON.stringify({ [source.current]: was, [newest]: is }, null, 2)}\n`);
        finding.notes.push("", changed.length
          ? `What the adapter says about itself at \`initialize\` changed:\n\n${changed.map((line) => `- \`${line}\``).join("\n")}`
          : "What the adapter says about itself at `initialize` is the same.");
      } catch (error) {
        finding.notes.push("", `The adapters could not be asked: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  }
  return finding;
}

function schemaNotes(changes: SchemaChanges): string[] {
  if (noSchemaChanges(changes)) return ["The ACP v2 schema says the same as before."];
  const list = (title: string, entries: string[]) => (entries.length ? [`- ${title}: ${entries.map((entry) => `\`${entry}\``).join(", ")}`] : []);
  return [
    "The ACP v2 schema changed:",
    "",
    ...list("**State rules changed** (every client applies these)", changes.stateRulesChanged),
    ...list("Methods added", changes.methodsAdded),
    ...list("Methods removed", changes.methodsRemoved),
    ...list("Types added", changes.typesAdded),
    ...list("Types removed", changes.typesRemoved),
    ...list("Types changed", changes.typesChanged.map(({ name, properties }) => (properties.length ? `${name} (${properties.join(" ")})` : name))),
  ];
}

/** Runs conformance scenarios against the new adapter and returns what the run printed. */
function live(finding: Finding, all: boolean): string {
  const agents = conformance()!.agents;
  const id = finding.source.probe!;
  const file = path.join(out, `${finding.source.id}.conformance-config.json`);
  writeFileSync(file, JSON.stringify({ agents: { ...agents, [id]: withAdapter(agents[id]!, finding.entry!) } }, null, 2));
  const run = spawnSync("node", ["run.ts", id, `--config=${file}`, ...(all ? [] : ["--free"])], { cwd: path.join(root, "conformance"), encoding: "utf8", timeout: 3_600_000 });
  cpSync(path.join(root, "conformance/reports", `${id}.md`), path.join(out, `${finding.source.id}.conformance.md`));
  finding.written.push(relative(path.join(out, `${finding.source.id}.conformance.md`)));
  return run.stdout.trim();
}

/** Starts the agent on one source and returns its report. */
function report(finding: Finding, command: string, commandArgs: string[]): Promise<string> {
  const brief = [
    readFileSync(path.join(here, "brief.md"), "utf8"),
    "## What was found",
    "",
    `Source: ${finding.source.package} (\`${finding.source.id}\`), from ${finding.source.current} to ${finding.latest}.`,
    `Why we depend on it: ${finding.source.why}`,
    "",
    ...finding.notes,
    "",
    "Files to read:",
    ...finding.written.map((file) => `- \`${file}\``),
  ].join("\n");
  return new Promise((resolve, reject) => {
    const child = spawn(command, commandArgs, { cwd: root, stdio: ["pipe", "pipe", "inherit"] });
    let said = "";
    child.stdout.on("data", (chunk) => (said += chunk));
    child.on("error", reject);
    child.on("exit", (code) => (code === 0 && said.trim() ? resolve(said.trim()) : reject(new Error(`the agent ended with ${code}`))));
    child.stdin.end(brief);
  });
}

mkdirSync(out, { recursive: true });
const unknown = wanted.filter((id) => !config.sources.some((source) => source.id === id));
if (unknown.length) throw new Error(`no such source: ${unknown.join(", ")}. Known: ${config.sources.map((source) => source.id).join(", ")}`);
const findings: Finding[] = [];
for (const source of config.sources.filter((entry) => wanted.length === 0 || wanted.includes(entry.id))) {
  try {
    const finding = await look(source);
    if (finding) findings.push(finding);
  } catch (error) {
    console.log(`FAILED    ${source.id}  ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  }
}

const liveFlag = flag("live");
if (liveFlag) {
  for (const finding of findings.filter((entry) => entry.entry)) {
    console.log(`\nconformance against ${finding.source.package}@${finding.latest}:`);
    const printed = live(finding, liveFlag === "--live=all");
    console.log(printed);
    finding.notes.push("", `The conformance scenarios${liveFlag === "--live=all" ? "" : " that spend no model turn"} against the new adapter:`, "", "```", printed, "```");
  }
}

const summary = [
  `# Upstream watch, ${today}`,
  "",
  findings.length ? `${findings.length} of ${config.sources.length} sources have a newer version.` : "Nothing is newer than what we use.",
  "",
  ...findings.flatMap((finding) => [`## ${finding.source.id}: ${finding.source.current} to ${finding.latest}`, "", `${finding.source.package}. ${finding.source.why}`, "", ...finding.notes, ""]),
].join("\n");
writeFileSync(path.join(out, "summary.md"), summary);
console.log(`\n${relative(path.join(out, "summary.md"))}`);

if (flag("report") && findings.length) {
  const given = flag("agent")?.split("=").slice(1).join("=").split(" ").filter(Boolean);
  const [command, ...commandArgs] = given?.length ? given : [config.agent.command, ...config.agent.args];
  mkdirSync(path.join(root, "docs/updates"), { recursive: true });
  for (const finding of findings) {
    const file = path.join(root, "docs/updates", `${today}-${finding.source.id}.md`);
    try {
      writeFileSync(file, `${await report(finding, command!, commandArgs)}\n`);
      console.log(`report    ${relative(file)}`);
    } catch (error) {
      console.log(`FAILED    report for ${finding.source.id}: ${error instanceof Error ? error.message : String(error)}`);
      process.exitCode = 1;
    }
  }
}
