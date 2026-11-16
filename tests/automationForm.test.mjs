import test from "node:test";
import assert from "node:assert/strict";
import { buildAutomationAction, buildEditedAutomationAction, automationFormFromAction, eventAutomationTrigger, automationSessions } from "../src/automationForm.ts";
const base = {runtime:"codex",mode:"new",source:"local",providerId:"",model:"",cwd:"/fixture/project",prompt:"inspect literal $text",sessionId:"",projectId:"",agentId:""};
const enabled = () => true;
const build = (patch={}, sessions=[], agents=[]) => buildAutomationAction({...base,...patch}, enabled, [{id:"provider",hidden:false}], sessions, agents);
test("编辑回填六种动作，原会话/成员路由与旧 Provider 不隐式迁移", () => {
 const sessions=[{id:"target",kind:"claude",agentRuntimeKind:"codex"}];
 const agents=[{id:"member",projectId:"project",runtimeKind:"claude_code",enabled:true}];
 const actions=[
  {kind:"run_agent",runtimeKind:"codex",providerId:null,model:null,cwd:"/fixture",prompt:"original"},
  {kind:"send_agent",runtimeKind:"codex",sessionId:"target",projectId:null,agentId:null,body:"original"},
  {kind:"send_agent",runtimeKind:"claude_code",sessionId:"",projectId:"project",agentId:"member",body:"original"},
  {kind:"broadcast_agent",runtimeKind:"pi",body:"original"},
  {kind:"launch",providerId:"saved-provider",cwd:"/fixture",projectId:null,agentId:null,prompt:"original"},
  {kind:"message",sessionId:"target",projectId:null,agentId:null,body:"original"},
  {kind:"broadcast",body:"original"},
 ];
 for(const action of actions){const form=automationFormFromAction(action,sessions,agents);assert.equal(form.prompt,"original");const next=buildEditedAutomationAction({...form,prompt:"edited"},action,form.runtime,enabled,[],sessions,agents);assert.equal(next.kind,action.kind);assert.equal(next.body??next.prompt,"edited");if(action.kind==="launch")assert.equal(next.providerId,"saved-provider");if(action.kind==="send_agent")assert.equal(next.agentId,action.agentId);}
 const legacy=actions[4],form=automationFormFromAction(legacy,sessions,agents);
 assert.equal(buildEditedAutomationAction({...form,runtime:"codex"},legacy,form.runtime,enabled,[],sessions,agents).kind,"run_agent");
 assert.throws(()=>automationFormFromAction({...legacy,projectId:"missing",agentId:"missing"},[],[]),/成员不存在/);
});
test("Agent 新建默认沿用本地配置，不写模型和 Provider", () => {
 for (const runtime of ["claude_code","codex","kimi_code","pi","opencode","zcode_cli"]) {
  assert.deepEqual(build({runtime}), {kind:"run_agent",runtimeKind:runtime,providerId:null,model:null,cwd:base.cwd,prompt:base.prompt});
 }
});
test("旧 Provider 和模型覆盖被忽略，原生聊天不能冒充新建 CLI", () => {
 for (const runtime of ["claude_code","codex","pi"]) {
  const action=build({runtime,source:"provider",providerId:"retired",model:"stale-model"});
  assert.equal(action.providerId,null); assert.equal(action.model,null);
 }
 assert.throws(()=>build({runtime:"codex_chat"}), /已有会话/);
 assert.throws(()=>build({cwd:""}), /工作目录/);
 assert.throws(()=>build({prompt:" "}), /执行任务/);
 assert.throws(()=>buildAutomationAction(base,()=>false,[],[],[]),/已启用/);
});
test("复用时核对会话类型，归档和命令宿主不可误选", () => {
 const rows=[{id:"target",kind:"terminal",agentRuntimeKind:"codex",cwd:"/fixture"},{id:"chat",agentRuntimeKind:"codex_chat"},{id:"archived",agentRuntimeKind:"codex",archivedAt:1},{id:"host",agentRuntimeKind:"codex",runtimeCommandHost:true}];
 assert.deepEqual(automationSessions(rows,"codex").map(s=>s.id),["target"]);
 assert.equal(build({mode:"session",sessionId:"target",model:"stale-model",source:"provider"},rows).kind,"send_agent");
 assert.equal(build({mode:"session",sessionId:"target"},rows).model,undefined);
 assert.throws(()=>build({mode:"session",sessionId:"chat"},rows),/类型匹配/);
});
test("项目成员类型和启用状态必须匹配；广播显式限定 Agent", () => {
 const agents=[{id:"member",projectId:"project",runtimeKind:"codex",enabled:true}];
 assert.equal(build({mode:"member",projectId:"project",agentId:"member"},[],agents).agentId,"member");
 assert.throws(()=>build({mode:"member",projectId:"wrong",agentId:"member"},[],agents),/匹配/);
 assert.throws(()=>build({mode:"member",projectId:"project",agentId:"member"},[],[{...agents[0],enabled:false}]),/启用/);
 assert.throws(()=>build({mode:"member",runtime:"codex_chat",projectId:"project",agentId:"member"},[],agents),/原生聊天/);
 assert.deepEqual(build({mode:"broadcast"}),{kind:"broadcast_agent",runtimeKind:"codex",body:base.prompt});
});
test("事件必须绑定明确来源，不再默认监听任意会话", () => {
 assert.deepEqual(eventAutomationTrigger("completed","runtime","","codex"),{kind:"on_session_event_filtered",event:"completed",runtimeKind:"codex",sessionId:null,projectId:null});
 assert.equal(eventAutomationTrigger("failed","session","source","codex").sessionId,"source");
 assert.equal(eventAutomationTrigger("completed","project","project","codex").projectId,"project");
 assert.throws(()=>eventAutomationTrigger("failed","session","","codex"),/具体事件来源/);
});
