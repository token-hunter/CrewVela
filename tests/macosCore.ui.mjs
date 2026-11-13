import assert from "node:assert/strict";
import { chromium } from "playwright";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
 for (const [platform, shells] of [["Win32", ["PowerShell", "CMD"]], ["MacIntel", ["zsh", "bash"]]]) {
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 }});
  await page.addInitScript(value => Object.defineProperty(navigator, "platform", { get: () => value }), platform);
  const errors=[]; page.on("pageerror", error => errors.push(error.message));
  await page.goto(new URL("/tests/fixtures/macos-core-ui.html?ui-preview=1", process.env.CREWVELA_UI_URL || "http://127.0.0.1:1432").href);
  await page.getByRole("button", { name: "新建工作面板标签", exact: true }).click();
  for (const shell of shells) await page.getByRole("menuitem", { name: `新建 ${shell} 终端`, exact: true }).waitFor();
  const absent = platform === "Win32" ? "zsh" : "PowerShell";
  assert.equal(await page.getByRole("menuitem", {name:`新建 ${absent} 终端`,exact:true}).count(),0);
  await page.getByRole("menuitem", {name:`新建 ${shells[1]} 终端`,exact:true}).click();
  assert.equal(await page.locator("output").textContent(),shells[1].toLowerCase());
  await page.getByRole("button", { name: "终端", exact: true }).click();
  assert.equal(await page.locator("output").textContent(),shells[0].toLowerCase());
  const keepAwake=page.locator("label").filter({hasText:"保持电脑运行"}).locator("input");
  const panel=page.locator("label").filter({hasText:"优先内置浏览器面板"}).locator("input");
  assert.equal(await keepAwake.isDisabled(),platform === "MacIntel");
  assert.equal(await panel.isDisabled(),platform === "MacIntel");
  if(platform === "MacIntel") await page.getByText("内置电脑控制暂未适配",{exact:false}).waitFor();
  else await page.getByRole("switch", {name: /^内置电脑控制/}).waitFor();
  assert.deepEqual(errors,[]);
  await page.screenshot({path:`.scratch/macos-apple-silicon-core/ui-${platform}.png`,fullPage:true});
  await page.goto(new URL("/tests/fixtures/codex-accounts-ui.html", process.env.CREWVELA_UI_URL || "http://127.0.0.1:1432").href);
  await page.locator(".cid-account-row").nth(4).waitFor();
  const restart = page.getByRole("button", {name:"重启 Codex",exact:true});
  assert.equal(await restart.isDisabled(), platform === "MacIntel");
  await page.getByRole("button", {name:"添加账户",exact:true}).click();
  assert.equal(await page.getByRole("button", {name:"打开登录窗口",exact:true}).isDisabled(), platform === "MacIntel");
  if(platform === "MacIntel") {
   await page.getByText("macOS 请在终端执行 cid add，完成后返回此页刷新登录态。",{exact:true}).waitFor();
   assert.match(await restart.getAttribute("title"),/手动重启/);
  }
  assert.deepEqual(errors,[]);
  await page.close();
 }
 console.log("Windows/macOS menus, terminal launch selection and unsupported capability display passed (browser simulation)");
} finally {await browser.close();}
