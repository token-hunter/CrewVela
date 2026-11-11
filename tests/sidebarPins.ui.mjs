import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  await page.goto(new URL("/tests/fixtures/session-list-actions-ui.html?pins=1", process.env.CREWVELA_UI_URL || "http://localhost:1420").href);
  const projectOrder = () => page.locator('.session-project-name').allTextContents();
  const titles = () => page.locator('.session-title').allTextContents();
  const toggleSession = async (name, label) => {
    await page.getByRole('button', { name: `${name}更多操作`, exact: true }).click();
    await page.getByRole('menuitem', { name: label, exact: true }).click();
  };
  const toggleProject = async (name, label) => {
    const head = page.locator('.session-project-head').filter({ has: page.getByText(name, { exact: true }) });
    await head.hover();
    await head.getByRole('button', { name: `${label}「${name}」`, exact: true }).click();
  };
  assert.deepEqual(await projectOrder(), ['项目 A', '项目 B', '未分组']);
  await toggleSession('构建检查', '置顶会话');
  assert.equal((await titles())[0], '构建检查');
  await toggleProject('项目 B', '置顶项目');
  assert.deepEqual(await projectOrder(), ['项目 B', '项目 A', '未分组']);
  assert.equal(await page.locator('#selection-status').innerText(), 'selected:session-shell');
  await page.reload();
  assert.deepEqual(await projectOrder(), ['项目 B', '项目 A', '未分组']);
  assert.equal((await titles())[1], '构建检查');
  await toggleSession('构建检查', '收藏');
  await page.getByPlaceholder('搜索标题 / 目录 / provider').fill('接口');
  assert.deepEqual(await titles(), ['接口检查']);
  await page.getByRole('button', { name: '清空搜索' }).click();
  await page.getByRole('button', { name: '终端 1', exact: true }).click();
  assert.deepEqual(await titles(), ['临时终端']);
  await toggleSession('临时终端', '置顶会话');
  await toggleSession('临时终端', '取消置顶会话');
  await page.getByRole('button', { name: '全部 5', exact: true }).click();
  await toggleSession('构建检查', '取消置顶会话');
  assert.equal(await page.getByLabel('已置顶会话').count(), 0);
  assert.equal(await page.locator('.session-favorite-mark').count(), 1);
  await toggleSession('构建检查', '取消收藏');
  await toggleProject('项目 B', '取消置顶项目');
  assert.deepEqual(await projectOrder(), ['项目 A', '项目 B', '未分组']);
  await page.getByRole('button', { name: '折叠项目会话' }).first().click();
  assert.equal((await titles()).includes('构建检查'), false);
  await page.getByRole('button', { name: '展开项目会话' }).click();
  assert.equal((await titles())[2], '构建检查');
  await page.reload();
  assert.deepEqual(await projectOrder(), ['项目 A', '项目 B', '未分组']);
  assert.equal((await titles())[2], '构建检查');
  console.log('PASS: 置顶、取消、刷新、收藏独立、搜索、终端筛选和折叠回归');
} finally {
  await browser.close();
}
