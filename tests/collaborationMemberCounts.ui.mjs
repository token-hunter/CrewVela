// Run against Vite: node tests/collaborationMemberCounts.ui.mjs
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { chromium } from 'playwright';

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 950 } });
try {
  await page.addInitScript(() => {
    localStorage.setItem('crewvela-runtime-preferences', JSON.stringify({ disabledRuntimes: ['zcode_cli'] }));
  });
  await page.goto(`${process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420'}/?ui-preview=1`);
  await page.locator('.unified-sidebar-settings-button').click();
  await page.locator('.settings-nav-item').filter({ hasText: '协作中心' }).click();
  const teamTab = page.getByRole('tab').filter({ hasText: '团队' });
  await teamTab.click();
  const cards = page.locator('.collab-agent-card');
  await cards.first().waitFor();
  async function checkCounts(expected) {
    await page.waitForFunction(n => document.querySelector('.collab-tabs [aria-selected="true"] b')?.textContent === String(n), expected);
    assert.equal(await teamTab.locator('b').textContent(), String(expected));
    assert.match(await page.locator('.collab-project-counts').innerText(), new RegExp(`${expected} 位成员`));
    assert.equal(await page.locator('.collab-team-filter-pill').first().locator('b').textContent(), String(expected));
    assert.equal(await cards.count(), expected);
  }
  await checkCounts(7);
  for (const runtime of ['pi', 'opencode']) {
    const logos = page.locator(`.collab-runtime-chip img[data-runtime="${runtime}"], .collab-agent-card img[data-runtime="${runtime}"]`);
    assert.ok(await logos.count() >= 2, `${runtime} has toolbar and member logos`);
    for (const logo of await logos.all()) assert.match(await logo.evaluate(el => getComputedStyle(el).filter), /invert\(1\)/, `${runtime} is visible against the dark background`);
  }
  assert.match(await page.locator('.collab-team-toolbar p').innerText(), /共 8 位，隐藏 1 位/);
  await mkdir('output/playwright', { recursive: true });
  await page.screenshot({ path: 'output/playwright/collaboration-member-counts.png' });
  await page.locator('.collab-team-search input').fill('前端');
  assert.equal(await cards.count(), 1);
  assert.equal(await teamTab.locator('b').textContent(), '7');
  await page.locator('.collab-team-search input').fill('');
  await page.locator('.collab-team-filter-pill').filter({ hasText: '审查' }).click();
  assert.equal(await cards.count(), 1);
  assert.equal(await teamTab.locator('b').textContent(), '7');
  await page.locator('.collab-team-filter-pill').first().click();
  async function setDisabled(disabledRuntimes) {
    await page.evaluate(disabled => {
      const value = JSON.stringify({ disabledRuntimes: disabled });
      localStorage.setItem('crewvela-runtime-preferences', value);
      window.dispatchEvent(new StorageEvent('storage', { key: 'crewvela-runtime-preferences', newValue: value }));
    }, disabledRuntimes);
  }
  await setDisabled([]);
  await checkCounts(8);
  assert.doesNotMatch(await page.locator('.collab-team-toolbar p').innerText(), /隐藏/);
  await setDisabled(['claude_code', 'codex', 'kimi_code', 'pi', 'opencode', 'zcode_cli', 'deepseek_harness', 'zcode']);
  await checkCounts(0);
  assert.match(await page.locator('.collab-team-toolbar p').innerText(), /共 8 位，隐藏 8 位/);
  console.log('PASS: counts agree with cards; hidden explanation; search/role filters; runtime changes; zero visible members');
} finally {
  await browser.close();
}
