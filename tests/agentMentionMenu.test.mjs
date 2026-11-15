import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function render(options, activeIndex = 0) {
  const selected = [], highlighted = [], effects = [];
  const listRef = { current: null };
  const jsx = (type, props) => ({ type, props: props ?? {} });
  const exports = {};
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/components/AgentMentionMenu.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  }).outputText, {
    exports,
    require(id) {
      if (id === "react") return { useRef: () => listRef, useEffect: (callback) => effects.push(callback) };
      if (id === "react/jsx-runtime") return { jsx, jsxs: jsx };
      if (id === "./AgentAvatar") return { AgentAvatar: "avatar" };
      if (id.startsWith("@phosphor-icons/react/dist/csr/")) return { [`${id.split("/").at(-1)}Icon`]: "icon" };
      throw new Error(`Unexpected import: ${id}`);
    },
  });
  const tree = exports.AgentMentionMenu({ id: "fixture-list", options, activeIndex, onSelect: (option) => selected.push(option), onHighlight: (index) => highlighted.push(index) });
  const visit = (node) => Array.isArray(node) ? node.flatMap(visit) : node && typeof node === "object" ? [node, ...visit(node.props?.children)] : [];
  return { tree, nodes: visit(tree), selected, highlighted, effects, listRef };
}

test("候选只展示具体 Agent，保留键盘索引及成员身份", () => {
  const options = [
    { key: "agent", token: "@审查员", kind: "agent", agentId: "exact-agent", runtimeKind: "codex", description: "Codex · 审查", section: "具体 Agent", targetCount: 1 },
    { key: "developer", token: "@开发员", kind: "agent", agentId: "developer", runtimeKind: "codex", description: "Codex · 开发", section: "具体 Agent", targetCount: 1 },
  ];
  const h = render(options, 1);
  const rows = h.nodes.filter((node) => node.props.role === "option");
  assert.equal(rows.length, 2);
  assert.equal(rows.filter((node) => node.props["aria-selected"]).length, 1);
  assert.equal(rows[1].props.id, "fixture-list-option-1");
  rows[1].props.onClick();
  assert.equal(h.selected[0], options[1]);
  rows[0].props.onClick();
  assert.equal(h.selected[1], options[0]);
  rows[1].props.onMouseMove();
  assert.deepEqual(h.highlighted, [1]);
  let prevented = false;
  rows[1].props.onMouseDown({ preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
});

test("键盘选项超出可视区域时只滚动菜单结果区", () => {
  const h = render([]);
  const active = { offsetTop: 400, offsetHeight: 42 };
  const list = { querySelector: () => active, scrollTop: 0, clientHeight: 180 };
  h.listRef.current = list;
  h.effects[0]();
  assert.equal(list.scrollTop, 262);
  active.offsetTop = 4;
  h.effects[0]();
  assert.equal(list.scrollTop, 4);
});

test("Esc 关闭后相同光标的 selection 事件不会重新打开候选", () => {
  const source = readFileSync(new URL("../src/components/MultiAgentControl.tsx", import.meta.url), "utf8");
  const file = ts.createSourceFile("MultiAgentControl.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  let callback;
  const visit = (node) => {
    if (ts.isJsxAttribute(node) && node.name.getText(file) === "onSelect" && node.initializer && ts.isJsxExpression(node.initializer) && ts.isArrowFunction(node.initializer.expression)) callback = node.initializer.expression;
    ts.forEachChild(node, visit);
  };
  visit(file);
  assert.ok(callback);
  const changes = [];
  const exports = {};
  vm.runInNewContext(ts.transpileModule(`exports.select = ${callback.getText(file)};`, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText, {
    exports, mentionCursor: 1, setMentionCursor: (value) => changes.push(["cursor", value]),
    setMentionOpen: (value) => changes.push(["open", value]), activeMentionAt: () => ({}),
  });
  exports.select({ currentTarget: { selectionStart: 1, value: "@" } });
  assert.deepEqual(changes, []);
  exports.select({ currentTarget: { selectionStart: 0, value: "@" } });
  assert.deepEqual(changes, [["cursor", 0], ["open", true]]);
});
