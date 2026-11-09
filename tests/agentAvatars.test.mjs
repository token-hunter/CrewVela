import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { AGENT_AVATARS, agentAvatarSource } from "../src/agentAvatars.ts";

test("12 个原创头像与后端白名单一致，资源不包含外部内容", () => {
  assert.equal(AGENT_AVATARS.length, 12);
  assert.equal(new Set(AGENT_AVATARS.map(a => a.id)).size, 12);
  const domain = readFileSync(new URL("../src-tauri/src/multi_agent/domain.rs", import.meta.url), "utf8");
  const whitelist = domain.match(/AGENT_AVATAR_IDS:.*?= &\[([^\]]+)\]/s)[1];
  assert.deepEqual([...whitelist.matchAll(/"([^"]+)"/g)].map(m => m[1]).sort(), AGENT_AVATARS.map(a => a.id).sort());
  for (const avatar of AGENT_AVATARS) {
    const svg = readFileSync(new URL(avatar.src), "utf8");
    assert.match(svg, /viewBox="0 0 32 32"/);
    assert.doesNotMatch(svg, /<script|<image|<foreignObject|href=|onload=|<animate/i);
    assert.equal(agentAvatarSource(avatar.id, "runtime-logo"), avatar.src);
  }
});
test("旧数据、默认头像和未知 ID 安全回退 Runtime 图标", () => {
  for (const avatar of [undefined, null, "unknown", "https://example.test/avatar.svg"]) {
    assert.equal(agentAvatarSource(avatar, "pi-logo"), "pi-logo");
    assert.equal(agentAvatarSource(avatar, "codex-logo"), "codex-logo");
  }
  assert.equal(agentAvatarSource("orbit", "pi-logo"), agentAvatarSource("orbit", "codex-logo"));
});
