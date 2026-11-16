// Real component and local IPC fixture only: never calls an Agent or edits user data.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const dir = '.scratch/conversation-clear-20261116/ui'; mkdirSync(dir, {recursive:true});
const browser = await chromium.launch({channel:'chrome'});
const errors = [];
async function fresh(query='', viewport={width:780,height:1000}) {
  const page = await browser.newPage({viewport}); page.setDefaultTimeout(10000);
  page.on('pageerror', error=>errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?${query}`);
  await page.locator('.conversation-members-trigger:not(:disabled)').waitFor();
  await page.waitForFunction(()=>document.querySelector('.conversation-transcript')?.textContent.includes('拆解方案') || new URLSearchParams(location.search).get('empty')==='1');
  return page;
}
const trigger = page=>page.getByRole('button',{name:'讨论记录操作',exact:true});
const menu = page=>page.getByRole('menu',{name:'讨论记录操作',exact:true});
const item = page=>page.getByRole('menuitem',{name:'清空讨论记录',exact:true});
const dialog = page=>page.getByRole('dialog',{name:'清空讨论记录',exact:true});
async function openConfirm(page) { await trigger(page).click(); await item(page).click(); await dialog(page).waitFor(); }
async function refresh(page) { await page.waitForFunction(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_snapshot').length>1); }
try {
  for (const theme of ['dark','light']) for (const width of [780,320]) {
    const page=await fresh(`theme=${theme}`,{width,height:860});
    await trigger(page).focus(); await page.keyboard.press('Enter'); await menu(page).waitFor();
    await page.waitForFunction(()=>document.activeElement?.getAttribute('role')==='menuitem');
    const bounds = await page.evaluate(()=>{
      const t=document.querySelector('[aria-label="讨论记录操作"]').getBoundingClientRect(), tabs=document.querySelector('.conversation-modes').getBoundingClientRect(), m=document.querySelector('.conversation-history-menu').getBoundingClientRect();
      const css=getComputedStyle(document.querySelector('.conversation-history-menu button'));
      return {right:t.left>=tabs.right-1,sameRow:Math.abs(t.top-tabs.top)<40,left:m.left,rightEdge:m.right,bottom:m.bottom,overflow:document.documentElement.scrollWidth>innerWidth,ordinary:css.color===getComputedStyle(document.querySelector('.collaboration-conversation')).color};
    });
    assert.equal(bounds.right,true); assert.equal(bounds.sameRow,true); assert.equal(bounds.overflow,false); assert.equal(bounds.ordinary,true); assert.ok(bounds.left>=0 && bounds.rightEdge<=width && bounds.bottom<860);
    assert.equal(await item(page).locator('svg').count(),1);
    await page.screenshot({path:`${dir}/${theme}-${width}-menu.png`});
    await page.keyboard.press('Escape'); assert.equal(await menu(page).count(),0); assert.equal(await trigger(page).evaluate(el=>el===document.activeElement),true);
    await trigger(page).click(); await page.locator('.conversation-heading').click({position:{x:1,y:1}}); assert.equal(await menu(page).count(),0);
    await page.getByRole('combobox',{name:'给团队发消息',exact:true}).fill('保留未发送的草稿');
    await page.locator('.conversation-members-trigger').click(); await page.getByRole('checkbox',{name:'Claude Code',exact:true}).uncheck(); await page.keyboard.press('Escape');
    const selected=await page.locator('.conversation-target').innerText();
    const before=await page.evaluate(()=>JSON.stringify(window.conversationMock.snapshot('A')));
    await openConfirm(page); assert.equal(await page.getByRole('button',{name:'取消',exact:true}).evaluate(el=>el===document.activeElement),true);
    await page.screenshot({path:`${dir}/${theme}-${width}-confirm.png`});
    await page.keyboard.press('Escape'); assert.equal(await dialog(page).count(),0); assert.equal(await page.locator('.conversation-message').count(),6);
    assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_clear').length),0);
    await openConfirm(page); await page.getByRole('button',{name:'清空记录',exact:true}).click(); await dialog(page).waitFor({state:'hidden'});
    assert.equal(await page.locator('.conversation-message').count(),0); assert.equal(await page.locator('.conversation-plan').count(),0);
    assert.equal(await page.getByRole('combobox',{name:'给团队发消息',exact:true}).inputValue(),'保留未发送的草稿');
    assert.equal(await page.locator('.conversation-target').innerText(),selected);
    assert.equal(await page.evaluate(()=>JSON.stringify(window.conversationMock.snapshot('A'))),before);
    const sent=await page.evaluate(()=>window.conversationMock.calls.find(c=>c.command==='ma_conversation_clear').args.input);
    assert.equal(sent.projectId,'A'); assert.equal(sent.expectedConversationId,'conversation-A'); assert.equal(sent.expectedMessageIds.length,7);
    await refresh(page); assert.equal(await page.locator('.conversation-message').count(),0);
    await trigger(page).click(); assert.equal(await item(page).isDisabled(),true); await page.keyboard.press('Escape');
    await page.reload(); await page.locator('.conversation-members-trigger:not(:disabled)').waitFor();
    // Fixture resets at navigation; backend persistence is tested against real SQLite.
    await page.close();
  }
  const running=await fresh('running=1'); await trigger(running).click(); assert.equal(await item(running).isDisabled(),true); await running.close();
  const empty=await fresh('empty=1'); await trigger(empty).click(); assert.equal(await item(empty).isDisabled(),true); await empty.close();
  const failed=await fresh(); await failed.evaluate(()=>{window.conversationMock.failClear=true;}); await openConfirm(failed); await failed.getByRole('button',{name:'清空记录',exact:true}).click();
  await dialog(failed).getByRole('alert').waitFor(); assert.match(await dialog(failed).innerText(),/清空失败/); assert.equal(await failed.locator('.conversation-message').count(),6);
  await failed.evaluate(()=>{window.conversationMock.failClear=false;}); await failed.getByRole('button',{name:'清空记录',exact:true}).click(); await dialog(failed).waitFor({state:'hidden'}); await failed.close();
  const pending=await fresh('fail=send-before-save'); await pending.getByRole('combobox',{name:'给团队发消息',exact:true}).fill('结果未确认'); await pending.getByRole('combobox',{name:'给团队发消息',exact:true}).press('Enter'); await pending.getByText('核对并重试',{exact:true}).waitFor(); await trigger(pending).click(); assert.equal(await item(pending).isDisabled(),true); await pending.close();
  const changed=await fresh(); await openConfirm(changed); await changed.evaluate(()=>{window.conversationMock.chats.A.messages.push({...window.conversationMock.chats.A.messages[0],id:'new-after-confirm'});});
  await dialog(changed).getByRole('alert').waitFor(); assert.match(await dialog(changed).innerText(),/记录已变化/); assert.equal(await changed.getByRole('button',{name:'清空记录',exact:true}).isDisabled(),true); await changed.close();
  const starts=await fresh(); await openConfirm(starts); await starts.evaluate(()=>{window.conversationMock.chats.A.rounds[0].status='running';});
  await dialog(starts).getByRole('alert').waitFor(); assert.equal(await starts.getByRole('button',{name:'清空记录',exact:true}).isDisabled(),true); await starts.close();
  const late=await fresh(); await late.evaluate(()=>{window.conversationMock.holdClear=true;}); await openConfirm(late); await late.getByRole('button',{name:'清空记录',exact:true}).click();
  assert.equal(await late.getByRole('button',{name:'正在清空…',exact:true}).isDisabled(),true);
  await late.keyboard.press('Escape'); assert.equal(await dialog(late).count(),1);
  await late.getByRole('combobox',{name:'协作项目',exact:true}).evaluate(el=>{el.value='B';el.dispatchEvent(new Event('change',{bubbles:true}));});
  await late.waitForFunction(()=>document.querySelector('[aria-label="协作项目"]')?.value==='B');
  await late.evaluate(()=>window.conversationMock.release('ma_conversation_clear',{...window.conversationMock.chats.A,messages:[],rounds:[],executionPlans:[]}));
  await late.waitForFunction(()=>!document.querySelector('[aria-label="讨论记录操作"]')?.disabled);
  await late.waitForFunction(()=>document.querySelector('.conversation-members-trigger')?.textContent.includes('成员 1'));
  assert.equal(await late.getByRole('combobox',{name:'协作项目',exact:true}).inputValue(),'B'); assert.equal(await dialog(late).count(),0); assert.equal(await late.locator('.conversation-message').count(),0);
  assert.equal(await late.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_clear').length),1); await late.close();
  const hidden=await fresh(); await openConfirm(hidden); await hidden.evaluate(()=>window.conversationMock.setActive(false)); await dialog(hidden).waitFor({state:'hidden'}); await hidden.close();
  assert.deepEqual(errors,[]);
  console.log('PASS conversation clear UI: menu/confirmation, dark/light, 320/780px, draft/member/task preservation, polling, empty/active/pending guards, retry, stale confirmation/project response, keyboard and deactivation.');
} finally { await browser.close(); }
