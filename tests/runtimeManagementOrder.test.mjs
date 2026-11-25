import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import * as taxonomy from "../src/runtimeTaxonomy.ts";
import * as installation from "../src/runtimeInstallation.ts";
import * as launch from "../src/runtimeLaunch.ts";
import { reorderRuntimeOrder, persistRuntimeOrder, loadRuntimeOrder } from "../src/runtimePreferences.ts";

const labels = { claude_code: "Claude Code", codex: "Codex", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", zcode_cli: "ZCode CLI", antigravity: "Antigravity CLI", zcode: "ZCode Desktop", codex_chat: "Codex Chat" };
function harness() {
  const disabled = new Set(["kimi_code"]);
  const toggles = [];
  const hooks = [];
  let hookIndex = 0;
  const exports = {};
  const jsx = (type, props) => ({ type, props: props ?? {} });
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/components/RuntimeManagementDialog.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  }).outputText, {
    exports,
    require(id) {
      if (id === "react") return {
        useState(value) { const index = hookIndex++; if (!(index in hooks)) hooks[index] = value; return [hooks[index], (next) => { hooks[index] = typeof next === "function" ? next(hooks[index]) : next; }]; },
        useRef(value) { const index = hookIndex++; return hooks[index] ??= { current: value }; },
        // The browser installer tests exercise the chat dependency effect and its IPC.
        useEffect() {},
      };
      if (id === "react/jsx-runtime") return { jsx, jsxs: jsx };
      if (id === "@tauri-apps/api/core") return { invoke: (command) => assert.fail(`Unexpected runtime IPC: ${command}`) };
      if (id === "./LocalizedErrorText") return { LocalizedErrorText: "localized-error" };
      if (id === "../useInterfaceLanguage") return {
        useTranslation: () => (source, values) => source.replace(/\{(\w+)\}/g, (token, key) => values?.[key] ?? token),
      };
      if (id === "@phosphor-icons/react") return { ArrowUp: "up-icon", ArrowDown: "down-icon", DotsSixVertical: "drag-icon", X: "close-icon" };
      if (id === "./RuntimeInstallWizard") return { RuntimeInstallWizard: "runtime-installer" };
      if (id === "../runtimeInstallation") return installation;
      if (id === "../runtimeLaunch") return launch;
      if (id === "../runtimeAvailability") return { useRuntimeAvailability: () => ({ prefs: { disabledRuntimes: [...disabled] }, ready: true, error: null, isRuntimeEnabled: (kind) => !disabled.has(kind), setRuntimeEnabled: async (...args) => toggles.push(args) }) };
      if (id === "../runtimeLogos") return { RUNTIME_LABELS: labels, RUNTIME_LOGOS: {} };
      if (id === "../runtimeTaxonomy") return taxonomy;
      if (id === "./ui/Modal") return { Modal: "modal" };
      if (id.endsWith(".css")) return {};
      throw new Error(`Unexpected import: ${id}`);
    },
  });
  return { render: (props) => { hookIndex = 0; return exports.RuntimeManagementDialog({ onClose() {}, ...props }); }, toggles };
}
function nodes(root) {
  if (Array.isArray(root)) return root.flatMap(nodes);
  if (!root || typeof root !== "object") return [];
  return [root, ...nodes(root.props?.children)];
}
function button(tree, name) {
  const match = nodes(tree).find((node) => node.type === "button" && node.props["aria-label"] === name);
  assert.ok(match, `Missing reorder button: ${name}`);
  return match;
}

test("管理 Runtime 在各自分组内上下移动，禁用成员可排序并持久化", () => {
  const h = harness();
  const available = ["claude_code", "zcode", "codex", "codex_chat", "kimi_code"];
  let order = [...available];
  let stored = null;
  const storage = { getItem: () => stored, setItem: (_key, value) => { stored = value; } };
  const render = () => h.render({ order, onReorder(kind, target, position) {
    order = reorderRuntimeOrder(order, kind, target, position);
    persistRuntimeOrder(order, storage);
  } });
  let tree = render();
  assert.equal(button(tree, "上移 Claude Code").props.disabled, true);
  assert.equal(button(tree, "下移 Kimi Code").props.disabled, true);
  assert.equal(button(tree, "上移 ZCode Desktop").props.disabled, true);
  assert.equal(button(tree, "下移 Codex Chat").props.disabled, true);
  button(tree, "上移 Codex").props.onClick();
  tree = render();
  assert.deepEqual(order.filter((kind) => taxonomy.runtimeCategory(kind) === "terminal"), ["codex", "claude_code", "kimi_code"]);
  assert.equal(button(tree, "上移 Codex").props.disabled, true);
  button(tree, "上移 Kimi Code").props.onClick();
  assert.deepEqual(loadRuntimeOrder(available, storage), order);
  assert.deepEqual(order.filter((kind) => taxonomy.runtimeCategory(kind) === "chat"), ["zcode", "codex_chat"]);
  tree = render();
  const checkbox = nodes(tree).find((node) => node.type === "input" && node.props["aria-label"] === "启用 Kimi Code");
  assert.equal(checkbox.props.checked, false);
  assert.deepEqual(h.toggles, [], "Reordering must not toggle availability");
  button(tree, "下移 ZCode Desktop").props.onClick();
  assert.deepEqual(order.filter((kind) => taxonomy.runtimeCategory(kind) === "chat"), ["codex_chat", "zcode"]);
});

