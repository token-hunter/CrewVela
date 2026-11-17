// Selected visual direction: library and its footer left, configuration/actions right.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1455";
const out = ".scratch/quick-commands-option-one";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const checks = [], errors = [];
const fixture = "/tests/fixtures/quick-commands-ui.html?ui-preview=1&open=0&settings=1&design=1";
try {
  for (const [width, height, theme] of [[1500,1050,"dark"], [1500,1050,"light"], [820,540,"dark"], [390,844,"light"]]) {
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
    page.on("pageerror", e => errors.push(e.message));
    await page.goto(base + fixture + `&theme=${theme}`);
    await page.getByRole("button", { name: "启动 Pi 本地配置", exact: true }).waitFor();
    await page.getByLabel("范围", { exact: true }).selectOption("project");
    for (const i of [0,2,4]) await page.locator(".quick-center-targets input").nth(i).check();
    const editor = page.getByRole("textbox", { name: "消息内容", exact: true });
    await editor.focus();
    const m = await page.evaluate(() => {
      const rect = s => { const r = document.querySelector(s).getBoundingClientRect(); return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}; };
      const a = [...document.querySelectorAll(".quick-center-targets > label")].slice(0,2).map(e => {const r=e.getBoundingClientRect();return{x:r.x,y:r.y}});
      return { dialog:rect(".settings-dialog"), library:rect(".quick-center-library"), tabs:rect(".quick-center-tabs"), detail:rect(".quick-center-detail"), leftFooter:rect(".quick-center-library-footer"), footer:rect(".quick-center-footer"), scope:rect(".quick-center-scope-row > label"), directory:rect(".quick-center-directory"), firstTargets:a, overflow:document.querySelector(".quick-center").scrollWidth > document.querySelector(".quick-center").clientWidth+1 };
    });
    assert.equal(m.overflow, false, `${width}x${height}: ${JSON.stringify(m)}`);
    assert.ok(await page.getByRole('tab', {name:'全部',exact:true}).evaluate(el => el.getBoundingClientRect().height < 55), "分类文字不逐字换行");
    assert.ok(m.dialog.x >= 0 && m.dialog.right <= width && m.dialog.bottom <= height);
    if (width > 1000) {
      assert.ok(m.tabs.right <= m.library.right && m.detail.x >= m.library.right);
      assert.ok(Math.abs(m.leftFooter.y-m.footer.y)<2, "两栏底部操作区对齐");
      assert.ok(Math.abs(m.scope.y-m.directory.y)<15 && m.directory.x>m.scope.right, "范围和目录同一行");
      assert.ok(Math.abs(m.firstTargets[0].y-m.firstTargets[1].y)<2 && m.firstTargets[1].x>m.firstTargets[0].x, "目标以双列展示");
      assert.ok(m.footer.bottom<=m.dialog.bottom);
    }
    await page.screenshot({ path: `${out}/settings-${theme}-${width}.png` });
    await page.getByRole("button", { name: "确认发送", exact: true }).click();
    await page.getByRole("region", { name: "执行结果" }).getByText("内容已写入", { exact:true }).first().waitFor();
    assert.deepEqual(await page.evaluate(() => window.__quickCalls.filter(c=>c.method==="send_session_message").map(c=>c.args.sessionId)), ["design-0", "design-2", "design-4"]);
    checks.push(`${theme} ${width}x${height}: 结构、控件可达与精确三目标发送`);

    // Embedded destructive confirmation remains readable and scrollable even when short.
    await page.getByRole("button", { name: "批量停止会话 停止会话", exact:true }).click();
    await page.getByRole("button", { name: "选择此范围全部", exact:true }).click();
    await page.getByRole("button", { name: "核对批量操作", exact:true }).click();
    const confirm = page.getByRole("group", { name:"批量操作二次确认" });
    await confirm.getByRole("button", {name:"取消操作",exact:true}).click();
    assert.equal(await page.evaluate(()=>window.__quickCalls.filter(c=>c.method==="stop_session").length),0);
    await page.close();
  }
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/dual-column-result.json`,JSON.stringify({checks,errors},null,2));
  console.log(JSON.stringify({passed:checks.length,checks},null,2));
} finally {await browser.close();}
