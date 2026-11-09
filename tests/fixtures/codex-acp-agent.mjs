// 假 codex-acp 适配器：native_codex_sessions 单测用的标准 ACP 对端。
// 覆盖：initialize(loadSession)、session/new、session/load、流式 thought/message、
// 工具调用、权限反向请求、session/cancel、session/set_config_option。
import readline from "node:readline";
const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
const configOptions = [
  { id: "model", type: "select", currentValue: "gpt-test", options: [
    { name: "Test Model", value: "gpt-test" },
    { name: "Alt Model", value: "gpt-alt" },
  ] },
];
let promptId;
let permissionId;
let goal = null;
let goalRequestId;
let goalRunning = false;
const publishGoal = () => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "session_info_update", _meta: { goal } } } });
const threadStatus = (type) => send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "session_info_update", _meta: { codex: { threadStatus: { type } } } } } });
for await (const line of readline.createInterface({ input: process.stdin })) {
  const frame = JSON.parse(line);
  const reply = (result) => send({ jsonrpc: "2.0", id: frame.id, result });
  if (!frame.method && frame.id === permissionId) {
    const outcome = frame.result?.outcome ?? {};
    const allowed = outcome.outcome === "selected" && (outcome.optionId === "always" || outcome.optionId === "once");
    if (permissionId === "goal-permission") {
      if (goal && goal.status === "active") { goal.status = allowed ? "complete" : "blocked"; publishGoal(); }
      threadStatus("idle"); goalRunning = false;
      if (goalRequestId) { send({ jsonrpc: "2.0", id: goalRequestId, result: {} }); goalRequestId = undefined; }
      continue;
    }
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "tool_call_update", toolCallId: "read", status: allowed ? "completed" : "failed" } } });
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: allowed ? "，批准后的结尾。" : "，已被拒绝。" } } } });
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "end_turn" } });
  } else if (frame.method === "initialize") {
    reply({ protocolVersion: 1, agentCapabilities: { loadSession: true, mcpCapabilities: { http: true } }, agentInfo: { name: "codex-fake", title: "Codex Fake", version: "1.0" }, _meta: { crewvelaGoal: { version: 1, nativeSubscriptions: true }, crewvelaPlugins: { version: 1, nativeMentions: true }, goal: { version: 1, controlMethod: "_session/goal", actions: ["set", "pause", "resume", "clear"] } } });
  } else if (frame.method === "_crewvela/plugins") {
    // Catalog reads can interleave session notifications. This catches callers
    // holding the publication mutex while the request pumps an update callback.
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "usage_update", used: 1, size: 258000 } } });
    reply({ marketplaces: [{ name: "test", plugins: [{ id: "figma@test", name: "figma", installed: true, enabled: true, availability: "AVAILABLE" }, { id: "disabled@test", installed: true, enabled: false, availability: "AVAILABLE" }] }], marketplaceLoadErrors: [] });
  } else if (frame.method === "session/new") {
    reply({ sessionId: "codex-fake", configOptions });
  } else if (frame.method === "session/load") {
    if (frame.params.sessionId === "missing") {
      send({ jsonrpc: "2.0", id: frame.id, error: { code: -32602, message: "session missing" } });
    } else {
      // 真实 codex-acp 的 load 响应不带 sessionId：验证连接层按请求 ID 回填
      send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: frame.params.sessionId, update: { sessionUpdate: "usage_update", used: 24000, size: frame.params.sessionId === "invalid-usage" ? 0 : 258000 } } });
      reply({ configOptions });
    }
  } else if (frame.method === "session/set_config_option") {
    if (frame.params.configId === "model") configOptions[0].currentValue = frame.params.value;
    reply({ configOptions });
  } else if (frame.method === "_session/goal") {
    const action = frame.params.action;
    if (action === "set" && frame.params.objective === "fail") {
      send({ jsonrpc: "2.0", id: frame.id, error: { code: -32602, message: "goal rejected" } });
      continue;
    }
    if (action === "clear") { goal = null; publishGoal(); reply({}); continue; }
    if (action === "pause") { if (goal) goal.status = "paused"; publishGoal(); reply({}); continue; }
    goal = { objective: frame.params.objective ?? goal?.objective ?? "resumed", status: "active", tokenBudget: null, tokensUsed: 123, timeUsedSeconds: 3, createdAt: 1, updatedAt: 2 };
    publishGoal();
    threadStatus("active");
    goalRunning = true;
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "正在执行真实目标协议。" } } } });
    if (goal.objective === "complete") { goal.status = "complete"; publishGoal(); threadStatus("idle"); goalRunning = false; reply({}); continue; }
    if (goal.objective === "early-ack") { reply({}); continue; }
    goalRequestId = frame.id;
    if (goal.objective === "approval") {
      promptId = frame.id;
      permissionId = "goal-permission";
      send({ jsonrpc: "2.0", id: permissionId, method: "session/request_permission", params: { options: [{ kind: "allow_once", optionId: "once" }, { kind: "reject_once", optionId: "reject" }], toolCall: { toolCallId: "goal-read", title: "目标审批" } } });
    }
  } else if (frame.method === "session/prompt") {
    promptId = frame.id;
    // 双向 JSON-RPC 各自分配请求 ID：权限反向请求刻意与 prompt 同号，验证连接层按 method 区分。
    permissionId = frame.id;
    const text = frame.params.prompt.find((block) => block.type === "text")?.text ?? "";
    if (text.startsWith("usage:")) {
      const usage = text === "usage:invalid" ? { used: -1, size: 0 } : text === "usage:compact" ? { used: 24000, size: 258000 } : { used: 168000, size: 258000 };
      send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "usage_update", ...usage, cost: { amount: 99 } } } });
      reply({ stopReason: "end_turn" });
      continue;
    }
    if (text === "cancel") {
      // 挂起等待 session/cancel：先流出思考与首段，便于测试中断时保留部分内容
      send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "先查看相关文件。" } } } });
      send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "流式开头" } } } });
      continue;
    }
    const links = frame.params.prompt.filter((block) => block.type === "resource_link");
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "先查看相关文件。" } } } });
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: links.length ? "附件已收到，流式开头" : "流式开头" } } } });
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: "codex-fake", update: { sessionUpdate: "tool_call", toolCallId: "read", title: "读取文件", kind: "read", rawInput: { path: "package.json" } } } });
    send({ jsonrpc: "2.0", id: permissionId, method: "session/request_permission", params: { options: [
      { kind: "allow_always", optionId: "always" },
      { kind: "allow_once", optionId: "once" },
      { kind: "reject_once", optionId: "reject" },
    ], toolCall: { toolCallId: "read", title: "读取文件", rawInput: { path: "package.json" } } } });
  } else if (frame.method === "session/cancel") {
    if (goalRunning) { if (goalRequestId) send({ jsonrpc: "2.0", id: goalRequestId, result: {} }); goalRequestId = undefined; setTimeout(() => threadStatus("idle"), 80); goalRunning = false; continue; }
    if (goalRequestId) { send({ jsonrpc: "2.0", id: goalRequestId, result: {} }); goalRequestId = undefined; threadStatus("idle"); continue; }
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "cancelled" } });
  } else throw new Error(`unsupported method ${frame.method}`);
}
