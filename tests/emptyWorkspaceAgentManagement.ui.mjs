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
 await card.getByRole('button',{name:'选择其他目录…',exact:true}).click();
 const custom='G:\\agent-test';
 await page.evaluate(value=>window.directoryMock.resolve(value),custom);
 await card.getByRole('button',{name:'选择 Agent',exact:true}).click();
 await card.getByRole('button',{name:'管理 Agent',exact:true}).click();
 const management=page.getByRole('dialog',{name:'管理 Runtime',exact:true});
 await management.waitFor();
 assert.equal(await page.getByRole('heading',{name:'模型设置',exact:true}).count(),0);
 const popover=card.getByRole('dialog',{name:'选择 Agent',exact:true});
 const codex=management.getByRole('checkbox',{name:'启用 Codex',exact:true});
 const assertDirectory=async()=>assert.equal(await card.locator('label:has(input:checked)').getAttribute('title'),custom);
 await assertDirectory();
 mkdirSync('output/playwright',{recursive:true});
 await page.screenshot({path:'output/playwright/empty-workspace-agent-management.png',fullPage:true});
 await codex.uncheck();
 assert.equal(await codex.isChecked(),false);
 await management.getByRole('button',{name:'关闭',exact:true}).click();
 await popover.waitFor();
 assert.equal(await popover.getByRole('button',{name:'Codex',exact:true}).count(),0,'disabled Agent disappears immediately');
 await assertDirectory();
 assert.equal(await popover.getByRole('textbox',{name:'搜索 Agent'}).evaluate(el=>el===document.activeElement),true);
 await popover.getByRole('button',{name:'管理 Agent',exact:true}).click();
 await codex.check();
 await management.getByRole('button',{name:'上移 Codex',exact:true}).click();
 await page.keyboard.press('Escape');
 await popover.waitFor();
 const terminal=popover.getByRole('group',{name:'CLI 终端',exact:true});
 assert.equal(await terminal.locator('[data-agent-option]').first().innerText(),'Codex','management order applies to launcher');
 assert.equal(await popover.getByRole('button',{name:'Codex',exact:true}).isVisible(),true,'re-enabled Agent returns immediately');
 await assertDirectory();
 // A rejected settings write reports an error, retains persisted settings, and supports recovery.
 await popover.getByRole('button',{name:'管理 Agent',exact:true}).click();
 await page.evaluate(()=>{
  const original=Storage.prototype.setItem;
  Storage.prototype.setItem=function(key,value){
   if(key==='crewvela-runtime-preferences'){Storage.prototype.setItem=original;throw Error('fixture save failure')}
   return original.call(this,key,value);
  };
 });
 await codex.uncheck();
 await management.getByRole('alert').waitFor();
 assert.match(await management.getByRole('alert').innerText(),/fixture save failure/);
 assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem('crewvela-runtime-preferences')).disabledRuntimes.includes('codex')),false,'failed write retains saved enabled state');
 await management.getByRole('button',{name:'重新加载配置',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('input[aria-label="启用 Codex"]')?.checked===true);
 assert.equal(await codex.isChecked(),true,'reload restores last saved state');
 await management.getByRole('button',{name:'关闭',exact:true}).click();
 await popover.waitFor();
 assert.equal(await popover.getByRole('button',{name:'Codex',exact:true}).isVisible(),true);
 await assertDirectory();
 await page.screenshot({path:'output/playwright/empty-workspace-agent-list.png',fullPage:true});
 // Retain the chosen directory when launching after management.
 const calls=await page.evaluate(()=>window.directoryMock.calls.length);
 await popover.getByRole('button',{name:'DeepSeek Harness',exact:true}).click();
 const chatLaunch = page.getByRole('dialog',{name:'新建会话',exact:true});
 await chatLaunch.getByRole('button',{name:'启动',exact:true}).click();
 await page.locator('.harness-chat').waitFor();
 assert.ok(await page.getByText('DeepSeek Harness · agent-test',{exact:true}).count());
 assert.equal(await page.evaluate(()=>window.directoryMock.calls.length),calls);
 console.log('PASS: direct management, directory retention, disable/enable sync, reorder, Escape, failure handling and Agent launch');
 await page.close();
 process.env.CREWVELA_UI_URL='http://127.0.0.1:1489';
 process.env.CREWVELA_UI_ENTRY='/tests/fixtures/empty-workspace-directory-ui.html';
 await import('./emptyWorkspaceState.ui.mjs');
} finally {await browser.close();server.kill()}
