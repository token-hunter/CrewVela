import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";
const source = readFileSync(new URL("../src/components/CodexChat.tsx", import.meta.url), "utf8");
const ast = ts.createSourceFile("CodexChat.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
let handler;
function walk(node) { if (ts.isVariableDeclaration(node) && node.name.getText(ast) === "applyEvent") handler = node.initializer.arguments[0]; ts.forEachChild(node, walk); }
walk(ast);
assert.ok(handler);
const js = ts.transpileModule(`return (${handler.getText(ast)});`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
test("Codex reasoning deltas append to historical messages that omit optional reasoning", () => {
  let snapshot = { history: [{ id: "message", role: "assistant", text: "old" }] };
  const sequence = { current: 0 };
  const bindings = { session: { id: "codex" }, sequence, revision: { current: 0 }, setSnapshot: update => { snapshot = update(snapshot); }, onSessionRecord() {}, applySnapshot() {}, setPermission() {}, setError() {}, setToolCalls() {} };
  const apply = new Function(...Object.keys(bindings), js)(...Object.values(bindings));
  apply({ sessionId: "codex", sequence: 1, kind: "assistant_delta", data: { id: "message", text: " reply", reasoning: "thought" } });
  assert.equal(snapshot.history[0].text, "old reply");
  assert.equal(snapshot.history[0].reasoning, "thought");
  apply({ sessionId: "other", sequence: 2, kind: "assistant_delta", data: { id: "message", text: "wrong" } });
  assert.equal(snapshot.history[0].text, "old reply", "foreign-session events remain ignored");
  apply({ sessionId: "codex", sequence: 1, kind: "assistant_delta", data: { id: "message", text: "duplicate" } });
  assert.equal(snapshot.history[0].text, "old reply", "duplicate event sequence remains ignored");
});
