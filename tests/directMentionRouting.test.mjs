import test from 'node:test';
import assert from 'node:assert/strict';
import { conversationRecipients, conversationMentionToken } from '../src/collaborationConversation.ts';
const agents = [
 {id:'claude',name:'Claude Code',enabled:true,projectId:'p'},
 {id:'codex',name:'Codex',enabled:true,projectId:'p'},
 {id:'kimi',name:'Kimi Code',enabled:true,projectId:'p'},
 {id:'off',name:'停用成员',enabled:false,projectId:'p'},
 {id:'foreign',name:'Codex',enabled:true,projectId:'other'},
];
test('普通消息仅发送到当前项目选中的具体成员，不进入全局广播',()=>{
 assert.deepEqual(conversationRecipients('讨论接口',['codex','codex','foreign','missing'],agents,'p'),['codex']);
 assert.throws(()=>conversationRecipients('讨论接口',['foreign','missing'],agents,'p'),/至少一位/);
});
test('显式多选@保留精确成员，带空格名称和大小写可正常解析',()=>{
 assert.deepEqual(conversationRecipients('@Claude Code @codex 互相检查接口',['kimi'],agents,'p'),['claude','codex']);
 assert.deepEqual(conversationRecipients('@Kimi Code @Codex',["claude"],agents,'p'),['codex','kimi']);
});
test('未知或停用目标不能悄悄降级为向当前团队发送',()=>{
 for(const body of ['@不存在 问候','@停用成员 测试','@CodexCLI 测试','@All 测试']) assert.throws(()=>conversationRecipients(body,['claude','codex'],agents,'p'),/未找到明确/);
});
test('同名成员以完整身份区分，不误投前缀相似的实例',()=>{
 const duplicate=[{...agents[1],id:'id-a'},{...agents[1],id:'id-ab'}];
 assert.equal(conversationMentionToken(duplicate[0],duplicate),'@Codex#id-a');
 assert.deepEqual(conversationRecipients('@Codex#id-ab 检查',[],duplicate,'p'),['id-ab']);
 assert.throws(()=>conversationRecipients('@Codex 检查',['id-a','id-ab'],duplicate,'p'),/未找到明确/);
});
test('含@的邮箱不误触发投递路由，项目间同名不改变当前成员身份',()=>{
 assert.deepEqual(conversationRecipients('联系 dev@example.com 后继续',['claude'],agents,'p'),['claude']);
 assert.deepEqual(conversationRecipients('@Codex 检查',[],agents,'p'),['codex']);
});
