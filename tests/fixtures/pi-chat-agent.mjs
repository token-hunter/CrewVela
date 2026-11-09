// pi --mode rpc 假对端：模拟握手三连、流式增量、工具事件、confirm 审批桥、
// abort 取消与 --session 恢复（写 pi-resumed-marker.json 标记收到恢复参数）。
// 覆盖 native_pi_sessions.rs 三个内嵌测试所需的全部协议面。
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
const sessionIdArg = (() => {
  const argv = process.argv;
  const index = argv.indexOf("--session");
  return index >= 0 ? argv[index + 1] : null;
})();
if (sessionIdArg) {
  // 后端恢复时传 --session <file>；按文件名回显 sessionId 并落 marker 供测试断言。
  fs.writeFileSync(
    path.resolve("pi-resumed-marker.json"),
    JSON.stringify({ session: sessionIdArg }),
  );
}
const sessionId = sessionIdArg
  ? path.basename(sessionIdArg).replace(/\.jsonl$/, "")
  : (() => {
      // 每次创建新 session 都用不同身份，以验证空会话重建只在授权场景放宽 ID 校验。
      const counterPath = path.resolve("pi-session-counter.json");
      let counter = 0;
      try {
        counter = JSON.parse(fs.readFileSync(counterPath, "utf8")).counter;
      } catch {}
      counter += 1;
      fs.writeFileSync(counterPath, JSON.stringify({ counter }));
      return `pi-fake-new-${counter}`;
    })();
const sessionFile = path.resolve(`${sessionId}.jsonl`);
let thinkingLevel = "off";
let model = { id: "test-model", name: "Pi Test Model", provider: "faux" };
let resolveDialog = null;
let inflightHold = false; // cancel 测试：等待 abort 才 settled

const emit = (type, extra = {}) => send({ type, ...extra });
const respond = (frame, data) =>
  send({ type: "response", command: frame.type, id: frame.id, success: true, data });

function promptTurn() {
  emit("message_update", {
    usage: { input: 10, output: 1, totalTokens: 11 },
    assistantMessageEvent: { type: "thinking_delta", contentIndex: 0, delta: "思考中" },
  });
  emit("message_update", {
    usage: { input: 10, output: 2, totalTokens: 12 },
    assistantMessageEvent: { type: "text_delta", contentIndex: 1, delta: "流式" },
  });
  emit("tool_execution_start", {
    toolCallId: "bash-1",
    toolName: "bash",
    args: { command: "rm -rf /tmp/x" },
  });
  send({
    type: "extension_ui_request",
    id: "dialog-1",
    method: "confirm",
    title: JSON.stringify({
      kind: "crewvela-permission",
      toolCallId: "bash-1",
      toolName: "bash",
      input: { command: "rm -rf /tmp/x" },
    }),
    message: "bash 调用：rm -rf /tmp/x",
  });
  resolveDialog = finishTurn;
}

function finishTurn(allowed) {
  emit("tool_execution_end", {
    toolCallId: "bash-1",
    toolName: "bash",
    result: {
      content: [{ type: "text", text: allowed ? "ok" : "用户在 CrewVela 审批卡片中拒绝了该工具调用" }],
    },
    isError: !allowed,
  });
  emit("message_update", {
    usage: { input: 10, output: 5, totalTokens: 15 },
    assistantMessageEvent: {
      type: "text_delta",
      contentIndex: 1,
      delta: allowed ? "，批准后的结尾。" : "，已被拒绝。",
    },
  });
  emit("agent_settled");
}

const commands = {
  get_state: (frame) =>
    respond(frame, {
      model,
      thinkingLevel,
      isStreaming: false,
      isCompacting: false,
      steeringMode: "all",
      followUpMode: "all",
      sessionFile,
      sessionId,
      autoCompactionEnabled: true,
      messageCount: 0,
      pendingMessageCount: 0,
    }),
  get_available_models: (frame) =>
    respond(frame, {
      models: [model, { id: "alt-model", name: "Pi Alt", provider: "faux" }],
    }),
  get_available_thinking_levels: (frame) =>
    respond(frame, { levels: ["off", "low", "high"] }),
  get_commands: (frame) =>
    respond(frame, {
      commands: [
        { name: "init", description: "初始化项目说明" },
        { name: "custom", description: "自定义命令", input: { hint: "[x]" } },
      ],
    }),
  set_thinking_level: (frame) => {
    thinkingLevel = frame.level;
    respond(frame, undefined);
    emit("thinking_level_changed", { level: thinkingLevel });
  },
  set_model: (frame) => {
    model = { id: frame.modelId, name: frame.modelId, provider: frame.provider };
    respond(frame, model);
  },
  prompt: (frame) => {
    // Pi core 延迟到首条 user/assistant 消息才创建 sessionFile。
    fs.writeFileSync(sessionFile, "");
    respond(frame, { disposition: "started" });
    promptTurn();
  },
  abort: (frame) => {
    respond(frame, undefined);
    if (resolveDialog) {
      // 审批等待期间被取消：由后端 reject 回写 confirmed:false 后再收到 abort。
      resolveDialog(false);
      resolveDialog = null;
    } else {
      inflightHold = false;
      emit("agent_settled");
    }
  },
};

for await (const line of readline.createInterface({ input: process.stdin })) {
  if (!line.trim()) continue;
  let frame;
  try {
    frame = JSON.parse(line);
  } catch {
    continue;
  }
  if (frame.type === "extension_ui_response") {
    if (resolveDialog) {
      const allowed = frame.confirmed === true;
      resolveDialog(allowed);
      resolveDialog = null;
    }
    continue;
  }
  const handler = commands[frame.type];
  if (handler) {
    handler(frame);
  } else {
    send({ type: "response", command: frame.type, id: frame.id, success: false, error: `Unknown command: ${frame.type}` });
  }
}
