// Actual App + settings in an isolated context. No real CLI or account operations.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1437';
const out = '.scratch/warm-night-theme-20261116/ui';
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome' });
const luminance = color => color.match(/[\d.]+/g).slice(0, 3).map(Number).map(n => n / 255).map(n => n <= .04045 ? n / 12.92 : ((n + .055) / 1.055) ** 2.4).reduce((v, n, i) => v + n * [.2126, .7152, .0722][i], 0);
const contrast = (a, b) => (Math.max(luminance(a), luminance(b)) + .05) / (Math.min(luminance(a), luminance(b)) + .05);
try {
  const page = await browser.newPage({ viewport: { width: 1512, height: 1040 } });
  page.setDefaultTimeout(10000);
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    if (!localStorage.getItem('crewvela-appearance')) localStorage.setItem('crewvela-appearance', JSON.stringify({ theme: 'ocean', background: 'solid', accent: 'clay', followSystem: false, terminalTransparent: false, terminalOpacity: .65 }));
  });
  await page.goto(`${base}/tests/fixtures/app-session-async-ui.html`);
  await page.waitForFunction(() => document.documentElement.dataset.appearanceTheme === 'ocean');
  await page.addStyleTag({ content: '*,*::before,*::after{transition:none!important;animation:none!important;}' });
  const openAppearance = async () => {
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('navigation', { name: '设置分类' }).getByRole('button', { name: '外观', exact: true }).click();
  };
  await openAppearance();
  const frame = page.getByRole('dialog', { name: '设置', exact: true });
  assert.deepEqual(await frame.locator('.appearance-theme-option > strong').allTextContents(), ['深色', '暖夜黑', '石墨黑', '浅色', '暖沙', '暖瓷白']);
  assert.equal(await frame.getByRole('button', { name: '暖夜黑', exact: true }).getAttribute('aria-pressed'), 'true');
  const colors = await page.evaluate(() => {
    const root = getComputedStyle(document.documentElement), frame = document.querySelector('.settings-native');
    return {
      tokens: Object.fromEntries(['--bg', '--bg-elev', '--bg-sunken', '--bg-sidebar', '--bg-hover', '--border', '--border-strong', '--text', '--text-dim', '--text-faint', '--terminal-bg', '--terminal-shell'].map(k => [k, root.getPropertyValue(k).trim()])),
      canvas: getComputedStyle(frame).backgroundColor, text: getComputedStyle(frame).color,
      nav: getComputedStyle(frame.querySelector('.settings-nav')).backgroundColor,
      dim: getComputedStyle(frame.querySelector('.settings-nav-group-label')).color,
      header: getComputedStyle(frame.querySelector('.settings-window-header')).backgroundColor,
      preview: getComputedStyle(frame.querySelector('[data-preview-theme="ocean"]')).backgroundColor,
      previewSidebar: getComputedStyle(frame.querySelector('[data-preview-theme="ocean"] .appearance-preview-sidebar')).backgroundColor,
      terminalText: getComputedStyle(frame.querySelector('.appearance-terminal-preview')).color,
      terminalDim: getComputedStyle(frame.querySelector('.appearance-terminal-preview span:nth-child(2)')).color,
      box: { width: frame.getBoundingClientRect().width, height: frame.getBoundingClientRect().height },
    };
  });
  assert.equal(colors.canvas, 'rgb(28, 27, 25)');
  assert.equal(colors.nav, 'rgb(19, 18, 17)'); assert.equal(colors.header, colors.nav);
  assert.equal(colors.preview, colors.canvas); assert.equal(colors.previewSidebar, colors.nav);
  for (const color of Object.values(colors.tokens)) {
    const [r, g, b] = color.slice(1).match(/../g).map(x => parseInt(x, 16));
    assert.ok(r >= g && g >= b, `neutral/warm theme token ${color} must not be blue`);
  }
  assert.ok(contrast(colors.text, colors.canvas) >= 4.5);
  assert.ok(contrast(colors.dim, colors.nav) >= 4.5);
  assert.equal(colors.terminalText, colors.text); assert.equal(colors.terminalDim, colors.dim);
  await frame.screenshot({ path: `${out}/reference-state.png` });
  for (const label of ['陶土', '紫色', '青色', '橙色', '粉色', '珊瑚色', '无强调色']) {
    await frame.getByRole('radio', { name: label, exact: true }).click();
    assert.equal(await frame.evaluate(el => getComputedStyle(el).backgroundColor), colors.canvas);
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('crewvela-appearance')).theme), 'ocean');
  }
  for (const label of ['柔光', '微纹理', '纯色']) {
    await frame.getByRole('button', { name: label, exact: true }).click();
    assert.equal(await frame.getByRole('button', { name: label, exact: true }).getAttribute('aria-pressed'), 'true');
  }
  await frame.getByRole('switch', { name: /全局透明模式/ }).locator('..').click();
  await frame.getByRole('slider', { name: '页面底板不透明度' }).focus();
  await page.keyboard.press('Home');
  for (let i = 0; i < 7; i++) await page.keyboard.press('ArrowRight');
  assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('crewvela-appearance')).terminalOpacity), .35);
  await frame.getByRole('button', { name: '完全透明', exact: true }).click();
  assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('crewvela-appearance')).terminalOpacity), 0);
  await frame.getByRole('switch', { name: /全局透明模式/ }).locator('..').click();
  await frame.getByRole('radio', { name: '陶土', exact: true }).click();
  const themes = [['dark', '深色'], ['ocean', '暖夜黑'], ['purple', '石墨黑'], ['light', '浅色'], ['sand', '暖沙'], ['aurora', '暖瓷白']];
  const paletteClasses = [];
  for (const [id, label] of themes) {
    await frame.getByRole('button', { name: label, exact: true }).click();
    await page.waitForFunction(id => document.documentElement.dataset.appearanceTheme === id, id);
    paletteClasses.push(await page.evaluate(() => document.documentElement.dataset.theme));
    assert.deepEqual(await frame.evaluate(el => ({ width: el.getBoundingClientRect().width, height: el.getBoundingClientRect().height })), colors.box);
  }
  assert.deepEqual(paletteClasses, ['dark', 'dark', 'dark', 'light', 'light', 'light']);
  await frame.getByRole('button', { name: '暖夜黑', exact: true }).click();
  await frame.getByRole('switch', { name: /跟随系统/ }).locator('..').click();
  await page.emulateMedia({ colorScheme: 'light' }); await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
  await page.emulateMedia({ colorScheme: 'dark' }); await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
  await frame.getByRole('button', { name: '暖夜黑', exact: true }).click();
  for (const width of [820, 687]) {
    await page.setViewportSize({ width, height: 800 });
    assert.equal(await frame.locator('.settings-content').evaluate(el => el.scrollWidth > el.clientWidth + 1), false);
    await frame.screenshot({ path: `${out}/narrow-${width}.png` });
  }
  await frame.getByRole('button', { name: '关闭设置', exact: true }).click();
  await page.reload(); await page.waitForFunction(() => document.documentElement.dataset.appearanceTheme === 'ocean');
  await openAppearance();
  assert.equal(await page.getByRole('button', { name: '暖夜黑', exact: true }).getAttribute('aria-pressed'), 'true');
  const persisted = await page.evaluate(() => JSON.parse(localStorage.getItem('crewvela-appearance')));
  assert.equal(persisted.theme, 'ocean'); assert.equal(persisted.background, 'solid'); assert.equal(persisted.terminalOpacity, 0); assert.equal(persisted.followSystem, false);
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/results.json`, JSON.stringify({ colors, persisted, paletteClasses, errors }, null, 2));
  console.log('PASS: old ocean becomes warm night; real App settings/preview tokens, six themes, seven accents, backgrounds, transparency, follow system, fixed geometry, narrow layouts and reload persistence.');
} finally { await browser.close(); }
