import assert from "node:assert/strict";
import test from "node:test";
import {
  loadRuntimeOrder,
  moveRuntimeOrder,
  normalizeRuntimeOrder,
  persistRuntimeOrder,
  reorderRuntimeOrder,
  NEW_SESSION_CHAT_ORDER_STORAGE_KEY,
  NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY,
  RUNTIME_PREFERENCES_STORAGE_KEY,
} from "../src/runtimePreferences.ts";

const runtimes = ["claude_code", "codex", "pi", "zcode"];

test("Runtime 顺序规范化时保留有效顺序、去重并补上新 Runtime", () => {
  assert.deepEqual(
    normalizeRuntimeOrder(["pi", "ghost", "pi", "codex"], runtimes),
    ["pi", "codex", "claude_code", "zcode"],
  );
  assert.deepEqual(normalizeRuntimeOrder(null, runtimes), runtimes);
});

test("键盘左右移动会交换相邻 Runtime 且边界操作不改变顺序", () => {
  assert.deepEqual(moveRuntimeOrder(runtimes, "codex", -1), ["codex", "claude_code", "pi", "zcode"]);
  assert.deepEqual(moveRuntimeOrder(runtimes, "codex", 1), ["claude_code", "pi", "codex", "zcode"]);
  assert.deepEqual(moveRuntimeOrder(runtimes, "claude_code", -1), runtimes);
  assert.deepEqual(moveRuntimeOrder(runtimes, "zcode", 1), runtimes);
});

test("拖放到目标前后会按横向位置插入", () => {
  assert.deepEqual(reorderRuntimeOrder(runtimes, "zcode", "codex", "before"), ["claude_code", "zcode", "codex", "pi"]);
  assert.deepEqual(reorderRuntimeOrder(runtimes, "claude_code", "pi", "after"), ["codex", "pi", "claude_code", "zcode"]);
  assert.deepEqual(reorderRuntimeOrder(runtimes, "pi", "pi", "after"), runtimes);
});

test("读取与保存复用 Runtime 偏好存储，存储异常向调用方传播", () => {
  let value = JSON.stringify({ order: ["zcode", "codex"] });
  const storage = {
    getItem(key) { assert.equal(key, RUNTIME_PREFERENCES_STORAGE_KEY); return value; },
    setItem(key, next) { assert.equal(key, RUNTIME_PREFERENCES_STORAGE_KEY); value = next; },
  };
  assert.deepEqual(loadRuntimeOrder(runtimes, storage), ["zcode", "codex", "claude_code", "pi"]);
  persistRuntimeOrder(["pi", "zcode", "codex", "claude_code"], storage);
  assert.deepEqual(loadRuntimeOrder(runtimes, storage), ["pi", "zcode", "codex", "claude_code"]);
  assert.throws(() => loadRuntimeOrder(runtimes, { getItem() { throw new Error("read failed"); } }), /read failed/);
  assert.throws(() => persistRuntimeOrder(runtimes, { setItem() { throw new Error("write failed"); } }), /write failed/);
});

test("新建会话的终端与聊天 Agent 顺序分别持久化，且不覆盖现有 Runtime 偏好", () => {
  const values = new Map();
  const storage = {
    getItem(key) { return values.get(key) ?? null; },
    setItem(key, value) { values.set(key, value); },
  };
  const terminalAgents = ["claude_code", "codex", "pi"];
  const chatAgents = ["claude_chat", "codex_chat", "pi_chat"];

  persistRuntimeOrder(
    ["pi", "claude_code", "codex"],
    storage,
    NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY,
  );
  persistRuntimeOrder(
    ["codex_chat", "claude_chat", "pi_chat"],
    storage,
    NEW_SESSION_CHAT_ORDER_STORAGE_KEY,
  );

  assert.notEqual(NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY, NEW_SESSION_CHAT_ORDER_STORAGE_KEY);
  assert.deepEqual(
    loadRuntimeOrder(terminalAgents, storage, NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY),
    ["pi", "claude_code", "codex"],
  );
  assert.deepEqual(
    loadRuntimeOrder(chatAgents, storage, NEW_SESSION_CHAT_ORDER_STORAGE_KEY),
    ["codex_chat", "claude_chat", "pi_chat"],
  );
  assert.deepEqual(loadRuntimeOrder(runtimes, storage), runtimes);
});
