import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const masterControlCss = readFileSync(new URL("../src/styles/master-control.css", import.meta.url), "utf8");
const gitWorkbenchCss = readFileSync(new URL("../src/styles/git-workbench.css", import.meta.url), "utf8");

function sourceFunction(name) {
  const start = app.indexOf(`function ${name}(`);
  assert.notEqual(start, -1, `expected ${name} to exist in App.tsx`);
  const bodyStart = app.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < app.length; index += 1) {
    if (app[index] === "{") depth += 1;
    if (app[index] === "}" && --depth === 0) return app.slice(start, index + 1);
  }
  assert.fail(`expected ${name} to have a complete function body`);
}

function sourceConstant(name) {
  const match = app.match(new RegExp(`^const ${name} = .*;$`, "m"));
  assert.ok(match, `expected ${name} to exist in App.tsx`);
  return match[0];
}

class MemoryStorage {
  values = new Map();
  reads = [];
  writes = [];
  failRead = false;
  failWrite = false;

  getItem(key) {
    this.reads.push(key);
    if (this.failRead) throw new Error("storage read failed");
    return this.values.get(key) ?? null;
  }

  setItem(key, value) {
    this.writes.push([key, value]);
    if (this.failWrite) throw new Error("storage write failed");
    this.values.set(key, String(value));
  }
}

