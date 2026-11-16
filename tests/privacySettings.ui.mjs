// Production SettingsDialog/PrivacySettingsPanel with explicit fixture IPC; no real directories or services.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1433',out='.scratch/privacy-settings-20261116/ui';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[];
try{
 for(const theme of ['dark','light'])for(const size of [{width:1515,height:1038},{width:820,height:540},{width:687,height:903}]){
  const page=await browser.newPage({viewport:size});page.on('pageerror',e=>errors.push(e.message));page.setDefaultTimeout(10000);
  await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=privacy&ui-preview=1&section=privacy&open=1&theme=${theme}`);
  const settings=page.locator('.settings-dialog'),privacy=page.getByRole('region',{name:'隐私设置',exact:true});await privacy.waitFor();
  assert.deepEqual(await settings.locator('.settings-nav-group').last().getByRole('button').allTextContents(),['归档会话','隐私']);
  assert.equal(await settings.getByRole('heading',{name:'隐私',exact:true}).count(),1);assert.equal(await privacy.locator('.privacy-data-row').count(),5);assert.equal(await privacy.getByRole('switch').count(),0);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true,'No root horizontal overflow');
  await page.evaluate(()=>document.activeElement?.blur());await page.screenshot({path:`${out}/${theme}-${size.width}.png`});if(size.width===1515)await settings.screenshot({path:`${out}/${theme}-dialog.png`});
  await privacy.getByRole('button',{name:'数据保存在哪里',exact:true}).click();const storage=page.getByRole('dialog',{name:'数据保存在哪里',exact:true});await storage.waitFor();assert.match(await storage.innerText(),/本机应用记录/);assert.match(await storage.innerText(),/不同记录与服务的删除范围不同/);await page.keyboard.press('Escape');assert.equal(await settings.isVisible(),true);
  await privacy.getByRole('button',{name:'Agent 与外部服务',exact:false}).click();const services=page.getByRole('dialog',{name:'Agent 与外部服务',exact:true});await services.waitFor();assert.match(await services.innerText(),/没有统一控制/);await services.getByRole('button',{name:'关闭',exact:true}).click();
  const location=privacy.getByRole('button',{name:'应用数据目录：查看位置',exact:true});await location.click();const directory=page.getByRole('dialog',{name:'应用数据目录',exact:true});await directory.getByText('C:/fixture/CrewVela',{exact:true}).waitFor();
  await page.evaluate(()=>window.__privacyFixture.copyFails=true);await directory.getByRole('button',{name:'复制路径',exact:true}).click();await directory.getByRole('alert').waitFor();assert.match(await directory.getByRole('alert').innerText(),/复制路径失败/);
  await page.evaluate(()=>window.__privacyFixture.copyFails=false);await directory.getByRole('button',{name:'复制路径',exact:true}).click();await directory.getByRole('status').filter({hasText:'已复制目录路径'}).waitFor();
  assert.equal(await page.evaluate(()=>window.__privacyFixture.calls.filter(c=>c.command==='plugin:clipboard-manager|write_text').at(-1).args.text),'C:/fixture/CrewVela');
  await page.evaluate(()=>window.__privacyFixture.openFails=true);await directory.getByRole('button',{name:'打开目录',exact:true}).click();await directory.getByRole('alert').waitFor();assert.match(await directory.getByRole('alert').innerText(),/打开目录失败/);
  await page.evaluate(()=>window.__privacyFixture.openFails=false);await directory.getByRole('button',{name:'打开目录',exact:true}).click();await directory.getByRole('status').filter({hasText:'已请求在文件管理器'}).waitFor();
  assert.deepEqual(await page.evaluate(()=>window.__privacyFixture.calls.filter(c=>c.command==='workspace_open_path').at(-1).args),{root:'C:/fixture/CrewVela',relativePath:'.'});
  await directory.screenshot({path:`${out}/location-${theme}-${size.width}.png`});await page.keyboard.press('Escape');assert.equal(await location.evaluate(e=>e===document.activeElement),true);assert.equal(await settings.isVisible(),true);
  await page.evaluate(()=>window.__privacyFixture.readFails=true);await location.click();await page.getByRole('dialog',{name:'应用数据目录',exact:true}).getByRole('alert').waitFor();assert.equal(await page.getByRole('button',{name:'打开目录',exact:true}).count(),0,'No guessed folder on read failure');
  await page.evaluate(()=>window.__privacyFixture.readFails=false);await page.getByRole('button',{name:'重新读取目录',exact:true}).click();await page.getByText('C:/fixture/CrewVela',{exact:true}).waitFor();await page.keyboard.press('Escape');
  assert.equal(await settings.isVisible(),true,'Read retry Escape closes only the directory dialog');
  const search=settings.locator('.settings-search input');
  // Existing compact navigation intentionally hides search at <=720px; exercise it where available.
  assert.equal(await search.isVisible(),size.width>720);if(size.width>720)await search.fill('隐私');
  await privacy.getByRole('button',{name:'会话与归档：管理',exact:true}).click();await settings.getByRole('heading',{name:'归档会话',exact:true}).waitFor();assert.equal(await search.inputValue(),'','Navigation shows the selected destination');assert.equal(await settings.getByRole('button',{name:'归档会话',exact:true}).evaluate(e=>e===document.activeElement),true,'Keyboard focus follows the selected destination');
  await settings.getByRole('button',{name:'隐私',exact:true}).click();await privacy.getByRole('button',{name:'协作记录与诊断：查看',exact:true}).click();await settings.getByRole('tab',{name:'协作记录',exact:true}).waitFor();assert.equal(await settings.getByRole('tab',{name:'协作记录',exact:true}).getAttribute('aria-selected'),'true');
  await settings.getByRole('button',{name:'隐私',exact:true}).click();await privacy.getByRole('button',{name:'共享记忆：管理',exact:true}).click();await settings.getByRole('heading',{name:'共享记忆',exact:true}).waitFor();
  await settings.getByRole('button',{name:'隐私',exact:true}).click();await privacy.getByRole('button',{name:'浏览器访问：设置',exact:true}).click();await settings.getByRole('heading',{name:'浏览器控制',exact:true}).waitFor();
  await settings.getByRole('button',{name:'隐私',exact:true}).click();await settings.getByRole('button',{name:'协作中心',exact:true}).click();assert.equal(await settings.getByRole('tab',{name:'概览',exact:true}).getAttribute('aria-selected'),'true','Direct collaboration entry keeps original default');
  assert.equal(await page.evaluate(()=>window.__privacyFixture.calls.some(c=>/delete|deploy|install|serve|stop|save/.test(c.command))),false,'Privacy does not mutate data or launch services');await page.close();
 }
 const late=await browser.newPage();await late.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=privacy&ui-preview=1&section=privacy&open=1`);await late.locator('.privacy-settings').waitFor();await late.evaluate(()=>window.__privacyFixture.deferRead=true);
 const entry=late.getByRole('button',{name:'应用数据目录：查看位置',exact:true});await entry.click();await late.waitForFunction(()=>window.__privacyFixture.pending.length===1);await late.keyboard.press('Escape');await entry.click();await late.waitForFunction(()=>window.__privacyFixture.pending.length===2);
 await late.evaluate(()=>window.__privacyFixture.finish('C:/fixture/old'));assert.equal(await late.getByRole('dialog',{name:'应用数据目录',exact:true}).getByText('C:/fixture/old',{exact:true}).count(),0);
 await late.evaluate(()=>window.__privacyFixture.finish('C:/fixture/CrewVela'));await late.getByText('C:/fixture/CrewVela',{exact:true}).waitFor();await late.close();
 assert.deepEqual(errors,[]);console.log('PASS: privacy design/navigation, dark/light responsive UI, policy excerpts, real directory API target, read/copy/open failures and retries, modal focus, late-response isolation and no data mutation');
}finally{await browser.close();}
