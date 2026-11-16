import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const out = '.scratch/automation-menu-dismiss-20261117';
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const errors = [];
try {
  for (const theme of ['dark', 'light']) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    page.on('pageerror', e => errors.push(e.message));
    await page.goto(`${base}/tests/fixtures/automation-execution-ui.html?theme=${theme}`);
    const cards = page.locator('.automation-card');
    const first = cards.nth(0).locator('.automation-card-menu');
    const second = cards.nth(1).locator('.automation-card-menu');
    const open = () => first.locator('summary').click();
    const isOpen = menu => menu.evaluate(el => el.open);
    await open();
    assert.equal(await isOpen(first), true);
    await page.screenshot({ path: `${out}/${theme}-open.png`, fullPage: true });
    await page.getByRole('heading', { name: '自动化', exact: true }).click();
    assert.equal(await isOpen(first), false, 'clicking page content outside the menu dismisses it');
    await open();
    await page.getByText('隔离示例 · 不调用真实 CLI', { exact: true }).click();
    assert.equal(await isOpen(first), false, 'clicks outside AutomationPanel also dismiss the menu');
    await open();
    await second.locator('summary').click();
    assert.equal(await isOpen(first), false);
    assert.equal(await isOpen(second), true, 'another trigger opens with one click');
    await second.locator('summary').click();
    assert.equal(await isOpen(second), false, 'trigger toggle still works');
    await open();
    await first.getByRole('button', { name: '打开原会话', exact: true }).click();
    assert.equal(await page.evaluate(() => window.__automationOpened), 'session-fixture');
    assert.equal(await isOpen(first), false, 'inside action executes before closing');
    await open();
    await page.keyboard.press('Escape');
    assert.equal(await isOpen(first), false);
    assert.equal(await first.locator('summary').evaluate(el => el === document.activeElement), true);
    await open();
    await cards.nth(0).getByRole('switch', { name: '暂停后续计划 仓库巡检', exact: true }).click();
    assert.equal(await isOpen(first), false);
    await page.waitForFunction(() => window.__automationFixture.calls.some(c => c.command === 'set_automation_enabled'));
    assert.equal(await page.locator('[data-automation-view="form"]').count(), 0, 'outside controls do not accidentally open card editing');
    // Rapid repeated header clicks may select text; the card intentionally ignores text selections.
    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    await open();
    await cards.nth(0).locator('.automation-card-schedule').click();
    await page.locator('[data-automation-view="form"]').waitFor();
    assert.equal(await page.getByLabel('名称', { exact: true }).inputValue(), '仓库巡检');
    await page.getByRole('button', { name: '取消', exact: true }).click();
    await cards.nth(0).waitFor();
    assert.equal(await isOpen(first), false, 'returning from editing does not restore an open menu');
    await open();
    // Nested components often stop bubbling: dismissal must still observe outside pointers.
    await page.locator('.automation-heading').evaluate(el => el.addEventListener('pointerdown', event => event.stopPropagation(), { once: true }));
    await page.getByRole('heading', { name: '自动化', exact: true }).click();
    assert.equal(await isOpen(first), false);
    await page.screenshot({ path: `${out}/${theme}-closed.png`, fullPage: true });
    assert.equal(await page.evaluate(() => window.__automationFixture.calls.filter(c => ['run_automation_now', 'delete_automation'].includes(c.command)).length), 0);
    await page.close();
  }
  assert.deepEqual(errors, []);
  console.log('PASS automation menu: outside panel/content, stopped bubbling, another trigger, inside action, toggle, Escape, plan control and card edit; dark/light.');
} finally { await browser.close(); }
