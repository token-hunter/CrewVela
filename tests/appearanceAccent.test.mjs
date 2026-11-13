import test from 'node:test';
import assert from 'node:assert/strict';
import { loadAppearance, saveAppearance } from '../src/appearance.ts';
import { parseCustomSkin, loadCustomSkins } from '../src/customSkins.ts';

test('removed accents migrate, remaining accents persist and invalid values still fall back', () => {
  const previous = globalThis.window;
  const values = new Map();
  globalThis.window = { localStorage: { getItem: k => values.get(k) ?? null, setItem: (k, v) => values.set(k, v) } };
  try {
    for (const accent of ['blue', 'green', 'none', 'clay', 'purple', 'cyan', 'orange', 'pink', 'coral', 'gray', 'invalid']) {
      values.set('crewvela-appearance', JSON.stringify({ theme: 'sand', accent, terminalOpacity: 0.4 }));
      const appearance = loadAppearance();
      assert.equal(appearance.accent, ['blue', 'green'].includes(accent) ? 'none' : accent === 'invalid' ? 'clay' : accent);
      assert.equal(appearance.theme, 'sand');
      assert.equal(appearance.terminalOpacity, 0.4);
      saveAppearance(appearance, 'light');
      assert.equal(loadAppearance().accent, appearance.accent);
      assert.equal(JSON.parse(values.get('crewvela-appearance')).accent, appearance.accent);
    }
  } finally { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; }
});

test('imported and previously saved skins migrate removed accents without losing skins', () => {
  const previous = globalThis.localStorage;
  try {
    for (const accent of ['blue', 'green', 'none', 'purple']) {
      const appearance = { theme: 'dark', background: 'solid', accent };
      const expected = ['blue', 'green'].includes(accent) ? 'none' : accent;
      assert.equal(parseCustomSkin(JSON.stringify({ appearance })).appearance.accent, expected);
      globalThis.localStorage = { getItem: () => JSON.stringify([{ id: 'saved', name: '保留皮肤', appearance }]) };
      const skins = loadCustomSkins();
      assert.equal(skins.length, 1);
      assert.equal(skins[0].appearance.accent, expected);
    }
    assert.throws(() => parseCustomSkin(JSON.stringify({ theme: 'dark', background: 'solid', accent: 'invalid' })), /不支持的强调色/);
  } finally { if (previous === undefined) delete globalThis.localStorage; else globalThis.localStorage = previous; }
});
