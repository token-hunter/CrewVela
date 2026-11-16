import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1437";
const out = ".scratch/pet-reminder-fix-20261116";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const errors = [], results = [];
async function open(query = "") {
  const page = await browser.newPage({ viewport: { width: 1000, height: 720 } });
  page.on("pageerror", error => errors.push(error.message));
  const clockStart = new Date("2026-11-16T00:00:00Z");
  await page.clock.install({ time: clockStart });
  await page.clock.pauseAt(new Date(clockStart.getTime() + 1000));
  // Identity shuffle: natural remind is near the end of the original 18-action deck.
  await page.addInitScript(() => { Math.random = () => 0.999999; });
  await page.goto(`${base}/tests/fixtures/desktop-pet-ui.html${query}`);
  await page.locator(".desktop-pet").waitFor();
  return page;
}
const action = page => page.locator(".desktop-pet").getAttribute("data-pet-action");
const sessions = (page, entries) => page.evaluate(entries => window.desktopPetFixture.setSessions(entries), entries);
const settings = (page, patch) => page.evaluate(patch => window.desktopPetFixture.patchSettings(patch), patch);
async function collect(page, steps = 40, delta = 10000) {
  const seen = new Set();
  for (let step = 0; step < steps; step++) {
    await page.clock.fastForward(delta);
    seen.add(await action(page));
  }
  return [...seen];
}
try {
  // The red-capable regression: each persistent error must leave room for all actions.
  for (const status of ["error", "waiting", "running", "none"]) {
    const page = await open();
    await sessions(page, status === "none" ? [] : [{ id: "session-a", status }]);
    const seen = await collect(page);
    results.push({ status, seen });
    assert.equal(seen.length, 18, `${status}: persistent status must not suppress any autonomous action`);
    await page.close();
  }
  console.log("PASS: sustained waiting/error and normal/running states retain all 18 actions");

  // Polling and status changes must neither reset the deck timer nor bypass cooldown.
  const reminders = await open();
  await sessions(reminders, [{ id: "first", status: "error" }]);
  assert.equal(await action(reminders), "remind");
  await reminders.clock.fastForward(3599);
  assert.equal(await action(reminders), "remind");
  await reminders.clock.fastForward(1);
  assert.equal(await action(reminders), "idle", "a 3.6-second reminder resumes the existing deck");
  let earlyReminders = 0, laterReminders = 0;
  const earlyActions = new Set();
  for (let second = 4; second <= 70; second++) {
    await sessions(reminders, [{ id: `poll-${second}`, status: second % 2 ? "waiting" : "error" }]);
    await reminders.clock.fastForward(1000);
    const current = await action(reminders);
    if (second < 59) { earlyActions.add(current); if (current === "remind") earlyReminders++; }
    if (second >= 61 && current === "remind") laterReminders++;
  }
  assert.equal(earlyReminders, 0, "polling/new errors cannot bypass the one-minute cooldown");
  assert.ok(earlyActions.size >= 8, "polling must allow autonomous actions to advance");
  assert.ok(laterReminders > 0, "an unresolved error is reminded again after cooldown");
  await sessions(reminders, []);
  const recovered = await collect(reminders, 22);
  assert.equal(recovered.length, 18, "cleared alerts restore the complete action cycle");
  // Repeated healthy/error transitions also leave the current action deadline intact.
  const flapping = new Set();
  for (let second = 0; second < 35; second++) {
    await sessions(reminders, second % 2 ? [] : [{ id: "flapping", status: "error" }]);
    await reminders.clock.fastForward(1000);
    flapping.add(await action(reminders));
  }
  assert.ok(flapping.size >= 5, "state flapping must not keep restarting action timers");
  await reminders.close();
  console.log("PASS: short reminders, cooldown/repeat, session polling/flapping and recovery");

  // All builtin roles use their own atlas and resume movement even with an error.
  const roles = await open("?status=error");
  assert.equal(await action(roles), "remind", "an initial error receives a reminder on mount");
  const ids = await roles.evaluate(() => window.desktopPetFixture.pets);
  for (const id of ids) {
    await settings(roles, { selectedId: id });
    assert.match(await roles.locator(".desktop-pet-sprite").evaluate(el => getComputedStyle(el).backgroundImage), new RegExp(`${id}-animation`));
    const seen = await collect(roles);
    assert.equal(seen.length, 18, `${id}: selecting a role preserves every action`);
  }
  await roles.screenshot({ path: `${out}/actions-restored.png` });
  await roles.close();

  // Unlike fastForward, runFor plays the actual RAF frames needed for movement.
  const moving = await open();
  for (let step = 0; step < 18 && await action(moving) !== "walk"; step++) await moving.clock.fastForward(10000);
  assert.equal(await action(moving), "walk");
  const movementStart = await moving.locator(".desktop-pet").boundingBox();
  await moving.clock.runFor(2600);
  const movementMiddle = await moving.locator(".desktop-pet").boundingBox();
  assert.notEqual(Math.round(movementMiddle.x), Math.round(movementStart.x), "walking visibly changes position");
  await moving.clock.runFor(3000);
  assert.equal(await action(moving), "run", "movement completes and the next original action follows");
  await moving.close();

  const interactions = await open();
  const pet = interactions.locator(".desktop-pet");
  const before = await pet.boundingBox();
  await interactions.mouse.move(before.x + 56, before.y + 56);
  await interactions.mouse.down();
  await interactions.mouse.move(180, 200);
  await interactions.mouse.up();
  const after = await pet.boundingBox();
  assert.equal(Math.round(after.x), 124); assert.equal(Math.round(after.y), 144);
  assert.deepEqual(await interactions.evaluate(() => window.desktopPetFixture.positions.at(-1)), { x: 124, y: 144 });
  await interactions.setViewportSize({ width: 300, height: 240 });
  // Deliver the resize deterministically while the animation clock is controlled.
  await interactions.evaluate(() => window.dispatchEvent(new Event("resize")));
  await interactions.clock.runFor(32);
  const resized = await pet.boundingBox();
  assert.ok(resized.x >= 8 && resized.y >= 8 && resized.x + resized.width <= 292 && resized.y + resized.height <= 232,
    JSON.stringify({resized,viewport:await interactions.evaluate(()=>({width:innerWidth,height:innerHeight,left:document.querySelector('.desktop-pet').style.left,top:document.querySelector('.desktop-pet').style.top}))}));
  await interactions.setViewportSize({ width: 1000, height: 720 });
  const frames = new Set();
  for (let frame = 0; frame < 4; frame++) { await interactions.clock.runFor(420); frames.add(await pet.getAttribute("data-pet-frame")); }
  assert.equal(frames.size, 3, "sprite frames continue advancing");
  await settings(interactions, { enabled: false });
  assert.equal(await pet.count(), 0);
  await sessions(interactions, [{ id: "hidden-error", status: "error" }]);
  await interactions.clock.fastForward(65000);
  await settings(interactions, { enabled: true });
  await pet.waitFor(); assert.equal(await action(interactions), "remind");
  await interactions.clock.fastForward(3600);
  assert.notEqual(await action(interactions), "remind", "reenabled pets also resume autonomous actions");
  const image = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
  await settings(interactions, { selectedId: "custom", customPets: [{ id: "custom", name: "自定义", description: "fixture", src: image, custom: true }] });
  assert.equal(await pet.locator("img").getAttribute("src"), image);
  await pet.locator("img").evaluate(image => image.decode());
  assert.equal(await pet.locator(".desktop-pet-sprite").count(), 0);
  await interactions.evaluate(() => window.desktopPetFixture.unmount());
  await interactions.clock.fastForward(120000);
  assert.equal(await pet.count(), 0);
  await interactions.close();
  console.log("PASS: 8 roles, sprite frames, movement, drag/save, resize bounds, enable/disable, custom pet and unmount");
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/ui-results.json`, JSON.stringify({ results, roles: ids, errors }, null, 2));
} finally { await browser.close(); }
