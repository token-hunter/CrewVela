// Run against Vite with CREWVELA_UI_URL set when it uses a non-default port.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1024 } });
  const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
  const open = async (theme = 'dark') => {
    await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&theme=${theme}`);
    await page.getByRole('button', { name: '打开设置回归', exact: true }).click();
    await page.getByRole('button', { name: '协作中心', exact: true }).click();
  };
  await open();
  const runtimeToggle = page.locator('.collab-runtime-toggle');
  assert.equal(await runtimeToggle.getAttribute('aria-expanded'), 'false');
  assert.equal(await page.locator('.collab-runtime-chip summary').first().isVisible(), false);
  await page.getByRole('button', { name: '管理 Runtime', exact: true }).click();
  await page.getByRole('heading', { name: '管理 Runtime', exact: true }).waitFor();
  await page.getByRole('button', { name: '关闭', exact: true }).click();
  await runtimeToggle.click();
  assert.equal(await page.locator('.collab-runtime-chip summary').first().isVisible(), true);
  await page.getByRole('button', { name: '检查更新', exact: true }).waitFor();
  for (const tab of [/^团队/, /^项目档案$/, /^协作记录$/, /^协作诊断/, /^概览$/]) {
    await page.getByRole('tab', { name: tab }).click();
    assert.equal(await page.getByRole('region', { name: '协作 Runtime', exact: true }).isVisible(), true, `${tab}: shared Runtime controls must remain visible`);
    assert.equal(await runtimeToggle.getAttribute('aria-expanded'), 'true', `${tab}: switching tabs must preserve Runtime expansion`);
    assert.equal(await page.locator('.collab-runtime-chip summary').first().isVisible(), true);
    for (const selector of ['.collab-guide-actions', '.collab-project-actions', '.collab-project-state', '.collab-project-counts']) {
      assert.equal(await page.locator(selector).isVisible(), true, `${tab}: ${selector} must remain visible`);
    }
  }
  await page.getByRole('tab', { name: '协作记录', exact: true }).click();
  await page.getByRole('button', { name: '管理 Runtime', exact: true }).click();
  await page.getByRole('heading', { name: '管理 Runtime', exact: true }).waitFor();
  await page.getByRole('button', { name: '关闭', exact: true }).click();
  await page.getByRole('tab', { name: '概览', exact: true }).click();
  await runtimeToggle.click();
  await page.getByRole('tab', { name: '协作记录', exact: true }).click();
  assert.equal(await runtimeToggle.getAttribute('aria-expanded'), 'false');
  assert.equal(await page.locator('.collab-runtime-content').isVisible(), false);
  assert.equal(await page.getByRole('button', { name: '管理 Runtime', exact: true }).isVisible(), true);
  await page.getByRole('tab', { name: '概览', exact: true }).click();
  assert.equal(await page.locator('.collab-activity-disclosure').getAttribute('open'), null);
  await page.getByRole('group', { name: '任务筛选' }).getByRole('button', { name: '执行中', exact: true }).click();
  assert.equal(await page.locator('.collab-task-row').count(), 1);
  assert.match(await page.locator('.collab-task-row').innerText(), /实现会话路由/);
  await page.getByRole('group', { name: '任务筛选' }).getByRole('button', { name: '全部', exact: true }).click();
  assert.equal(await page.locator('.collab-task-row').count(), 6);
  const stage = page.locator('.collab-stage-title').first();
  await stage.click();
  assert.equal(await stage.getAttribute('aria-expanded'), 'false');
  await stage.click();
  await page.locator('.collab-task-menu summary').first().click();
  await page.getByRole('button', { name: '查看任务详情', exact: true }).click();
  await page.getByRole('button', { name: '关闭详情', exact: true }).click();
  await page.locator('.collab-activity-disclosure > summary').click();
  await page.getByRole('button', { name: '查看全部活动', exact: true }).click();
  assert.equal(await page.getByRole('tab', { name: '协作记录', exact: true }).getAttribute('aria-selected'), 'true');
  await page.getByRole('tab', { name: '概览', exact: true }).click();
  await page.getByRole('button', { name: '查看诊断 →', exact: true }).click();
  assert.equal(await page.getByRole('tab', { name: /协作诊断/ }).getAttribute('aria-selected'), 'true');
  for (const theme of ['dark', 'light']) {
    for (const size of [{ width: 1440, height: 1024 }, { width: 820, height: 540 }, { width: 687, height: 903 }]) {
      await page.setViewportSize(size);
      await open(theme);
      await page.getByRole('group', { name: '任务筛选' }).getByRole('button', { name: '全部', exact: true }).click();
      const geometry = await page.evaluate(() => {
        const content = document.querySelector('.settings-content');
        const tasks = document.querySelector('.collab-task-panel');
        return { overflow: content.scrollWidth > content.clientWidth + 1, fullWidth: tasks.clientWidth === document.querySelector('.collab-main-grid').clientWidth };
      });
      assert.equal(geometry.overflow, false, `${theme} ${size.width}: content must not overflow`);
      assert.equal(geometry.fullWidth, true);
      assert.equal(await page.getByRole('button', { name: '关闭设置', exact: true }).isVisible(), true);
      await page.getByRole('tab', { name: '协作记录', exact: true }).click();
      await runtimeToggle.click();
      assert.equal(await page.getByRole('button', { name: '管理 Runtime', exact: true }).isVisible(), true);
      assert.equal(await page.locator('.collab-runtime-chip summary').first().isVisible(), true);
      assert.equal(await page.locator('.settings-content').evaluate(el => el.scrollWidth > el.clientWidth + 1), false, `${theme} ${size.width}: records with Runtime must not overflow`);
      if (process.env.CREWVELA_UI_SCREENSHOTS) {
        await page.screenshot({ path: `${process.env.CREWVELA_UI_SCREENSHOTS}/records-${theme}-${size.width}.png` });
      }
    }
  }
  await page.goto(`${base}/tests/fixtures/collaboration-member-dialog-ui.html?state=runtime-update-failed`);
  await page.locator('.collab-runtime-toggle').click();
  await page.locator('.collab-runtime-chip summary').filter({ hasText: /^Codex$/ }).click();
  await page.getByRole('button', { name: '升级运行时', exact: true }).click();
  await page.locator('.collab-runtime-toggle').click();
  await page.getByRole('alert').filter({ hasText: /Codex.*回归测试：升级失败/ }).waitFor();
  assert.equal(await page.locator('.collab-runtime-content').isVisible(), false);
  console.log('PASS: shared controls across all five tabs, preserved Runtime expansion, records management dialog, filters, details, responsive geometry and visible upgrade failures');
} finally { await browser.close(); }
