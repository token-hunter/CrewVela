import test from 'node:test';
import assert from 'node:assert/strict';
import { loadAppearance, saveAppearance } from '../src/appearance.ts';

test('removed accents migrate, remaining accents persist and invalid values still fall back', () => {
  const previous = globalThis.window;
  const values = new Map();
  globalThis.window = { localStorage: { getItem: k => values.get(k) ?? null, setItem: (k, v) => values.set(k, v) } };
  try {
    for (const accent of ['blue', 'green', 'none', 'clay', 'purple', 'cyan', 'orange', 'pink', 'coral', 'gray', 'invalid']) {
      values.set('crewvela-appearance', JSON.stringify({ theme: 'sand', accent, terminalOpacity: 0.4 }));
      const appearance = loadAppearance();
      assert.equal(appearance.accent, ['blue', 'green', 'gray'].includes(accent) ? 'none' : accent === 'invalid' ? 'clay' : accent);
      assert.equal(appearance.theme, 'sand');
      assert.equal(appearance.terminalOpacity, 0.4);
      saveAppearance(appearance, 'light');
      assert.equal(loadAppearance().accent, appearance.accent);
      assert.equal(JSON.parse(values.get('crewvela-appearance')).accent, appearance.accent);
    }
  } finally { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; }
});
