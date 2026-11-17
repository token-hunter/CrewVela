// Real components and App with isolated IPC. No native CLI/model calls.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {chromium} from 'playwright';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1434',out='.scratch/collaboration-task-link-20261116/ui';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[];
const settings=p=>p.getByRole('dialog',{name:'设置',exact:true});
const all=p=>settings(p).getByRole('group',{name:'任务筛选',exact:true}).getByRole('button',{name:'全部',exact:true});
const calls=(p,cmd)=>p.evaluate(cmd=>window.conversationMock.calls.filter(c=>c.command===cmd),cmd);
async function fresh(extra='',viewport={width:1120,height:900}){
 const p=await browser.newPage({viewport});p.setDefaultTimeout(10000);p.on('pageerror',e=>errors.push(e.message));
 await p.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?task-link=1&internal=1&${extra}`);
 await p.getByRole('button',{name:'查看任务 · 3',exact:true}).waitFor();return p;
}
async function taskView(p,project='A',count=3){
 await p.waitForFunction(({project,count})=>document.querySelector('.collab-project-picker select')?.value===project&&document.querySelectorAll('.collab-task-row').length===count,{project,count});
 assert.equal(await all(p).getAttribute('aria-pressed'),'true');
 await p.waitForFunction(()=>document.activeElement?.matches('.collab-task-panel'));
}
async function openDetails(p){
 const row=settings(p).locator('.collab-task-row').filter({hasText:'列表与筛选'});
 await row.locator('summary').click();await row.getByRole('button',{name:'查看任务详情',exact:true}).click();
 const detail=p.getByRole('dialog',{name:'协作任务详情',exact:true});await detail.waitFor();return detail;
}
try{
 const p=await fresh('evidence=1');
 assert.equal(await p.locator('.conversation-project-icon').count(),0);
 assert.equal(await p.locator('.conversation-context').innerText(),'查看任务 · 3');
 assert.match(await p.getByLabel('协作项目',{exact:true}).getAttribute('title'),/\/fixture\/todo-app/);
 const editor=p.getByRole('combobox',{name:'给团队发消息',exact:true});await editor.fill('保留我的草稿');
 await p.getByRole('button',{name:'查看任务 · 3',exact:true}).click();await taskView(p);
 assert.equal(await settings(p).locator('.collab-task-row').filter({hasText:'讨论内部执行'}).count(),0);
 assert.equal(await p.evaluate(()=>window.conversationMock.snapshot('A').tasks.length),4,'Raw snapshot retains discussion execution');
 const info=settings(p).locator('.collab-project-info');assert.equal(await info.getAttribute('open'),null);
 await info.locator('summary').click();assert.equal(await info.locator('code').innerText(),'/fixture/todo-app');await info.locator('summary').click();
 const detail=await openDetails(p);assert.match(await detail.innerText(),/Codex[\s\S]*独立工作树[\s\S]*通过组件测试及构建[\s\S]*npm test/);
 assert.equal(await detail.locator('.collab-task-run').count(),2);
 for(const attempt of [1,2]){const run=detail.getByRole('region',{name:`执行尝试 ${attempt}`,exact:true});await run.locator('summary').click();assert.match(await run.locator('pre').innerText(),new RegExp(`尝试${attempt}日志0[\\s\\S]*尝试${attempt}日志24`));}
 const confirm=detail.getByRole('button',{name:'确认原进程已停止并允许重试',exact:true});
 p.once('dialog',d=>d.dismiss());await confirm.click();assert.equal((await calls(p,'ma_confirm_run_stopped')).length,0);
 p.once('dialog',d=>d.accept());await confirm.click();await confirm.waitFor({state:'detached'});
 assert.deepEqual((await calls(p,'ma_confirm_run_stopped')).map(c=>c.args),[{runId:'evidence-2',confirmedStopped:true}]);
 assert.equal((await calls(p,'ma_retry_task')).length,0,'Confirm stop does not retry automatically');
 await p.keyboard.press('Escape');assert.equal(await settings(p).isVisible(),true);
 await settings(p).getByRole('tab',{name:'协作记录',exact:true}).click();
 await p.evaluate(()=>window.conversationMock.openTasks('A'));await taskView(p);
 await settings(p).getByLabel('当前项目',{exact:true}).selectOption('B');
 await p.waitForFunction(()=>document.querySelector('.collab-task-row')?.textContent.includes('文档任务'));
 await p.evaluate(()=>window.conversationMock.openTasks('A'));await taskView(p);
 await settings(p).getByRole('button',{name:'待处理',exact:true}).click();await p.waitForTimeout(3250);
 assert.equal(await all(p).getAttribute('aria-pressed'),'false','Polling does not reset a manual filter');
 await p.evaluate(()=>window.conversationMock.openTasks('A'));await taskView(p);
 await settings(p).getByRole('button',{name:'隐私',exact:true}).click();
 await settings(p).getByRole('button',{name:'协作记录与诊断：查看',exact:true}).click();
 assert.equal(await settings(p).getByRole('tab',{name:'协作记录',exact:true}).getAttribute('aria-selected'),'true','Privacy link is not hijacked by an old task request');
 await settings(p).getByRole('button',{name:'关闭设置',exact:true}).click();assert.equal(await editor.inputValue(),'保留我的草稿');
 await p.getByRole('tab',{name:'任务执行',exact:true}).click();
 await p.getByRole('button',{name:'查看记录',exact:true}).click();
 await p.getByRole('button',{name:'查看完整项目记录',exact:true}).click();await taskView(p);
 await p.evaluate(()=>{window.conversationMock.holdProject='A';});
 await p.waitForFunction(()=>window.conversationMock.pending.some(item=>item.command==='ma_snapshot'&&item.args.projectId==='A'));
 await p.evaluate(()=>window.conversationMock.openTasks('B'));await taskView(p,'B',1);
 assert.match(await settings(p).locator('.collab-task-row').innerText(),/文档任务/);
 await p.evaluate(()=>{window.conversationMock.holdProject='';while(window.conversationMock.pending.length)window.conversationMock.release(window.conversationMock.pending[0].command);});
 assert.equal(await settings(p).getByLabel('当前项目',{exact:true}).inputValue(),'B');
 await p.evaluate(()=>window.conversationMock.openTasks('missing'));
 await settings(p).getByRole('alert').filter({hasText:'请求的项目已不可用'}).waitFor();assert.equal(await settings(p).locator('.collab-task-row').count(),0,'Unavailable targets do not fall back to another project');assert.equal(await settings(p).getByLabel('当前项目',{exact:true}).inputValue(),'');
 await p.evaluate(()=>window.conversationMock.openTasks('A'));await taskView(p);assert.equal(await settings(p).getByRole('alert').filter({hasText:'请求的项目已不可用'}).count(),0);await p.close();

 const failed=await fresh('evidence=1&confirm-fails=1');await failed.getByRole('button',{name:'查看任务 · 3',exact:true}).click();await taskView(failed);const failedDetail=await openDetails(failed);
 failed.once('dialog',d=>d.accept());await failedDetail.getByRole('button',{name:'确认原进程已停止并允许重试',exact:true}).click();
 await failedDetail.getByRole('alert').filter({hasText:'解除冻结失败'}).waitFor();assert.equal((await calls(failed,'ma_retry_task')).length,0);await failed.close();

 for(const theme of ['dark','light'])for(const size of [{width:1015,height:900},{width:390,height:844}]){
  const v=await fresh(`theme=${theme}`,size);await v.screenshot({path:`${out}/header-${theme}-${size.width}.png`});
  await v.getByRole('button',{name:'查看任务 · 3',exact:true}).click();await taskView(v);
  assert.equal(await v.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);await v.screenshot({path:`${out}/tasks-${theme}-${size.width}.png`});
  const d=await openDetails(v),b=await d.boundingBox();assert.ok(b.x>=0&&b.x+b.width<=size.width+1&&b.y>=0&&b.y+b.height<=size.height+1);
  const colors=await d.evaluate(el=>{const probe=document.createElement('span');probe.style.color='var(--bg-elev)';el.append(probe);const expected=getComputedStyle(probe).color;probe.remove();return {background:getComputedStyle(el).backgroundColor,expected,text:getComputedStyle(el.querySelector('.collab-task-evidence')).color};});
  assert.equal(colors.background,colors.expected,'Task details follow the current theme background');assert.notEqual(colors.background,colors.text,'Task evidence is readable');
  await v.screenshot({path:`${out}/details-${theme}-${size.width}.png`});await v.close();
 }

 const app=await browser.newPage({viewport:{width:1440,height:900}});app.setDefaultTimeout(10000);app.on('pageerror',e=>errors.push(e.message));
 await app.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
 await app.getByRole('button',{name:'设置',exact:true}).waitFor();
 await app.getByRole('button',{name:'设置',exact:true}).click();
 await app.waitForFunction(()=>window.appSessionMock);
 await app.evaluate(()=>window.appSessionMock.emit('crewvela-conversation-open-settings',{projectId:'B',target:'tasks'}));await taskView(app,'B',1);
 assert.match(await settings(app).locator('.collab-task-row').innerText(),/工程任务 B/);
 await settings(app).getByLabel('当前项目',{exact:true}).selectOption('A');
 await app.waitForFunction(()=>document.querySelector('.collab-task-row')?.textContent.includes('工程任务 A'));
 await settings(app).getByRole('tab',{name:'协作记录',exact:true}).click();
 await app.evaluate(()=>window.appSessionMock.emit('crewvela-conversation-open-settings',{projectId:'B',target:'tasks'}));await taskView(app,'B',1);
 await settings(app).getByRole('button',{name:'关闭设置',exact:true}).click();
 await app.getByRole('button',{name:'切换右侧栏',exact:true}).click();
 await app.getByRole('button',{name:'多智能体',exact:true}).click();await app.getByRole('button',{name:'查看任务 · 1',exact:true}).waitFor();
 await app.getByRole('button',{name:'查看任务 · 1',exact:true}).click();await taskView(app,'B',1);await app.close();
 assert.deepEqual(errors,[]);console.log('PASS: current-project task links, App dock/event routing, repeated requests, filters, hidden discussion tasks, full evidence, stop confirmation/cancel/failure, project scope, privacy navigation and dark/light narrow UI');
}finally{await browser.close();}
