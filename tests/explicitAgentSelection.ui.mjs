import assert from 'node:assert/strict';
import { chromium } from 'playwright';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 780, height: 1000 } });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.goto(`${process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432'}/tests/fixtures/collaboration-conversation-ui.html?empty=1`);
  await page.locator('.conversation-members button').last().waitFor();
  const editor = page.getByRole('combobox', { name: '给团队发消息', exact: true });
  await editor.fill('互相介绍一下'); await page.locator('.conversation-target').click();
  const picker = page.locator('.conversation-recipient-menu');
  assert.equal(await picker.getByRole('checkbox').count(), 3);
  assert.equal(await picker.getByText('群组', { exact: true }).count(), 0);
  await picker.getByRole('button', { name: '取消全选', exact: true }).click();
  assert.equal(await picker.locator('input:checked').count(), 0);
  await picker.getByRole('checkbox').nth(1).check(); await picker.getByRole('checkbox').nth(2).check();
  assert.equal(await picker.locator('input:checked').count(), 2);
  assert.equal(await picker.isVisible(), true);
  await picker.getByRole('checkbox').nth(1).uncheck();
  await page.locator('.conversation-target').click(); await editor.fill('@Kimi Code 自我介绍一下'); await editor.press('Enter');
  await page.getByRole('combobox', { name: '给团队插话', exact: true }).waitFor();
  const first = await page.evaluate(() => window.conversationMock.calls.find(item => item.command === 'ma_conversation_send').args.input);
  assert.deepEqual(first.participantAgentIds, ['kimi']); assert.equal(first.projectId, 'A');
  await page.getByRole('button', { name: '讨论记录操作', exact: true }).click();
  await page.getByRole('menuitem', { name: '暂停讨论', exact: true }).click();
  await page.getByRole('button', { name: '继续输入', exact: true }).waitFor();
  await editor.fill('@');
  const mentions = page.locator('.multi-agent-mention-menu');
  await mentions.getByRole('option').first().waitFor();
  try { await mentions.getByRole('button', { name: '全选', exact: true }).click({ timeout: 3000 }); }
  catch (error) {
    console.log(await page.evaluate(() => {
      const target = document.querySelector('.multi-agent-mention-header button');
      const rect = target.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
      const chain = []; let node = target;
      while (node && chain.length < 8) { const s = getComputedStyle(node); chain.push({ tag: node.tagName, cls: node.className, z: s.zIndex, position: s.position, overflow: s.overflow, contain: s.contain }); node = node.parentElement; }
      return { target: { x: rect.x, y: rect.y, width: rect.width, height: rect.height }, hit: hit?.className, chain };
    }));
    await page.screenshot({ path: '.scratch/collaboration-conversation/mention-blocked.jpg' });
    throw error;
  }
  assert.equal(await editor.inputValue(), '@Claude Code @Codex @Kimi Code ');
  await editor.pressSequentially('互相介绍一下'); await editor.press('Enter');
  await page.waitForFunction(() => window.conversationMock.calls.filter(item => item.command === 'ma_conversation_send').length === 2);
  const second = await page.evaluate(() => window.conversationMock.calls.filter(item => item.command === 'ma_conversation_send')[1].args.input);
  assert.deepEqual(second.participantAgentIds, ['claude', 'codex', 'kimi']);
  assert.deepEqual(errors, []);
  console.log('PASS: concrete members, multi-select, select-all/clear, precise recipients and @ full selection');
} finally { await browser.close(); }
