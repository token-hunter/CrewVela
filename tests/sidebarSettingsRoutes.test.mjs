import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const sidebarSource = readFileSync(
  new URL("../src/components/AppSidebar.tsx", import.meta.url),
  "utf8",
);

test("首页快捷入口路由到设置分区，多智能体打开右侧总控", () => {
  // 侧栏快捷入口已泛化为 onOpenSection，具体打开哪个分区由快捷入口配置决定。
  assert.match(source, /onOpenSection=\{openSettingsSection\}/);
  assert.match(source, /RightSidebarActivityBar/);
  assert.doesNotMatch(source, /onOpenMultiAgent=/);
  assert.doesNotMatch(source, /onOpenKnowledge=/);
  assert.match(sidebarSource, /props\.onOpenSection\(option\.id\)/);
  assert.doesNotMatch(sidebarSource, /KnowledgeBaseDialog/);
});

test("侧栏快捷入口可自定义，且默认保留自动化", () => {
  assert.match(sidebarSource, /aria-label="自定义快捷入口"/);
  assert.match(sidebarSource, /sidebar-shortcut-picker/);
  assert.match(sidebarSource, /SIDEBAR_SHORTCUT_OPTIONS\.filter/);
  assert.match(
    readFileSync(new URL("../src/sidebarShortcuts.ts", import.meta.url), "utf8"),
    /DEFAULT_SIDEBAR_SHORTCUTS: SettingsSection\[\] = \[\s*"automation",\s*\]/,
  );
});
