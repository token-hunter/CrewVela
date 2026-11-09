// Run against Vite: node tests/cliSessionPets.ui.mjs
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const baseUrl = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
mkdirSync("output/playwright", { recursive: true });

try {
  const page = await browser.newPage({ viewport: { width: 1360, height: 800 }, deviceScaleFactor: 1 });
  await page.goto(`${baseUrl}/tests/fixtures/cli-session-pets-ui.html`);
  const pets = page.locator(".cli-session-pet");
  await page.locator('[data-case="pi"] .pane-head').waitFor();

  assert.equal(await pets.count(), 5, "only the five approved CLI runtimes render the new pets");
  for (const runtime of ["codex", "kimi_code", "zcode_cli", "opencode", "pi"]) {
    const pet = page.locator(`.cli-session-pet[data-runtime="${runtime}"]`);
    assert.equal(await pet.count(), 1, `${runtime} pet exists once`);
    assert.equal(await pet.getAttribute("data-status"), "running", `${runtime} starts animated`);
    const accessibleName = await pet.getAttribute("aria-label");
    assert.ok(accessibleName?.length, `${runtime} has an accessible name`);
    const dimensions = await pet.evaluate(element => {
      const rect = element.getBoundingClientRect();
      return { width: rect.width, height: rect.height };
    });
    assert.equal(dimensions.width, 56, `${runtime} stays within the header width`);
    assert.equal(dimensions.height, 32, `${runtime} stays within the header height`);
    const geometry = await pet.evaluate(element => {
      const actions = element.closest(".pane-head-actions");
      const bounds = selector => {
        const rect = actions.querySelector(selector).getBoundingClientRect();
        return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
      };
      const rect = element.getBoundingClientRect();
      return {
        pet: { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom },
        handoff: bounds(".pane-switch-agent"),
        maximize: bounds(".pane-maximize"),
        close: bounds(".pane-close"),
      };
    });
    assert.ok(geometry.handoff.right <= geometry.pet.left || geometry.pet.right <= geometry.handoff.left, `${runtime} does not overlap the handoff button`);
    assert.ok(geometry.pet.right <= geometry.maximize.left, `${runtime} does not overlap maximize`);
    assert.ok(geometry.pet.right <= geometry.close.left, `${runtime} does not overlap close`);
  }

  const runtimeHeaderWidth = await page.locator('[data-case="codex"] .pane-head').evaluate(element => element.getBoundingClientRect().width);
  assert.ok(runtimeHeaderWidth >= 400 && runtimeHeaderWidth <= 430, `runtime header exercises the narrow ~420px layout (got ${runtimeHeaderWidth}px)`);

  assert.equal(await page.locator("[data-case='legacy-claude'] .session-pet").count(), 1, "legacy Claude pet remains");
  assert.equal(await page.locator("[data-case='legacy-claude'] .cli-session-pet").count(), 0, "Claude keeps its existing pet");
  assert.equal(await page.locator("[data-case='zcode-native'] .cli-session-pet").count(), 0, "native ZCode does not get a CLI pet");
  assert.equal(await page.locator("[data-case='unsupported-runtime'] .cli-session-pet").count(), 0, "unsupported runtimes stay unchanged");
  const retiredLogo = page.locator("[data-case='unsupported-runtime'] .pane-runtime-logo");
  assert.equal(await retiredLogo.getAttribute("title"), "已停用的运行时");
  assert.ok(await retiredLogo.evaluate(el => el.complete && el.naturalWidth > 0), "unknown historical runtime has a decodable fallback logo");
  assert.equal(await page.locator("[data-case='powershell'] .cli-session-pet").count(), 0, "shell panes stay unchanged");

  await page.screenshot({ path: "output/playwright/cli-session-pets-running.png", fullPage: true });
  const sampleAnimation = page.locator('.cli-session-pet[data-runtime="codex"] .pet-codex-hand-left');
  const runningTimeBefore = await sampleAnimation.evaluate(element => element.getAnimations()[0]?.currentTime);
  await page.waitForTimeout(240);
  const runningTimeAfter = await sampleAnimation.evaluate(element => element.getAnimations()[0]?.currentTime);
  assert.ok(Number(runningTimeAfter) > Number(runningTimeBefore), "running animation advances");

  const assertInsidePetStage = async (runtime, selector, animationTimes) => {
    const part = page.locator(`.cli-session-pet[data-runtime="${runtime}"] ${selector}`);
    for (const time of animationTimes) {
      await part.evaluate((element, currentTime) => {
        const animation = element.getAnimations()[0];
        if (!animation) throw new Error("motion animation is missing");
        animation.currentTime = currentTime;
      }, time);
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const bounds = await part.evaluate(element => {
        const stage = element.closest("svg").getBoundingClientRect();
        const partRect = element.getBoundingClientRect();
        return { stage: { left: stage.left, right: stage.right, top: stage.top, bottom: stage.bottom }, part: { left: partRect.left, right: partRect.right, top: partRect.top, bottom: partRect.bottom } };
      });
      assert.ok(bounds.part.left >= bounds.stage.left && bounds.part.right <= bounds.stage.right, `${runtime} stays inside the stage horizontally`);
      assert.ok(bounds.part.top >= bounds.stage.top && bounds.part.bottom <= bounds.stage.bottom, `${runtime} stays inside the stage vertically`);
    }
  };
  await assertInsidePetStage("kimi_code", ".pet-kimi-flying-star", [1152, 1728]);
  await assertInsidePetStage("pi", ".pet-pi-token", [858, 1188]);

  await page.getByTestId("toggle-hidden").click();
  for (const [runtime, selector] of [
    ["codex", ".pet-codex-hand-left"],
    ["kimi_code", ".pet-kimi-flying-star"],
    ["zcode_cli", ".pet-zcode-body"],
    ["opencode", ".pet-opencode-bracket-left"],
    ["pi", ".pet-pi-token"],
  ]) {
    const motionPart = page.locator(`.cli-session-pet[data-runtime="${runtime}"] ${selector}`);
    assert.equal(await motionPart.evaluate(element => getComputedStyle(element).animationPlayState), "paused", `${runtime} pauses when its pane is hidden`);
  }
  const hiddenTimeBefore = await sampleAnimation.evaluate(element => element.getAnimations()[0]?.currentTime);
  await page.waitForTimeout(240);
  const hiddenTimeAfter = await sampleAnimation.evaluate(element => element.getAnimations()[0]?.currentTime);
  assert.equal(hiddenTimeAfter, hiddenTimeBefore, "hidden pet animation time remains paused");
  await page.screenshot({ path: "output/playwright/cli-session-pets-hidden.png", fullPage: true });

  await page.getByTestId("toggle-hidden").click();
  await page.emulateMedia({ reducedMotion: "reduce" });
  assert.equal(await sampleAnimation.evaluate(element => getComputedStyle(element).animationName), "none", "reduced motion disables pet animation");
  await page.emulateMedia({ reducedMotion: "no-preference" });

  await page.getByTestId("set-idle").click();
  for (const pet of await pets.all()) {
    assert.equal(await pet.getAttribute("data-status"), "idle", "non-running statuses use a static pose");
  }
  assert.equal(await sampleAnimation.evaluate(element => getComputedStyle(element).animationName), "none", "stopping removes active animation");
  await page.screenshot({ path: "output/playwright/cli-session-pets-idle.png", fullPage: true });

  await page.getByTestId("set-running").click();
  const restartedTime = await sampleAnimation.evaluate(element => element.getAnimations()[0]?.currentTime);
  assert.ok(Number(restartedTime) < 120, "restarting begins the motion cycle from the start");
  const narrowCase = page.locator('[data-case="codex"]');
  const narrowPane = narrowCase.locator(".pane");
  await narrowPane.evaluate(element => {
    element.style.width = "300px";
    element.style.minWidth = "0";
    element.style.maxWidth = "300px";
  });
  const narrowGeometry = await narrowPane.evaluate(element => {
    const pane = element.getBoundingClientRect();
    const close = element.querySelector(".pane-close").getBoundingClientRect();
    const pet = element.querySelector(".cli-session-pet");
    return {
      paneWidth: pane.width,
      paneRight: pane.right,
      closeRight: close.right,
      petDisplay: getComputedStyle(pet).display,
    };
  });
  assert.equal(narrowGeometry.paneWidth, 300, "narrow pane fixture is 300px wide");
  assert.equal(narrowGeometry.petDisplay, "none", "pet yields its space in a narrow pane");
  assert.ok(narrowGeometry.closeRight <= narrowGeometry.paneRight, "close control stays inside a 300px pane");
  await narrowCase.locator(".pane-maximize").click();
  assert.equal(await narrowCase.getAttribute("data-maximized"), "true", "maximize remains clickable at 300px");
  await narrowCase.locator(".pane-maximize").click();
  await narrowCase.locator(".pane-close").click();
  assert.equal(await narrowCase.getAttribute("data-closed"), "true", "close remains clickable at 300px");
  console.log("PASS: five CLI pets; Claude and unsupported panes; dimensions and button hit areas; running, idle, hidden and reduced-motion behavior");
  console.log("Screenshots: output/playwright/cli-session-pets-running.png, output/playwright/cli-session-pets-hidden.png, output/playwright/cli-session-pets-idle.png");
} finally {
  await browser.close();
}
