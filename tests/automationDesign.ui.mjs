// Rendered production components with explicit fixture IPC. No existing plans or CLI are touched.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const browser=await chromium.launch({channel:'chrome'}),errors=[];
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1433';
const out='.scratch/automation-style-parity-20261116/ui';mkdirSync(out,{recursive:true});
try{
 for(const theme of ['dark','light']){
  const page=await browser.newPage({viewport:{width:1487,height:1058}});page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/automation-execution-ui.html?design=1&theme=${theme}`);await page.locator('.automation-card').first().waitFor();
  const history=page.getByRole('region',{name:'执行记录 仓库巡检',exact:true});await history.waitFor();assert.equal(await history.locator('details').count(),3);
  assert.match(await page.locator('.automation-execution-id').first().innerText(),/执行 r0/,'Use actual receipt identity, not schedule count');
  const running=page.locator('.automation-card').filter({has:page.getByRole('heading',{name:'仓库巡检',exact:true})});
  await running.getByRole('switch').uncheck();
  await page.waitForFunction(()=>!window.__automationFixture.entries[0].enabled);
  assert.equal(await running.getByRole('button',{name:'停止本轮',exact:true}).isVisible(),true,'Pausing the plan keeps current-run controls');
  assert.equal(await page.evaluate(()=>window.__automationFixture.entries[0].recentRuns.at(-1).stage),'running');
  const paused=page.locator('.automation-card').filter({has:page.getByRole('heading',{name:'文档整理',exact:true})});await paused.getByRole('button',{name:'开启计划',exact:true}).click();
  await page.waitForFunction(()=>window.__automationFixture.entries[3].enabled);assert.equal(await page.evaluate(()=>window.__automationFixture.calls.filter(c=>c.command==='run_automation_now').length),0,'Enable only updates scheduling, never invokes Run Now');
  const seed=await page.evaluate(()=>window.__automationFixture.entries);seed[0].enabled=true;seed[3].enabled=false;
  const sessions=await page.evaluate(()=>window.__automationFixture.sessions);
  for(const size of [{width:1487,height:1058},{width:820,height:540},{width:687,height:903}]){
   const frame=await browser.newPage({viewport:size});frame.on('pageerror',e=>errors.push(e.message));
   await frame.addInitScript(({seed,sessions})=>{window.__automationSeed=seed;window.__automationSessions=sessions;},{seed,sessions});
   await frame.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=automation&open=1&section=automation&theme=${theme}`);await frame.locator('.automation-card').first().waitFor();
   assert.equal(await frame.getByRole('heading',{name:'自动化',exact:true}).count(),1,'One visible list heading');
   assert.equal(await frame.getByRole('region',{name:'执行记录 仓库巡检',exact:true}).locator('details').count(),3);
   assert.equal(await frame.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
   const create=frame.getByRole('button',{name:'新建自动化',exact:true});
   assert.equal(await create.evaluate(e=>e.scrollWidth<=e.clientWidth+1&&e.scrollHeight<=e.clientHeight+1),true,'New automation label is not clipped');
   await frame.locator('.automation-card-menu > summary').first().click();await frame.locator('.automation-card-menu > summary').first().press('Escape');
   assert.equal(await frame.locator('.automation-card-menu[open]').count(),0);assert.equal(await frame.locator('.settings-dialog').isVisible(),true);
   await frame.locator('.automation-card-menu > summary').first().click();await frame.getByRole('button',{name:'查看全部执行记录',exact:true}).click();
   assert.equal(await frame.getByRole('region',{name:'执行记录 仓库巡检',exact:true}).isVisible(),true,'View all records keeps already selected history open');
   await frame.evaluate(()=>document.activeElement?.blur());
   await frame.locator('.settings-content').evaluate(e=>e.scrollTop=0);
   await frame.screenshot({path:`${out}/design-${theme}-${size.width}.png`,fullPage:true});
   if(size.width===1487)await frame.locator('.settings-dialog').screenshot({path:`${out}/design-${theme}-dialog.png`});
   await create.click();await frame.getByPlaceholder('例如：每天早上巡检依赖更新').waitFor();
   assert.equal(await frame.getByRole('heading',{name:'创建自动化',exact:true}).count(),1,'Creation retains one settings heading');
   await frame.getByRole('button',{name:'取消',exact:true}).click();await frame.getByRole('heading',{name:'自动化',exact:true}).waitFor();
   await frame.close();
  }
  const completed=page.locator('.automation-card').filter({has:page.getByRole('heading',{name:'日报摘要',exact:true})});
  await completed.locator('.automation-card-menu > summary').click();await completed.getByRole('button',{name:'删除 日报摘要',exact:true}).click();
  await completed.getByRole('button',{name:'取消',exact:true}).click();assert.equal(await page.evaluate(()=>window.__automationFixture.calls.filter(c=>c.command==='delete_automation').length),0);
  await completed.locator('.automation-card-menu > summary').click();await completed.getByRole('button',{name:'删除 日报摘要',exact:true}).click();await completed.getByRole('button',{name:'确认删除',exact:true}).click();
  await completed.waitFor({state:'detached'});assert.equal(await page.evaluate(()=>window.__automationFixture.calls.filter(c=>c.command==='delete_automation').length),1,'Confirmed deletion still invokes existing API once');
  await page.close();
 }
 const narrow=await browser.newPage({viewport:{width:390,height:844}});await narrow.goto(`${base}/tests/fixtures/automation-execution-ui.html`);await narrow.locator('.automation-card').first().waitFor();
 assert.equal(await narrow.getByRole('button',{name:'新建自动化',exact:true}).evaluate(e=>e.getBoundingClientRect().width>100&&e.scrollHeight<=e.clientHeight+1),true,'390px standalone button retains readable text');
 await narrow.close();assert.deepEqual(errors,[]);console.log('PASS: selected automation design, real IDs, default history, independent plan controls, menu Escape, form transition and dark/light responsive UI');
}finally{await browser.close();}
