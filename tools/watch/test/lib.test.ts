import assert from "node:assert/strict";
import test from "node:test";
import { changelogBetween, differences, matches, newer, noSchemaChanges, schemaChanges } from "../lib.ts";

test("versions are compared as numbers", () => {
  assert.equal(newer("0.109.1", "0.112.0"), true);
  assert.equal(newer("2.9.1", "2.12.0"), true, "12 is more than 9");
  assert.equal(newer("2.12.0", "2.9.1"), false);
  assert.equal(newer("1.6.0", "1.6.0"), false);
  assert.equal(newer("1.6.0-beta.2", "1.6.0"), true, "a release is newer than its pre-release");
  assert.equal(newer("1.6.0", "1.6.0-beta.2"), false);
  assert.equal(newer("v1.2", "1.2.1"), true);
});

test("files are chosen by pattern", () => {
  assert.equal(matches("schema/v2/schema.unstable.json", "schema/v2/*"), true);
  assert.equal(matches("schema/v2/deep/file.json", "schema/v2/*"), false);
  assert.equal(matches("dist/index.js", "dist/**/*.js"), true);
  assert.equal(matches("dist/v2/tools/call.js", "dist/**/*.js"), true);
  assert.equal(matches("dist/v2/tools/call.js.map", "dist/**/*.js"), false);
  assert.equal(matches("sdk.d.ts", "sdk.d.ts"), true);
  assert.equal(matches("sdkXd.ts", "sdk.d.ts"), false);
});

test("a changelog is cut to the versions in between", () => {
  const text = ["# Changelog", "", "## 0.112.0", "- newest", "", "## [0.111.0] - 2026-11-20", "- middle", "", "## v0.109.1", "- ours", "", "## 0.108.0", "- old"].join("\n");
  assert.equal(changelogBetween(text, "0.109.1", "0.112.0"), "## 0.112.0\n- newest\n\n## [0.111.0] - 2026-11-20\n- middle");
  assert.equal(changelogBetween(text, "0.109.1", "0.111.0"), "## [0.111.0] - 2026-11-20\n- middle");
  assert.equal(changelogBetween(text, "0.112.0", "0.113.0"), "");
  assert.equal(changelogBetween("no headings here", "1.0.0", "2.0.0"), "");
});

test("two schemas are compared by what the library uses of them", () => {
  const before = {
    "x-methods": { agent: { "session/new": {}, "session/old": {} }, client: { "fs/read": {} } },
    $defs: {
      Kept: { description: "says one thing", type: "object", properties: { a: { type: "string" } } },
      Grown: { type: "object", properties: { a: { type: "string" }, gone: { type: "number" }, turned: { type: "string" } } },
      Ruled: { type: "object", "x-state": { session: [{ action: "upsert", target: "plan" }] } },
      Scoped: { type: "object", "x-scope": { session: ["sessionId"] } },
      Dropped: { type: "string" },
    },
  };
  const after = {
    "x-methods": { agent: { "session/new": {}, "session/fork": {} }, client: { "fs/read": {} } },
    $defs: {
      Kept: { description: "says it differently", type: "object", properties: { a: { type: "string", description: "new words" } } },
      Grown: { type: "object", properties: { a: { type: "string" }, fresh: { type: "boolean" }, turned: { type: "number" } } },
      Ruled: { type: "object", "x-state": { session: [{ action: "replace", target: "plan" }] } },
      Scoped: { type: "object", "x-scope": { session: ["sessionId"], toolCall: ["toolCallId"] } },
      Fresh: { type: "string" },
    },
  };
  assert.deepEqual(schemaChanges(before, after), {
    methodsAdded: ["agent: session/fork"],
    methodsRemoved: ["agent: session/old"],
    typesAdded: ["Fresh"],
    typesRemoved: ["Dropped"],
    typesChanged: [{ name: "Grown", properties: ["+fresh", "-gone", "~turned"] }, { name: "Ruled", properties: [] }, { name: "Scoped", properties: [] }],
    // A scope is not a state rule: the generator of the rules reads `x-state` only.
    stateRulesChanged: ["Ruled"],
  });
  assert.equal(noSchemaChanges(schemaChanges(before, before)), true);
  assert.equal(noSchemaChanges(schemaChanges(before, after)), false);
});

test("two answers are compared by path", () => {
  assert.deepEqual(
    differences({ capabilities: { session: { fork: {}, title: {} } }, version: 1 }, { capabilities: { session: { title: { more: true }, status: {} } }, version: 1 }),
    ["capabilities.session.fork: removed", "capabilities.session.status: added", "capabilities.session.title.more: added"],
  );
  assert.deepEqual(differences({ a: [1, 2] }, { a: [1, 3] }), ["a: changed"]);
  assert.deepEqual(differences({ a: 1 }, { a: 1 }), []);
});
