// Production component with isolated IPC. No real plans, messages or Agent processes.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const browser=await chromium.launch({channel:'chrome'});
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const errors=[];
const out='.scratch/automation-edit-20261116/ui';mkdirSync(out,{recursive:true});
async function fresh(query='',width=1440,seed){
 const page=await browser.newPage({viewport:{width,height:1000}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
 if(seed)await page.addInitScript(seed=>{window.__automationEditSeed=seed;},seed);
 await page.goto(`${base}/tests/fixtures/automation-execution-ui.html?${query}`);await page.locator('.automation-card').first().waitFor();return page;
}
const calls=(page,command)=>page.evaluate(command=>window.__automationFixture.calls.filter(c=>c.command===command),command);
const editForm=page=>page.locator('.automation-form');
try{
 for(const theme of ['dark','light'])for(const width of [1440,390]){
  const page=await fresh(`theme=${theme}`,width);
  const card=page.locator('.automation-card').first();
  const original=await page.evaluate(()=>structuredClone(window.__automationFixture.entries[0]));
  await card.locator('.automation-card-menu > summary').click();
  await card.getByRole('button',{name:'编辑定时任务',exact:true}).waitFor();
  await page.screenshot({path:`${out}/menu-${theme}-${width}.png`,fullPage:true});
  await card.getByRole('button',{name:'编辑定时任务',exact:true}).click();
  await editForm(page).waitFor();
  assert.equal(await page.getByLabel('名称',{exact:true}).inputValue(),original.name);
  assert.equal(await page.getByLabel('执行 Agent',{exact:true}).inputValue(),'codex');
  assert.equal(await page.getByLabel('执行任务',{exact:true}).inputValue(),original.action.prompt);
  assert.equal(await page.getByLabel('工作目录',{exact:true}).inputValue(),original.action.cwd);
  await page.getByRole('button',{name:'取消',exact:true}).click();
  assert.equal((await calls(page,'update_automation')).length,0);
  await card.locator('.automation-card-schedule').click();await editForm(page).waitFor();
  await page.getByLabel('名称',{exact:true}).fill('编辑后的计划');
  await page.getByLabel('执行任务',{exact:true}).fill('新的任务内容');
  await page.getByLabel('时间',{exact:true}).fill('11:15');
  await page.screenshot({path:`${out}/form-${theme}-${width}.png`,fullPage:true});
  await page.getByRole('button',{name:'保存修改',exact:true}).click();
  await editForm(page).waitFor({state:'hidden'});
  const updated=await page.evaluate(()=>window.__automationFixture.entries[0]);
  assert.equal((await calls(page,'create_automation')).length,0);
  assert.equal((await calls(page,'update_automation')).length,1);
  assert.equal(updated.id,original.id);assert.equal(updated.name,'编辑后的计划');assert.equal(updated.action.prompt,'新的任务内容');
  assert.deepEqual(updated.trigger,{kind:'daily',hour:11,minute:15});
  for(const field of ['recentRuns','runCount','createdAt','enabled','requestReceipts'])assert.deepEqual(updated[field],original[field]);
  await card.getByRole('switch').uncheck();assert.equal(await editForm(page).count(),0);
  await card.getByRole('button',{name:/执行记录/}).click();assert.equal(await editForm(page).count(),0);
  await card.getByRole('button',{name:'打开会话',exact:true}).click();assert.equal(await editForm(page).count(),0);
  await card.locator('.automation-card-menu > summary').click();
  await card.getByRole('button',{name:'打开原会话',exact:true}).click();assert.equal(await editForm(page).count(),0);
  assert.equal((await calls(page,'run_automation_now')).length,0);
  await card.focus();await page.keyboard.press('Enter');await editForm(page).waitFor();
  assert.equal(await page.getByLabel('名称',{exact:true}).inputValue(),'编辑后的计划');
  assert.equal(await page.getByLabel('执行任务',{exact:true}).inputValue(),'新的任务内容');
  assert.equal(await page.getByLabel('时间',{exact:true}).inputValue(),'11:15');
  await page.getByRole('button',{name:'取消',exact:true}).click();
  await page.getByRole('button',{name:'新建自动化',exact:true}).click();
  await page.getByRole('button',{name:'保存自动化',exact:true}).waitFor();
  assert.equal(await page.getByLabel('名称',{exact:true}).inputValue(),'');
  assert.equal(await page.getByLabel('执行任务',{exact:true}).inputValue(),'');
  await page.getByRole('button',{name:'取消',exact:true}).click();
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  await page.close();
 }
 const failed=await fresh('fail-edit=1');await failed.locator('.automation-card').first().getByRole('button',{name:'仓库巡检',exact:true}).click();
 await failed.getByLabel('名称',{exact:true}).fill('失败后重试');
 await failed.getByRole('button',{name:'保存修改',exact:true}).click();
 await failed.getByRole('alert').filter({hasText:'保存失败'}).waitFor();
 assert.equal(await failed.getByLabel('名称',{exact:true}).inputValue(),'失败后重试');
 assert.equal(await failed.evaluate(()=>window.__automationFixture.entries[0].name),'仓库巡检');
 await failed.getByRole('button',{name:'保存修改',exact:true}).click();await editForm(failed).waitFor({state:'hidden'});
 assert.equal((await calls(failed,'update_automation')).length,2);await failed.close();
 const source=await fresh();const seed=await source.evaluate(()=>structuredClone(window.__automationFixture.entries[1]));await source.close();
 const host=await browser.newPage({viewport:{width:1440,height:1000}});
 await host.addInitScript(seed=>{window.__automationSeed=[seed];},seed);
 await host.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=automation&open=1&section=automation`);
 await host.locator('.automation-card-schedule').click();
 await host.getByRole('heading',{name:'编辑定时任务',exact:true}).waitFor();
 assert.equal(await host.getByRole('heading',{name:'创建自动化',exact:true}).count(),0);
 await host.screenshot({path:`${out}/settings-edit.png`,fullPage:true});
 await host.getByRole('button',{name:'保存修改',exact:true}).click();
 await host.getByRole('heading',{name:'自动化',exact:true}).waitFor();
 assert.equal(await host.evaluate(()=>window.__automationCalls.filter(c=>c.command==='update_automation').length),1);await host.close();
 for(const trigger of [{kind:'once',at:Date.parse('2026-11-15T10:20:30.456+08:00')},{kind:'interval',minutes:37},{kind:'weekly',weekdays:[2,4,7],hour:13,minute:47},{kind:'cron',expr:'15 10 * * 1-5'},{kind:'on_session_event',event:'failed'},{kind:'on_session_event_filtered',event:'completed',runtimeKind:'codex',sessionId:null,projectId:null}]){
  const page=await fresh('',1440,[{...seed,trigger}]);await page.locator('.automation-card-schedule').click();await editForm(page).waitFor();
  await page.getByRole('button',{name:'保存修改',exact:true}).click();await editForm(page).waitFor({state:'hidden'});
  assert.deepEqual((await calls(page,'update_automation'))[0].args.input.trigger,trigger,'Unchanged trigger round-trips without broadening scope or losing milliseconds');await page.close();
 }
 assert.deepEqual(errors,[]);
 console.log('PASS automation editing: menu/card/name/keyboard, all trigger kinds, save by ID, cancel/new isolation, failure retry, independent controls, history retention, dark/light/narrow');
}finally{await browser.close();}
