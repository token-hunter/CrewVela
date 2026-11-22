import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_APPEARANCE,
  DEFAULT_ARCHIVED_SESSIONS_SETTINGS_STYLE,
  DEFAULT_AUTOMATION_SETTINGS_STYLE,
  DEFAULT_BROWSER_CONTROL_SETTINGS_STYLE,
  DEFAULT_GENERAL_SETTINGS_STYLE,
  DEFAULT_GIT_SETTINGS_STYLE,
  DEFAULT_NOTIFICATION_SETTINGS_STYLE,
  DEFAULT_PRIVACY_SETTINGS_STYLE,
  DEFAULT_WORKTREE_SETTINGS_STYLE,
  loadAppearance,
  resolveArchivedSessionsSettingsStyle,
  resolveAutomationSettingsStyle,
  resolveBrowserControlSettingsStyle,
  resolveGeneralSettingsStyle,
  resolveGitSettingsStyle,
  resolveNotificationSettingsStyle,
  resolvePrivacySettingsStyle,
  resolveWorktreeSettingsStyle,
  saveAppearance,
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

test("8 项设置页创意风格全部默认为现有经典设计，并支持各自模式持久化", () => {
  withMockStorage(() => {
    assert.equal(DEFAULT_GENERAL_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_GIT_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_WORKTREE_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_NOTIFICATION_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_AUTOMATION_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_BROWSER_CONTROL_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_ARCHIVED_SESSIONS_SETTINGS_STYLE, "classic");
    assert.equal(DEFAULT_PRIVACY_SETTINGS_STYLE, "classic");

    assert.equal(resolveGeneralSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveGitSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveWorktreeSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveNotificationSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveAutomationSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveBrowserControlSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolveArchivedSessionsSettingsStyle(DEFAULT_APPEARANCE), "classic");
    assert.equal(resolvePrivacySettingsStyle(DEFAULT_APPEARANCE), "classic");

    saveAppearance(
      {
        ...DEFAULT_APPEARANCE,
        generalSettingsStyle: "coffee",
        gitSettingsStyle: "railway",
        worktreeSettingsStyle: "greenhouse",
        notificationSettingsStyle: "mixer",
        automationSettingsStyle: "sundial",
        browserControlSettingsStyle: "patchbay",
        archivedSessionsSettingsStyle: "tea",
        privacySettingsStyle: "vault",
      },
      "dark",
    );

    const loaded = loadAppearance();
    assert.equal(resolveGeneralSettingsStyle(loaded), "coffee");
    assert.equal(resolveGitSettingsStyle(loaded), "railway");
    assert.equal(resolveWorktreeSettingsStyle(loaded), "greenhouse");
    assert.equal(resolveNotificationSettingsStyle(loaded), "mixer");
    assert.equal(resolveAutomationSettingsStyle(loaded), "sundial");
    assert.equal(resolveBrowserControlSettingsStyle(loaded), "patchbay");
    assert.equal(resolveArchivedSessionsSettingsStyle(loaded), "tea");
    assert.equal(resolvePrivacySettingsStyle(loaded), "vault");
  });
});

test("非法、损坏或已移除的旧页面风格缓存值自动回退到经典默认设计", () => {
  withMockStorage((store) => {
    store.set(
      "crewvela-appearance",
      JSON.stringify({
        ...DEFAULT_APPEARANCE,
        generalSettingsStyle: "invalid_a",
        gitSettingsStyle: "invalid_b",
        worktreeSettingsStyle: "invalid_c",
        notificationSettingsStyle: "bellbox",
        automationSettingsStyle: "invalid_e",
        browserControlSettingsStyle: "invalid_g",
        archivedSessionsSettingsStyle: "invalid_h",
        privacySettingsStyle: "invalid_f",
      }),
    );

    const loaded = loadAppearance();
    assert.equal(loaded.generalSettingsStyle, undefined);
    assert.equal(loaded.gitSettingsStyle, undefined);
    assert.equal(loaded.worktreeSettingsStyle, undefined);
    assert.equal(loaded.notificationSettingsStyle, undefined);
    assert.equal(loaded.automationSettingsStyle, undefined);
    assert.equal(loaded.browserControlSettingsStyle, undefined);
    assert.equal(loaded.archivedSessionsSettingsStyle, undefined);
    assert.equal(loaded.privacySettingsStyle, undefined);

    assert.equal(resolveGeneralSettingsStyle(loaded), "classic");
    assert.equal(resolveGitSettingsStyle(loaded), "classic");
    assert.equal(resolveWorktreeSettingsStyle(loaded), "classic");
    assert.equal(resolveNotificationSettingsStyle(loaded), "classic");
    assert.equal(resolveAutomationSettingsStyle(loaded), "classic");
    assert.equal(resolveBrowserControlSettingsStyle(loaded), "classic");
    assert.equal(resolveArchivedSessionsSettingsStyle(loaded), "classic");
    assert.equal(resolvePrivacySettingsStyle(loaded), "classic");
  });
});
