import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
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

test("恢复的原版会话资源与动态状态帧完整，保留不同工作帧", () => {
  for (const name of ["agent-claude-code.png", "agent-codex.png", "agent-kimi-code.svg", "agent-pi.png", "agent-opencode.png", "model-zhipu.svg", "model-deepseek.svg", "model-minimax.svg", "claude-color.png", "native-agent-zcode-open.png", "native-agent-dolphin.png"]) {
    assert.equal(existsSync(new URL(`../src/assets/${name}`, import.meta.url)), true, name);
  }
  const frames = ["sleep", "working-both", "working-left", "working-right", "working-smoke-1", "working-smoke-2", "working-smoke-3"];
  const hashes = frames.map(frame => createHash("sha256").update(readFileSync(new URL(`../src/assets/claude-code-pet-${frame}.png`, import.meta.url))).digest("hex"));
  assert.equal(new Set(hashes).size, frames.length);
});

test("恢复的模型 SVG 不加载外部资源，Lobe Icons 许可随安装包分发", () => {
  for (const name of ["model-zhipu.svg", "model-deepseek.svg", "model-minimax.svg"]) {
    const source = readFileSync(new URL(`../src/assets/${name}`, import.meta.url), "utf8");
    assert.match(source, /<svg/);
    assert.doesNotMatch(source, /<script|<image|<foreignObject|href=|onload=/i);
  }
  const license = readFileSync(new URL("../THIRD_PARTY_NOTICES/lobe-icons-LICENSE", import.meta.url), "utf8");
  assert.match(license, /Copyright \(c\) 2023 LobeHub/);
  assert.match(license, /Permission is hereby granted/);
  for (const config of ["tauri.conf.json", "tauri.windows.conf.json"]) {
    const { bundle } = JSON.parse(readFileSync(new URL(`../src-tauri/${config}`, import.meta.url), "utf8"));
    assert.equal(bundle.resources["../THIRD_PARTY_NOTICES/lobe-icons-LICENSE"], "licenses/lobe-icons-LICENSE.txt");
  }
});
