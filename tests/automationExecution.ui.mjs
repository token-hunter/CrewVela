import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const browser=await chromium.launch({channel:'chrome'}), errors=[];
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1432';
mkdirSync('.scratch/automation-execution/ui',{recursive:true});
try {
 for(const theme of ['dark','light'])for(const width of [1487,390]){
  const page=await browser.newPage({viewport:{width,height:1058}});page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/automation-execution-ui.html?theme=${theme}`);
  await page.locator('.automation-card').first().waitFor();
  assert.equal(await page.locator('.automation-card').count(),4);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true,'No horizontal overflow');
  if(width===1487){const cards=await page.locator('.automation-card').evaluateAll(es=>es.map(e=>e.getBoundingClientRect().toJSON()));assert.equal(Math.round(cards[0].y),Math.round(cards[1].y),'two columns');}
  await page.screenshot({path:`.scratch/automation-execution/ui/${theme}-${width}.jpg`,fullPage:true});
  if(width===1487){
    const seed=await page.evaluate(()=>window.__automationFixture.entries);
    const frame=await browser.newPage({viewport:{width:1487,height:1058}});
    await frame.addInitScript(seed=>window.__automationSeed=seed,seed);
    await frame.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=automation&section=automation&open=1&theme=${theme}`);
    await frame.locator('.automation-card').first().waitFor();
    assert.equal(await frame.locator('.settings-dialog').count(),1);
    const cards=await frame.locator('.automation-card').evaluateAll(es=>es.map(e=>e.getBoundingClientRect().toJSON()));
    assert.equal(Math.round(cards[0].y),Math.round(cards[1].y),'Actual settings host retains two columns');
    await frame.screenshot({path:`.scratch/automation-execution/ui/settings-${theme}-1487.jpg`,fullPage:true});
    await frame.close();
  }
  await page.getByRole('button',{name:'已完成1',exact:true}).click();
  assert.equal(await page.locator('.automation-card').count(),1);
  await page.getByRole('button',{name:'全部4',exact:true}).click();
  const failed=page.locator('.automation-card').filter({has:page.getByRole('heading',{name:'项目测试',exact:true})});
  await failed.getByRole('button',{name:'失败重试',exact:true}).click();
  await page.waitForFunction(()=>window.__automationFixture.calls.some(c=>c.command==='run_automation_now'));
  await page.waitForFunction(()=>window.__automationFixture.entries[2].recentRuns.length===2);
  assert.equal(await page.evaluate(()=>window.__automationFixture.entries[2].enabled),true);
  const runCall=await page.evaluate(()=>window.__automationFixture.calls.find(c=>c.command==='run_automation_now'));
  assert.equal(runCall.args.retryOf,'r2');assert.match(runCall.args.requestId,/^[0-9a-f-]{36}$/);
  const running=page.locator('.automation-card').filter({has:page.getByRole('heading',{name:'仓库巡检',exact:true})});
  await running.getByRole('button',{name:'停止本轮',exact:true}).click();
  await page.getByRole('alert').filter({hasText:'停止身份无法确认'}).waitFor();
  assert.equal(await page.locator('.execution-running').count(),1,'Failed stop must preserve running state');
  await running.getByRole('button',{name:'打开会话',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.__automationOpened),'session-fixture');
  await running.getByRole('button',{name:'执行记录 1',exact:true}).click();
  await page.getByRole('region',{name:'执行记录 仓库巡检'}).waitFor();
  await page.close();
 }
 const lost=await browser.newPage({viewport:{width:1487,height:1058}});
 await lost.goto(`${base}/tests/fixtures/automation-execution-ui.html?fail=1`);
 const failed=lost.locator('.automation-card').filter({has:lost.getByRole('heading',{name:'项目测试',exact:true})});
 await failed.getByRole('button',{name:'失败重试',exact:true}).click();await lost.getByRole('alert').filter({hasText:'模拟响应丢失'}).waitFor();
 await failed.getByRole('button',{name:'失败重试',exact:true}).click();
 await lost.waitForFunction(()=>window.__automationFixture.calls.filter(c=>c.command==='run_automation_now').length===2);
 const requests=await lost.evaluate(()=>window.__automationFixture.calls.filter(c=>c.command==='run_automation_now').map(c=>c.args.requestId));
 assert.equal(requests[0],requests[1],'Lost response retry keeps request identity');
 await lost.waitForFunction(()=>document.querySelector('.execution-queued'));
 assert.equal(await lost.evaluate(()=>window.__automationFixture.entries[2].recentRuns.length),2,'No second side effect');
 const unknown=lost.locator('.automation-card').filter({has:lost.getByRole('heading',{name:'文档整理',exact:true})});
 await unknown.getByRole('button',{name:'已核对原会话',exact:true}).click();
 await unknown.getByRole('button',{name:'确认原任务不再执行',exact:true}).click();
 await unknown.getByText('已核对·结果未知',{exact:true}).waitFor();assert.equal(await unknown.getByText('已完成',{exact:true}).count(),0);
 await lost.close();
 assert.deepEqual(errors,[]);console.log('automationExecution UI passed: dark/light, 2 columns/mobile, filters, retry identity/lost receipt, stop refusal, exact session, acknowledgement without success');
}finally{await browser.close();}
