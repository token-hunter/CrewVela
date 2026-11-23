import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1432";
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
const presets = [
  ["深色", "dark"], ["暖夜黑", "ocean"], ["石墨黑", "purple"],
  ["浅色", "light"], ["暖沙", "sand"], ["暖瓷白", "aurora"],
];

async function checkPalette(host, surfaces) {
  const colors = await host.evaluate((element, samples) => {
    const probe = document.createElement("span");
    probe.hidden = true;
    document.body.append(probe);
    try {
      return samples.map(([selector, property, token]) => {
        const surface = element.querySelector(selector);
        probe.style.color = `var(${token})`;
        return [selector, surface && getComputedStyle(surface)[property], getComputedStyle(probe).color];
      });
    } finally {
      probe.remove();
    }
  }, surfaces);
  for (const [selector, actual, expected] of colors) assert.equal(actual, expected, selector);
  return colors[0][1];
}

try {
  const context = await browser.newContext({ viewport: { width: 1500, height: 900 }, colorScheme: "dark" });
  const app = await context.newPage();
  const detached = await context.newPage();
  const errors = [];
  for (const page of [app, detached]) page.on("pageerror", error => errors.push(error.message));
  await app.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
  await app.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  const workPanel = app.getByRole("complementary", { name: "右侧栏", exact: true });
  await workPanel.getByRole("button", { name: "多智能体" }).click();
  const openSettings = async () => {
    await app.locator(".unified-sidebar-settings-button").click();
    await app.getByRole("button", { name: "外观", exact: true }).click();
  };
  const choose = (label) => app.getByRole("button", { name: label, exact: true }).click();
  const closeSettings = () => app.getByRole("button", { name: "关闭设置", exact: true }).click();
  const appHost = workPanel.locator(".collaboration-control-sidebar");
  const detachedHost = detached.locator(".collaboration-control-sidebar");
  const chatSurfaces = [
    [".team-work-views", "backgroundColor", "--bg"],
    [".conversation-composer", "backgroundColor", "--bg-elev"],
    [".conversation-composer textarea", "color", "--text"],
    [".conversation-header", "borderBottomColor", "--border"],
  ];
  const waitTheme = (page, id) => page.waitForFunction(theme => document.documentElement.dataset.appearanceTheme === theme, id);
  await workPanel.getByRole("combobox", { name: "给团队发消息", exact: true }).fill("主题切换时保留这段草稿");
  await openSettings();
  await choose("暖夜黑");
  await detached.goto(`${base}/tests/fixtures/retained-control-ui.html?mode=window&preserve-storage=1`);
  await waitTheme(detached, "ocean");
  await detachedHost.locator(".conversation-composer").waitFor();

  const dockedColors = [];
  for (const [label, id] of presets) {
    await choose(label);
    await waitTheme(app, id);
    await waitTheme(detached, id);
    dockedColors.push(await checkPalette(appHost, chatSurfaces));
    assert.equal(await checkPalette(detachedHost, chatSurfaces), dockedColors.at(-1));
  }
  assert.equal(new Set(dockedColors).size, presets.length);
  await closeSettings();
  await workPanel.getByRole("button", { name: "在 Bot 模式打开", exact: true }).click();
  const rail = appHost.getByRole("navigation", { name: "协作项目列表" });
  await rail.waitFor();
  await openSettings();
  for (const [index, [label, id]] of presets.entries()) {
    await choose(label);
    await waitTheme(app, id);
    assert.equal(await checkPalette(appHost, [
      ...chatSurfaces,
      [".collaboration-project-navigation", "backgroundColor", "--bg-sidebar"],
    ]), dockedColors[index]);
  }
  await closeSettings();
  assert.equal(await workPanel.getByRole("combobox", { name: "给团队发消息", exact: true }).inputValue(), "主题切换时保留这段草稿");
  await workPanel.getByRole("tab", { name: "任务执行", exact: true }).click();
  await appHost.locator(".team-task-list").waitFor();
  await checkPalette(appHost, [
    [".team-tasks-panel", "backgroundColor", "--bg"],
    [".team-task-list", "backgroundColor", "--bg-elev"],
  ]);
  await openSettings();
  await app.getByRole("switch", { name: /跟随系统/ }).press("Space");
  await waitTheme(app, "dark");
  await waitTheme(detached, "dark");
  await app.emulateMedia({ colorScheme: "light" });
  await detached.emulateMedia({ colorScheme: "light" });
  await waitTheme(app, "light");
  await waitTheme(detached, "light");
  assert.equal(await checkPalette(detachedHost, chatSurfaces), dockedColors[3]);
  assert.deepEqual(errors, []);
  console.log("PASS: 六套主题联动辅助栏、Bot 模式和独立窗口，支持实时切换、跟随系统并保留草稿");
} finally {
  await browser.close();
}
