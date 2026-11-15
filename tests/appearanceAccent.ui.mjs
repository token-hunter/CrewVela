import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';
const server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1488', '--strictPort'], {stdio:'pipe'});
await new Promise((resolve,reject)=>{server.stdout.on('data',d=>{if(d.toString().includes('1488'))resolve()});server.on('error',reject);server.on('exit',code=>reject(Error(`vite exited ${code}`)))});
const browser = await chromium.launch({channel:'chrome',headless:true});
mkdirSync('output/playwright',{recursive:true});
try {
 for (const old of ['blue','green']) {
  const page=await browser.newPage({viewport:{width:1440,height:1100}});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(accent=>{if(!localStorage.getItem('crewvela-appearance'))localStorage.setItem('crewvela-appearance',JSON.stringify({theme:'dark',background:'solid',accent}))},old);
  await page.goto('http://127.0.0.1:1488/tests/fixtures/appearance-accent-ui.html');
  const none=page.getByRole('radio',{name:'无强调色',exact:true});
  await none.waitFor();
  assert.equal(await none.getAttribute('aria-checked'),'true');
  assert.equal(await page.getByRole('radio',{name:/^(蓝色|绿色)$/}).count(),0);
  assert.equal(await page.getByRole('radio',{name:/^(无强调色|陶土|紫色|青色|橙色|粉色|珊瑚色|灰色)$/}).count(),8);
  for(const [label,theme,expected] of [['深色','dark','#c9c9c6'],['浅色','light','#454543'],['深海','ocean','#c9c9c6'],['石墨黑','purple','#c9c9c6'],['暖沙','sand','#454543'],['暖瓷白','aurora','#454543']]) {
   await page.getByRole('button',{name:label,exact:true}).click();
   await page.waitForFunction(t=>document.documentElement.dataset.appearanceTheme===t,theme);
   assert.equal(await page.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()),expected);
   await none.scrollIntoViewIfNeeded();
   await page.screenshot({path:`output/playwright/accent-none-${theme}.png`,fullPage:true});
  }
  const toggle=page.getByRole('switch'); await toggle.locator('..').click(); assert.equal(await toggle.getAttribute('aria-checked'),'true'); await toggle.locator('..').click(); assert.equal(await toggle.getAttribute('aria-checked'),'false');
  await page.getByRole('button',{name:'微纹理',exact:true}).click(); assert.equal(await page.getByRole('button',{name:'微纹理',exact:true}).getAttribute('aria-pressed'),'true');
  const purple=page.getByRole('radio',{name:'紫色',exact:true}); await purple.click();
  assert.equal(await purple.getAttribute('aria-checked'),'true');
  await none.focus(); await page.keyboard.press('Space');
  assert.equal(await none.getAttribute('aria-checked'),'true');
  await page.reload();await none.waitFor();assert.equal(await none.getAttribute('aria-checked'),'true');
  assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem('crewvela-appearance')).accent),'none');
  await page.getByRole('button',{name:'恢复默认'}).click();
  assert.equal(await page.getByRole('radio',{name:'陶土',exact:true}).getAttribute('aria-checked'),'true');
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('PASS: blue/green migration, 8 choices, 6 themes, mouse/keyboard selection, reload persistence and reset');
} finally {await browser.close();server.kill()}
