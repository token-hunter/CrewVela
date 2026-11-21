import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_APPEARANCE,
  DEFAULT_PROJECT_MANAGEMENT_STYLE,
  loadAppearance,
  loadProjectGalleryDensity,
  loadProjectPinboardNeat,
  resolveProjectManagementStyle,
  saveAppearance,
  saveProjectGalleryDensity,
  saveProjectPinboardNeat,
} from "../src/appearance.ts";

function withMockStorage(fn) {
  const previous = globalThis.window;
  const store = new Map();
  globalThis.window = {
    localStorage: {
      getItem: (key) => store.get(key) ?? null,
      setItem: (key, value) => store.set(key, String(value)),
    },
  };
  try {
    fn(store);
  } finally {
    if (previous === undefined) delete globalThis.window;
    else globalThis.window = previous;
  }
}

test("项目管理模式默认为经典列表，支持 5 种模式持久化并兜底非法值与已移除旧值", () => {
  withMockStorage((store) => {
    assert.equal(DEFAULT_PROJECT_MANAGEMENT_STYLE, "classic");
    assert.equal(resolveProjectManagementStyle(DEFAULT_APPEARANCE), "classic");

    for (const style of ["classic", "gallery", "vinyl", "pinboard", "studio"]) {
      saveAppearance({ ...DEFAULT_APPEARANCE, projectManagementStyle: style }, "dark");
      const loaded = loadAppearance();
      assert.equal(loaded.projectManagementStyle, style);
      assert.equal(resolveProjectManagementStyle(loaded), style);
    }

    for (const legacyOrInvalid of ["invalid_mode", "coverflow"]) {
      store.set(
        "crewvela-appearance",
        JSON.stringify({ ...DEFAULT_APPEARANCE, projectManagementStyle: legacyOrInvalid }),
      );
      const fallback = loadAppearance();
      assert.equal(fallback.projectManagementStyle, undefined);
      assert.equal(resolveProjectManagementStyle(fallback), "classic");
    }
  });
});

test("精选画廊密度与相纸灵感墙对齐状态可独立持久化", () => {
  withMockStorage(() => {
    assert.equal(loadProjectGalleryDensity(), "medium");
    saveProjectGalleryDensity("large");
    assert.equal(loadProjectGalleryDensity(), "large");
    saveProjectGalleryDensity("compact");
    assert.equal(loadProjectGalleryDensity(), "compact");

    assert.equal(loadProjectPinboardNeat(), false);
    saveProjectPinboardNeat(true);
    assert.equal(loadProjectPinboardNeat(), true);
    saveProjectPinboardNeat(false);
    assert.equal(loadProjectPinboardNeat(), false);
  });
});
