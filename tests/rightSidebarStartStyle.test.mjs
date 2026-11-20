import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_APPEARANCE,
  DEFAULT_RIGHT_SIDEBAR_START_STYLE,
  loadAppearance,
  loadRightSidebarBreezeOn,
  loadRightSidebarLampOn,
  resolveRightSidebarStartStyle,
  saveAppearance,
  saveRightSidebarBreezeOn,
  saveRightSidebarLampOn,
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

test("右侧栏开始页样式默认为极简列表，支持 4 种模式持久化并兜底非法值", () => {
  withMockStorage((store) => {
    assert.equal(DEFAULT_RIGHT_SIDEBAR_START_STYLE, "minimal");
    assert.equal(resolveRightSidebarStartStyle(DEFAULT_APPEARANCE), "minimal");

    for (const style of ["minimal", "lamp", "clothesline", "cassette"]) {
      saveAppearance({ ...DEFAULT_APPEARANCE, rightSidebarStartStyle: style }, "dark");
      const loaded = loadAppearance();
      assert.equal(loaded.rightSidebarStartStyle, style);
      assert.equal(resolveRightSidebarStartStyle(loaded), style);
    }

    store.set(
      "crewvela-appearance",
      JSON.stringify({ ...DEFAULT_APPEARANCE, rightSidebarStartStyle: "unknown_style" }),
    );
    const fallback = loadAppearance();
    assert.equal(fallback.rightSidebarStartStyle, undefined);
    assert.equal(resolveRightSidebarStartStyle(fallback), "minimal");
  });
});

test("拉绳台灯默认关灯并持久化开关记忆，晾衣架微风默认开启并可记忆", () => {
  withMockStorage(() => {
    assert.equal(loadRightSidebarLampOn(), false);
    saveRightSidebarLampOn(true);
    assert.equal(loadRightSidebarLampOn(), true);
    saveRightSidebarLampOn(false);
    assert.equal(loadRightSidebarLampOn(), false);

    assert.equal(loadRightSidebarBreezeOn(), true);
    saveRightSidebarBreezeOn(false);
    assert.equal(loadRightSidebarBreezeOn(), false);
    saveRightSidebarBreezeOn(true);
    assert.equal(loadRightSidebarBreezeOn(), true);
  });
});
