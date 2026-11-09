import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readStyles } from "./helpers/loadStyles.mjs";

const settingsSource = readFileSync(
  new URL("../src/components/SettingsDialog.tsx", import.meta.url),
  "utf8",
);
const stylesSource = readStyles();

test("设置采用全屏工作区而不是居中小弹窗", () => {
  assert.match(stylesSource, /\.settings-overlay\s*\{[^}]*padding:\s*0;/s);
  assert.match(
    stylesSource,
    /\.settings-overlay\s*>\s*\.settings-dialog[\s\S]*?width:\s*100vw;[\s\S]*?height:\s*100dvh;/,
  );
  assert.match(stylesSource, /border-radius:\s*0;/);
  assert.match(settingsSource, /settings-content-inner/);
});

test("全屏设置提供返回、搜索和分组导航", () => {
  assert.match(settingsSource, /className="settings-back"/);
  assert.match(settingsSource, /aria-label="返回应用"/);
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

test("窄屏仍保持全屏并使用可访问的图标导航", () => {
  assert.match(
    stylesSource,
    /@media \(max-width:\s*720px\)[\s\S]*?grid-template-columns:\s*68px minmax\(0,\s*1fr\);/,
  );
  assert.match(
    stylesSource,
    /@media \(max-width:\s*720px\)[\s\S]*?\.settings-dialog \.settings-nav\s*\{[^}]*display:\s*flex;/,
  );
  assert.match(settingsSource, /aria-label=\{item\.label\}/);
});

test("移除功能不再出现在设置路由中", () => {
  assert.doesNotMatch(settingsSource, /"(?:voice|modelEvaluation|studio|workflowCanvas)"/);
});
