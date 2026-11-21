import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 940 }, reducedMotion: "no-preference" });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(() => localStorage.setItem("crewvela-appearance", JSON.stringify({ theme: "dark", accent: "clay" })));
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html?bulk-actions=stopped", base).href);
  await page.waitForFunction(() => window.appSessionMock);
  await page.evaluate(() => {
    const app = window.appSessionMock;
    const invoke = app.invoke.bind(app);
    const motion = window.motionMock = { pending: [], sent: 0 };
    app.setSessions(app.bulkSessions().filter(item => ["one", "two", "other"].includes(item.id)).map((item, index) => ({
      ...item, title: ["Claude Code", "Codex CLI", "Other workspace"][index],
      agentRuntimeKind: index === 0 ? "claude_code" : "codex", status: "running", ptyId: index + 1, updatedAt: Date.now(),
    })));
    app.invoke = async (command, args) => {
      const value = await invoke(command, args);
      if (command !== "pty_poll_all") return value;
      const communicationDeliveries = motion.pending.splice(0);
      motion.sent += communicationDeliveries.length;
      return { ...value, communicationDeliveries };
    };
  });

  const grid = page.locator(".main-terminal-host:not([hidden]) > .grid");
  const flight = id => grid.locator(`[data-message-id="${id}"]`);
  async function deliver(id, fromSessionId = "one", toSessionId = "two", count = 1) {
    const expected = await page.evaluate(({ id, fromSessionId, toSessionId, count }) => {
      const mock = window.motionMock;
      for (let i = 0; i < count; i++) mock.pending.push({ id, fromSessionId, toSessionId });
      return mock.sent + count;
    }, { id, fromSessionId, toSessionId, count });
    await page.waitForFunction(expected => window.motionMock.sent >= expected, expected);
  }
  async function appearance() {
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await page.locator(".settings-nav").getByRole("button", { name: "外观", exact: true }).click();
    return page.getByRole("switch", { name: /会话通信动效/ });
  }
  async function closeSettings() {
    await page.keyboard.press("Escape");
    await page.locator(".settings-dialog").waitFor({ state: "hidden" });
  }

  await grid.getByText("Claude Code", { exact: true }).first().waitFor();
  assert.equal(await grid.locator(".pane:not(.pane-hidden)").count(), 2);
  await deliver("default-off");
  assert.equal(await grid.locator(".terminal-communication-motion").count(), 0);

  const toggle = await appearance();
  assert.equal(await toggle.isChecked(), false);
  assert.equal(await toggle.evaluate(input => {
    const row = input.closest("section");
    return Boolean(row.previousElementSibling?.querySelector(".appearance-accent-section"))
      && row.nextElementSibling?.classList.contains("appearance-terminal-section");
  }), true);
  await toggle.press("Space");
  assert.equal(await toggle.isChecked(), true);
  await page.getByRole("radio", { name: "紫色", exact: true }).click();
  await closeSettings();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("crewvela-appearance")).communicationMotion === true);

  await deliver("outgoing", "one", "two", 2);
  const outgoing = flight("outgoing");
  const path = outgoing.locator("path");
  await path.waitFor();
  assert.equal(await outgoing.count(), 1);
  assert.equal(await path.evaluate(el => getComputedStyle(el).stroke), "rgb(155, 122, 240)");
  const coords = (await path.getAttribute("d")).match(/-?[\d.]+/g).map(Number);
  assert.ok(coords[0] < coords[6] && coords[3] < coords[1], "forward path goes left to right and bends above the endpoints");
  const moves = await outgoing.locator(".terminal-communication-light").evaluate(async element => {
    const before = element.getCTM().e;
    await new Promise(resolve => setTimeout(resolve, 105));
    return element.getCTM().e - before;
  });
  assert.ok(moves > 5, `light should move along the path, moved ${moves}px`);
  await page.waitForFunction(() => {
    const receiver = document.querySelector('[data-message-id="outgoing"] .terminal-communication-receiver');
    return receiver && Number(getComputedStyle(receiver).opacity) > 0.3;
  }, null, { timeout: 2000 });
  await outgoing.waitFor({ state: "detached" });

  await deliver("reply", "two", "one");
  const reply = flight("reply");
  await reply.locator("path").waitFor();
  const back = (await reply.locator("path").getAttribute("d")).match(/-?[\d.]+/g).map(Number);
  assert.ok(back[0] > back[6] && back[3] > back[1], "reply reverses direction and bends below the endpoints");
  await grid.getByRole("button", { name: "最大化会话", exact: true }).first().click();
  await reply.waitFor({ state: "detached" });
  await deliver("only-one-visible");
  assert.equal(await grid.locator(".terminal-communication-motion").count(), 0);
  await grid.getByRole("button", { name: "还原会话", exact: true }).click();
  await deliver("cross-workspace", "one", "other");
  assert.equal(await flight("cross-workspace").count(), 0);

  await page.emulateMedia({ reducedMotion: "reduce" });
  await deliver("reduced-motion");
  assert.equal(await flight("reduced-motion").count(), 0);
  await page.emulateMedia({ reducedMotion: "no-preference" });

  await appearance();
  await page.getByRole("radio", { name: "无强调色", exact: true }).click();
  await closeSettings();
  await deliver("neutral");
  const neutral = flight("neutral").locator("path");
  await neutral.waitFor();
  assert.equal(await neutral.evaluate(el => getComputedStyle(el).stroke), "rgb(201, 201, 198)");

  const again = await appearance();
  await page.getByText("会话通信动效", { exact: true }).click();
  assert.equal(await again.isChecked(), false);
  await closeSettings();
  await deliver("disabled-again");
  assert.equal(await grid.locator(".terminal-communication-motion").count(), 0);
  assert.deepEqual(errors, []);
  console.log("PASS: visual movement in both directions, accent linkage, default off, deduplication and visible terminal scope");
} finally {
  await browser.close();
}
