// Actual guide, isolated project/clipboard IPC. Never sends an Agent message.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base=process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const output='.scratch/communication-compact-20261116/ui'; mkdirSync(output,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}), errors=[];
async function fresh(query='',viewport={width:1280,height:960}) {
  const page=await browser.newPage({viewport});page.setDefaultTimeout(10000);
  page.on('pageerror',error=>errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/communication-guide-ui.html?${query}`);
  await page.getByRole('heading',{name:'会话通信',exact:true}).waitFor();return page;
}
const source=page=>page.getByRole('combobox',{name:'示例发送方'});
const target=page=>page.getByRole('combobox',{name:'示例接收方'});
const copy=page=>page.getByRole('button',{name:'复制示例',exact:true});
async function layout(page) {
  const shape=await page.locator('.communication-guide').evaluate(el=>{
    const rect=el.getBoundingClientRect(), footer=el.querySelector('footer').getBoundingClientRect();
    const active=el.querySelector('.communication-scopes [aria-pressed="true"]');
    return {overflow:el.scrollWidth>el.clientWidth,bodyOverflow:el.querySelector('.communication-guide-body').scrollWidth>el.querySelector('.communication-guide-body').clientWidth,right:rect.right,footerBottom:footer.bottom,height:innerHeight,accent:getComputedStyle(active,'::after').backgroundColor,copyColor:getComputedStyle(el.querySelector('.communication-copy')).backgroundColor};
  });
  assert.equal(shape.overflow,false);assert.equal(shape.bodyOverflow,false);
  assert(shape.right<=page.viewportSize().width+1);assert(shape.footerBottom<=shape.height+1);
  assert.equal(shape.accent,shape.copyColor);
}
try {
  for(const theme of ['dark','light'])for(const viewport of [{width:1280,height:960},{width:320,height:600}]) {
    const page=await fresh(`sample=1&theme=${theme}`,viewport), guide=page.locator('.communication-guide');
    assert.equal(await source(page).isVisible(),true);assert.equal(await target(page).isVisible(),true);
    assert.equal(await page.locator('.communication-help').evaluate(el=>el.open),false);
    await guide.screenshot({path:`${output}/sessions-${theme}-${viewport.width}.png`});
    await copy(page).click();await page.getByRole('status').filter({hasText:'尚未发送消息'}).waitFor();
    assert.match(await page.evaluate(()=>window.communicationMock.copied),/「OpenCode」/);
    const logo=await guide.locator('img[data-runtime="opencode"]').evaluate(img=>({loaded:img.complete&&img.naturalWidth>0,filter:getComputedStyle(img).filter}));
    assert(logo.loaded);assert.equal(logo.filter,theme==='dark'?'invert(1)':'none');
    await layout(page);
    await page.getByRole('button',{name:'项目成员',exact:true}).click();
    assert.equal(await copy(page).isDisabled(),true);assert.equal(await page.getByRole('button',{name:'打开通信诊断'}).isDisabled(),true);
    await page.getByRole('combobox',{name:'通信示例项目'}).selectOption('a');
    await page.waitForFunction(()=>document.querySelector('select[aria-label="示例发送方"]')?.value==='a-claude');
    await guide.screenshot({path:`${output}/project-${theme}-${viewport.width}.png`});
    await copy(page).click();await page.getByRole('status').filter({hasText:'尚未发送消息'}).waitFor();
    assert.match(await page.evaluate(()=>window.communicationMock.copied),/本项目的「甲审查」/);
    await layout(page);
    await page.getByRole('button',{name:'打开协作中心',exact:true}).click();
    await page.getByRole('button',{name:'打开通信诊断',exact:true}).click();
    assert.deepEqual(await page.evaluate(()=>window.communicationMock.navigation),[{kind:'center',projectId:'a'},{kind:'diagnostics',projectId:'a',scope:'project'}]);
    await page.locator('.communication-help summary').click();assert.equal(await page.locator('.communication-help').evaluate(el=>el.open),true);
    await page.keyboard.press('Escape');await guide.waitFor({state:'detached'});await page.close();
  }
  const page=await fresh();
  await page.getByRole('button',{name:'关闭通信指南'}).focus();await page.keyboard.press('Tab');
  assert.equal(await page.getByRole('button',{name:'独立会话',exact:true}).evaluate(el=>el===document.activeElement),true);
  await source(page).selectOption('a-codex');assert.notEqual(await target(page).inputValue(),'a-codex');
  assert.equal(await target(page).locator('option[value="a-codex"]').count(),0);
  await page.evaluate(()=>window.communicationMock.setFailCopy(true));await copy(page).click();
  await page.getByRole('alert').filter({hasText:'模拟剪贴板拒绝'}).waitFor();
  await page.evaluate(()=>{window.communicationMock.setFailCopy(false);window.communicationMock.holdCopy(true);});
  const prior=await page.evaluate(()=>window.communicationMock.calls.filter(item=>item==='plugin:clipboard-manager|write_text').length);
  await copy(page).click();await page.waitForFunction(()=>document.querySelector('.communication-copy')?.disabled);
  await page.locator('.communication-copy').dispatchEvent('click');assert.equal(await page.evaluate(()=>window.communicationMock.calls.filter(item=>item==='plugin:clipboard-manager|write_text').length),prior+1);
  await target(page).selectOption('a-pi');
  await page.evaluate(()=>window.communicationMock.releaseCopy());await copy(page).waitFor();
  await page.waitForFunction(()=>!document.querySelector('.communication-copy')?.disabled);
  assert.equal(await page.locator('.communication-copy-state').count(),0);
  await page.evaluate(()=>window.communicationMock.holdCopy(false));await copy(page).click();
  await page.getByRole('status').filter({hasText:'尚未发送消息'}).waitFor();assert.match(await page.evaluate(()=>window.communicationMock.copied),/甲文档/);
  await page.getByRole('button',{name:'打开通信诊断'}).click();assert.deepEqual(await page.evaluate(()=>window.communicationMock.navigation.at(-1)),{kind:'diagnostics',projectId:undefined,scope:'sessions'});
  await page.evaluate(()=>window.communicationMock.holdA(true));await page.getByRole('button',{name:'项目成员',exact:true}).click();
  const project=page.getByRole('combobox',{name:'通信示例项目'});await project.selectOption('a');
  await page.waitForFunction(()=>window.communicationMock.calls.filter(item=>item==='ma_snapshot').length===1);
  await project.selectOption('b');await page.waitForFunction(()=>document.querySelector('select[aria-label="示例发送方"]')?.value==='b-claude');
  await page.evaluate(async()=>{window.communicationMock.releaseA();await new Promise(requestAnimationFrame);});
  assert.equal(await source(page).inputValue(),'b-claude');assert.equal(await source(page).locator('option[value="foreign"],option[value="disabled"],option[value="a-claude"]').count(),0);
  await copy(page).click();await page.getByRole('status').filter({hasText:'尚未发送消息'}).waitFor();assert.match(await page.evaluate(()=>window.communicationMock.copied),/乙审查/);
  assert.deepEqual(await page.evaluate(()=>window.communicationMock.calls.filter(command=>!['ma_list_projects','ma_snapshot','plugin:clipboard-manager|write_text'].includes(command))),[]);await page.close();
  for(const variant of ['none','one','long']) {
    const empty=await fresh(`sessions=${variant}`,{width:320,height:480});
    if(variant!=='long')assert.equal(await copy(empty).isDisabled(),true);
    await layout(empty);await empty.close();
  }
  const bad=await fresh('scope=project&project=missing');await bad.getByRole('alert').filter({hasText:'所选项目已不可用'}).waitFor();
  assert.equal(await copy(bad).isDisabled(),true);await bad.getByRole('button',{name:'打开协作中心'}).click();
  assert.deepEqual(await bad.evaluate(()=>window.communicationMock.navigation.at(-1)),{kind:'center',projectId:undefined});await bad.close();
  const retry=await fresh();await retry.evaluate(()=>window.communicationMock.setFailList(true));await retry.getByRole('button',{name:'项目成员',exact:true}).click();
  await retry.getByRole('alert').filter({hasText:'读取协作项目失败'}).waitFor();assert.equal(await copy(retry).isDisabled(),true);
  await retry.evaluate(()=>window.communicationMock.setFailList(false));await retry.getByRole('button',{name:'重新读取'}).click();
  await retry.waitForFunction(()=>document.querySelector('select[aria-label="通信示例项目"]')?.options.length===3);await retry.close();
  const app=await browser.newPage({viewport:{width:1440,height:960}});app.setDefaultTimeout(10000);app.on('pageerror',error=>errors.push(error.message));
  await app.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
  await app.getByRole('button',{name:'新建会话',exact:true}).first().waitFor();
  const shortcuts=app.getByRole('navigation',{name:'主导航',exact:true});
  assert.equal(await shortcuts.getByRole('button',{name:'会话通信',exact:true}).count(),0);
  await shortcuts.getByRole('button',{name:'Bot 模式',exact:true}).waitFor();
  await app.locator('.workspace-tabs').getByRole('button',{name:'会话通信',exact:true}).click();
  await app.getByRole('heading',{name:'会话通信',exact:true}).waitFor();
  await app.getByRole('button',{name:'关闭通信指南',exact:true}).click();
  await app.locator('.communication-guide').waitFor({state:'detached'});
  await app.evaluate(()=>window.dispatchEvent(new CustomEvent('crewvela-open-communication-guide',{detail:{scope:'project',projectId:'B'}})));
  await app.getByRole('button',{name:'打开协作中心',exact:true}).click();
  const settings=app.getByRole('dialog',{name:'设置',exact:true});
  await settings.getByRole('tab',{name:'概览',exact:true}).waitFor();
  await app.waitForFunction(()=>document.querySelector('.collab-project-picker select')?.value==='B');
  assert.equal(await settings.getByRole('tab',{name:'概览',exact:true}).getAttribute('aria-selected'),'true');
  assert.equal(await app.locator('.communication-guide').evaluate(el=>el.hidden),true);
  await settings.getByRole('button',{name:'关闭设置',exact:true}).click();
  await app.getByRole('button',{name:'打开通信诊断',exact:true}).click();
  await settings.getByRole('tab',{name:'协作诊断',exact:true}).waitFor();
  assert.equal(await settings.getByRole('tab',{name:'协作诊断',exact:true}).getAttribute('aria-selected'),'true');
  assert.equal(await settings.getByRole('combobox',{name:'当前项目',exact:true}).inputValue(),'B');await app.close();
  assert.deepEqual(errors,[]);
  console.log('PASS: compact scopes, sender/recipient selection, project isolation/stale replies, copy-only semantics, clipboard errors/retry/dedup, empty/invalid/long states, navigation, keyboard, dark/light and narrow layouts');
} finally {await browser.close();}
