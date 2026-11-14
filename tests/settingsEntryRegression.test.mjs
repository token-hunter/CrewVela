import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const settings = readFileSync(new URL("../src/components/SettingsDialog.tsx", import.meta.url), "utf8");
const modelStyles = readFileSync(new URL("../src/styles/model-settings.css", import.meta.url), "utf8");

test("主页设置按钮进入常规设置，模型专用入口保持独立", () => {
  assert.match(app, /onOpenSettings=\{openGeneralSettings\}/);
  assert.match(app, /const openGeneralSettings = useCallback\(\s*\(\) => openSettingsSection\("generalSettings"\)/);
  assert.match(app, /const openProviderSettings = useCallback\(\s*\(\) => openSettingsSection\("providers"\)/);
  assert.match(settings, /initialSection = "generalSettings"/);
});

test("模型页只改变内容布局，不覆盖窗口尺寸和分类导航", () => {
  assert.doesNotMatch(modelStyles, /width:\s*100vw|height:\s*100dvh/);
  assert.doesNotMatch(modelStyles, /\.settings-content\s*\{[^}]*grid-column:\s*1/);
  assert.doesNotMatch(settings, /<aside[^>]+hidden=/);
  assert.match(settings, /<aside className="settings-nav">/);
  assert.match(modelStyles, /model-provider-fields/);
});
