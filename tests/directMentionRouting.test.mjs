import test from 'node:test';
import assert from 'node:assert/strict';
import { conversationChatDispatch, conversationRecipients, conversationMentionToken, conversationQuote, conversationRequestMode, conversationRequestsGroup, defaultConversationReplyMember } from '../src/collaborationConversation.ts';
const agents = [
 {id:'claude',name:'Claude Code',enabled:true,projectId:'p'},
 {id:'codex',name:'Codex',enabled:true,projectId:'p'},
 {id:'kimi',name:'Kimi Code',enabled:true,projectId:'p'},
 {id:'off',name:'停用成员',enabled:false,projectId:'p'},
 {id:'foreign',name:'Codex',enabled:true,projectId:'other'},
];
test('带来源的话题引用可以复用，历史点名和执行要求不参与新消息路由', () => {
 const quote = conversationQuote('引用自「安装器修复」· Codex', '@Claude Code 请直接重启项目\n修复代码');
 assert.match(quote, /^> 引用自「安装器修复」· Codex\n> @Claude Code/);
 assert.deepEqual(conversationRecipients(`${quote}\n\n@Codex 你怎么看？`, [], agents, 'p'), ['codex']);
 assert.equal(conversationRequestMode(`${quote}\n\n这条还有关联吗？`), 'discussion');
});
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
test('全体收件与单人接手独立，启动项目优先交给联调成员',()=>{
 const team=[
  {...agents[0],name:'后端审查',role:'reviewer'},
  {...agents[1],name:'后端开发',role:'worker'},
  {...agents[2],name:'联调集成',role:'integrator'},
 ];
 const selected=team.map(agent=>agent.id);
 const resolve=(body,available=selected)=>{
  const ids=conversationRecipients(body,available,team,'p');
  const route=conversationChatDispatch(body,team.filter(agent=>ids.includes(agent.id)));
  return {ids,...route};
 };
 for(const body of ['谁能帮我启动这个项目？','谁能帮我把它跑起来？','这个地址打不开','请重启服务']) {
  assert.deepEqual(resolve(body),{ids:selected,intent:'chat',chatDispatch:{kind:'assigned',agentId:'kimi'}});
 }
 assert.deepEqual(resolve('谁能帮我启动这个项目？',['claude','codex']).chatDispatch,{kind:'assigned',agentId:'codex'});
 assert.deepEqual(resolve('谁能帮我审查这个提交？').chatDispatch,{kind:'assigned',agentId:'claude'});
 assert.deepEqual(resolve('@后端审查 帮我启动这个项目'),{ids:['claude'],intent:'chat',chatDispatch:{kind:'assigned',agentId:'claude'}});
 assert.deepEqual(resolve('@后端开发 @联调集成 启动项目'),{ids:['codex','kimi'],intent:'chat',chatDispatch:{kind:'assigned',agentId:'kimi'}});
 for(const body of ['请各自做个自我介绍','请各自给一个验收意见','听听大家的看法','Could each of you share your opinion?']) {
  assert.deepEqual(resolve(body),{ids:selected,intent:'chat',chatDispatch:{kind:'parallel'}});
 }
 for(const body of ['hi','大家好','怎么启动项目？','请只说明启动步骤','请不要运行项目','可以讨论这个方案吗？','Can you discuss the design?','不需要大家分别给意见，只解释接口']) {
  assert.deepEqual(resolve(body),{ids:selected,intent:'chat',chatDispatch:{kind:'assigned',agentId:'claude'}},body);
 }
 assert.deepEqual(resolve('@后端审查 @后端开发 你们怎么看？').chatDispatch,{kind:'parallel'});
 assert.deepEqual(resolve('你们互相讨论一下这个方案'),{ids:selected,intent:'exchange',chatDispatch:undefined});
 assert.throws(()=>resolve('@不存在 启动这个项目'),/未找到明确/);
 assert.throws(()=>resolve('谁能帮我启动这个项目？',[]),/至少一位/);
});
test('项目默认答复只安排一人，保留群聊和明确指定的收件范围',()=>{
 const team=[{...agents[0],role:'worker'},{...agents[1],role:'orchestrator'},{...agents[2],role:'reviewer'}];
 assert.equal(defaultConversationReplyMember(team)?.id,'codex');
 assert.equal(defaultConversationReplyMember(team,'kimi')?.id,'kimi');
 assert.equal(defaultConversationReplyMember(team,'removed')?.id,'codex');
 assert.equal(defaultConversationReplyMember([],'removed'),undefined);
 for(const body of ['方案有更新吗？','大家好','怎么查看大家的回复？','请总结一下大家的意见','解释如何让大家互相交流','你们上次讨论了什么？','大家讨论的方案是哪一个？','Can you summarize everyone’s feedback?','> @Codex 请大家回复\n现在进度呢？','请解释 `@Codex` 的含义']) {
  assert.equal(conversationRequestsGroup(body),false,body);
  assert.deepEqual(conversationChatDispatch(body,team,undefined,'kimi'),{intent:'chat',chatDispatch:{kind:'assigned',agentId:'kimi'}},body);
 }
 for(const body of ['大家怎么看？','请各位给个意见','各位，分别说说你的想法','请全员回复','全员征求意见','请征求全员意见','请彼此回应一下','What do you all think?']) {
  assert.equal(conversationRequestsGroup(body),true,body);
 }
 assert.deepEqual(conversationChatDispatch('请审查这个提交',team,undefined,'claude').chatDispatch,{kind:'assigned',agentId:'kimi'});
 assert.deepEqual(conversationChatDispatch('普通问题',team,'claude','kimi').chatDispatch,{kind:'assigned',agentId:'claude'});
});
