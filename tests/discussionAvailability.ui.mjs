// Real React + isolated IPC, no model calls or edits to user projects.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const browser = await chromium.launch({channel:'chrome'});
try {
  for (const theme of ['dark','light']) {
    const context = await browser.newContext({viewport:{width:780,height:1100}});
    const page = await context.newPage();
    const errors=[]; page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&unsupported=1&manual=codex&theme=${theme}`);
    await page.getByRole('button',{name:'选择参与成员，已选 1 位，共 3 位'}).waitFor();
    await page.getByRole('button',{name:'选择参与成员，已选 1 位，共 3 位'}).click();
    await page.getByRole('checkbox',{name:'Codex',exact:true}).waitFor();
    assert.equal(await page.getByRole('checkbox',{name:'Codex',exact:true}).isEnabled(),false);
    assert.match(await page.locator('.conversation-member-menu').innerText(),/用户接管/);
    await page.keyboard.press('Escape');
    await page.getByRole('tab',{name:'聊天',exact:true}).click();
    const editor=page.getByRole('combobox',{name:'给团队发消息',exact:true});
    await editor.fill('@Codex hi');
    await page.getByRole('button',{name:'发送消息',exact:true}).click();
    await page.getByRole('alert').filter({hasText:/Codex.*用户接管/}).waitFor();
    assert.equal(await editor.inputValue(),'@Codex hi');
    assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_send').length),0);
    assert.equal(await page.evaluate(()=>localStorage.getItem('crewvela-conversation-pending:A')),null);
    mkdirSync('.scratch/discussion-runtime-diagnosis-20261116',{recursive:true});
    await page.screenshot({path:`.scratch/discussion-runtime-diagnosis-20261116/manual-${theme}.png`});
    await editor.fill('hi');
    await page.getByRole('button',{name:'发送消息',exact:true}).click();
    await page.waitForFunction(()=>window.conversationMock.calls.some(c=>c.command==='ma_conversation_send'));
    assert.deepEqual(await page.evaluate(()=>window.conversationMock.calls.find(c=>c.command==='ma_conversation_send').args.input.participantAgentIds),['claude']);
    assert.deepEqual(errors,[]);
    await context.close();
  }
  console.log('PASS: manual member excluded from defaults, explained in menu, explicit mention blocked before persistence, normal member sends, dark/light');
} finally { await browser.close(); }
