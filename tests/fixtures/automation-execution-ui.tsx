import React from 'react';
import { createRoot } from 'react-dom/client';
import { IconContext } from '@phosphor-icons/react';
import { AutomationPanel } from '../../src/components/AutomationPanel';
import '../../src/styles.css';
const q = new URLSearchParams(location.search);
document.documentElement.dataset.theme=q.get('theme')==='light'?'light':'dark';
const now = new Date('2026-11-16T10:00:00+08:00').getTime();
const names = ['仓库巡检','日报摘要','项目测试','文档整理'];
let entries:any[] = ['running','completed','failed','unconfirmed'].map((stage,index) => ({
  id:`a${index}`,name:names[index],enabled:index!==3,trigger:{kind:'daily',hour:9,minute:0},action:{kind:'run_agent',runtimeKind:index===0?'codex':'claude_code',providerId:null,model:null,cwd:'C:/fixture/todo-app',prompt:'检查项目并返回结果'},
  maxRuns:null,maxConsecutiveFailures:3,runCount:12-index,consecutiveFailures:stage==='failed'?1:0,nextRunAt:now+86400000,lastRunAt:now,createdAt:now,requestReceipts:{},
  recentRuns:[{id:`r${index}`,at:now,updatedAt:now,ok:stage!=='failed',origin:'schedule',stage,detail:({running:'已接收，正在执行',completed:'已收到与本任务关联的完成回传',failed:'Agent 返回认证失败',unconfirmed:'只确认已入队，请核对原会话'} as Record<string,string>)[stage],targets:[{sessionId:'session-fixture',messageId:`automation-r${index}-m`,projectId:null,taskId:null,runId:null,stage,detail:'本轮精确目标'}]}]
}));
if(q.get('empty'))entries=[];
const calls:any[]=[]; let fail=q.get('fail')==='1';
(window as any).__automationFixture={calls,get entries(){return entries;}};
(window as any).__TAURI_INTERNALS__={transformCallback:()=>1, async invoke(command:string,args:any){
  calls.push({command,args});
  if(command==='list_automations')return structuredClone(entries);
  if(command==='get_runtime_preferences')return {disabledRuntimes:[]};
  if(command==='set_automation_enabled'){entries=entries.map(a=>a.id===args.id?{...a,enabled:args.enabled}:a);return entries.find(a=>a.id===args.id);}
  if(command==='run_automation_now'){
    await new Promise(r=>setTimeout(r,150));
    const a=entries.find(a=>a.id===args.id);
    if(!a.requestReceipts[args.requestId]){a.requestReceipts[args.requestId]='manual-run';a.recentRuns.push({id:'manual-run',at:now,ok:true,stage:'queued',detail:'消息仍在队列中',origin:'manual',targets:[]});}
    if(fail){fail=false;throw Error('模拟响应丢失：请刷新核对，不要重复派发');}
    return structuredClone(entries);
  }
  if(command==='stop_automation_run')throw Error('原会话的停止身份无法确认；请打开会话处理');
  if(command==='acknowledge_automation_run') {const a=entries.find(a=>a.id===args.id);a.recentRuns.find((r:any)=>r.id===args.runId).stage='acknowledged';return structuredClone(entries);}
  if(command==='ma_probe_runtimes')return [{kind:'codex',installed:true,version:'fixture',executable:'C:/fixture/codex'}];
  if(command==='ma_list_projects')return [];
  if(command==='plugin:event|listen')return 1;
  if(command==='plugin:event|unlisten')return;
  throw Error(`隔离 UI 未模拟 ${command}`);
}};
const sessions:any[]=[{id:'session-fixture',title:'隔离测试会话',kind:'claude',agentRuntimeKind:'codex',status:'running',cwd:'C:/fixture/todo-app'}];
window.addEventListener('crewvela-automation-open-session',(e:any)=>(window as any).__automationOpened=e.detail.sessionId);
createRoot(document.getElementById('root')!).render(<IconContext.Provider value={{weight:'regular'}}><main className="settings-native" style={{maxWidth:1240,margin:'0 auto',padding:24}}><small>隔离示例 · 不调用真实 CLI</small><AutomationPanel embedded sessions={sessions} providers={[]}/></main></IconContext.Provider>);
