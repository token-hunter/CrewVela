import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readStyles } from "./helpers/loadStyles.mjs";

const sidebarSource = readFileSync(
  new URL("../src/components/ControlSidebar.tsx", import.meta.url),
  "utf8",
);
const stylesSource = readStyles();
const multiAgentSource = readFileSync(
  new URL("../src/components/MultiAgentControl.tsx", import.meta.url),
  "utf8",
);

test("总控只保留一个可见对话容器且不恢复旧的模式切换器", () => {
  assert.doesNotMatch(sidebarSource, /aria-label="切换对话模式"/);
  assert.doesNotMatch(sidebarSource, /hidden=\{controlSurface/);
  assert.doesNotMatch(sidebarSource, /control-unified-title">总控</);
  assert.doesNotMatch(sidebarSource, /control-header-meta/);
  assert.equal(
    sidebarSource.match(/<MultiAgentControl/g)?.length,
    1,
    "右侧只应存在一个总控对话组件",
  );
});

test("统一总控恢复快捷指挥的六项高频操作入口", () => {
  assert.match(multiAgentSource, /aria-label="快捷指挥发送目标"/);
  assert.match(multiAgentSource, /multi-agent-quick-target-trigger/);
  assert.match(multiAgentSource, /multi-agent-quick-target-menu/);
  assert.match(multiAgentSource, /BroadcastIcon/);
  assert.doesNotMatch(
    multiAgentSource,
    /<select[\s\S]{0,300}aria-label="快捷指挥发送目标"/,
  );
  assert.match(multiAgentSource, /aria-label="统一权限模式"/);
  assert.match(multiAgentSource, /快捷指令/);
  assert.match(multiAgentSource, /打开1个deepseek、1个glm、1个kimi、1个minimax/);
  assert.match(multiAgentSource, /清除对话记录/);
  assert.match(multiAgentSource, /event\.ctrlKey[\s\S]*event\.shiftKey/);
  assert.match(multiAgentSource, /event\.key === "Tab"[\s\S]*applyQuickPromptCompletion/);
});

test("快捷指挥操作由总控层提供真实回调，不是静态按钮", () => {
  assert.match(sidebarSource, /onQuickBroadcast=/);
  assert.match(sidebarSource, /onQuickPermissionModeChange=/);
  assert.match(sidebarSource, /onClearQuickHistory=/);
  assert.match(multiAgentSource, /await onQuickBroadcast\?\./);
  assert.match(multiAgentSource, /await onQuickPermissionModeChange\?\./);
  assert.match(multiAgentSource, /await onClearQuickHistory\?\./);
});

test("统一输入框按目标和语义自动选择快速或协作路由", () => {
  assert.match(multiAgentSource, /resolveUnifiedControlRoute\(/);
  assert.match(multiAgentSource, /await onQuickCommand\?\.\(message\)/);
  assert.doesNotMatch(multiAgentSource, /onQuickVoiceCommand/);
  assert.match(multiAgentSource, /route === "multi_agent"/);
  assert.match(multiAgentSource, /surface !== "quick"/);
  assert.match(multiAgentSource, /multi-agent-trust-notice/);
  assert.match(stylesSource, /\.control-header-right[\s\S]{0,100}margin-left:\s*auto/);
});

test("总控群组目标保留 Kimi Code 并移除停用的 CLI", () => {
  assert.match(
    multiAgentSource,
    /runtimeKind:\s*"kimi_code"[\s\S]{0,120}token:\s*"@KimiCode"/,
  );
  assert.doesNotMatch(multiAgentSource, /runtimeKind:\s*"minimax_code"|@MCode/);
});

test("总控顶部采用单行上下文抽屉而不是常驻第二行", () => {
  assert.match(multiAgentSource, /<b>上下文<\/b>/);
  assert.match(multiAgentSource, /multi-agent-context-overview/);
  assert.match(multiAgentSource, /打开协作详情/);
  assert.doesNotMatch(multiAgentSource, /<b>协作上下文<\/b>/);
  assert.match(stylesSource, /\.control-header[\s\S]{0,180}position:\s*absolute/);
  assert.match(stylesSource, /\.multi-agent-detail-content\.compact[\s\S]{0,240}position:\s*absolute/);
});
