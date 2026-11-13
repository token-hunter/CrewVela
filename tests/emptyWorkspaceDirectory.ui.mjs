import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright';
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1489','--strictPort'],{stdio:'pipe'});
await new Promise((resolve,reject)=>{server.stdout.on('data',d=>{if(d.toString().includes('1489'))resolve()});server.on('error',reject);server.on('exit',code=>reject(Error(`vite exited ${code}`)))});
const browser=await chromium.launch({channel:'chrome',headless:true});
try {
 const page=await browser.newPage({viewport:{width:1440,height:1000}});
 page.setDefaultTimeout(15000);
 const setup=()=>{
  window.directoryMock={calls:[],pending:[],resolve(value){this.pending.shift().resolve(value)},reject(){this.pending.shift().reject(Error('fixture picker failure'))}};
  window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},transformCallback:()=>1,invoke:async(command,args)=>{
   if(command==='plugin:dialog|open'){window.directoryMock.calls.push(args);return new Promise((resolve,reject)=>window.directoryMock.pending.push({resolve,reject}))}
   if(command==='plugin:event|listen')return 1;
   return null;
  }};
 };
 await page.addInitScript(setup);
 await page.goto('http://127.0.0.1:1489/tests/fixtures/empty-workspace-directory-ui.html?ui-preview=1&sessions=empty',{waitUntil:'domcontentloaded',timeout:60000});
 await page.locator('.ews-card').waitFor({timeout:60000});

 const card=page.locator('.ews-card');
 const picker=page.getByRole('button',{name:'选择其他目录…',exact:true});
  const checkedPath=()=>card.locator('label:has(input:checked)').getAttribute('title');
 const original=await checkedPath();
 const pick=async value=>{
  const count=await page.evaluate(()=>window.directoryMock.calls.length);
  await picker.click();
  assert.equal(await page.evaluate(()=>window.directoryMock.calls.length),count+1,'click opens picker immediately');
  assert.equal(await card.getByRole('button',{name:'选择 Agent',exact:true}).isDisabled(),true);
  await page.evaluate(value=>window.directoryMock.resolve(value),value);
  await picker.waitFor();
 };
 await pick(null);
 assert.equal(await checkedPath(),original,'cancel retains existing project');
 const custom='G:\\目录测试\\new-project';
 await pick(custom);
 assert.equal(await checkedPath(),custom);
 assert.equal(await card.getByRole('radio',{name:'new-project',exact:true}).isChecked(),true);
 if(!await page.locator('.right-sidebar-start').isVisible())await page.getByRole('button',{name:'切换右侧栏',exact:true}).click();
 assert.equal(await page.locator('.right-sidebar-start').getAttribute('data-workspace-root'),custom,'right sidebar follows directory');
 await pick(null);assert.equal(await checkedPath(),custom,'cancel retains custom directory');
 await picker.click(); await page.evaluate(()=>window.directoryMock.reject());
 await card.getByRole('alert').waitFor();assert.match(await card.getByRole('alert').innerText(),/fixture picker failure/);
 assert.equal(await checkedPath(),custom,'failure retains directory');
 // Switch back to an existing project and match a selected existing path.
 await card.locator('label').first().click();assert.equal(await checkedPath(),original);
 assert.equal(await page.locator('.right-sidebar-start').getAttribute('data-workspace-root'),original);
 await pick(original);assert.equal(await card.getByRole('radio').count(),1,'existing path is reused');
 await pick(custom);
 // Workspace tabs retain independent directory choices.
 await page.getByRole('button',{name:'新增页签',exact:true}).click();
 await card.waitFor();assert.equal(await checkedPath(),original);
 await page.getByRole('button',{name:'工作区 1',exact:true}).click();assert.equal(await checkedPath(),custom);
 // Picking while leaving the tab must not change the new tab when the dialog resolves.
 await picker.click();await page.getByRole('button',{name:'工作区 2',exact:true}).click();
 await page.evaluate(()=>window.directoryMock.resolve('G:\\stale'));
 assert.equal(await checkedPath(),original);
 await page.getByRole('button',{name:'工作区 1',exact:true}).click();assert.equal(await checkedPath(),custom);
 mkdirSync('output/playwright',{recursive:true});
 await page.screenshot({path:'output/playwright/empty-workspace-directory.png',fullPage:true});
 const dialogCalls=await page.evaluate(()=>window.directoryMock.calls.length);
 await card.getByRole('button',{name:'选择 Agent',exact:true}).click();
 await page.getByRole('dialog',{name:'选择 Agent',exact:true}).getByRole('button',{name:'DeepSeek Harness',exact:true}).click();
 await page.locator('.harness-chat').waitFor();
 assert.equal(await page.evaluate(()=>window.directoryMock.calls.length),dialogCalls,'Agent must not ask for directory again');
 assert.equal(await page.locator('.right-sidebar-start').getAttribute('data-workspace-root'),custom);
 assert.ok(await page.getByText('DeepSeek Harness · new-project',{exact:true}).count());
 console.log('PASS: immediate picker, selection, cancel/error retention, right sidebar, existing projects, independent tabs, stale dialog and native chat launch');
 await page.close();
 for (const agent of ['Codex CLI','Claude Code']) {
  const cliPage=await browser.newPage({viewport:{width:1440,height:1000}});
  cliPage.setDefaultTimeout(15000);
  await cliPage.addInitScript(setup);
  await cliPage.goto('http://127.0.0.1:1489/tests/fixtures/empty-workspace-directory-ui.html?ui-preview=1&sessions=empty');
  await cliPage.getByRole('button',{name:'选择其他目录…',exact:true}).click();
  await cliPage.evaluate(value=>window.directoryMock.resolve(value),custom);
  await cliPage.getByRole('button',{name:'选择 Agent',exact:true}).click();
  await cliPage.getByRole('dialog',{name:'选择 Agent',exact:true}).getByRole('button',{name:agent==='Codex CLI'?'Codex':agent,exact:true}).click();
  if(agent==='Claude Code')await cliPage.getByRole('button',{name:/使用本地 Claude Code 配置/}).click();
  await cliPage.locator('.session-item').filter({hasText:`${agent} · new-project`}).first().waitFor();
  assert.equal(await cliPage.evaluate(()=>window.directoryMock.calls.length),1,`${agent}: reuse chosen path without a second dialog`);
  await cliPage.close();
 }
 console.log('PASS: Codex CLI and local Claude Code reuse selected directory');
 process.env.CREWVELA_UI_URL='http://127.0.0.1:1489';
 process.env.CREWVELA_UI_ENTRY='/tests/fixtures/empty-workspace-directory-ui.html';
 await import('./emptyWorkspaceState.ui.mjs');
 await import('./rightSidebarStart.ui.mjs');


}finally{await browser.close();server.kill()}
