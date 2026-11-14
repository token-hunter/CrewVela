import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readStyles } from "./helpers/loadStyles.mjs";

const settingsSource = readFileSync(
  new URL("../src/components/SettingsDialog.tsx", import.meta.url),
  "utf8",
);
const stylesSource = readStyles();

test("所有设置页共用固定窗口与分类导航，模型页只调整内容", () => {
  assert.match(settingsSource, /className=\{`settings-dialog settings-native\$\{activeSection === "providers" \? " settings-model-focus" : ""\}`\}/);
  assert.match(settingsSource, /labelledBy="settings-dialog-title"/);
  assert.match(settingsSource, /className="settings-window-header"/);
  assert.match(stylesSource, /width: min\(1280px, calc\(100vw - 64px\)\)/);
  assert.match(stylesSource, /height: min\(900px, calc\(100dvh - 64px\)\)/);
  assert.match(settingsSource, /className="settings-content" key=\{activeSection\}/);
  assert.doesNotMatch(settingsSource, /settings-feature-dialog|settings-workspace-dialog|collaboration-content|shared-memory-content/);
  assert.match(settingsSource, /<aside className="settings-nav">/);
  assert.doesNotMatch(settingsSource, /<aside[^>]+hidden=/);
});

test("统一设置提供固定关闭入口、搜索和分组导航", () => {
  assert.match(settingsSource, /aria-label="关闭设置"/);
  assert.equal(settingsSource.match(/aria-label="关闭设置"/g)?.length, 1);
  assert.match(settingsSource, /aria-label="搜索设置"/);
  assert.match(settingsSource, /settings-nav-group/);
  assert.doesNotMatch(settingsSource, /工作流画布|Benchmark|赛博女友/);
  assert.deepEqual(
    settingsSource.match(/label: "(?:通用|智能体|集成|数据)"/g),
    ['label: "通用"', 'label: "智能体"', 'label: "集成"', 'label: "数据"'],
    "设置分组应按 通用/智能体/集成/数据 顺序排列",
  );
  assert.doesNotMatch(settingsSource, /label: "体验"/);
  assert.equal(
    settingsSource.match(/(?:label|title): "模型设置"/g)?.length,
    2,
    "模型入口和页面标题应统一命名为模型设置",
  );
  assert.doesNotMatch(settingsSource, /模型服务管理/);
});

test("窄屏仍保留窗口关闭入口和可访问的图标导航", () => {
  assert.match(
    stylesSource,
    /@media \(max-width:\s*720px\)[\s\S]*?grid-template-columns:\s*64px minmax\(0,\s*1fr\);/,
  );
  assert.match(
    stylesSource,
    /@media \(max-width:\s*720px\)[\s\S]*?\.settings-dialog.settings-native \.settings-nav\s*\{[^}]*display:\s*flex;/,
  );
  assert.match(settingsSource, /aria-label=\{item\.label\}/);
});

test("移除功能不再出现在设置路由中", () => {
  assert.doesNotMatch(settingsSource, /"(?:voice|modelEvaluation|studio|workflowCanvas)"/);
});
