import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';

const dir = '.scratch/whale-girl-eight-20261117';
mkdirSync(dir, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1100, height: 850 } });
const errors = [];
page.on('pageerror', e => errors.push(e.message));
try {
  await page.clock.install();
  await page.goto(`${process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420'}/tests/fixtures/native-mascot-ui.html`);
  await page.locator('select').selectOption('deepseek_harness');
  const mascot = page.locator('.native-whale-girl');
  await mascot.waitFor();
  await page.locator('.native-whale-girl[data-renderer="flow"]').waitFor();
  await page.evaluate(async () => {
    const image = new Image();
    image.src = getComputedStyle(document.querySelector('.native-whale-girl-sprite')).backgroundImage.slice(5, -2);
    await image.decode();
    if (image.naturalWidth < 768 || image.naturalHeight !== image.naturalWidth) throw Error('Invalid HD rest image');
  });
  const motions = ['眨眼', '摆尾', '挥手', '轻跳', '歪头', '思考', '伸懒腰', '点头'];
  const seen = new Set();
  for (let i = 0; i < 240; i++) {
    seen.add(await mascot.getAttribute('data-motion'));
    await page.clock.runFor(80);
  }
  assert.deepEqual([...seen], motions, 'all eight gestures play automatically');
  await mascot.screenshot({ path: `${dir}/rendered-character.png` });
  for (const name of motions) {
    for (let i = 0; i < 8 && await mascot.getAttribute('data-motion') !== name; i++) await mascot.click();
    assert.equal(await mascot.getAttribute('data-motion'), name);
    // Start at the same first frame, even when the automatic loop was already on this gesture.
    for (let i = 0; i < 8; i++) await mascot.click();
    await page.clock.runFor(300);
    const before = await mascot.locator('canvas').evaluate(canvas => canvas.toDataURL());
    await page.clock.runFor(name === '眨眼' ? 2180 : 900);
    assert.notEqual(await mascot.locator('canvas').evaluate(canvas => canvas.toDataURL()), before, `${name}: actual pixels change`);
    await mascot.screenshot({ path: `${dir}/gesture-${motions.indexOf(name) + 1}.png` });
  }
  await page.getByLabel('播放动画').uncheck();
  const frozen = await mascot.getAttribute('data-frame');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-frame'), frozen);
  assert.equal(await mascot.isDisabled(), true);
  await page.getByLabel('播放动画').check();
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }); document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForFunction(() => document.querySelector('.native-whale-girl').dataset.motionPaused === 'true');
  assert.equal(await mascot.getAttribute('data-motion-paused'), 'true');
  const hidden = await mascot.getAttribute('data-motion');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-motion'), hidden);
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }); document.dispatchEvent(new Event('visibilitychange')); });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.waitForFunction(() => document.querySelector('.native-whale-girl').disabled);
  assert.equal(await mascot.getAttribute('data-motion'), '眨眼');
  assert.equal(await mascot.getAttribute('data-frame'), '0');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-frame'), '0');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.waitForFunction(() => !document.querySelector('.native-whale-girl').disabled);
  await page.getByRole('button', { name: '查看技能', exact: true }).click();
  assert.match(await page.locator('output').textContent(), /deepseek_harness 命令入口已响应/);
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 700 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    const box = await mascot.boundingBox();
    assert.ok(box.x >= 0 && box.x + box.width <= width);
    assert.ok(Math.abs(box.width - box.height) < 1, 'square frames preserve character proportions');
    await page.screenshot({ path: `${dir}/narrow-${width}.png` });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.locator('select').selectOption('all');
  assert.equal(await page.locator('[data-preview-kind]').count(), 7);
  assert.equal(await page.locator('.native-whale-girl').count(), 1);
  assert.equal(await page.getByRole('button', { name: '查看命令', exact: true }).count(), 5);
  assert.equal(await page.getByRole('button', { name: '查看技能与插件', exact: true }).count(), 1);
  await page.screenshot({ path: `${dir}/all-mascots.png`, fullPage: true });
  assert.deepEqual(errors, []);
  writeFileSync(`${dir}/ui-results.json`, JSON.stringify({ passed: true, motions, automatic: true, changingFrames: true, inactivePause: true, hiddenPause: true, reducedMotion: true, narrowWidths: [390, 320], skillEntry: true, otherMascots: 6, errors }, null, 2));
  console.log('Whale girl: eight gestures, pauses, reduced motion, responsive layout and existing entries passed.');
} finally { await browser.close(); }
