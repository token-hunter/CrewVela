// Keyless ACP peer: deliberately has no modes and resume has no sessionId.
import readline from "node:readline";
const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
const options = [{ id: "model", type: "select", options: [{ group: "test", options: [{ name: "model-a", value: '["test","model-a"]' }] }] }];
let promptId;
let permissionId;
for await (const line of readline.createInterface({ input: process.stdin })) {
  const frame = JSON.parse(line);
  const reply = (result) => send({ jsonrpc: "2.0", id: frame.id, result });
  if (!frame.method && frame.id === permissionId) {
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: frame.result.outcome.optionId === "once" ? "approved" : "rejected" } } } });
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "end_turn" } });
  } else if (frame.method === "initialize") {
    reply({ protocolVersion: 1, agentCapabilities: { mcpCapabilities: { http: true }, sessionCapabilities: { resume: {}, close: {} } }, agentInfo: { name: "fake", version: "1" } });
  } else if (["session/new", "session/resume"].includes(frame.method)) {
    if (!frame.params.mcpServers?.some((server) => server.type === "http" && server.name === "crewvela") || process.env.DSH_PERMISSION_MODE !== process.argv[2]) {
      send({ jsonrpc: "2.0", id: frame.id, error: { code: -32602, message: "MCP or sandbox mismatch" } });
    } else if (frame.params.sessionId === "missing") {
      send({ jsonrpc: "2.0", id: frame.id, error: { code: -32602, message: "session missing" } });
    } else reply(frame.method === "session/new" ? { sessionId: "fake", configOptions: options } : { configOptions: options });
  } else if (frame.method === "session/set_config_option") {
    if (frame.params.value !== '["test","model-a"]') throw new Error("incorrect model value");
    reply({ configOptions: options });
  } else if (frame.method === "session/prompt") {
    promptId = frame.id;
    // Both peers own their ID space: the reverse request deliberately collides.
    permissionId = frame.id;
    if (frame.params.prompt[0].text === "cancel") continue;
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "fake", update: { sessionUpdate: "tool_call", toolCallId: "write", title: "bash", rawInput: { command: "touch example.txt" } } } });
    send({ jsonrpc: "2.0", id: permissionId, method: "session/request_permission", params: { options: [{ kind: "allow_always", optionId: "always" }, { kind: "allow_once", optionId: "once" }, { kind: "reject_once", optionId: "reject" }], toolCall: { toolCallId: "write" } } });
  } else if (frame.method === "session/cancel") {
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "cancelled" } });
  } else if (frame.method === "session/close") reply({});
  else throw new Error(`unsupported method ${frame.method}`);
}
