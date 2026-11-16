import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const out = '.scratch/new-team-conversation-20261117';
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const errors = [];
const trigger = page => page.getByRole('button', { name: '讨论记录操作', exact: true });
const newItem = page => page.getByRole('menuitem', { name: '新建会话', exact: true });
const clearItem = page => page.getByRole('menuitem', { name: '清空讨论记录', exact: true });
const editor = page => page.locator('textarea');
async function fresh(query = '', width = 780) {
  const page = await browser.newPage({ viewport: { width, height: 1000 } });
  page.setDefaultTimeout(10000);
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?${query}`);
  await page.locator('.conversation-members-trigger:not(:disabled)').waitFor();
  return page;
}
async function create(page) { await trigger(page).click(); await newItem(page).click(); }
try {
  for (const theme of ['dark', 'light']) for (const width of [780, 320]) {
    const page = await fresh(`theme=${theme}`, width);
    await page.waitForFunction(() => document.querySelector('.conversation-transcript').textContent.includes('拆解方案'));
    const before = await page.evaluate(() => structuredClone(window.conversationMock.chats.A));
    await page.getByRole('tab', { name: '执行项目', exact: true }).click();
    await editor(page).fill('旧会话草稿');
    await trigger(page).click();
    assert.deepEqual(await page.getByRole('menuitem').allTextContents(), ['新建会话', '清空讨论记录']);
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menuitem' && document.activeElement?.textContent === '新建会话');
    await page.keyboard.press('ArrowDown');
    assert.equal(await clearItem(page).evaluate(el => el === document.activeElement), true);
    await page.keyboard.press('Home');
    assert.equal(await newItem(page).evaluate(el => el === document.activeElement), true);
    await page.screenshot({ path: `${out}/menu-${theme}-${width}.png`, fullPage: true });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await page.keyboard.press('Enter');
    await page.waitForFunction(old => window.conversationMock.chats.A.conversation.id !== old, before.conversation.id);
    await page.waitForFunction(() => document.querySelector('textarea').value === '');
    assert.equal(await page.getByRole('tab', { name: '聊天', exact: true }).getAttribute('aria-selected'), 'true');
    assert.equal(await page.locator('.conversation-message').count(), 0);
    const after = await page.evaluate(() => ({ chat: window.conversationMock.chats.A, archived: window.conversationMock.archivedConversations, calls: window.conversationMock.calls }));
    assert.deepEqual(after.archived[0], before, 'old messages and plans are retained');
    assert.equal(after.chat.rounds.length, 0);
    assert.equal(after.chat.executionPlans.length, 0);
    assert.equal(after.calls.filter(c => c.command === 'ma_conversation_clear').length, 0, 'new conversation has its own action');
    assert.equal(after.calls.filter(c => c.command === 'ma_conversation_new').length, 1);
    assert.equal(after.calls.find(c => c.command === 'ma_conversation_new').args.input.expectedConversationId, before.conversation.id);
    await trigger(page).click();
    assert.equal(await newItem(page).isEnabled(), true, 'empty conversation can still start a new one');
    assert.equal(await clearItem(page).isDisabled(), true);
    await page.keyboard.press('Escape');
    await page.screenshot({ path: `${out}/new-${theme}-${width}.png`, fullPage: true });
    await page.close();
  }
  const running = await fresh('running=1'); await trigger(running).click(); assert.equal(await newItem(running).isDisabled(), true); await running.close();
  const pending = await fresh('fail=send-before-save');
  await editor(pending).fill('未确认的发送'); await editor(pending).press('Enter');
  await pending.getByText('核对并重试', { exact: true }).waitFor(); await trigger(pending).click();
  assert.equal(await newItem(pending).isDisabled(), true); await pending.close();
  const failure = await fresh();
  await editor(failure).fill('失败时保留草稿');
  await failure.evaluate(() => { window.conversationMock.failNew = true; });
  await create(failure);
  await failure.getByRole('alert').filter({ hasText: '新建会话失败' }).waitFor();
  assert.equal(await editor(failure).inputValue(), '失败时保留草稿');
  assert.equal(await failure.evaluate(() => window.conversationMock.chats.A.conversation.id), 'conversation-A');
  await failure.evaluate(() => { window.conversationMock.failNew = false; }); await create(failure);
  await failure.waitForFunction(() => document.querySelector('textarea').value === ''); await failure.close();
  const held = await fresh();
  await editor(held).fill('旧草稿');
  await held.evaluate(() => { window.conversationMock.holdNew = true; });
  await trigger(held).click();
  await newItem(held).evaluate(el => { el.click(); el.click(); });
  assert.equal(await held.evaluate(() => window.conversationMock.calls.filter(c => c.command === 'ma_conversation_new').length), 1);
  await editor(held).fill('请求期间的新草稿');
  await held.evaluate(() => {
    const mock = window.conversationMock, chat = mock.chats.A;
    chat.conversation.id = 'new-held-conversation'; chat.messages = []; chat.rounds = []; chat.executionPlans = [];
    mock.release('ma_conversation_new', structuredClone(chat));
  });
  await held.waitForFunction(() => !document.querySelector('[aria-label="讨论记录操作"]').disabled);
  assert.equal(await editor(held).inputValue(), '请求期间的新草稿'); await held.close();
  assert.deepEqual(errors, []);
  console.log('PASS new team conversation: menu order/keyboard, fresh context and retained history, mode/draft reset, empty/running/pending states, failure/retry, double-click protection and concurrent draft; dark/light/320px.');
} finally { await browser.close(); }
