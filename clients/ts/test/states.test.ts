// The reducer must give exactly the state the Rust core gives, for every recorded session.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { Conversation, Mirror, type Direction, type Json } from "../src/index.ts";

const testdata = path.join(import.meta.dirname, "../../../testdata");

function frames(file: string): { dir: Direction; m: Json }[] {
  return readFileSync(file, "utf8")
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line))
    .filter((frame) => frame.m !== undefined);
}

const plain = (value: unknown): unknown => JSON.parse(JSON.stringify(value));

for (const set of readdirSync(path.join(testdata, "transcripts"))) {
  for (const file of readdirSync(path.join(testdata, "transcripts", set))) {
    const name = `${set}/${path.basename(file, ".ndjson")}`;
    test(`recorded session ${name} gives the reference state`, () => {
      const conversation = new Conversation();
      for (const frame of frames(path.join(testdata, "transcripts", set, file))) {
        conversation.applyMessage(frame.dir, frame.m);
      }
      const expected = JSON.parse(readFileSync(path.join(testdata, "states", `${name}.json`), "utf8"));
      assert.deepStrictEqual(plain(conversation), expected);
    });
  }
}

// What an app sees through the daemon: live, and as a replay. Both must end in the same state.
for (const set of readdirSync(path.join(testdata, "daemon"))) {
  const names = new Set(readdirSync(path.join(testdata, "daemon", set)).map((file) => file.split(".")[0]!));
  for (const name of names) {
    const base = path.join(testdata, "daemon", set, name);
    const expected = JSON.parse(readFileSync(`${base}.state.json`, "utf8"));
    for (const kind of ["live", "replay"]) {
      test(`through the daemon, ${set}/${name} ${kind} gives the reference state`, () => {
        const mirror = new Mirror();
        for (const frame of frames(`${base}.${kind}.ndjson`)) {
          if (frame.dir === "out") {
            mirror.sent(frame.m);
          } else {
            // An attach that announces a replay starts from nothing.
            if ((frame.m as { result?: { catchUp?: string } }).result?.catchUp === "replay") mirror.reset();
            mirror.received(frame.m);
          }
        }
        assert.deepStrictEqual(plain(mirror.conversation), expected);
        assert.ok(mirror.seq > 0);
      });
    }
  }
}
