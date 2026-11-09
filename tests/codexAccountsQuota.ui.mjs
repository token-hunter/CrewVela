import assert from "node:assert/strict";
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = []; page.on("pageerror", error => errors.push(error.message));
  await page.goto(new URL("/tests/fixtures/codex-accounts-ui.html", process.env.CREWVELA_UI_URL || "http://127.0.0.1:1421").href);
  await page.locator(".cid-account-row").nth(4).waitFor();
  const labels = await page.locator(".cid-status").allTextContents();
  assert.deepEqual(labels, ["额度用尽", "额度用尽", "额度紧张", "额度未知", "可用"]);
  await page.getByRole("button", { name: "显示账户 1信息" }).click();
  await page.getByText("fixture-0@example.com", { exact: true }).waitFor();
  await page.getByRole("button", { name: "隐藏账户 1信息" }).click();
  assert.equal(await page.getByText("fixture-0@example.com", { exact: true }).count(), 0);
  assert.deepEqual(errors, []);
  mkdirSync("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/codex-account-quotas.png", fullPage: true });
  console.log("PASS: account quota labels honor both windows, and reveal/hide still works (Mock IPC)");
} finally { await browser.close(); }
