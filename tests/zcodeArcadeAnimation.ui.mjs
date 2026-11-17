import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';

const dir = '.scratch/zcode-arcade-animation-20261117';
mkdirSync(dir, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1100, height: 850 } });
const errors = [];
page.on('pageerror', e => errors.push(e.message));
try {
  await page.clock.install({ time: new Date('2026-11-17T00:00:00Z') });
  await page.clock.pauseAt(new Date('2026-11-17T00:00:01Z'));
  await page.goto(`${process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420'}/tests/fixtures/native-mascot-ui.html`);
  await page.locator('select').selectOption('zcode');
  const mascot = page.locator('.native-zcode-arcade');
  await mascot.waitFor();
  await page.evaluate(async () => {
    const image = new Image();
    image.src = getComputedStyle(document.querySelector('.native-zcode-arcade-sprite')).backgroundImage.slice(5, -2);
    await image.decode();
    if (!image.naturalWidth || image.naturalHeight !== image.naturalWidth * 2) throw Error('Invalid 4 x 8 atlas');
  });
  await mascot.screenshot({ path: `${dir}/idle.png` });
  const motions = ['摇杆漂移', '手柄翻接', '蹲身闪避', '扳机瞄准', '蓄力出招', '耳麦报点', '节奏踏步', '通关亮牌'];
  const seen = new Set();
  for (let i = 0; i < 360; i++) {
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
    const poses = new Set([await mascot.locator('span').getAttribute('style')]);
    const peaks = [1200, 800, 900, 1200, 1800, 1000, 1200, 1700];
    for (let elapsed = 0; elapsed < peaks[motions.indexOf(name)]; elapsed += 80) {
      await page.clock.runFor(80);
      poses.add(await mascot.locator('span').getAttribute('style'));
    }
    assert.ok(poses.size >= 2, `${name}: actually advances through different frames`);
    await mascot.screenshot({ path: `${dir}/gesture-${motions.indexOf(name) + 1}.png` });
  }
  await page.getByLabel('播放动画').uncheck();
  const frozen = await mascot.getAttribute('data-frame');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-frame'), frozen);
  assert.equal(await mascot.isDisabled(), true);
  await page.getByLabel('播放动画').check();
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }); document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForFunction(() => document.querySelector('.native-zcode-arcade').dataset.motionPaused === 'true');
  assert.equal(await mascot.getAttribute('data-motion-paused'), 'true');
  const hidden = await mascot.getAttribute('data-motion');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-motion'), hidden);
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }); document.dispatchEvent(new Event('visibilitychange')); });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.waitForFunction(() => document.querySelector('.native-zcode-arcade').disabled);
  assert.equal(await mascot.getAttribute('data-motion'), '摇杆漂移');
  assert.equal(await mascot.getAttribute('data-frame'), '0');
  await page.clock.runFor(20000);
  assert.equal(await mascot.getAttribute('data-frame'), '0');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.waitForFunction(() => !document.querySelector('.native-zcode-arcade').disabled);
  await page.getByRole('button', { name: '查看技能与插件', exact: true }).click();
  assert.match(await page.locator('output').textContent(), /zcode 命令入口已响应/);
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
  assert.equal(await page.locator('.native-zcode-arcade').count(), 1);
  assert.equal(await page.locator('[data-preview-kind="zcode"] .native-zcode-mascot').count(), 0, 'welcome no longer mounts the old character');
  const artwork = new Set([await mascot.locator('span').getAttribute('style')]);
  for (const selector of ['.native-clawd', '.native-whale-girl']) {
    const other = page.locator(selector);
    artwork.add(await other.locator('span').getAttribute('style'));
    for (let i = 0; i < 8; i++) {
      assert.equal(motions.includes(await other.getAttribute('data-motion')), false, 'gaming gestures are separate from the other eight-action characters');
      await other.click();
    }
  }
  assert.equal(artwork.size, 3, 'each character keeps its own generated artwork');
  assert.equal(await page.getByRole('button', { name: '查看命令', exact: true }).count(), 5);
  assert.equal(await page.getByRole('button', { name: '查看技能与插件', exact: true }).count(), 1);
  await page.screenshot({ path: `${dir}/all-mascots.png`, fullPage: true });
  assert.deepEqual(errors, []);
  writeFileSync(`${dir}/ui-results.json`, JSON.stringify({ passed: true, motions, automatic: true, changingFrames: true, inactivePause: true, hiddenPause: true, reducedMotion: true, narrowWidths: [390, 320], skillEntry: true, otherMascots: 6, errors }, null, 2));
  console.log('ZCode arcade: eight exclusive gestures, actual frame changes, pause/reduced motion, responsive layout, skills/plugins entry and six other mascots passed.');
} finally { await browser.close(); }
