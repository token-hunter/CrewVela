import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";
import ts from "typescript";
const require = createRequire(import.meta.url);
const code = ts.transpileModule(readFileSync(new URL("../src/hooks/useSessionData.ts", import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
function harness() {
  const pending = [], effects = [], state = {}, setters = {};
  for (const key of ["Providers", "KeyStates", "Sessions", "ArchivedSessions", "Projects", "Presets", "ControlDetached", "ControlSurface", "LastRightSidebarTab", "FilesOpen", "ControlOpen"]) setters[`set${key}`] = value => { state[key] = typeof value === "function" ? value(state[key] ?? []) : value; };
  const module = { exports: {} };
  const invoke = (command, args) => new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
  const load = name => name === "react" ? { useCallback: fn => fn, useRef: value => ({ current: value }), useState: initial => [initial, value => { state.Ready = value; }], useEffect: effect => effects.push(effect) }
    : name === "@tauri-apps/api/core" ? { invoke } : name === "../sessionSnapshot" ? { sameSessionSnapshot: () => false }
    : name === "../controlSurface" ? { loadControlSurface: () => "quick" } : require(name);
  new Function("require", "module", "exports", "window", code)(load, module, module.exports, { setTimeout: () => 1, clearTimeout() {} });
  const api = module.exports.useSessionData({ uiPreview: false, ...setters });
  return { api, pending, state, effects };
}
const tick = () => new Promise(resolve => setImmediate(resolve));

test("provider refresh ignores a late older list and its stale key states", async () => {
  const h = harness();
  const old = h.api.refreshProviders(), latest = h.api.refreshProviders();
  const lists = h.pending.filter(item => item.command === "list_providers");
  lists[1].resolve([{ id: "latest" }]); await tick();
  h.pending.find(item => item.command === "get_key_state" && item.args.id === "latest").resolve(true); await latest;
  lists[0].resolve([{ id: "old" }]); await tick();
  h.pending.find(item => item.command === "get_key_state" && item.args.id === "old")?.resolve(false); await old;
  assert.deepEqual(h.state.Providers, [{ id: "latest" }]);
  assert.deepEqual(h.state.KeyStates, { latest: true });
});

for (const [method, commands, fields] of [["refreshSessions", ["list_sessions", "list_archived_sessions"], ["Sessions", "ArchivedSessions"]], ["refreshProjects", ["list_projects"], ["Projects"]], ["refreshPresets", ["list_launch_presets"], ["Presets"]]]) {
  test(`${method} keeps the newest completed user refresh`, async () => {
    const h = harness(); const old = h.api[method](), latest = h.api[method]();
    for (const command of commands) h.pending.filter(item => item.command === command)[1].resolve([{ id: "latest" }]);
    await latest;
    for (const command of commands) h.pending.filter(item => item.command === command)[0].resolve([{ id: "old" }]);
    await old;
    for (const field of fields) assert.deepEqual(h.state[field], [{ id: "latest" }]);
  });
}


test("initial session readiness requires a successful snapshot and survives later failures", async t => {
  const loggedErrors = [];
  t.mock.method(console, "error", (...args) => loggedErrors.push(args));
  const h = harness();
  assert.equal(h.api.sessionsReady, false);
  const failing = h.api.refreshSessions();
  h.pending.find(item => item.command === "list_sessions").reject(new Error("initial unavailable"));
  h.pending.find(item => item.command === "list_archived_sessions").resolve([]);
  await failing;
  assert.notEqual(h.state.Ready, true, "a failed initial read cannot authorize assignment cleanup");
  const successful = h.api.refreshSessions();
  h.pending.filter(item => item.command === "list_sessions")[1].resolve([]);
  h.pending.filter(item => item.command === "list_archived_sessions")[1].resolve([]);
  await successful;
  assert.equal(h.state.Ready, true, "an empty but successfully loaded session list is ready");
  const laterFailure = h.api.refreshSessions();
  h.pending.filter(item => item.command === "list_sessions")[2].reject(new Error("later unavailable"));
  h.pending.filter(item => item.command === "list_archived_sessions")[2].resolve([]);
  await laterFailure;
  assert.equal(h.state.Ready, true, "temporary later failures do not erase the established snapshot");
  assert.equal(loggedErrors.length, 2, "both failures remain reported");
});


test("an older poll snapshot cannot roll back a newer explicit session refresh", async () => {
  const h = harness();
  const pollEffect = h.effects.find(effect => effect.toString().includes("pollControlWindow"));
  assert.ok(pollEffect);
  const dispose = pollEffect();
  const refresh = h.api.refreshSessions();
  h.pending.find(item => item.command === "list_sessions").resolve([{ id: "fresh" }]);
  h.pending.find(item => item.command === "list_archived_sessions").resolve([]);
  await refresh;
  h.pending.find(item => item.command === "is_control_window_open").resolve(false);
  h.pending.find(item => item.command === "take_control_dock_request").resolve(false);
  h.pending.find(item => item.command === "session_snapshot").resolve([{ id: "stale" }]);
  await tick(); dispose();
  assert.deepEqual(h.state.Sessions, [{ id: "fresh" }]);
});

test("unmount invalidates outstanding explicit reads", async () => {
  const h = harness();
  const cleanup = h.effects[0]();
  const refresh = h.api.refreshProjects();
  cleanup();
  h.pending.find(item => item.command === "list_projects").resolve([{ id: "late" }]);
  await refresh;
  assert.equal(h.state.Projects, undefined);
});
