import test from "node:test";
import assert from "node:assert/strict";

import {
  hasMentionToken,
  toggleMentionToken,
  selectMentionTokens,
  mentionTextSegments,
} from "../src/multiAgentMention.ts";

test('highlighting preserves exact text, line breaks and complete member handles',()=>{
 const value='请 @模型验收-Claude 和 @聊天-ZCodeCLI\n检查接口';
 const parts=mentionTextSegments(value,['@模型验收-Claude','@聊天-ZCodeCLI']);
 assert.equal(parts.map(p=>p.text).join(''),value);
 assert.deepEqual(parts.filter(p=>p.mention).map(p=>p.text),['@模型验收-Claude','@聊天-ZCodeCLI']);
});
test('highlighting excludes email, unknown targets and partial handles',()=>{
 const value='mail@Codex @Codex-2 @Unknown @Code';
 assert.equal(mentionTextSegments(value,['@Codex']).some(p=>p.mention),false);
});
test('long names, regex characters, case and repeated mentions retain original spelling',()=>{
 const value='@Claude Code @codex @Codex @dev[1]';
 assert.deepEqual(mentionTextSegments(value,['@Claude','@Claude Code','@Codex','@dev[1]']).filter(p=>p.mention).map(p=>p.text),['@Claude Code','@codex','@Codex','@dev[1]']);
});

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
