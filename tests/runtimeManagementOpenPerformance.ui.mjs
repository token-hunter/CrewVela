import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1448';
const output='.scratch/runtime-management-open-performance';mkdirSync(output,{recursive:true});
const browser=await chromium.launch({channel:'chrome'});
const errors=[],timings=[];
try {
 const page=await browser.newPage({viewport:{width:1280,height:960}});page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`${base}/tests/fixtures/runtime-installer-ui.html?installed=1&previous-job=1&slow-local=1&slow-updates=1`);
 const open=page.locator('.collab-runtime-management-row').filter({has:page.locator('img[data-runtime="claude_code"]')}).getByRole('button',{name:'安装与版本',exact:true});
 const measured=await open.evaluate(async button=>{
  const started=performance.now();button.click();
  await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
  return {ms:performance.now()-started,visible:!!document.querySelector('.runtime-installer')};
 });
 assert.equal(measured.visible,true);assert.ok(measured.ms<500,`first paint ${measured.ms}ms`);timings.push(measured);
 await page.waitForTimeout(100);
 assert.equal(await page.evaluate(()=>window.runtimeInstallerFixture.calls.filter(c=>c.command==='runtime_install_environment').length),1,'opening a previous completed installation must not start two overlapping native inspections');
 const back=page.getByRole('button',{name:'返回 Runtime 列表'});
 const before=Date.now();await back.click();await open.waitFor();assert.ok(Date.now()-before<500,'slow probes cannot block return');
 await page.getByRole('button',{name:'关闭',exact:true}).click();await page.getByRole('button',{name:'重新打开管理'}).waitFor();
 await page.waitForTimeout(1600);assert.equal(await page.locator('.runtime-installer').count(),0,'late result must not reopen the installer');
 await page.getByRole('button',{name:'重新打开管理'}).click();
 for(let index=0;index<9;index++) {
  const measured=await open.evaluate(async button=>{const at=performance.now();button.click();await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return {ms:performance.now()-at,visible:!!document.querySelector('.runtime-installer')};});
  assert.equal(measured.visible,true);assert.ok(measured.ms<500,`reopen paint ${measured.ms}ms`);timings.push(measured);
  await page.getByRole('button',{name:'返回 Runtime 列表',exact:true}).click();
 }
 // Local installation facts must not wait for slow official-version queries.
 for (const theme of ['dark','light']) {
  await page.goto(`${base}/tests/fixtures/runtime-installer-ui.html?installed=1&slow-updates=1&theme=${theme}`);
  const row=page.locator('.collab-runtime-management-row').filter({has:page.locator('img[data-runtime="claude_code"]')});
  const started=Date.now();await row.getByRole('button',{name:'安装与版本',exact:true}).click();
  await page.getByRole('button',{name:'升级或修复安装',exact:true}).waitFor();
  assert.ok(Date.now()-started<500,'local checks cannot wait for official network lookup');
  assert.equal(await page.getByText('官方版本 正在确认…',{exact:true}).count(),1);
  await page.getByText('安装详情',{exact:true}).click();
  await page.getByText('C:/fixture/official/claude_code.exe',{exact:true}).waitFor();
  await page.screenshot({path:`${output}/${theme}.png`});
  await page.getByRole('button',{name:'返回 Runtime 列表',exact:true}).click();
  const codex=page.locator('.collab-runtime-management-row').filter({has:page.locator('img[data-runtime="codex"]')});
  await codex.getByRole('button',{name:'安装与版本',exact:true}).click();
  await page.getByText('官方版本 0.169.0',{exact:true}).waitFor();
  assert.equal(await page.getByText('官方版本 2.1.325',{exact:true}).count(),0,'late previous-runtime lookup must not overwrite the current runtime');
  await page.setViewportSize({width:820,height:600});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await page.screenshot({path:`${output}/${theme}-narrow.png`});
  await page.setViewportSize({width:1280,height:960});
 }
 await page.goto(`${base}/tests/fixtures/runtime-installer-ui.html?installed=1&update-fails=1`);
 await page.locator('.collab-runtime-management-row').filter({has:page.locator('img[data-runtime="claude_code"]')}).getByRole('button',{name:'安装与版本',exact:true}).click();
 await page.getByText(/官方版本暂未确认：.*fixture official version unavailable/).waitFor();
 assert.equal(await page.getByRole('button',{name:'升级或修复安装',exact:true}).isEnabled(),true,'official lookup failure does not erase a verified local installation');
 assert.equal(await page.getByText('官方版本 尚未确认',{exact:true}).count(),1);
 assert.deepEqual(errors,[]);writeFileSync(`${output}/timings.json`,JSON.stringify(timings,null,2));
 console.log('PASS Runtime first paint, one inspection, local/network independence, official lookup failure, responsive return/close, stale result isolation and dark/light/narrow layout',timings);
} finally {await browser.close();}
