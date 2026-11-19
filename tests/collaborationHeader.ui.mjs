// Real components with isolated IPC; no Agent or project is modified.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1433';
const dir = process.env.CREWVELA_HEADER_SCREENSHOTS || '.scratch/collaboration-header-20261116/ui';
mkdirSync(dir, {recursive:true});
const browser = await chromium.launch({channel:'chrome'});
const errors = [];
async function fresh(query='', viewport={width:1015,height:1402}) {
  const page = await browser.newPage({viewport}); page.setDefaultTimeout(10000);
  page.on('pageerror', error=>errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&${query}`);
  await page.locator('.conversation-members-trigger:not(:disabled)').waitFor();
  await page.waitForFunction(()=>document.querySelector('.conversation-modes [aria-selected="true"]'));
  return page;
}
async function geometry(page) {
  return page.evaluate(()=>{
    const menu = document.querySelector('.conversation-member-menu');
    const bounds = menu?.getBoundingClientRect();
    const header = document.querySelector('.conversation-header');
    const select = header.querySelector('select'), css=getComputedStyle(select), selectRect=select.getBoundingClientRect();
    const caret=header.querySelector('.conversation-project-select > svg').getBoundingClientRect();
    const visible = element=>!!element && getComputedStyle(element).display!=='none' && element.getBoundingClientRect().height>0;
    return {overflow:document.body.scrollWidth>innerWidth,headerHeight:header.getBoundingClientRect().height,headerOverflow:header.scrollWidth>header.clientWidth,project:{font:parseFloat(css.fontSize),weight:parseInt(css.fontWeight),height:selectRect.height,width:selectRect.width,border:css.borderTopWidth,background:css.backgroundColor,ellipsis:css.textOverflow,caret:caret.width},menu:bounds?{left:bounds.left,right:bounds.right,top:bounds.top,bottom:bounds.bottom,scroll:menu.scrollHeight>menu.clientHeight}:null,images:[...document.querySelectorAll('img')].filter(visible).every(img=>img.complete&&img.naturalWidth>0)};
  });
}
try {
  for (const theme of ['dark','light']) for (const size of [{width:1015,height:1402},{width:780,height:1000},{width:390,height:844},{width:300,height:420}]) {
    const page=await fresh(`header=1&theme=${theme}`,size), trigger=page.locator('.conversation-members-trigger');
    const before=await geometry(page); assert.equal(before.overflow,false);assert.equal(before.headerOverflow,false);assert.ok(before.headerHeight<195,'Header stays below the old 205px wide layout while retaining the selected design typography');assert.equal(before.images,true);
    assert.ok(before.project.font>=17&&before.project.font<=22,'Project heading follows the selected prototype across breakpoints');assert.ok(before.project.weight>=600);assert.ok(before.project.height===34);assert.ok(before.project.width<=321);assert.equal(before.project.border,'0px');assert.equal(before.project.background,'rgba(0, 0, 0, 0)');assert.equal(before.project.caret,12);
    assert.equal(await page.getByRole('combobox',{name:'协作项目',exact:true}).getAttribute('title'),'模型设置实测-20261115\n项目目录：/fixture/crewvela-model-live-e79907a4ca');
    assert.equal(await page.locator('.conversation-member-menu').count(),0);
    await trigger.click();const menu=page.locator('.conversation-member-menu');await menu.waitFor();
    assert.equal(await menu.getByRole('checkbox').count(),3);
    await menu.getByRole('checkbox',{name:'聊天-ZCodeCLI',exact:true}).uncheck();
    assert.equal(await page.locator('.conversation-target').innerText(),'分工与托管 · 已选 2 位');
    assert.equal(await page.locator('.conversation-send-scope').innerText(),'发给全体 · 3 位');
    assert.match(await trigger.getAttribute('aria-label'),/已选 2 位，共 3 位/);
    let rect=(await geometry(page)).menu;assert.ok(rect.left>=0&&rect.right<=size.width+1&&rect.top>=0&&rect.bottom<=size.height+1);
    await page.mouse.move(8,70);
    await page.screenshot({path:`${dir}/${theme}-${size.width}-open.png`});
    await page.keyboard.press('Escape');assert.equal(await menu.count(),0);assert.equal(await trigger.evaluate(el=>el===document.activeElement),true);
    await trigger.click();await page.locator('.conversation-header').click({position:{x:2,y:2}});assert.equal(await menu.count(),0);
    await page.getByRole('tab',{name:'任务执行',exact:true}).click();
    await page.getByRole('tab',{name:'团队聊天',exact:true}).click();
    await page.locator('.conversation-target').click();
    const bottom=page.locator('.conversation-recipient-menu');assert.equal(await bottom.locator('input:checked').count(),2);
    await bottom.getByRole('checkbox').nth(2).check();await page.keyboard.press('Escape');
    assert.equal(await page.locator('.conversation-target').evaluate(el=>el===document.activeElement),true);
    await trigger.click();assert.equal(await menu.locator('input:checked').count(),3);
    await page.locator('.conversation-target').focus();assert.equal(await menu.count(),0);
    await page.locator('.conversation-target').click();assert.equal(await bottom.isVisible(),true);
    await page.keyboard.press('Escape');
    const project=page.getByRole('combobox',{name:'协作项目',exact:true});await trigger.click();await project.selectOption('B');
    await page.waitForFunction(()=>document.querySelector('.conversation-members-trigger')?.textContent.includes('成员 1'));
    assert.equal(await menu.count(),0);await trigger.click();assert.equal(await menu.getByRole('checkbox',{name:'文档 Codex',exact:true}).count(),1);await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>window.conversationMock.calls.some(item=>/send|interrupt|control|execute_plan|create|pause/.test(item.command))),false,'UI choices do not dispatch work');
    await page.close();
  }
  const long=await fresh('members=many&long=1',{width:390,height:540});await long.locator('.conversation-members-trigger').click();
  const longName=await long.evaluate(()=>window.conversationMock.projects[0].name);assert.ok((await long.getByLabel('协作项目',{exact:true}).getAttribute('title')).startsWith(longName));assert.equal((await geometry(long)).project.ellipsis,'ellipsis');
  assert.equal((await geometry(long)).headerOverflow,false);assert.equal((await geometry(long)).overflow,false);
  assert.equal((await geometry(long)).menu.scroll,true);await long.getByRole('checkbox',{name:'开发成员-23：很长的成员名称用于检查省略与布局',exact:true}).uncheck();
  await long.screenshot({path:`${dir}/long-members.png`});
  await long.setViewportSize({width:300,height:420});await long.waitForTimeout(80);const resized=(await geometry(long)).menu;assert.ok(resized.left>=0&&resized.right<=301&&resized.bottom<=421);
  await long.keyboard.press('Escape');await long.close();
  const native=await fresh();const nativeProject=native.getByRole('combobox',{name:'协作项目',exact:true});await nativeProject.focus();await nativeProject.press('End');await native.waitForFunction(()=>document.querySelector('.conversation-heading select')?.value==='B');await nativeProject.press('Home');await native.waitForFunction(()=>document.querySelector('.conversation-heading select')?.value==='A');await native.close();
  const none=await fresh('members=none');await none.locator('.conversation-members-trigger').click();await none.locator('.conversation-member-menu').getByText('还没有团队成员。',{exact:false}).waitFor();await none.getByRole('button',{name:'添加成员',exact:true}).click();
  assert.equal(await none.locator('.conversation-member-menu').count(),0);assert.equal(await none.evaluate(()=>window.conversationMock.calls.filter(item=>item.command==='fixture_open_settings').length),1);await none.close();
  const busy=await fresh();await busy.evaluate(()=>window.conversationMock.holdSend=true);await busy.getByRole('combobox',{name:'给团队发消息',exact:true}).fill('测试成员界面');await busy.getByRole('button',{name:'发送消息',exact:true}).click();
  await busy.waitForFunction(()=>window.conversationMock.pending.some(item=>item.command==='ma_conversation_send'));await busy.locator('.conversation-members-trigger').click();assert.equal(await busy.locator('.conversation-member-menu input:disabled').count(),3);await busy.keyboard.press('Escape');await busy.close();
  const navigation=await fresh();await navigation.getByRole('button',{name:'管理项目与成员',exact:true}).click();
  assert.equal(await navigation.evaluate(()=>window.conversationMock.calls.find(item=>item.command==='fixture_open_settings').args.projectId),'A');
  await navigation.getByRole('button',{name:'创建协作项目',exact:true}).click();await navigation.getByRole('dialog',{name:'创建协作项目',exact:true}).waitFor();await navigation.keyboard.press('Escape');
  await navigation.getByRole('button',{name:'查看任务 · 3',exact:true}).click();await navigation.getByRole('dialog',{name:'设置',exact:true}).waitFor();await navigation.getByRole('button',{name:'打开工作台',exact:true}).click();
  assert.equal(await navigation.evaluate(()=>window.conversationMock.calls.find(item=>item.command==='plugin:event|emit'&&item.args.event==='multi-agent-workspace-open').args.payload.projectId),'A');await navigation.close();
  const keyboard=await fresh(), keyboardTrigger=keyboard.locator('.conversation-members-trigger');await keyboardTrigger.focus();await keyboardTrigger.press('Enter');
  await keyboard.waitForFunction(()=>document.activeElement?.matches('.conversation-member-menu input'));
  await keyboard.keyboard.press('Space');await keyboard.keyboard.press('Tab');assert.equal(await keyboard.locator('.conversation-member-menu input').nth(1).evaluate(el=>el===document.activeElement),true);
  await keyboard.keyboard.press('Space');await keyboard.keyboard.press('Escape');assert.equal(await keyboardTrigger.evaluate(el=>el===document.activeElement),true);
  assert.match(await keyboardTrigger.getAttribute('aria-label'),/已选 1 位，共 3 位/);await keyboard.getByRole('combobox',{name:'给团队发消息',exact:true}).fill('检查接口');await keyboard.getByRole('button',{name:'发送消息',exact:true}).click();
  await keyboard.getByRole('combobox',{name:'给团队插话',exact:true}).waitFor();
  assert.deepEqual(await keyboard.evaluate(()=>window.conversationMock.calls.find(item=>item.command==='ma_conversation_send').args.input.participantAgentIds),['claude','codex','kimi']);
  await keyboard.getByRole('button',{name:'讨论记录操作',exact:true}).click();
  await keyboard.getByRole('menuitem',{name:'暂停讨论',exact:true}).waitFor();
  await keyboard.close();
  const instances=await fresh('instances=2');await instances.locator('[data-instance="0"] .conversation-members-trigger').click();await instances.evaluate(()=>window.conversationMock.selectInstance(1));
  await instances.waitForFunction(()=>!document.querySelector('.conversation-member-menu'));await instances.locator('[data-instance="1"] .conversation-members-trigger').click();assert.equal(await instances.locator('.conversation-member-menu').count(),1);
  await instances.evaluate(()=>window.conversationMock.setActive(false));await instances.locator('.conversation-member-menu').waitFor({state:'detached'});await instances.close();
  for(const theme of ['dark','light']) {
    const logos=await fresh(`theme=${theme}`);await logos.evaluate(()=>{
      const team=window.conversationMock.snapshot('A').agents;
      for(const runtimeKind of ['pi','opencode']) team.push({...team[0],id:runtimeKind,name:runtimeKind,runtimeKind,avatarId:null});
      window.conversationMock.chats.A.supportedDiscussionRuntimes.push('pi','opencode');
    });
    await logos.waitForFunction(()=>document.querySelector('.conversation-members-trigger')?.textContent.includes('成员 5'));await logos.locator('.conversation-members-trigger').click();
    const actual=await logos.locator('.conversation-member-menu img[data-runtime="pi"]').evaluate(img=>({loaded:img.complete&&img.naturalWidth>0,filter:getComputedStyle(img).filter}));
    assert.equal(actual.loaded,true);assert.equal(actual.filter,theme==='dark'?'invert(1)':'none');await logos.close();
  }
  assert.deepEqual(errors,[]);console.log('PASS: compact header, shared recipient selection, exact project isolation, modal dismissal/focus, busy guards, empty/long teams, responsive dark/light and isolated instances');
} finally { await browser.close(); }
