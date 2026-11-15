import assert from "node:assert/strict";
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
const browser = await chromium.launch({ channel: "chrome", headless: true });
const luminance = color => {
  const [r,g,b] = color.match(/[\d.]+/g).slice(0,3).map(Number).map(n => { const s=n/255; return s<=0.04045?s/12.92:((s+0.055)/1.055)**2.4; });
  return .2126*r+.7152*g+.0722*b;
};
const ratio = (a,b) => (Math.max(luminance(a),luminance(b))+.05)/(Math.min(luminance(a),luminance(b))+.05);
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.setDefaultTimeout(10000);
  const errors=[]; page.on("pageerror", error=>errors.push(error.message));
  await page.addInitScript(() => {
    if (!localStorage.getItem("crewvela-appearance")) localStorage.setItem("crewvela-appearance", JSON.stringify({theme:"aurora", background:"solid",accent:"none",followSystem:false,terminalOpacity:.65}));
  });
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1436"}/tests/fixtures/app-session-async-ui.html`);
  await page.waitForFunction(()=>document.documentElement.dataset.appearanceTheme==="aurora"&&document.documentElement.dataset.theme==="light");
  await page.addStyleTag({content:"*,*::before,*::after{transition:none!important;animation:none!important;}"});
  const openAppearance = async () => {
    await page.getByRole("button",{name:"设置",exact:true}).click();
    await page.locator(".settings-nav").getByRole("button",{name:"外观",exact:true}).click();
  };
  await openAppearance();
  assert.equal(await page.locator(".appearance-follow-system small").innerText(), "浅色");
  assert.deepEqual(await page.locator(".appearance-theme-option > strong").allTextContents(), ["深色","深海","石墨黑","浅色","暖沙","暖瓷白"]);
  assert.equal(await page.getByRole("button",{name:"极光",exact:true}).count(),0);
  assert.equal(await page.getByRole("button",{name:"暖瓷白",exact:true}).getAttribute("aria-pressed"),"true");
  assert.equal(await page.getByRole("tab",{name:"皮肤",exact:true}).count(),0);
  const colors = await page.evaluate(()=>{
    const frame=document.querySelector(".settings-native"), nav=frame.querySelector(".settings-nav"), label=nav.querySelector(".settings-nav-group-label");
    return {bg:getComputedStyle(frame).backgroundColor,text:getComputedStyle(frame).color,nav:getComputedStyle(nav).backgroundColor,secondary:getComputedStyle(label).color,scheme:getComputedStyle(frame).colorScheme};
  });
  assert.equal(colors.bg,"rgb(253, 250, 247)"); assert.equal(colors.nav,"rgb(235, 229, 224)"); assert.equal(colors.scheme,"light");
  assert.ok(ratio(colors.text,colors.bg)>=4.5); assert.ok(ratio(colors.secondary,colors.nav)>=4.5);
  for(const label of ["陶土","紫色","青色","橙色","粉色","珊瑚色","灰色","无强调色"]) {
    await page.getByRole("radio",{name:label,exact:true}).click();
    assert.equal(await page.locator(".settings-native").evaluate(el=>getComputedStyle(el).backgroundColor),colors.bg);
    assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem("crewvela-appearance")).theme),"aurora");
  }
  await page.locator(".settings-content").evaluate(el=>{el.scrollTop=0;});
  mkdirSync("output/playwright",{recursive:true});
  await page.screenshot({path:"output/playwright/warm-porcelain-actual.png",fullPage:true});
  for(const width of [820,687]) {
    await page.setViewportSize({width,height:800});
    assert.equal(await page.locator(".settings-content").evaluate(el=>el.scrollWidth>el.clientWidth+1),false);
  }
  await page.getByRole("button",{name:"关闭设置",exact:true}).click();
  await page.reload();
  await page.waitForFunction(()=>document.documentElement.dataset.theme==="light"&&document.documentElement.dataset.appearanceTheme==="aurora");
  await openAppearance();
  assert.equal(await page.getByRole("button",{name:"暖瓷白",exact:true}).getAttribute("aria-pressed"),"true");
  assert.deepEqual(errors,[]);
  console.log(`PASS: old aurora loads warm light theme; three dark/three light; all 8 accents independent; primary contrast ${ratio(colors.text,colors.bg).toFixed(2)}, navigation secondary ${ratio(colors.secondary,colors.nav).toFixed(2)}; narrow layout and reload persistence`);
} finally { await browser.close(); }
