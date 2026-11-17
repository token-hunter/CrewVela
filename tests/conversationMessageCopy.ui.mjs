// Uses the production component and native clipboard IPC contract, without writing the user's clipboard.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1444';
mkdirSync('.scratch/conversation-message-copy',{recursive:true});
const browser=await chromium.launch({channel:'chrome'});
const errors=[];
try {
 for(const theme of ['dark','light']) {
  const page=await browser.newPage({viewport:{width:1000,height:1000}});page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?copy=1&theme=${theme}`);
  const user=page.locator('[data-message-id="m1"]'),agent=page.locator('[data-message-id="m2"]'),empty=page.locator('[data-message-id="m3"]');
  const copy=user.getByRole('button',{name:'复制消息',exact:true});
  await copy.waitFor();assert.equal(await empty.getByRole('button',{name:'复制消息'}).count(),0);
  await copy.hover();await user.getByRole('tooltip',{name:'复制消息',exact:true}).waitFor();
  await page.screenshot({path:`.scratch/conversation-message-copy/${theme}.png`});
  await copy.click();await user.locator('.conversation-copy-feedback').getByText('已复制',{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.conversationMock.clipboardWrites.at(-1)),await page.evaluate(()=>window.conversationMock.chats.A.messages.find(m=>m.id==='m1').body));
  // Keyboard action copies raw Markdown, code fences, Unicode and original newlines.
  const agentCopy=agent.getByRole('button',{name:'复制消息'});await agentCopy.focus();await page.keyboard.press('Enter');
  await agent.locator('.conversation-copy-feedback').getByText('已复制',{exact:true}).waitFor();
  const last=await page.evaluate(()=>window.conversationMock.clipboardWrites.at(-1));
  assert.equal(last,await page.evaluate(()=>window.conversationMock.chats.A.messages.find(m=>m.id==='m2').body));
  assert.ok(last.includes('```ts\nconst answer = 42;\n```'));assert.ok(!last.includes('已完成'));assert.ok(!last.includes('Claude Code'));
  await page.waitForFunction(()=>!document.querySelector('[data-message-id="m2"] .conversation-copy-feedback').textContent);
  // Rejection stays visible and does not claim clipboard success; retry succeeds.
  await page.evaluate(()=>window.conversationMock.copyError='测试：剪贴板写入被拒绝');
  const before=await page.evaluate(()=>window.conversationMock.clipboardWrites.length);
  await agentCopy.click();await agent.getByRole('alert').getByText(/剪贴板写入被拒绝/).waitFor();
  assert.equal(await agent.locator('.conversation-copy-feedback').textContent(),'');
  assert.equal(await page.evaluate(()=>window.conversationMock.clipboardWrites.length),before);
  await page.evaluate(()=>window.conversationMock.copyError='');await agentCopy.click();
  await agent.locator('.conversation-copy-feedback').getByText('已复制',{exact:true}).waitFor();assert.equal(await agent.getByRole('alert').count(),0);
  // Pending write: disable duplicate clicks; a changing streamed body must not inherit stale success.
  await page.evaluate(()=>window.conversationMock.holdCopy=true);await agentCopy.click();
  assert.equal(await agentCopy.isDisabled(),true);
  await page.evaluate(()=>{const m=window.conversationMock.chats.A.messages.find(m=>m.id==='m2');m.body='更新后的回复：第二个版本';m.updatedAt=Date.now();});
  await agent.getByText('更新后的回复：第二个版本',{exact:true}).waitFor();
  await page.evaluate(()=>{window.conversationMock.holdCopy=false;window.conversationMock.pendingCopies.splice(0).forEach(resolve=>resolve());});
  await page.waitForFunction(()=>!document.querySelector('[data-message-id="m2"] .conversation-copy-button').disabled);
  assert.equal(await agent.locator('.conversation-copy-feedback').textContent(),'');
  assert.equal(await page.evaluate(()=>window.conversationMock.clipboardWrites.at(-1)),last);
  await agentCopy.click();await agent.locator('.conversation-copy-feedback').getByText('已复制',{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.conversationMock.clipboardWrites.at(-1)),'更新后的回复：第二个版本');
  await page.setViewportSize({width:390,height:844});await user.getByRole('button',{name:'复制消息'}).scrollIntoViewIfNeeded();
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  const button=await user.getByRole('button',{name:'复制消息'}).boundingBox();assert.ok(button.x>=0&&button.x+button.width<=390);
  if(theme==='dark')await page.screenshot({path:'.scratch/conversation-message-copy/narrow.png'});
  // Changing projects while a write is pending must not show old feedback in the new chat.
  await page.evaluate(()=>window.conversationMock.holdCopy=true);await agentCopy.click();
  await page.getByRole('combobox',{name:'协作项目',exact:true}).selectOption('B');
  await page.waitForFunction(()=>!document.querySelector('[data-message-id="m2"]'));
  await page.evaluate(()=>window.conversationMock.pendingCopies.splice(0).forEach(resolve=>resolve()));
  assert.equal(await page.locator('.conversation-copy-feedback').filter({hasText:'已复制'}).count(),0);
  await page.close();
 }
 const plans=await browser.newPage();await plans.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html`);
 await plans.locator('.team-plan-card').waitFor();assert.equal(await plans.locator('[data-message-id="m3"] .conversation-copy-button').count(),0);
 await plans.close();
 assert.deepEqual(errors,[]);
 console.log('PASS message copy: user/agent exact body, native IPC, hover/keyboard, success reset, rejection/retry, streaming snapshot, project isolation, empty/plan exclusion, dark/light/narrow');
} finally {await browser.close();}
