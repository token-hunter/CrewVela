import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

const app = fs.readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const bar = fs.readFileSync(
  new URL("../src/components/RightSidebarActivityBar.tsx", import.meta.url),
  "utf8",
);
const team = fs.readFileSync(new URL("../src/teamMessages.ts", import.meta.url), "utf8");

test("活动栏注册浏览器标签", () => {
  assert.match(bar, /\["browser", "浏览器"\]/);
  assert.match(bar, /GlobeIcon/);
});

test("normalizeRightSidebarTab 接受 browser", () => {
  assert.match(team, /value === "browser"/);
});

test("App 保活挂载浏览器面板并传递 active", () => {
  assert.match(app, /\[\.\.\.mountedRightSidebarTabs\]\.filter\(tab => sidebarTabKind\(tab\) === "browser"\)\.map/);
  assert.match(app, /MemoBrowserPanel panelId=\{tab === "browser" \? undefined : tab\.slice\(8\)\} active=\{!guideRequest && activeRightSidebarTab === tab && !sidebarMenuOpen && !appToolsMenuOpen\}/);
});

test("浏览器旧宽度迁移到共享宽度，新浏览器实例复用共享值", () => {
  const ast = ts.createSourceFile("App.tsx", app, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const names = ["parseRightSidebarWidth", "loadRightSidebarWidth", "loadRightSidebarTab", "saveRightSidebarWidth"];
  const functions = ast.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text)).map(node => node.getText(ast));
  assert.equal(functions.length, names.length);
  const js = ts.transpileModule(functions.join("\n") + "\nreturn loadRightSidebarWidth;", { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const storage = new Map([["tab", "browser:instance-1"], ["browser-width", "650"]]);
  const bindings = {
    window: { innerWidth: 1200, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) } },
    RIGHT_SIDEBAR_TAB_STORAGE_KEY: "tab", RIGHT_SIDEBAR_WIDTH_STORAGE_KEY: "shared-width", BROWSER_SIDEBAR_WIDTH_STORAGE_KEY: "browser-width",
    FILE_SIDEBAR_WIDTH_STORAGE_KEY: "files-width", GIT_SIDEBAR_WIDTH_STORAGE_KEY: "git-width", CONTROL_SIDEBAR_WIDTH_STORAGE_KEY: "control-width",
    DEFAULT_RIGHT_SIDEBAR_WIDTH: 380, sidebarTabKind: tab => tab.split(":", 1)[0], normalizeRightSidebarTab: value => value,
  };
  const load = new Function(...Object.keys(bindings), js)(...Object.values(bindings));
  assert.equal(load(), 650);
  assert.equal(storage.get("shared-width"), "650", "legacy browser preference must persist into the shared key");
  storage.set("tab", "browser:instance-2"); storage.set("browser-width", "420");
  assert.equal(load(), 650, "new browser instances must preserve the shared width");
  storage.set("shared-width", "9000"); assert.equal(load(), 1200, "restored width cannot exceed the window");
  storage.set("shared-width", "invalid"); storage.set("browser-width", "invalid");
  assert.equal(load(), 380, "corrupt persisted widths fall back to the shared default");
});

test("关闭右侧栏与切页签会收起浏览器面板", () => {
  assert.match(app, /setBrowserOpen\(false\)/);
  assert.match(app, /setBrowserOpen\(true\)/);
});
