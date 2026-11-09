import test from "node:test";
import assert from "node:assert/strict";

import {
  hasMentionToken,
  toggleMentionToken,
} from "../src/multiAgentMention.ts";

test("target menu adds an Agent mention without changing the prompt", () => {
  assert.deepEqual(toggleMentionToken("实现登录页", "@ClaudeCode"), {
    value: "实现登录页 @ClaudeCode ",
    selected: true,
  });
});

test("target menu removes an already selected Agent mention", () => {
  assert.deepEqual(toggleMentionToken("@ClaudeCode 实现登录页", "@ClaudeCode"), {
    value: "实现登录页",
    selected: false,
  });
});

test("mention matching respects complete handles", () => {
  assert.equal(hasMentionToken("@Codex-2 审查代码", "@Codex"), false);
  assert.equal(hasMentionToken("请让 @Codex 审查代码", "@Codex"), true);
});
