// A minimal MCP server on stdio with one tool, for the scenario `session.mcp_server`.
// No dependency: MCP on stdio is one JSON-RPC message per line.

import { createInterface } from "node:readline";

const tool = {
  name: "conformance_marker",
  description: "Returns the conformance marker. Takes no arguments.",
  inputSchema: { type: "object", properties: {} },
};
const answers = {
  initialize: (params) => ({
    protocolVersion: params?.protocolVersion ?? "2025-06-18",
    capabilities: { tools: {} },
    serverInfo: { name: "marker", version: "0.0.0" },
  }),
  "tools/list": () => ({ tools: [tool] }),
  "tools/call": () => ({ content: [{ type: "text", text: "OKAPI-902" }] }),
  ping: () => ({}),
};

createInterface({ input: process.stdin }).on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  if (message.id === undefined) return;
  const answer = answers[message.method];
  const reply = answer
    ? { jsonrpc: "2.0", id: message.id, result: answer(message.params) }
    : { jsonrpc: "2.0", id: message.id, error: { code: -32601, message: `no such method: ${message.method}` } };
  process.stdout.write(`${JSON.stringify(reply)}\n`);
});
