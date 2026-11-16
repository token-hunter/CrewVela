import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const settings = readFileSync(new URL("../src/components/SettingsDialog.tsx", import.meta.url), "utf8");


test("主页设置按钮进入常规设置，移除的设置入口不再可达", () => {
  assert.match(app, /onOpenSettings=\{openGeneralSettings\}/);
  assert.match(app, /const openGeneralSettings = useCallback\(\s*\(\) => openSettingsSection\("generalSettings"\)/);
  assert.doesNotMatch(app, /openSettingsSection\("providers"\)/);
  assert.match(settings, /initialSection = "generalSettings"/);
});

test("保留设置分类导航，不再加载模型管理页面", () => {
 assert.match(settings, /<aside\b[^>]*className="settings-nav"[^>]*>/);
 assert.doesNotMatch(settings, /ModelAssignmentsPanel|ProviderQuotaCell|ComputerControlSettingsPanel|PluginMarketPanel/);
});
