import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import test from "node:test";
import { NEUTRAL_AGENT_ICONS } from "../src/neutralAgentAssets.ts";

test("全部中性 Agent 图形具有本地、可解码且不引用外部图片的 SVG", () => {
  assert.deepEqual(Object.keys(NEUTRAL_AGENT_ICONS).sort(), ["claude", "codex", "deepseek", "kimi", "minimax", "opencode", "pi", "zcode"]);
  assert.equal(new Set(Object.values(NEUTRAL_AGENT_ICONS)).size, 8);
  for (const url of Object.values(NEUTRAL_AGENT_ICONS)) {
    const svg = readFileSync(new URL(url), "utf8");
    assert.match(svg, /viewBox="0 0 256 256"/);
    assert.match(svg, /<path/);
    assert.doesNotMatch(svg, /<script|<image|<foreignObject|href=|onload=/i);
  }
});

test("中性图标的完整许可随各平台安装资源分发", () => {
  const root = new URL("../", import.meta.url);
  const license = readFileSync(new URL("THIRD_PARTY_NOTICES/phosphor-icons-LICENSE", root), "utf8");
  assert.match(license, /Copyright \(c\) 2020 Phosphor Icons/);
  assert.match(license, /Permission is hereby granted/);
  assert.match(license, /THE SOFTWARE IS PROVIDED "AS IS"/);
  for (const config of ["tauri.conf.json", "tauri.windows.conf.json"]) {
    const { bundle } = JSON.parse(readFileSync(new URL(`src-tauri/${config}`, root), "utf8"));
    assert.equal(bundle.resources["../THIRD_PARTY_NOTICES/phosphor-icons-LICENSE"], "licenses/phosphor-icons-LICENSE.txt");
    for (const resource of Object.keys(bundle.resources)) assert.ok(existsSync(new URL(`src-tauri/${resource}`, root)));
  }
});

test("历史品牌图形不再留在当前分发资源路径", () => {
  for (const name of ["agent-claude-code.png", "agent-codex.png", "agent-kimi-code.svg", "agent-pi.png", "agent-opencode.png", "model-zhipu.svg", "model-deepseek.svg", "model-minimax.svg", "claude-color.png", "native-agent-zcode-open.png", "native-agent-dolphin.png"]) {
    assert.equal(existsSync(new URL(`../src/assets/${name}`, import.meta.url)), false, name);
  }
});
