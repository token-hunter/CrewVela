import test from "node:test";
import assert from "node:assert/strict";

const values = new Map();
globalThis.window = {
  location: { search: "?ui-preview=1" },
  addEventListener() {},
  localStorage: {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  },
};
const { isRuntimeEnabled, refreshRuntimePreferences, setRuntimeEnabled } =
  await import("../src/runtimeAvailability.ts");

test("preferences fail closed until loaded and persist independent runtime switches", async () => {
  assert.equal(isRuntimeEnabled("codex"), false);
  await refreshRuntimePreferences();
  assert.equal(isRuntimeEnabled("codex"), true);
  assert.equal(isRuntimeEnabled("oh_my_pi"), false);
  await setRuntimeEnabled("codex", false);
  assert.equal(isRuntimeEnabled("codex"), false);
  assert.equal(isRuntimeEnabled("claude_code"), true);
  await refreshRuntimePreferences();
  assert.equal(isRuntimeEnabled("codex"), false);
  await setRuntimeEnabled("codex", true);
  assert.equal(isRuntimeEnabled("codex"), true);
  await assert.rejects(setRuntimeEnabled("oh_my_pi", true), /已停用/);
  assert.equal(isRuntimeEnabled("minimax_code"), false);
  await assert.rejects(setRuntimeEnabled("minimax_code", true), /已停用/);
});

test("invalid saved configuration closes execution and can recover by refreshing", async () => {
  values.set("crewvela-runtime-preferences", "{}");
  await refreshRuntimePreferences();
  assert.equal(isRuntimeEnabled("claude_code"), false);
  await assert.rejects(setRuntimeEnabled("codex", true), /格式无效/);
  values.set("crewvela-runtime-preferences", '{"disabledRuntimes":[]}');
  await refreshRuntimePreferences();
  assert.equal(isRuntimeEnabled("claude_code"), true);
});

function deferred() {
  let resolve, reject;
  const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
}
let nativeModuleId = 0;
async function nativeStore({ get, set, registration = Promise.resolve() }) {
  const order = [];
  let changed;
  globalThis.window = {
    location: { search: "" },
    __TAURI_INTERNALS__: {
      transformCallback(callback) { changed = callback; return 1; },
      invoke(command, args) {
        order.push(command);
        if (command === "plugin:event|listen") return registration.then(() => 1);
        if (command === "get_runtime_preferences") return get();
        if (command === "set_runtime_enabled") return set(args);
        throw new Error(`unexpected command ${command}`);
      },
    },
  };
  const store = await import(`../src/runtimeAvailability.ts?native=${++nativeModuleId}`);
  return { store, order, change: (payload) => changed({ payload }) };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));

test("native listener registers before reading and a change supersedes a delayed snapshot", async () => {
  const registered = deferred();
  const read = deferred();
  const { store, order, change } = await nativeStore({
    registration: registered.promise, get: () => read.promise,
  });
  const refresh = store.refreshRuntimePreferences();
  await flush();
  assert.deepEqual(order, ["plugin:event|listen"]);
  registered.resolve();
  await flush();
  assert.deepEqual(order, ["plugin:event|listen", "get_runtime_preferences"]);
  change({ disabledRuntimes: ["codex"] });
  assert.equal(store.isRuntimeEnabled("codex"), false);
  assert.equal(store.isRuntimeEnabled("claude_code"), true);
  assert.equal(store.refreshRuntimePreferences(), refresh);
  read.resolve({ disabledRuntimes: [] });
  await refresh;
  assert.equal(store.isRuntimeEnabled("codex"), false);
});

test("successful mutation supersedes an older in-flight read", async () => {
  const read = deferred();
  let reads = 0;
  const { store } = await nativeStore({
    get: () => ++reads === 1 ? Promise.resolve({ disabledRuntimes: [] }) : read.promise,
    set: () => Promise.resolve({ disabledRuntimes: ["codex"] }),
  });
  await store.refreshRuntimePreferences();
  const refresh = store.refreshRuntimePreferences();
  await flush();
  await store.setRuntimeEnabled("codex", false);
  read.resolve({ disabledRuntimes: [] });
  await refresh;
  assert.equal(store.isRuntimeEnabled("codex"), false);
  assert.equal(store.isRuntimeEnabled("claude_code"), true);
});

test("a newer change supersedes an old mutation response and invalid events fail closed", async () => {
  const mutation = deferred();
  const { store, change } = await nativeStore({
    get: () => Promise.resolve({ disabledRuntimes: [] }), set: () => mutation.promise,
  });
  await store.refreshRuntimePreferences();
  const saving = store.setRuntimeEnabled("codex", false);
  change({ disabledRuntimes: ["pi"] });
  mutation.resolve({ disabledRuntimes: ["codex"] });
  await saving;
  assert.equal(store.isRuntimeEnabled("codex"), true);
  assert.equal(store.isRuntimeEnabled("pi"), false);
  change({});
  assert.equal(store.isRuntimeEnabled("codex"), false);
  await assert.rejects(store.setRuntimeEnabled("codex", true), /格式无效/);
});

test("a failed old read cannot overwrite an authoritative event", async () => {
  const read = deferred();
  const { store, change } = await nativeStore({ get: () => read.promise });
  const refresh = store.refreshRuntimePreferences();
  await flush();
  change({ disabledRuntimes: ["codex"] });
  read.reject(new Error("stale read failed"));
  await refresh;
  assert.equal(store.isRuntimeEnabled("claude_code"), true);
  assert.equal(store.isRuntimeEnabled("codex"), false);
});

test("a read started during a mutation cannot suppress its authoritative response", async () => {
  const mutation = deferred();
  const { store } = await nativeStore({
    get: () => Promise.resolve({ disabledRuntimes: [] }), set: () => mutation.promise,
  });
  await store.refreshRuntimePreferences();
  const saving = store.setRuntimeEnabled("codex", false);
  await store.refreshRuntimePreferences();
  mutation.resolve({ disabledRuntimes: ["codex"] });
  await saving;
  assert.equal(store.isRuntimeEnabled("codex"), false);
});
