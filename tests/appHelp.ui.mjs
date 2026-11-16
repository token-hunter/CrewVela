import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1433';
const out='.scratch/app-help-20261116/ui';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[];
const release=(status='available')=>({currentVersion:'0.1.0',latestVersion:status==='ahead'?'0.0.9':status==='current'?'0.1.0':'0.2.0',status,releaseUrl:'https://github.com/token-hunter/CrewVela/releases/tag/v0.2.0',releaseNotes:'更新说明\n<img src=x onerror=alert(1)>\n[链接](https://evil.invalid)'});
try{
 for(const theme of ['dark','light'])for(const size of [{width:1440,height:900},{width:820,height:540}]){
  const page=await browser.newPage({viewport:size});page.on('pageerror',e=>errors.push(e.message));page.setDefaultTimeout(10000);
  await page.goto(`${base}/tests/fixtures/app-session-async-ui.html?app-help=1&theme=${theme}`);const trigger=page.getByRole('button',{name:'更多工具',exact:true});await trigger.waitFor();
  const open=async()=>{await trigger.click();await page.getByRole('menu',{name:'更多工具',exact:true}).getByRole('menuitem',{name:'帮助',exact:true}).click();await page.getByRole('menu',{name:'帮助',exact:true}).waitFor();};
  const select=async name=>{await open();await page.getByRole('menu',{name:'帮助',exact:true}).getByRole('menuitem',{name,exact:true}).click();};
  await trigger.click();const root=page.getByRole('menu',{name:'更多工具',exact:true});assert.deepEqual(await root.getByRole('menuitem').allTextContents(),['视图','帮助']);
  const help=root.getByRole('menuitem',{name:'帮助',exact:true});await help.focus();await page.keyboard.press('ArrowRight');const submenu=page.getByRole('menu',{name:'帮助',exact:true});await submenu.waitFor();
  assert.equal(await submenu.getByRole('menuitem').first().evaluate(e=>e===document.activeElement),true);await page.keyboard.press('ArrowDown');assert.equal(await submenu.getByRole('menuitem').nth(1).evaluate(e=>e===document.activeElement),true);
  const box=await submenu.boundingBox();assert.ok(box.x>=0&&box.y>=0&&box.x+box.width<=size.width&&box.y+box.height<=size.height,'Submenu stays in viewport');
  await page.screenshot({path:`${out}/menu-${theme}-${size.width}.png`});await page.keyboard.press('ArrowLeft');assert.equal(await submenu.count(),0);assert.equal(await help.evaluate(e=>e===document.activeElement),true);await page.keyboard.press('Escape');assert.equal(await root.count(),0);
  await select('隐私政策');const policy=page.getByRole('dialog',{name:'隐私政策',exact:true});await policy.waitFor();assert.match(await policy.innerText(),/CLI 原生历史/);assert.equal(await page.evaluate(()=>window.appSessionMock.help.checks),0,'Privacy is offline');
  await policy.screenshot({path:`${out}/privacy-${theme}-${size.width}.png`});await policy.locator('.app-help-content').evaluate(el=>el.scrollTop=el.scrollHeight);assert.equal(await policy.getByRole('button',{name:'关闭',exact:true}).isVisible(),true);await page.keyboard.press('Escape');assert.equal(await trigger.evaluate(e=>e===document.activeElement),true);
  await page.evaluate(()=>window.appSessionMock.help.version='7.8.9+fixture');await select('关于 CrewVela');const about=page.getByRole('dialog',{name:'关于 CrewVela',exact:true});await about.getByText('版本 7.8.9+fixture',{exact:true}).waitFor();
  await about.getByRole('img',{name:'CrewVela Logo',exact:true}).waitFor();assert.equal(await about.getByRole('button').count(),1);assert.equal(await about.locator('header,footer,dl').count(),0);
  assert.ok((await about.boundingBox()).width<=400);assert.equal(await about.getByRole('heading',{name:'CrewVela',exact:true}).count(),1);
  await page.evaluate(()=>document.activeElement?.blur());await about.screenshot({path:`${out}/about-${theme}-${size.width}.png`});await about.getByRole('button',{name:'关闭帮助窗口',exact:true}).click();
  await select('检查更新…');const update=page.getByRole('dialog',{name:'检查更新',exact:true});await update.getByRole('status').waitFor();await page.waitForFunction(()=>window.appSessionMock.help.pending.length===1);assert.equal(await page.evaluate(()=>window.appSessionMock.help.checks),1);
  await page.evaluate(()=>window.appSessionMock.help.fail('发布源不可用（HTTP 404）：尚无公开稳定发行版'));await update.getByRole('alert').waitFor();assert.doesNotMatch(await update.innerText(),/已是公开最新稳定版/);await update.screenshot({path:`${out}/update-error-${theme}-${size.width}.png`});
  await update.getByRole('button',{name:'重新检查',exact:true}).click();await page.waitForFunction(()=>window.appSessionMock.help.pending.length===1);await page.evaluate(value=>window.appSessionMock.help.complete(value),release());await update.getByRole('heading',{name:'发现新版本',exact:true}).waitFor();
  await update.getByText('发行说明',{exact:true}).click();assert.match(await update.locator('pre').innerText(),/<img/);assert.equal(await update.locator('.app-update-result img').count(),0,'Remote notes stay text');
  await page.evaluate(()=>window.appSessionMock.help.openFails=true);await update.getByRole('button',{name:'前往发布页下载',exact:true}).click();await update.getByRole('alert').waitFor();assert.match(await update.getByRole('alert').innerText(),/https:\/\/github.com\/token-hunter\/CrewVela\/releases/);
  await page.evaluate(()=>window.appSessionMock.help.openFails=false);await update.getByRole('button',{name:'前往发布页下载',exact:true}).click();await update.getByRole('alert').waitFor({state:'detached'});assert.equal(await page.evaluate(()=>window.appSessionMock.help.opens.at(-1)),'https://github.com/token-hunter/CrewVela/releases/tag/v0.2.0');
  await update.screenshot({path:`${out}/update-available-${theme}-${size.width}.png`});
  for(const status of ['current','ahead']){await update.getByRole('button',{name:'重新检查',exact:true}).click();await page.waitForFunction(()=>window.appSessionMock.help.pending.length===1);await page.evaluate(value=>window.appSessionMock.help.complete(value),release(status));await update.getByRole('heading',{name:status==='current'?'已是公开最新稳定版':'当前版本高于公开稳定版',exact:true}).waitFor();}
  await update.getByRole('button',{name:'重新检查',exact:true}).click();await page.waitForFunction(()=>window.appSessionMock.help.pending.length===1);await page.evaluate(value=>window.appSessionMock.help.complete(value),{...release(),releaseUrl:'https://evil.invalid/file.exe'});await update.getByRole('alert').waitFor();assert.match(await update.innerText(),/未知链接/);
  const before=await page.evaluate(()=>window.appSessionMock.help.checks);
  await update.getByRole('button',{name:'重新检查',exact:true}).evaluate(el=>{el.click();el.click();});await page.waitForFunction(()=>window.appSessionMock.help.pending.length===1);assert.equal(await page.evaluate(()=>window.appSessionMock.help.checks),before+1,'One in-flight check per window');
  await update.getByRole('button',{name:'关闭',exact:true}).click();await select('关于 CrewVela');await page.getByText('版本 7.8.9+fixture',{exact:true}).waitFor();
  await page.evaluate(value=>window.appSessionMock.help.complete(value),release());assert.equal(await page.getByRole('dialog',{name:'关于 CrewVela',exact:true}).count(),1,'Late check cannot replace the newer dialog');
  await page.getByRole('dialog',{name:'关于 CrewVela',exact:true}).getByRole('button',{name:'关闭帮助窗口',exact:true}).click();
  await page.evaluate(()=>window.appSessionMock.help.versionFails=true);await select('关于 CrewVela');await page.getByRole('dialog',{name:'关于 CrewVela',exact:true}).getByRole('alert').waitFor();
  await page.evaluate(()=>{window.appSessionMock.help.versionFails=false;window.appSessionMock.help.version='9.9.9';});await page.getByRole('button',{name:'重新读取版本',exact:true}).click();await page.getByText('版本 9.9.9',{exact:true}).waitFor();
  await page.getByRole('dialog',{name:'关于 CrewVela',exact:true}).getByRole('button',{name:'关闭帮助窗口',exact:true}).click();await open();await page.keyboard.press('Escape');assert.equal(await submenu.count(),0);assert.equal(await root.count(),1);await page.keyboard.press('Escape');assert.equal(await root.count(),0);
  await page.close();
 }
 const selected=await browser.newPage({viewport:{width:1280,height:850}});await selected.goto(`${base}/?ui-preview=1`);await selected.locator('.session-item').filter({hasText:'需求拆解'}).click({position:{x:35,y:12}});await selected.locator('.session-item.active').waitFor();
 await selected.getByRole('button',{name:'更多工具',exact:true}).click();await selected.getByRole('menuitem',{name:'帮助',exact:true}).click();await selected.keyboard.press('Escape');await selected.keyboard.press('Escape');assert.match(await selected.locator('.session-item.active').innerText(),/需求拆解/,'Menu Escape preserves current session');await selected.close();
 assert.deepEqual(errors,[]);console.log('PASS: real App help menu, keyboard/viewport/focus, offline policy, runtime version, links, request dedup/staleness, 404/retry/current/ahead/update/invalid-source states and dark/light UI');
}finally{await browser.close();}
