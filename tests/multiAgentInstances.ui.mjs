import assert from 'node:assert/strict';
import { chromium } from 'playwright';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 780, height: 1000 } });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.goto(`${process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432'}/tests/fixtures/collaboration-conversation-ui.html?instances=2&plan=pending`);
  await page.locator('[data-instance="0"] .conversation-plan').waitFor();
  const before = await page.evaluate(() => window.conversationMock.calls.length);
  await page.evaluate(() => window.conversationMock.selectInstance(1));
  await page.locator('[data-instance="1"] .conversation-plan').waitFor();
  assert.equal(await page.locator('[data-instance="0"]').isVisible(), false);
  await page.waitForTimeout(1700);
  const calls = await page.evaluate(() => window.conversationMock.calls);
  assert.ok(calls.length > before);
  assert.equal(calls.filter(item => ['ma_approve_task', 'ma_conversation_execute_plan', 'ma_conversation_send', 'ma_start_project'].includes(item.command)).length, 0, 'views never automatically approve or start work');
  await page.evaluate(() => window.conversationMock.setActive(false));
  await page.waitForTimeout(200);
  const inactive = await page.evaluate(() => window.conversationMock.calls.length);
  await page.waitForTimeout(1700);
  assert.equal(await page.evaluate(() => window.conversationMock.calls.length), inactive, 'inactive views stop polling');
  assert.equal(await page.locator('[data-instance="1"] .conversation-plan').count(), 1);
  assert.deepEqual(errors, []);
  console.log('PASS: multiple views preserve state, stop polling when inactive and never auto-approve or start tasks');
} finally { await browser.close(); }
