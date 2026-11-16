import test from "node:test";
import assert from "node:assert/strict";
import { loadAppearance, saveAppearance, resolveBaseTheme, resolveAppearanceTheme } from "../src/appearance.ts";

test("six themes resolve to three dark and three light palettes", () => {
  assert.deepEqual(["dark", "ocean", "purple", "light", "sand", "aurora"].map(resolveBaseTheme), ["dark", "dark", "dark", "light", "light", "light"]);
});

test("old ocean preferences remain compatible with warm night and retain independent settings", () => {
  const previous = globalThis.window;
  const stored = { theme: "ocean", background: "soft", accent: "cyan", followSystem: false, terminalTransparent: true, terminalOpacity: 0.35 };
  const values = new Map([["crewvela-appearance", JSON.stringify(stored)]]);
  globalThis.window = { localStorage: { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) } };
  try {
    const appearance = loadAppearance();
    assert.deepEqual(appearance, stored);
    assert.equal(resolveAppearanceTheme(appearance, false), "ocean");
    saveAppearance(appearance, resolveBaseTheme(appearance.theme));
    assert.equal(values.get("crewvela-theme"), "dark");
    assert.deepEqual(loadAppearance(), stored);
    assert.equal(resolveAppearanceTheme({ ...appearance, followSystem: true }, false), "light");
  } finally {
    if (previous === undefined) delete globalThis.window; else globalThis.window = previous;
  }
});

test("old aurora configuration keeps its identity and other preferences while becoming light", () => {
  const previous = globalThis.window;
  const stored = { theme: "aurora", background: "texture", accent: "purple", followSystem: false, terminalTransparent: true, terminalOpacity: 0.45 };
  const values = new Map([["crewvela-appearance", JSON.stringify(stored)]]);
  globalThis.window = { localStorage: { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) } };
  try {
    const appearance = loadAppearance();
    assert.deepEqual(appearance, stored);
    assert.equal(resolveAppearanceTheme(appearance, true), "aurora");
    saveAppearance(appearance, resolveBaseTheme(appearance.theme));
    assert.equal(values.get("crewvela-theme"), "light");
    assert.deepEqual(loadAppearance(), stored);
    assert.equal(resolveAppearanceTheme({ ...appearance, followSystem: true }, true), "dark");
    assert.equal(resolveAppearanceTheme({ ...appearance, followSystem: true }, false), "light");
  } finally {
    if (previous === undefined) delete globalThis.window; else globalThis.window = previous;
  }
});
