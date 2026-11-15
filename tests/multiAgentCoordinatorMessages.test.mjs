import test from 'node:test';
import assert from 'node:assert/strict';
import { orderedConversationMessages, latestConversationRound, conversationIsActive, CONVERSATION_STATUS } from '../src/collaborationConversation.ts';
const row=(id,projectId='p',updatedAt=1)=>({id,projectId,body:id,createdAt:1,updatedAt,status:'completed',senderType:'agent'});
test('公共对话按消息身份去重，内容相同但属于不同成员的发言不能合并',()=>{
 const messages=[{...row('a'),agentId:'claude',body:'认可'}, {...row('b'),agentId:'codex',body:'认可'}, {...row('a','p',2),body:'更新后的回复'}];
 const result=orderedConversationMessages(messages,'p');
 assert.equal(result.length,2); assert.equal(result[0].body,'更新后的回复'); assert.equal(result[1].agentId,'codex');
});
test('迟到的旧修订和其他项目回复不能覆盖当前会话',()=>{
 const result=orderedConversationMessages([{...row('a','p',5),body:'new'},{...row('a','p',1),body:'old'},row('foreign','other')],'p');
 assert.equal(result.length,1); assert.equal(result[0].body,'new');
});
test('相同时刻保持服务器顺序，不按随机UUID重排用户消息和成员回复',()=>{
 assert.deepEqual(orderedConversationMessages([row('z-user'),row('a-agent')],'p').map(m=>m.id),['z-user','a-agent']);
});
test('当前轮次取最新创建顺序，暂停或完成不能继续显示为运行',()=>{
 const rounds=[{id:'z',createdAt:1,status:'running'},{id:'a',createdAt:1,status:'paused'}];
 assert.equal(latestConversationRound(rounds).id,'a');
 for(const status of ['queued','running','waiting_approval','stopping','unconfirmed']) assert.equal(conversationIsActive({status}),true);
 for(const status of ['completed','failed','cancelled','paused','needs_decision']) assert.equal(conversationIsActive({status}),false);
});
test('回复结束与工程验收不同，未确认和停止中保持独立状态',()=>{
 assert.equal(CONVERSATION_STATUS.completed,'回复结束'); assert.equal(CONVERSATION_STATUS.unconfirmed,'结果未确认'); assert.equal(CONVERSATION_STATUS.stopping,'正在停止');
 assert.equal(orderedConversationMessages([{...row('pending'),body:'',status:'queued'}],'p')[0].body,'');
});
