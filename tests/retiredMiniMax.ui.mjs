import assert from "node:assert/strict";
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const url = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  page.setDefaultTimeout(8000);
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => 1,
      invoke: async command => {
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        throw Error("Unexpected native call: " + command);
      },
    };
  });
  mkdirSync(".scratch/minimax-retirement", { recursive: true });
  const errors = [];
  page.on("pageerror", e => { errors.push(e.message); console.error(e.message); });
  await page.goto(new URL("/tests/fixtures/retired-minimax.html?ui-preview=1", url).href);
  const toggle = page.getByRole("checkbox", { name: /启用成员/ });
  await toggle.waitFor();
  assert.equal(await toggle.isDisabled(), true);
  assert.equal(await toggle.isChecked(), false);
  const runtime = page.getByRole("combobox", { name: "Runtime", exact: true });
  assert.equal(await runtime.inputValue(), "minimax_code");
  assert.equal(await runtime.locator('option[value="minimax_code"]').evaluate(option => option.disabled), true);
  await page.screenshot({ path: ".scratch/minimax-retirement/legacy-member.png" });
  await runtime.selectOption("codex");
  await toggle.check();
  await page.getByRole("button", { name: "保存成员", exact: true }).click();
  const saved = JSON.parse(await page.locator("output").innerText());
  assert.equal(saved.runtimeKind, "codex");
  assert.equal(saved.enabled, true);
  await page.goto(new URL("/tests/fixtures/retired-minimax.html?ui-preview=1&mode=manage", url).href);
  assert.equal(await page.getByRole("checkbox").count(), 2);
  assert.equal(await page.getByRole("checkbox", { name: /停用|MiniMax/ }).count(), 0);
  console.log("PASS: legacy edit and management controls");
  await page.goto(new URL("/?ui-preview=1", url).href);
  await page.locator(".sidebar-new-session-button").waitFor();
  await page.locator(".sidebar-new-session-button").click();
  const menu = page.getByRole("menu").filter({ has: page.getByRole("menuitem", { name: /Kimi Code/ }) });
  await menu.waitFor();
  assert.equal(await menu.getByRole("menuitem", { name: /MiniMax|MCode/ }).count(), 0);
  await page.screenshot({ path: ".scratch/minimax-retirement/new-menu.png" });
  assert.deepEqual(errors, []);
  console.log("PASS: legacy member is disabled, can migrate to Codex, and stale management/new-session entries cannot expose the removed CLI");
} finally { await browser.close(); }