function sidebarWidthHelpers(storage, innerWidth = 1600, localStorageGetter = null) {
  const helperNames = [
    "parseRightSidebarWidth",
    "loadRightSidebarWidth",
    "clampSidebarWidth",
    "saveRightSidebarWidth",
    "loadRightSidebarTab",
  ];
  const constantNames = [
    "RIGHT_SIDEBAR_WIDTH_STORAGE_KEY",
    "RIGHT_SIDEBAR_TAB_STORAGE_KEY",
    "FILE_SIDEBAR_WIDTH_STORAGE_KEY",
    "CONTROL_SIDEBAR_WIDTH_STORAGE_KEY",
    "GIT_SIDEBAR_WIDTH_STORAGE_KEY",
    "BROWSER_SIDEBAR_WIDTH_STORAGE_KEY",
    "DEFAULT_RIGHT_SIDEBAR_WIDTH",
  ];
  const declarations = [
    ...constantNames.map(sourceConstant),
    ...helperNames.map(sourceFunction),
    "globalThis.widthHelpers = { parseRightSidebarWidth, loadRightSidebarWidth, clampSidebarWidth, saveRightSidebarWidth };",
  ].join("\n");
  const compiled = ts.transpileModule(declarations, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText;
  const window = { innerWidth };
  Object.defineProperty(window, "localStorage", {
    get: localStorageGetter ?? (() => storage),
  });
  const context = {
    window,
    sidebarTabKind: tab => String(tab).split(":", 1)[0],
    normalizeRightSidebarTab: value => value,
  };
  vm.runInNewContext(compiled, context);
  return context.widthHelpers;
}

test("所有右侧功能页共用一个 380px 默认宽度和共享 state", () => {
  assert.match(app, /crewvela-right-sidebar-width/);
  assert.match(app, /DEFAULT_RIGHT_SIDEBAR_WIDTH = 380/);
  assert.match(app, /const \[rightSidebarDesiredWidth, setRightSidebarDesiredWidth\] = useState\(loadRightSidebarWidth\)/);
  assert.match(app, /const rightSidebarWidth = Math\.min\(rightSidebarDesiredWidth, workspaceAvailableWidth\)/);
  assert.doesNotMatch(app, /set(?:File|Control|Git|Browser)SidebarWidth/);
  assert.doesNotMatch(app, /DEFAULT_(?:FILE|CONTROL|GIT|BROWSER)_SIDEBAR_WIDTH/);
});

test("有效共享宽度优先于旧的分类宽度", () => {
  const storage = new MemoryStorage();
  storage.values.set("crewvela-right-sidebar-width", "444");
  storage.values.set("crewvela-right-sidebar-tab", "files");
  storage.values.set("crewvela-file-sidebar-width", "760");
  const helpers = sidebarWidthHelpers(storage);

  assert.equal(helpers.loadRightSidebarWidth(), 444);
  assert.deepEqual(storage.reads, ["crewvela-right-sidebar-width"]);
  assert.deepEqual(storage.writes, []);
});

test("共享值缺失或非法时，按上次页签类别读取对应旧宽度", () => {
  const cases = [
    ["start", "crewvela-control-sidebar-width", "390", 390],
    ["terminal:main", "crewvela-control-sidebar-width", "410", 410],
    ["multi_agent", "crewvela-control-sidebar-width", "420", 420],
    ["files", "crewvela-file-sidebar-width", "760", 760],
    ["git", "crewvela-git-sidebar-width", "430", 430],
    ["browser", "crewvela-browser-sidebar-width", "480", 480],
  ];

  for (const [tab, legacyKey, legacyValue, expected] of cases) {
    const storage = new MemoryStorage();
    storage.values.set("crewvela-right-sidebar-tab", tab);
    storage.values.set(legacyKey, legacyValue);
    const helpers = sidebarWidthHelpers(storage);
    assert.equal(helpers.loadRightSidebarWidth(), expected, `legacy width for ${tab}`);
    assert.ok(storage.reads.includes(legacyKey), `expected ${legacyKey} to be read for ${tab}`);
  }

  const migrated = new MemoryStorage();
  migrated.values.set("crewvela-right-sidebar-tab", "files");
  migrated.values.set("crewvela-file-sidebar-width", "760");
  assert.equal(sidebarWidthHelpers(migrated).loadRightSidebarWidth(), 760);
  assert.equal(migrated.values.get("crewvela-right-sidebar-width"), "760");
  migrated.values.set("crewvela-right-sidebar-tab", "git");
  migrated.values.set("crewvela-git-sidebar-width", "430");
  assert.equal(sidebarWidthHelpers(migrated).loadRightSidebarWidth(), 760);

  const invalid = new MemoryStorage();
  invalid.values.set("crewvela-right-sidebar-width", "Infinity");
  invalid.values.set("crewvela-right-sidebar-tab", "git");
  invalid.values.set("crewvela-git-sidebar-width", "not-a-width");
  assert.equal(sidebarWidthHelpers(invalid).loadRightSidebarWidth(), 380);
  assert.equal(invalid.values.get("crewvela-right-sidebar-width"), "380");
});

test("没有旧宽度时首次默认值也写入共享 key，写入失败仍采用读取结果", () => {
  const empty = new MemoryStorage();
  assert.equal(sidebarWidthHelpers(empty).loadRightSidebarWidth(), 380);
  assert.equal(empty.values.get("crewvela-right-sidebar-width"), "380");

  const failedWrite = new MemoryStorage();
  failedWrite.failWrite = true;
  failedWrite.values.set("crewvela-right-sidebar-tab", "files");
  failedWrite.values.set("crewvela-file-sidebar-width", "760");
  assert.equal(sidebarWidthHelpers(failedWrite).loadRightSidebarWidth(), 760);
  assert.deepEqual(failedWrite.writes, [["crewvela-right-sidebar-width", "760"]]);
});

test("读取宽度会拒绝非法值并限制在窗口或工作区容器内", () => {
  const storage = new MemoryStorage();
  storage.values.set("crewvela-right-sidebar-width", "1800");
  const helpers = sidebarWidthHelpers(storage, 1200);

  assert.equal(helpers.loadRightSidebarWidth(), 1200);
  assert.equal(helpers.parseRightSidebarWidth("", 1000), null);
  assert.equal(helpers.parseRightSidebarWidth("NaN", 1000), null);
  assert.equal(helpers.parseRightSidebarWidth("-12", 1000), 0);
  assert.equal(helpers.clampSidebarWidth(600, 480), 480);
  assert.equal(helpers.clampSidebarWidth(-12, 480), 0);
  assert.match(app, /\.main-workspace"\)\?\.clientWidth/);
  assert.match(app, /Math\.min\(rightSidebarDesiredWidth, workspaceAvailableWidth\)/);
});

test("localStorage 读取异常回到默认宽度，保存异常不会阻止当前宽度更新", () => {
  const storage = new MemoryStorage();
  storage.failRead = true;
  const helpers = sidebarWidthHelpers(storage);
  assert.equal(helpers.loadRightSidebarWidth(), 380);
  assert.deepEqual(storage.writes, [["crewvela-right-sidebar-width", "380"]]);
  assert.doesNotThrow(() => helpers.saveRightSidebarWidth(512));
  assert.deepEqual(storage.writes, [
    ["crewvela-right-sidebar-width", "380"],
    ["crewvela-right-sidebar-width", "512"],
  ]);

  const failedWrite = new MemoryStorage();
  failedWrite.failWrite = true;
  assert.doesNotThrow(() => sidebarWidthHelpers(failedWrite).saveRightSidebarWidth(512));
  assert.deepEqual(failedWrite.writes, [["crewvela-right-sidebar-width", "512"]]);

  const getterFailure = sidebarWidthHelpers(null, 1600, () => {
    throw new Error("localStorage getter failed");
  });
  assert.doesNotThrow(() => getterFailure.saveRightSidebarWidth(512));

  const persistBody = app.split("const persistRightSidebarWidth = useCallback((width: number) => {")[1].split("}, [clampRightSidebarWidth]);")[0];
  assert.match(persistBody, /setRightSidebarDesiredWidth\(next\);[\s\S]*saveRightSidebarWidth\(next\)/);
  assert.match(sourceFunction("saveRightSidebarWidth"), /window\.localStorage\.setItem\(RIGHT_SIDEBAR_WIDTH_STORAGE_KEY/);
  assert.doesNotMatch(sourceFunction("saveRightSidebarWidth"), /(?:FILE|CONTROL|GIT|BROWSER)_SIDEBAR_WIDTH_STORAGE_KEY/);
});

test("拖拽、键盘和双击复位都更新同一份宽度 state", () => {
  const moveBody = app.split("const moveRightSidebarResize = (")[1].split("const stopRightSidebarResize =")[0];
  const stopBody = app.split("const stopRightSidebarResize = (")[1].split("const resizeRightSidebarWithKeyboard =")[0];
  const keyboardBody = app.split("const resizeRightSidebarWithKeyboard = (")[1].split("const selectRightSidebarTab =")[0];
  assert.match(moveBody, /setRightSidebarDesiredWidth\(next\)/);
  assert.match(stopBody, /persistRightSidebarWidth\(width\)/);
  assert.match(keyboardBody, /persistRightSidebarWidth\(displayedWidth \+ \(event\.key === "ArrowLeft" \? 24 : -24\)\)/);
  assert.match(app, /setPointerCapture\(event\.pointerId\)/);
  assert.match(app, /onDoubleClick=\{\(\) => persistRightSidebarWidth\(DEFAULT_RIGHT_SIDEBAR_WIDTH\)\}/);
  assert.match(app, /role="separator"/);
});

test("遗留的 control-expanded/git-expanded 固定宽度死 CSS 已移除", () => {
  assert.doesNotMatch(masterControlCss, /control-expanded/);
  assert.doesNotMatch(gitWorkbenchCss, /git-expanded/);
});
