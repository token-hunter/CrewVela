import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright';
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1492','--strictPort'],{stdio:'pipe'});
await new Promise((resolve,reject)=>{server.stdout.on('data',d=>{if(d.toString().includes('1492'))resolve()});server.on('error',reject);server.on('exit',code=>reject(Error(`vite exited ${code}`)))});
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
 await page.goto('http://127.0.0.1:1492/tests/fixtures/empty-workspace-directory-ui.html?ui-preview=1&sessions=empty',{waitUntil:'domcontentloaded',timeout:60000});
 await page.locator('.ews-card').waitFor({timeout:60000});

 if(!await page.locator('.right-sidebar-start').isVisible())await page.getByRole('button',{name:'切换右侧栏',exact:true}).click();
 await page.locator('.right-sidebar-start').getByRole('button',{name:'终端',exact:true}).click();
 const host=page.locator('.right-sidebar-terminal.active');
 const pane=host.locator('.pane');
 await pane.waitFor();
 const verify=async label=>{
  const h=await host.boundingBox(),p=await pane.boundingBox();
  assert.ok(Math.abs(h.height-p.height)<=2,`${label}: terminal must fill panel (panel=${h.height}, terminal=${p.height})`);
  assert.equal(await pane.locator('.pane-head').isVisible(),false,`${label}: no duplicate terminal header`);
 };
 await verify('initial');
 for(const size of [{width:1440,height:1316},{width:1100,height:540}]){
  await page.setViewportSize(size);await page.waitForTimeout(100);await verify(`${size.width}x${size.height}`);
 }
 await page.getByRole('tab',{name:'开始',exact:true}).click();
 await page.locator('.right-sidebar-start').waitFor();
 await page.getByRole('tab',{name:'PowerShell',exact:true}).click();
 await verify('tab restore');
 await page.getByRole('button',{name:'切换右侧栏',exact:true}).click();
 await page.getByRole('button',{name:'切换右侧栏',exact:true}).click();
 await page.waitForTimeout(200);await verify('sidebar restore');
 await page.setViewportSize({width:1440,height:1000});
 mkdirSync('output/playwright',{recursive:true});
 await page.locator('.unified-right-sidebar').screenshot({path:'output/playwright/sidebar-terminal-fixed.png'});
 await page.goto('http://127.0.0.1:1492/tests/fixtures/terminal-init-ui.html?sidebar=1');
 await page.locator('.xterm').waitFor();
 await page.waitForFunction(()=>window.terminalMock.terminal.rows>20);
 assert.equal(await page.locator('.pane-head').isVisible(),false);
 const nativeHost=page.locator('.right-sidebar-terminal');
 assert.ok(Math.abs((await nativeHost.boundingBox()).height-(await nativeHost.locator('.pane').boundingBox()).height)<=2);
 await page.evaluate(()=>window.dispatchEvent(new CustomEvent('pty:chunk',{detail:{'1':{data:'PS G:\\tmp> ready\r\n',exitCode:null}}})));
 await page.waitForFunction(()=>window.terminalMock.terminal.buffer.active.getLine(0).translateToString().includes('ready'));
 await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type('test');
 await page.getByRole('alert').filter({hasText:'fixture input refused'}).waitFor();
 await page.setViewportSize({width:1100,height:540});
 await page.waitForFunction(()=>window.terminalMock.terminal.rows>10);
 await page.getByRole('button',{name:'关闭终端',exact:true}).click();
 await page.locator('.xterm').waitFor({state:'detached'});
 process.env.CREWVELA_UI_URL='http://127.0.0.1:1492';
 await import('./terminalInit.ui.mjs');
 console.log('PASS: sidebar height, hidden header, resizing, tab restore, real xterm output/input/error/cleanup and existing terminal regression');
}finally{await browser.close();server.kill()}
