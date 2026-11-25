import test from 'node:test';
import assert from 'node:assert/strict';
import {COLLABORATION_RUNTIMES,TERMINAL_RUNTIMES,CHAT_RUNTIMES,isLegacyCollaborationRuntime,isRetiredRuntime,runtimeCategory} from '../src/runtimeTaxonomy.ts';
test('new collaboration members expose eight CLI engines including Antigravity',()=>{
 assert.deepEqual(COLLABORATION_RUNTIMES,['claude_code','codex','kimi_code','pi','opencode','zcode_cli','antigravity','deepseek_harness']);
 assert.equal(new Set(COLLABORATION_RUNTIMES).size,8);
 for(const runtime of ['claude_chat','codex_chat','kimi_chat','pi_chat','opencode_chat','zcode'])assert.equal(COLLABORATION_RUNTIMES.includes(runtime),false);
});
test('standalone chats and old Desktop identity survive CLI-only member admission',()=>{
 assert.equal(TERMINAL_RUNTIMES.length,7);
 assert.equal(CHAT_RUNTIMES.length,7);
 assert.equal(runtimeCategory('antigravity'),'terminal');
 assert.equal(isRetiredRuntime('antigravity'),false);
 assert.equal(runtimeCategory('deepseek_harness'),'chat');
 assert.equal(runtimeCategory('zcode'),'chat');
 assert.equal(isLegacyCollaborationRuntime('zcode'),true);
 assert.equal(isRetiredRuntime('zcode'),false);
 assert.equal(isLegacyCollaborationRuntime('zcode_cli'),false);
});
