// The parts of the watch script that only compute: comparing versions, choosing files,
// cutting a changelog, and saying what changed between two ACP schemas.

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type JsonObject = { [key: string]: Json };

const isObject = (value: Json | undefined): value is JsonObject => typeof value === "object" && value !== null && !Array.isArray(value);

/** Whether version `b` is newer than `a`. Numbers are compared as numbers; a pre-release is older than its release. */
export function newer(a: string, b: string): boolean {
  const parse = (version: string) => {
    const [main = "", pre] = version.replace(/^v/, "").split("-", 2);
    return { parts: main.split(".").map((part) => Number.parseInt(part, 10) || 0), pre };
  };
  const [one, two] = [parse(a), parse(b)];
  for (let at = 0; at < Math.max(one.parts.length, two.parts.length); at += 1) {
    const difference = (two.parts[at] ?? 0) - (one.parts[at] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  if (one.pre !== undefined && two.pre === undefined) return true;
  if (one.pre !== undefined && two.pre !== undefined) return two.pre > one.pre;
  return false;
}

/** Whether a path matches a pattern: `*` is anything but `/`, `**` is anything. */
export function matches(file: string, pattern: string): boolean {
  const source = pattern
    .split(/(\*\*\/|\*\*|\*)/)
    .map((part) => (part === "**/" ? "(?:.*/)?" : part === "**" ? ".*" : part === "*" ? "[^/]*" : part.replace(/[.+?^${}()|[\]\\]/g, "\\$&")))
    .join("");
  return new RegExp(`^${source}$`).test(file);
}

/**
 * The part of a changelog about the versions after `from`, up to and including `to`. Sections
 * are found by a heading that names a version, newest first, as changelogs are written.
 * Empty when the versions are not found.
 */
export function changelogBetween(text: string, from: string, to: string): string {
  const lines = text.split("\n");
  const versionOf = (line: string) => /^#{1,3}\s.*?\bv?(\d+\.\d+\.\d+(?:-[\w.]+)?)\b/.exec(line)?.[1];
  const kept: string[] = [];
  let keeping = false;
  for (const line of lines) {
    const version = versionOf(line);
    if (version !== undefined) keeping = newer(from, version) && !newer(to, version);
    if (keeping) kept.push(line);
  }
  return kept.join("\n").trim();
}

/** The value without its explanations, with keys in order: two schemas that say the same compare equal. */
function shape(value: Json): Json {
  if (Array.isArray(value)) return value.map(shape);
  if (!isObject(value)) return value;
  return Object.fromEntries(Object.keys(value).filter((key) => key !== "description" && key !== "title").sort().map((key) => [key, shape(value[key]!)]));
}
const same = (a: Json | undefined, b: Json | undefined) => JSON.stringify(shape(a ?? null)) === JSON.stringify(shape(b ?? null));

export interface SchemaChanges {
  /** Methods by side: `agent: session/fork`. */
  methodsAdded: string[];
  methodsRemoved: string[];
  typesAdded: string[];
  typesRemoved: string[];
  /** Types whose shape changed, with the properties that were added, removed or changed. */
  typesChanged: { name: string; properties: string[] }[];
  /** Types whose `x-state` changed: the state rules, which every client applies. */
  stateRulesChanged: string[];
}

/** What changed between two ACP schema files, by what the library uses of them. */
export function schemaChanges(before: Json, after: Json): SchemaChanges {
  const defs = (schema: Json) => (isObject(schema) && isObject(schema["$defs"]) ? schema["$defs"] : {});
  const methods = (schema: Json) => {
    const sides = isObject(schema) && isObject(schema["x-methods"]) ? schema["x-methods"] : {};
    return new Map(Object.entries(sides).flatMap(([side, list]) => Object.entries(isObject(list) ? list : {}).map(([name, value]) => [`${side}: ${name}`, value] as const)));
  };
  const [oldDefs, newDefs, oldMethods, newMethods] = [defs(before), defs(after), methods(before), methods(after)];
  const changes: SchemaChanges = {
    methodsAdded: [...newMethods.keys()].filter((name) => !oldMethods.has(name)).sort(),
    methodsRemoved: [...oldMethods.keys()].filter((name) => !newMethods.has(name)).sort(),
    typesAdded: Object.keys(newDefs).filter((name) => !(name in oldDefs)).sort(),
    typesRemoved: Object.keys(oldDefs).filter((name) => !(name in newDefs)).sort(),
    typesChanged: [],
    stateRulesChanged: [],
  };
  for (const name of Object.keys(newDefs).sort()) {
    const [was, is] = [oldDefs[name], newDefs[name]];
    if (was === undefined || !isObject(was) || !isObject(is)) continue;
    // Only `x-state`: the rule generator reads nothing else. A changed `x-scope` shows as a changed type.
    if (!same(was["x-state"], is["x-state"])) changes.stateRulesChanged.push(name);
    if (same(was, is)) continue;
    const [oldProperties, newProperties] = [isObject(was["properties"]) ? was["properties"] : {}, isObject(is["properties"]) ? is["properties"] : {}];
    const properties = [
      ...Object.keys(newProperties).filter((key) => !(key in oldProperties)).map((key) => `+${key}`),
      ...Object.keys(oldProperties).filter((key) => !(key in newProperties)).map((key) => `-${key}`),
      ...Object.keys(newProperties).filter((key) => key in oldProperties && !same(oldProperties[key], newProperties[key])).map((key) => `~${key}`),
    ];
    changes.typesChanged.push({ name, properties });
  }
  return changes;
}

export const noSchemaChanges = (changes: SchemaChanges): boolean =>
  Object.values(changes).every((list) => list.length === 0);

/** Where two JSON values differ, as paths: `capabilities.session.fork: added`. */
export function differences(before: Json, after: Json, at = ""): string[] {
  if (isObject(before) && isObject(after)) {
    return [...new Set([...Object.keys(before), ...Object.keys(after)])].sort().flatMap((key) => {
      const where = at ? `${at}.${key}` : key;
      if (!(key in before)) return [`${where}: added`];
      if (!(key in after)) return [`${where}: removed`];
      return differences(before[key]!, after[key]!, where);
    });
  }
  return JSON.stringify(before) === JSON.stringify(after) ? [] : [`${at || "(the value)"}: changed`];
}
