// Full production settings host in read-only preview, distinct from IPC fixture tests.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  for (const size of [{ width: 1440, height: 900 }, { width: 820, height: 540 }, { width: 1920, height: 1080 }]) {
    const page = await browser.newPage({ viewport: size });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const nav = page.locator('.settings-nav');
    await nav.getByRole('button', { name: '电脑控制', exact: true }).click();
    const baseline = await page.locator('.settings-dialog').boundingBox();
    await nav.getByRole('button', { name: '插件市场', exact: true }).click();
    const region = page.getByRole('region', { name: '插件详情与安装', exact: true });
    await region.getByRole('heading', { name: '网页测试', exact: true }).waitFor();
    assert.deepEqual(await page.locator('.settings-dialog').boundingBox(), baseline);
    assert.equal(await page.locator('.pm-directory-row').count(), 16);
    assert.equal(await page.locator('.settings-content').evaluate(el => el.scrollWidth > el.clientWidth), false);
    if (size.width >= 1440) {
      const cta = await region.locator('.pm-install-submit').boundingBox();
      const host = await page.locator('.settings-content').boundingBox();
      assert.ok(cta.y + cta.height <= host.y + host.height, 'primary action fits actual modal content height');
    }
    await page.getByRole('textbox', { name: '搜索插件或使用场景' }).fill('PDF 处理');
    await region.getByRole('heading', { name: 'PDF 处理', exact: true }).waitFor();
    assert.equal(await page.locator('.pm-directory-row').count(), 1);
    mkdirSync('output/plugin-market-split', { recursive: true });
    await page.screenshot({ path: `output/plugin-market-split/host-${size.width}.png` });
    assert.deepEqual(errors, []);
    await page.close();
  }
  console.log('PASS: full settings host, stable dialog, actual available-height CTA, filters and three viewport sizes');
} finally { await browser.close(); }