test("排序保存错误在管理弹窗内展示，按钮不嵌套在开关标签中", () => {
  const h = harness();
  const orderError = "保存 Runtime 排序失败：存储不可写";
  const tree = h.render({ order: ["claude_code", "codex"], onReorder() {}, orderError });
  const alert = nodes(tree).find((node) => node.props.role === "alert");
  assert.ok(alert);
  assert.equal(nodes(alert).find((node) => node.type === "localized-error")?.props.source, orderError);
  for (const label of nodes(tree).filter((node) => node.type === "label")) {
    assert.equal(nodes(label.props.children).some((node) => node.type === "button"), false);
  }
});

test("七种 CLI 管理入口进入和返回不改变开关或排序", () => {
  const h = harness();
  const order = Object.freeze(["claude_code", "codex", "kimi_code", "pi", "opencode", "zcode_cli", "antigravity"]);
  const props = { order, onReorder() { assert.fail("opening installation must not reorder runtimes"); } };
  let tree = h.render(props);
  const entries = nodes(tree).filter(node => node.type === "button" && node.props.className?.includes("collab-runtime-install-entry"));
  assert.equal(entries.length, 7);
  for (const kind of order) {
    nodes(row(tree, kind)).find(node => node.type === "button" && node.props.className?.includes("collab-runtime-install-entry")).props.onClick();
    tree = h.render(props);
    const installer = nodes(tree).find(node => node.type === "runtime-installer");
    assert.equal(installer.props.kind, kind);
    assert.deepEqual(h.toggles, []);
    installer.props.onBack(); tree = h.render(props);
  }
  assert.ok(button(tree, "上移 Codex"));
  assert.deepEqual(nodes(tree).filter(node => node.props["data-runtime-order"]).map(node => node.props["data-runtime-order"]), [...order]);
  assert.equal(nodes(row(tree, "kimi_code")).find(node => node.type === "input").props.checked, false);
});

function row(tree, kind) {
  return nodes(tree).find(node => node.props["data-runtime-order"] === kind);
}
function dragEvent(clientY = 100) {
  return { clientY, target: { closest: () => null }, currentTarget: { getBoundingClientRect: () => ({ top: 100, height: 80 }), contains: () => false },
    dataTransfer: { dropEffect: "none", setData() {} }, preventDefault() {}, stopPropagation() {} };
}

test("鼠标拖拽按目标上下半区排序，禁用项也可移动且不切换开关", () => {
  const h = harness();
  const available = ["claude_code", "codex", "kimi_code", "zcode", "codex_chat"];
  let order = [...available], stored = null;
  const storage = { getItem: () => stored, setItem: (_key, value) => { stored = value; } };
  const render = () => h.render({ order, onReorder(kind, target, position) {
    order = reorderRuntimeOrder(order, kind, target, position); persistRuntimeOrder(order, storage);
  } });
  let tree = render();
  row(tree, "codex").props.onDragStart(dragEvent());
  tree = render();
  assert.match(row(tree, "codex").props.className, /dragging/);
  row(tree, "kimi_code").props.onDragEnter(dragEvent(170));
  tree = render();
  assert.equal(row(tree, "kimi_code").props["data-drop-position"], "after");
  row(tree, "kimi_code").props.onDrop(dragEvent(170));
  assert.deepEqual(order, ["claude_code", "kimi_code", "codex", "zcode", "codex_chat"]);
  tree = render();
  assert.doesNotMatch(row(tree, "codex").props.className, /dragging/);
  row(tree, "kimi_code").props.onDragStart(dragEvent());
  row(tree, "claude_code").props.onDrop(dragEvent(105));
  assert.deepEqual(order.slice(0, 3), ["kimi_code", "claude_code", "codex"]);
  assert.deepEqual(loadRuntimeOrder(available, storage), order);
  assert.deepEqual(h.toggles, []);
  tree = render();
  assert.equal(nodes(tree).find(node => node.props["aria-label"] === "启用 Kimi Code").props.checked, false);
  let prevented = false;
  row(tree, "kimi_code").props.onClickCapture({ preventDefault() { prevented = true; }, stopPropagation() {} });
  assert.equal(prevented, true, "drop must not produce a label click that toggles the runtime");
});

test("跨组、自身、外部拖放与取消不会重排；按钮和开关不开始拖拽", () => {
  const h = harness(), calls = [];
  const render = () => h.render({ order: ["claude_code", "codex", "zcode", "codex_chat"], onReorder: (...args) => calls.push(args) });
  let tree = render();
  row(tree, "codex").props.onDragStart(dragEvent());
  const cross = dragEvent(170);
  row(tree, "zcode").props.onDragOver(cross);
  assert.equal(cross.dataTransfer.dropEffect, "none");
  row(tree, "zcode").props.onDrop(cross);
  row(tree, "claude_code").props.onDrop(dragEvent());
  row(tree, "codex").props.onDragStart(dragEvent());
  row(tree, "codex").props.onDrop(dragEvent());
  row(tree, "codex").props.onDragStart(dragEvent());
  row(tree, "claude_code").props.onDragOver(dragEvent());
  row(tree, "codex").props.onDragEnd();
  tree = render();
  assert.equal(row(tree, "claude_code").props["data-drop-position"], undefined);
  assert.doesNotMatch(row(tree, "codex").props.className, /dragging/);
  const control = dragEvent(); let prevented = false;
  control.target.closest = () => ({}); control.preventDefault = () => { prevented = true; };
  row(tree, "codex").props.onDragStart(control);
  row(tree, "claude_code").props.onDrop(dragEvent());
  assert.equal(prevented, true);
  assert.deepEqual(calls, []);
});
