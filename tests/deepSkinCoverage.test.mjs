import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readStyles } from "./helpers/loadStyles.mjs";

const styles = readStyles();

// 六套内置 deep skin 已移除（DEEP_SKINS 为空数组），样式表里不应再出现其选择器。
const deadSkinIds = [
  "deep-orange-machine",
  "deep-rain-office",
  "deep-navigation-room",
  "deep-paper-lab",
  "deep-midnight-records",
  "deep-snow-station",
];

test("removed built-in deep skins leave no selectors in styles.css", () => {
  for (const id of deadSkinIds) {
    assert.doesNotMatch(styles, new RegExp(`data-deep-skin=["']${id}["']`), `${id} selectors should be removed`);
  }
});

test("legacy dynamic- skin prefix selectors are removed", () => {
  assert.doesNotMatch(styles, /data-deep-skin\^="dynamic-"/);
});

test("generic [data-deep-skin] coverage rules still exist for custom skins", () => {
  const count = styles.match(/data-deep-skin[\[=\]]/g)?.length ?? 0;
  assert.ok(count >= 40, `expected generic [data-deep-skin] coverage rules, found ${count}`);
  assert.match(styles, /:root\[data-deep-skin\] \.app-shell/);
  assert.match(styles, /:root\[data-deep-skin\] \.workspace-tabs/);
});

test("new feature surfaces participate in the deep-skin contract", () => {
  const featureSurfaces = [
    ".knowledge-dialog",
    ".automation-dialog",
    ".notification-card",
    ".pet-settings-card",
    ".skin-settings-panel",
    ".appearance-settings-section",
    ".collaboration-settings-section",
  ];

  for (const selector of featureSurfaces) {
    assert.match(styles, new RegExp(selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
});
