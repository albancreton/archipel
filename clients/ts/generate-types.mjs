// Writes src/generated/bridge.ts from schema/bridge.schema.json. Run by `just gen`.
//
// Only the daemon's own additions are generated. The generator cannot read the ACP v2 schema
// (it trips over that schema's extension keywords), and the reducer does not need ACP types:
// it follows the state rules. An app that wants them can take them from the ACP SDK.
import { compileFromFile } from "json-schema-to-typescript";
import { writeFileSync } from "node:fs";
import path from "node:path";

const types = await compileFromFile(path.join(import.meta.dirname, "../../schema/bridge.schema.json"), {
  bannerComment: "",
  additionalProperties: false,
  unreachableDefinitions: true,
});
const banner = "// Generated from schema/bridge.schema.json by `just gen`. Do not edit.\n\n";
writeFileSync(path.join(import.meta.dirname, "src/generated/bridge.ts"), banner + types);
