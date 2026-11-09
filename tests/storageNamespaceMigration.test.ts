import test from "node:test";
import assert from "node:assert/strict";
import { migrateStorageNamespace, namespaceFingerprint, SETTINGS_SUFFIXES } from "../src/storageNamespaceMigration.ts";
import { initializeStorage } from "../src/initializeStorage.ts";
import { loadWorkspaceTabs, saveWorkspaceTabs } from "../src/workspaceTabs.ts";

const allowedNamespaces = new Set(await Promise.all(["previous-suite", "first-suite", "second-suite"].map(namespaceFingerprint)));

function fixture(entries: Record<string, string> = {}) {
  const values = new Map(Object.entries(entries));
  const storage = {
    get length() { return values.size; },
    key: (index: number) => [...values.keys()][index] ?? null,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
    clear: () => { values.clear(); },
  };
  return { storage, values };
}

test("moves all known settings and dynamic project modes without rewriting identities or user text", async () => {
  const raw = JSON.stringify({ tabs: [{ id: "workspace-original", title: "Original text" }], assignments: { "session-original": "workspace-original" }, width: 0 });
  const entries = Object.fromEntries(SETTINGS_SUFFIXES.map((suffix) => [`previous-suite-${suffix}`, raw]));
  entries["previous-suite:codex-restart-required"] = "true";
  entries["previous-suite-collaboration-auto:project-original"] = "true";
  entries["unrelated-setting"] = "keep";
  const { storage, values } = fixture(entries);
  const result = await migrateStorageNamespace(storage, allowedNamespaces);
  assert.equal(result.migrated, SETTINGS_SUFFIXES.length + 2);
  assert.equal(result.retained, 0);
  assert.equal(result.failed, 0);
  for (const suffix of SETTINGS_SUFFIXES) assert.equal(values.get(`crewvela-${suffix}`), raw);
  assert.equal(values.get("crewvela:codex-restart-required"), "true");
  assert.equal(values.get("crewvela-collaboration-auto:project-original"), "true");
  assert.equal(values.get("unrelated-setting"), "keep");
  assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 0, retained: 0, failed: 0 });
});

test("preserves current settings and ambiguous previous sources", async () => {
  const { storage, values } = fixture({
    "crewvela-workspace-tabs": "current", "previous-suite-workspace-tabs": "previous",
    "first-suite-control-draft": "first", "second-suite-control-draft": "second",
  });
  assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 0, retained: 3, failed: 0 });
  assert.equal(values.get("crewvela-workspace-tabs"), "current");
  assert.equal(values.get("previous-suite-workspace-tabs"), "previous");
  assert.equal(values.has("crewvela-control-draft"), false);
});

test("merges notification IDs while preserving newer read state and retaining corrupt sources", async () => {
  const { storage, values } = fixture({
    "crewvela-notification-history-v1": JSON.stringify([{ id: "same", read: true }, { id: "new" }]),
    "previous-suite-notification-history-v1": JSON.stringify([{ id: "same", read: false }, { id: "old" }]),
  });
  assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 1, retained: 0, failed: 0 });
  assert.deepEqual(JSON.parse(values.get("crewvela-notification-history-v1")!), [{ id: "same", read: true }, { id: "new" }, { id: "old" }]);
  values.set("previous-suite-notification-history-v1", "{corrupt");
  assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 0, retained: 1, failed: 0 });
  assert.equal(values.get("previous-suite-notification-history-v1"), "{corrupt");
});

test("failed writes, removals and unavailable storage never delete the original data", async () => {
  for (const operation of ["setItem", "removeItem"] as const) {
    const { storage, values } = fixture({ "previous-suite-control-draft": "unsaved user text" });
    storage[operation] = () => { throw new Error("Storage unavailable"); };
    assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 0, retained: 1, failed: 1 });
    assert.equal(values.get("previous-suite-control-draft"), "unsaved user text");
  }
});

test("same-suffix settings in unrelated namespaces are never copied or removed", async () => {
  const entries = {
    "other-app-workspace-tabs": "private workspace",
    "notes-control-draft": "private note",
    "other-app-collaboration-auto:project-original": "true",
    "other-app:codex-restart-required": "true",
    "previous-suite-extra-workspace-tabs": "not the released namespace",
  };
  const { storage, values } = fixture(entries);
  assert.deepEqual(await migrateStorageNamespace(storage, allowedNamespaces), { migrated: 0, retained: 0, failed: 0 });
  assert.deepEqual(Object.fromEntries(values), entries);
  assert.deepEqual(await migrateStorageNamespace(storage), { migrated: 0, retained: 0, failed: 0 });
});

test("transient migration failure blocks default autosave, then retry restores the original workspace", async (t) => {
  const state = { tabs: [{ id: "workspace-original", title: "Original workspace" }], activeTabId: "workspace-original", assignments: { "session-original": "workspace-original" } };
  const raw = JSON.stringify(state);
  const { storage, values } = fixture({ "previous-suite-workspace-tabs": raw });
  const previousWindow = globalThis.window;
  globalThis.window = { localStorage: storage } as unknown as Window & typeof globalThis;
  t.after(() => {
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  });
  const write = storage.setItem;
  let fail = true;
  storage.setItem = (key, value) => {
    if (fail) { fail = false; throw new Error("Storage quota exhausted"); }
    write(key, value);
  };
  const start = async () => {
    await initializeStorage(storage, allowedNamespaces);
    const loaded = loadWorkspaceTabs();
    saveWorkspaceTabs(loaded);
    return loaded;
  };
  await assert.rejects(start(), /本地设置迁移失败/);
  assert.equal(values.has("crewvela-workspace-tabs"), false);
  assert.equal(values.get("previous-suite-workspace-tabs"), raw);
  assert.deepEqual(await start(), state);
  assert.equal(values.has("previous-suite-workspace-tabs"), false);
});
