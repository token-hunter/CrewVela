import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const sessionData = readFileSync(
  new URL("../src/hooks/useSessionData.ts", import.meta.url),
  "utf8",
);
const control = readFileSync(new URL("../src/components/ControlSidebar.tsx", import.meta.url), "utf8");

test("访问过的右侧面板保持挂载，关闭只隐藏外壳", () => {
  assert.match(app, /mountedRightSidebarTabs/);
  assert.match(app, /right-sidebar-panel/);
  assert.match(app, /memo\(GitSourceControlPanel\)/);
  assert.match(app, /memo\(FileExplorerPanel\)/);
  assert.match(app, /\[\.\.\.mountedRightSidebarTabs\]\.filter\(tab => sidebarTabKind\(tab\) === "files"\)\.map\(tab => \(/);
  assert.match(app, /<div key=\{tab\} className=\{`right-sidebar-panel\$\{activeRightSidebarTab === tab/);
  // 快照守卫随轮询逻辑搬入 useSessionData hook。
  assert.match(sessionData, /sameSessionSnapshot/);
  // 无活跃页签但有已挂载页签时外壳仍渲染（保持挂载），只是隐藏。
  assert.match(app, /\{\(activeRightSidebarTab \|\| mountedRightSidebarTabs\.size > 0\) && \(/);
  // 拖拽把手不再局限于文件页签。
  assert.doesNotMatch(app, /\{activeRightSidebarTab === "files" && \(\s*<div\s*className="right-sidebar-resizer"/);
});

test("隐藏的多智能体面板停止交互但保留连接和组件状态", () => {
  assert.match(control, /active\?: boolean/);
  assert.match(control, /const isActive = active \?\? open/);
  assert.doesNotMatch(app, /NativeClaudeSidebar/);
});
