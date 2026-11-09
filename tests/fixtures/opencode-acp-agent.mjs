// 假 opencode CLI ACP 对端（模拟 `opencode acp`）：native_acp_sessions opencode_chat flavor 单测用。
// 覆盖：initialize（protocolVersion 1 + 能力面 + terminal-auth meta）、session/new（configOptions
// model/effort/mode + available_commands_update）、session/load（同 id + 响应前回放历史
// 通知 —— opencode 原生行为，验证回放护栏）、流式输出、思考流、工具调用、权限反向请求
// （与 prompt 同号，验证按 method 区分）、session/cancel、session/set_config_option。
import readline from "node:readline";
const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
let model = "opencode-test";
let effort = "medium";
let mode = "build";
// 每轮完整文本，session/load 时按 opencode 原生行为在响应前回放。
let turns = [];
let promptId;
let permissionId;
const configOptions = () => [
  { id: "model", type: "select", currentValue: model, options: [
    { name: "OpenCode Test", value: "opencode-test" },
    { name: "OpenCode Alt", value: "opencode-alt" },
  ] },
  { id: "effort", type: "select", currentValue: effort, options: [
    { name: "Medium", value: "medium" },
    { name: "High", value: "high" },
  ] },
  { id: "mode", type: "select", currentValue: mode, options: [
    { name: "Build", value: "build" },
    { name: "Plan", value: "plan" },
  ] },
];
const chunk = (text) => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "opencode-fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } } } });
const thought = (text) => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "opencode-fake", update: { sessionUpdate: "agent_thought_chunk", content: { type: "text", text } } } });
const commandsUpdate = () => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "opencode-fake", update: { sessionUpdate: "available_commands_update", availableCommands: [
  { name: "/compact", description: "压缩上下文" },
  { name: "/init", description: "初始化项目说明", input: { hint: "[force]" } },
] } } });
for await (const line of readline.createInterface({ input: process.stdin })) {
  const frame = JSON.parse(line);
  const reply = (result) => send({ jsonrpc: "2.0", id: frame.id, result });
  if (!frame.method && frame.id === permissionId) {
    const outcome = frame.result?.outcome ?? {};
    const allowed = outcome.outcome === "selected" && outcome.optionId !== "reject";
    const ending = allowed ? "，批准后的结尾。" : "，已被拒绝。";
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "opencode-fake", update: { sessionUpdate: "tool_call_update", toolCallId: "read", status: allowed ? "completed" : "failed" } } });
    chunk(ending);
    turns.push("流式回复" + ending);
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "end_turn" } });
  } else if (frame.method === "initialize") {
    reply({ protocolVersion: 1, agentCapabilities: {
      loadSession: true,
      promptCapabilities: { image: true, audio: false, embeddedContext: true },
      sessionCapabilities: { list: {}, resume: {}, close: {}, fork: {} },
      mcpCapabilities: { http: true, sse: true },
    }, authMethods: [{ id: "opencode-login", name: "Login with opencode", description: "Run `opencode auth login` in the terminal" }], agentInfo: { name: "OpenCode", version: "1.18.34" } });
  } else if (frame.method === "session/new") {
    turns = [];
    reply({ sessionId: "opencode-fake", configOptions: configOptions() });
    commandsUpdate();
  } else if (frame.method === "session/load") {
    if (frame.params.sessionId === "missing") {
      send({ jsonrpc: "2.0", id: frame.id, error: { code: -32602, message: "session not found: missing" } });
    } else {
      // opencode 在 session/load 响应前回放持久化历史（native_acp_sessions 需靠回放护栏丢弃）。
      for (const turn of turns) chunk(turn);
      reply({ sessionId: frame.params.sessionId, configOptions: configOptions() });
    }
  } else if (frame.method === "session/set_config_option") {
    if (frame.params.configId === "model") model = frame.params.value;
    else if (frame.params.configId === "effort") effort = frame.params.value;
    else if (frame.params.configId === "mode") mode = frame.params.value;
    reply({ configOptions: configOptions() });
    // set_config 一定发生在 start 返回之后（update 处理器已挂载），重发一次命令面板保证确定性。
    commandsUpdate();
  } else if (frame.method === "session/set_mode") {
    mode = frame.params.modeId;
    reply({});
  } else if (frame.method === "session/prompt") {
    promptId = frame.id;
    // 双向 JSON-RPC 各自分配请求 ID：权限反向请求刻意与 prompt 同号，验证连接层按 method 区分。
    permissionId = frame.id;
    const text = frame.params.prompt.find((block) => block.type === "text")?.text ?? "";
    if (text === "fixture free limit") {
      process.stderr.write('timestamp=2026-11-08T00:00:00Z level=ERROR run=fixture message="stream error" providerID=opencode modelID=fixture session.id=opencode-fake small=false error.error="AI_APICallError: Free usage limit reached. apiKey=DO_NOT_DISPLAY_FIXTURE_SECRET"\n');
      continue; // Real OpenCode leaves the ACP prompt unresolved after this error.
    }
    if (text === "cancel") continue;
    const links = frame.params.prompt.filter((block) => block.type === "resource_link");
    thought("思考一下");
    chunk(links.length ? "附件已收到，流式开头" : "流式回复");
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "opencode-fake", update: { sessionUpdate: "tool_call", toolCallId: "read", title: "读取文件", kind: "read", rawInput: { path: "package.json" } } } });
    send({ jsonrpc: "2.0", id: permissionId, method: "session/request_permission", params: { sessionId: "opencode-fake", toolCall: { toolCallId: "read", title: "读取文件" }, options: [
      { kind: "allow_once", optionId: "once" },
      { kind: "allow_always", optionId: "always" },
      { kind: "reject_once", optionId: "reject" },
    ] } });
  } else if (frame.method === "session/cancel") {
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "cancelled" } });
  } else if (frame.method === "session/close" || frame.method === "session/list") {
    reply(frame.method === "session/close" ? {} : { sessions: [] });
  } else throw new Error(`unsupported method ${frame.method}`);
}
