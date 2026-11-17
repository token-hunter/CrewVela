import test from 'node:test';
import assert from 'node:assert/strict';
import { conversationRequestIntent, conversationRequestMode } from '../src/collaborationConversation.ts';
test('普通问候、单人问题和只分析不进入工程执行',()=>{
 for(const body of ['hi','@Claude 解释这个函数','请解释分工开发项目的流程','你们先不要开发项目，只聊方案','分析一下怎么分工实现这个功能','示例：`请你们分工实现这个项目`'])assert.equal(conversationRequestMode(body,'discussion'),'discussion',body);
 assert.equal(conversationRequestIntent('hi'),'chat');
 assert.equal(conversationRequestIntent('@Codex 帮我解释这个函数'),'chat');
 assert.equal(conversationRequestIntent('不要互聊，只解释代码'),'chat');
 assert.equal(conversationRequestMode('Explain how to build this project together','discussion'),'discussion');
 assert.equal(conversationRequestIntent('Do not discuss it yet'),'chat');
});
test('明确分工需求生成现有计划提案，交流需求开启有界接话',()=>{
 for(const body of ['你们分工实现这个项目','请大家协作开发这个功能','帮我安排团队一起修复这个 bug','请大家分别实现项目的不同页面'])assert.equal(conversationRequestMode(body,'discussion'),'execution_plan',body);
 assert.equal(conversationRequestMode('按这个需求拟定计划','execution_plan'),'execution_plan');
 for(const body of ['@Claude @Codex 你们聊一下这个方案','请互相回应一下思路','大家讨论一下接口边界'])assert.equal(conversationRequestIntent(body),'exchange',body);
 assert.equal(conversationRequestIntent('@Claude @Codex discuss this design'),'exchange');
 assert.equal(conversationRequestMode('Please work together to build this project','discussion'),'execution_plan');
});
test('分工调整是一次性请求，普通追问仍保持聊天',()=>{
 for(const body of ['请调整分工：增加测试','生成分工：登录功能','更新分工：先处理页面','@Claude @Codex 帮我做登录页，先给分工。']) assert.equal(conversationRequestMode(body),'execution_plan');
 for(const body of ['hi','这个分工的验收是什么意思','不要调整分工，先解释']) assert.equal(conversationRequestMode(body),'discussion');
});
