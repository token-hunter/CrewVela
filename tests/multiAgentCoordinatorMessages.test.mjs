import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const control = readFileSync(new URL("../src/components/MultiAgentControl.tsx", import.meta.url), "utf8");

test("连续重复的协调器错误只显示一次", () => {
  assert.match(control, /role === "assistant"/);
  assert.match(control, /previous\.content\.trim\(\) === normalizedContent/);
  assert.match(control, /previous\.surface === surface/);
});

test("快照重建的消息带 multi_agent surface，跨来源去重不失效", () => {
  assert.match(control, /coordinatorMessagesFromSnapshot[\s\S]{0,900}surface:\s*"multi_agent"/);
});

test("等待协调器回复时有思考中提示，计划徽章文案不误导", () => {
  assert.match(control, /正在思考，可能需要一分钟/);
  assert.match(control, /确认后启动/);
  assert.doesNotMatch(control, /自动执行/);
});

test("文字路径也接收流式回复并渲染", () => {
  assert.match(control, /textStreamRequestRef/);
  assert.match(control, /setStreamedReply\(\{ requestId: update\.requestId, text: update\.text \}\)/);
  assert.match(control, /\{streamedReply && \(/);
});

test("纯通信计划免确认自动落地", () => {
  assert.match(control, /planIsLowRiskCommunication/);
  assert.match(control, /applyPendingPlan\(nextPlan, nextContext\)/);
});
