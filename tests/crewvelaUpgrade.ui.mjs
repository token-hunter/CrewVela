import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", error => pageErrors.push(error.message));
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("crewvela-storage-retry-fixture")) {
      sessionStorage.setItem("crewvela-storage-retry-fixture", "ready");
      Object.defineProperty(window, "localStorage", {
        configurable: true,
        get() { throw new DOMException("Fixture storage unavailable", "SecurityError"); },
      });
    }
  });
  await page.goto(`${base}/?ui-preview=1`);
  await page.getByRole("button", { name: "重试加载", exact: true }).waitFor({ state: "visible" });
  assert.match(await page.getByRole("alert").innerText(), /原有数据已保留/);
  assert.equal(await page.getByRole("button", { name: "设置", exact: true }).count(), 0);
  await page.getByRole("button", { name: "重试加载", exact: true }).click();
  await page.getByRole("button", { name: "设置", exact: true }).waitFor({ state: "visible" });
  assert.match(await page.title(), /CrewVela/);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.locator(".settings-nav").getByRole("button", { name: "共享记忆", exact: true }).click();
  await page.getByRole("button", { name: "停止服务", exact: true }).waitFor({ state: "visible" });
  assert.deepEqual(pageErrors, []);
  console.log("PASS: unavailable storage blocks App initialization, retry starts the main view, and the renamed managed-service contract renders correctly");
  await context.close();
} finally {
  await browser.close();
}
