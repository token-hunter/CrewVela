import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const exports={};
new Function('exports',ts.transpileModule(readFileSync(new URL('../src/conversationActivity.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText)(exports);
const {conversationMemberActivity:activity}=exports;
const now=100000;
const message={id:'m',projectId:'A',runId:'r',taskId:'t',agentId:'a',status:'running',createdAt:48000,updatedAt:now,body:''};
const run={id:'r',projectId:'A',taskId:'t',agentId:'a',status:'running',createdAt:48000,startedAt:48000,lastHeartbeatAt:now,finishedAt:null};
const event=(seq,payload,createdAt=now-2000,extra={})=>({seq,projectId:'A',runId:'r',taskId:'t',agentId:'a',eventType:'agent_activity',payload,createdAt,...extra});
test('heartbeat and empty response do not masquerade as thinking',()=>{
 const state=activity(message,run,[],[],now);assert.equal(state.phase,'waiting');assert.equal(state.stale,true);assert.equal(state.heartbeat,'normal');assert.equal(state.lastActivityAt,null);assert.equal(state.elapsed,52);
});
test('analysis requires a correlated real signal; unrelated attempts/projects are excluded',()=>{
 const events=[event(1,{kind:'analysis'},now,{runId:'old'}),event(2,{kind:'reply'},now,{projectId:'B'}),event(3,{kind:'analysis'},now,{agentId:'b'}),event(4,{kind:'analysis'},now,{taskId:'wrong'})];
 assert.equal(activity(message,run,events,[],now).phase,'waiting');
 assert.equal(activity(message,run,[event(5,{kind:'analysis'})],[],now).phase,'analysis');
});
test('tool updates retain paths and simultaneous tools; late completion does not erase active work',()=>{
 const events=[event(1,{kind:'tool',toolId:'one',operation:'read',path:'src/auth.ts',status:'running'},now-5000),event(2,{kind:'tool',toolId:'two',operation:'search',status:'running'},now-4000),event(3,{kind:'tool',toolId:'one',status:'completed'},now-2000)];
 const state=activity(message,run,events,[],now);assert.equal(state.phase,'search');assert.equal(state.steps.find(x=>x.id==='tool:one').label,'读取 src/auth.ts');assert.equal(state.steps.find(x=>x.id==='tool:one').status,'completed');assert.equal(state.lastActivityAt,now-2000);
});
test('pending approval wins over stale analysis, unrelated approval does not',()=>{
 const events=[event(1,{kind:'analysis'},48001)];
 const approval={projectId:'A',runId:'r',status:'pending'};
 const state=activity(message,run,events,[approval],now);assert.equal(state.phase,'approval');assert.equal(state.stale,false);
 assert.equal(activity(message,run,events,[{...approval,runId:'other'}],now).stale,true);
});
test('terminal runs freeze duration and stop unfinished tool animations without claiming success',()=>{
 const events=[event(1,{kind:'tool',toolId:'one',operation:'read',status:'running'},55000)];
 const state=activity({...message,status:'completed'}, {...run,status:'succeeded',finishedAt:60000},events,[],now);
 assert.equal(state.phase,'completed');assert.equal(state.live,false);assert.equal(state.elapsed,12);assert.equal(state.stale,false);assert.equal(state.steps.find(x=>x.id==='tool:one').status,'stopped');
 assert.equal(activity({...message,status:'failed'},{...run,status:'failed',finishedAt:60000},events,[],now).phase,'failed');
 assert.equal(activity({...message,status:'cancelled'},run,events,[],now).phase,'cancelled');
});
test('tool summaries do not expose raw command arguments or reasoning text',()=>{
 const state=activity(message,run,[event(1,{kind:'tool',toolId:'t',operation:'command',rawInput:{command:'secret'},thinking:'private'})],[],now);
 assert.equal(state.steps[1].label,'运行命令');assert.ok(!JSON.stringify(state).includes('secret'));assert.ok(!JSON.stringify(state).includes('private'));
});
test('unverified stop is never presented as a finished process',()=>{
 const state=activity({...message,status:'unconfirmed'},{...run,status:'failed',sessionLocator:{stopUnverified:true}},[],[],now);
 assert.equal(state.phase,'unconfirmed');assert.equal(state.live,true);assert.equal(state.stale,false);assert.ok(!state.steps.some(x=>x.id==='end'));
});
test('only a web tool signal is labeled as web retrieval',()=>{
 assert.equal(activity(message,run,[event(1,{kind:'tool',toolId:'local',operation:'search',status:'running'})],[],now).label,'正在检索');
 assert.equal(activity(message,run,[event(2,{kind:'tool',toolId:'web',operation:'web',status:'running'})],[],now).label,'正在检索网页');
});
test('pending tools are distinguished from confirmed execution',()=>{
 const state=activity(message,run,[event(1,{kind:'tool',toolId:'one',operation:'read',status:'pending'})],[],now);
 assert.equal(state.phase,'tool_pending');assert.equal(state.steps[1].status,'pending');
 assert.equal(activity(message,run,[event(2,{kind:'tool',toolId:'two',operation:'command'})],[],now).phase,'tool_pending');
});
test('a successful transport still awaiting a human decision is not labeled completed',()=>{
 const state=activity({...message,status:'needs_decision'},{...run,status:'succeeded',finishedAt:now-1000},[],[],now);
 assert.equal(state.phase,'approval');assert.equal(state.label,'等待你确认');assert.equal(state.live,false);
});
