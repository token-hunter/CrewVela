import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { LEGACY_NAME, resolveConfigDirectory } from '../scripts/legacy-brand.mjs';
import { migrateStorageNamespace } from '../src/storageNamespaceMigration.ts';
import { loadAppearance } from '../src/appearance.ts';

test('brand migration scripts discover old configuration until the canonical entry exists', () => {
  const root = mkdtempSync(join(tmpdir(), 'crewvela-brand-script-'));
  try {
    assert.equal(resolveConfigDirectory(root), join(root, 'CrewVela'));
    mkdirSync(join(root, LEGACY_NAME));
    assert.equal(resolveConfigDirectory(root), join(root, LEGACY_NAME));
    mkdirSync(join(root, 'CrewVela'));
    assert.equal(resolveConfigDirectory(root), join(root, 'CrewVela'));
  } finally {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + '\\') || resolve(root).startsWith(resolve(tmpdir()) + '/'));
    rmSync(root, { recursive: true });
  }
});

test('brand migration retains old appearance values and current settings take precedence', async () => {
  const appearance = { theme: 'purple', background: 'texture', accent: 'green', followSystem: true, terminalTransparent: true, terminalOpacity: 0.8 };
  const values = new Map([[`${LEGACY_NAME}-appearance`, JSON.stringify(appearance)], [`${LEGACY_NAME}-theme`, 'dark']]);
  const storage = { get length() { return values.size; }, key: i => [...values.keys()][i] ?? null,
    getItem: k => values.get(k) ?? null, setItem: (k, v) => values.set(k, v), removeItem: k => values.delete(k) };
  const result = await migrateStorageNamespace(storage);
  assert.equal(result.migrated, 2);
  const previous = globalThis.window;
  globalThis.window = { localStorage: storage };
  try { assert.deepEqual(loadAppearance(), { ...appearance, accent: "none" }); } finally { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; }
  values.set(`${LEGACY_NAME}-appearance`, JSON.stringify({ theme: 'light' }));
  assert.equal((await migrateStorageNamespace(storage)).retained, 1);
  assert.equal(values.get('crewvela-appearance'), JSON.stringify(appearance));
});
