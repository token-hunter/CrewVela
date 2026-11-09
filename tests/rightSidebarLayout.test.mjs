import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const sidebar = readFileSync(new URL("../src/components/AppSidebar.tsx", import.meta.url), "utf8");
const activity = readFileSync(new URL("../src/components/RightSidebarActivityBar.tsx", import.meta.url), "utf8");
const tabs = readFileSync(new URL("../src/components/WorkspaceTabs.tsx", import.meta.url), "utf8");

test("多智能体、项目文件和 Git 统一进入右侧栏页签", () => {
  for (const label of ["多智能体", "工作区文件", "源代码控制"]) {
    assert.match(activity, new RegExp(label));
  }
  assert.doesNotMatch(activity, /Claude Code/);
  assert.doesNotMatch(app, /NativeClaudeSidebar/);
  assert.doesNotMatch(activity, /label: "上下文"/);
  assert.match(app, /activeRightSidebarTab/);
  assert.match(app, /unified-right-sidebar/);
  assert.doesNotMatch(sidebar, /sidebar-claude-shortcut/);
  assert.doesNotMatch(sidebar, />多智能体</);
  assert.doesNotMatch(sidebar, /sidebar-git-shortcut/);
});

test("标题栏开关恢复最近使用的右侧页签", () => {
  assert.match(tabs, /aria-label="切换右侧栏"/);
  assert.match(app, /lastRightSidebarTab/);
  assert.match(app, /RIGHT_SIDEBAR_TAB_STORAGE_KEY/);
  assert.match(app, /closeRightSidebar/);
});
