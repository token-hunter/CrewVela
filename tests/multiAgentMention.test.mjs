import test from "node:test";
import assert from "node:assert/strict";

import {
  hasMentionToken,
  toggleMentionToken,
  selectMentionTokens,
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

test("selecting all expands exact handles without duplicating selected members or changing instructions", () => {
  const tokens = ["@开发员", "@审查员", "@开发员"];
  const selected = selectMentionTokens("@开发员 互相介绍一下", tokens, true);
  assert.equal(selected, "@开发员 互相介绍一下 @审查员 ");
  assert.equal(selectMentionTokens(selected, tokens, true), selected);
  assert.equal(selectMentionTokens(selected, tokens, false), "互相介绍一下");
  assert.equal(selectMentionTokens("@开发员-2 互相介绍一下", ["@开发员"], false), "@开发员-2 互相介绍一下");
});
