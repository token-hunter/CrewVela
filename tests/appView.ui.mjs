import assert from "node:assert/strict";
import { chromium } from "playwright";
import { mkdirSync, writeFileSync } from "node:fs";

const base=process.env.CREWVELA_UI_URL||"http://127.0.0.1:1437";
const out=".scratch/view-menu-dedup-20261116";mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:"chrome",headless:true});
const errors=[],results=[];
async function openView(page) {
  await page.getByRole("button",{name:"更多工具",exact:true}).click();
  await page.getByRole("menu",{name:"更多工具",exact:true}).getByRole("menuitem",{name:"视图",exact:true}).click();
  return page.getByRole("menu",{name:"视图",exact:true});
}
async function viewPanel(page,name) {
  const sidebar=page.getByRole("complementary",{name:"右侧栏",exact:true});
  if (!await sidebar.isVisible()) await page.getByRole("button",{name:"切换右侧栏",exact:true}).click();
  const label={"文件":"文件","Git 更改":"Git","浏览器":"浏览器","终端":"PowerShell","多智能体":"多智能体"}[name];
  const tab=sidebar.getByRole("tab",{name:label,exact:true});
  if (await tab.count()) { await tab.first().click(); return; }
  await sidebar.getByRole("button",{name:"新建工作面板标签",exact:true}).click();
  const tool={"文件":"工作区文件","Git 更改":"源代码控制","浏览器":"浏览器","终端":"新建 PowerShell 终端","多智能体":"多智能体"}[name];
  await sidebar.getByRole("menuitem",{name:tool,exact:true}).click();
}
try {
 for(const theme of ["dark","light"]) {
  const page=await browser.newPage({viewport:{width:1500,height:1000}});
  page.setDefaultTimeout(10000);
  page.on("pageerror",error=>errors.push(error.message));
  await page.addInitScript(theme=>{Object.defineProperty(navigator,"platform",{get:()=>"Win32"});localStorage.setItem("crewvela-appearance",JSON.stringify({theme,accent:"purple",followSystem:false}));},theme);
  await page.addInitScript(()=>{
    const callbacks=new Map();let next=0;
    window.viewPortCalls=[];
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:"main"},currentWebview:{label:"main"}},
      transformCallback:fn=>{callbacks.set(++next,fn);return next;},
      invoke:async(command,args)=>{
        window.viewPortCalls.push({command,args});
        if(["stop_session","delete_session","browser_panel_close","launch_terminal"].includes(command))throw Error(`view must not invoke ${command}`);
        if(command==="plugin:window|is_fullscreen")return false;
        if(command==="plugin:event|listen")return next;
        if(command==="workspace_list_directory")return [];
        if(command==="get_runtime_preferences")return {disabledRuntimes:[]};
        if(command.startsWith("ma_list_") || command==="ma_probe_runtimes")return [];
        if(command==="browser_panel_current_url")return "";
        return null;
      }};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:(_event,id)=>callbacks.delete(id)};
  });
  await page.goto(`${base}/?ui-preview=1`);
  let menu=await openView(page);
  assert.deepEqual(await menu.getByRole("menuitem").allTextContents(),["全屏"]);
  assert.equal(await menu.getByRole("menuitemcheckbox").count(),0);
  assert.equal(await menu.getByRole("separator").count(),0);
  await page.keyboard.press("Escape");await page.keyboard.press("Escape");
  await page.getByRole("button",{name:"收起侧栏",exact:true}).click();
  await page.locator(".app-shell.sidebar-collapsed").waitFor();
  menu=await openView(page);
  assert.deepEqual(await menu.getByRole("menuitem").allTextContents(),["全屏"]);
  await page.keyboard.press("Escape");await page.keyboard.press("Escape");
  await page.getByRole("button",{name:"展开侧栏",exact:true}).click();
  await page.locator(".app-shell:not(.sidebar-collapsed)").waitFor();

  const sidebar=page.getByRole("complementary",{name:"右侧栏",exact:true});
  for(const [name,label] of [["文件","文件"],["Git 更改","Git"],["浏览器","浏览器"],["终端","PowerShell"],["多智能体","多智能体"]]) {
    await viewPanel(page,name);
    const tab=sidebar.getByRole("tab",{name:label,exact:true});await tab.waitFor();
    assert.equal(await tab.count(),1);
    if(name==="浏览器") {
      const address=sidebar.locator(".right-sidebar-panel.active").getByRole("textbox",{name:"地址栏",exact:true});
      await address.fill("https://example.com/view-menu");await address.press("Enter");
      await page.waitForFunction(()=>window.viewPortCalls.some(call=>call.command==="browser_panel_open"));
      const browserMenu=await openView(page);
      await page.waitForFunction(()=>window.viewPortCalls.some(call=>call.command==="browser_panel_set_visible"&&call.args.visible===false));
      await page.keyboard.press("Escape");await page.keyboard.press("Escape");
      assert.equal(await address.inputValue(),"https://example.com/view-menu");
    }
    const active=await sidebar.locator(".right-sidebar-panel.active").elementHandle();
    const total=await sidebar.getByRole("tab").count();
    await viewPanel(page,name);
    assert.equal(await sidebar.getByRole("tab").count(),total,`${name}: repeated viewing reuses tabs`);
    await page.getByRole("button",{name:"切换右侧栏",exact:true}).click();
    assert.equal(await active.evaluate(el=>el.isConnected),true,`${name}: hide keeps its panel mounted`);
    await page.getByRole("button",{name:"切换右侧栏",exact:true}).click();
    assert.equal(await active.evaluate(el=>el.classList.contains("active")),true,`${name}: toggle restores the existing instance`);
    assert.equal(await tab.count(),1);
  }
  await sidebar.getByRole("button",{name:"收起工作面板",exact:true}).click();
  // Original toolbar/tab controls remain available after duplicate menu entries are removed.
  await viewPanel(page,"文件");
  await sidebar.getByRole("button",{name:"新建工作面板标签",exact:true}).click();
  await sidebar.getByRole("menuitem",{name:"工作区文件",exact:true}).click();
  const fileTabs=sidebar.getByRole("tab",{name:"文件",exact:true});assert.equal(await fileTabs.count(),2);
  await fileTabs.nth(0).click();await viewPanel(page,"文件");
  assert.equal(await fileTabs.nth(0).getAttribute("aria-selected"),"true");assert.equal(await fileTabs.count(),2);
  await sidebar.getByRole("button",{name:"收起工作面板",exact:true}).click();
  await page.getByRole("button",{name:"聚焦",exact:true}).click();
  await page.waitForFunction(()=>document.querySelectorAll(".terminal-wrap .grid > .pane:not(.pane-hidden)").length===1);
  await page.getByRole("button",{name:"总览",exact:true}).click();
  await page.waitForFunction(()=>document.querySelectorAll(".terminal-wrap .grid > .pane:not(.pane-hidden)").length>1);

  // Keyboard navigation descends/returns one level; no keys leak into terminals.
  await page.getByRole("button",{name:"更多工具",exact:true}).click();
  const root=page.getByRole("menu",{name:"更多工具",exact:true});
  await root.getByRole("menuitem",{name:"视图",exact:true}).focus();await page.keyboard.press("ArrowRight");
  menu=page.getByRole("menu",{name:"视图",exact:true});
  const fullscreen=menu.getByRole("menuitem",{name:"全屏",exact:true});
  assert.equal(await fullscreen.evaluate(el=>el===document.activeElement),true);
  await page.keyboard.press("End");assert.equal(await fullscreen.evaluate(el=>el===document.activeElement),true);
  await page.keyboard.press("ArrowRight");assert.equal(await page.locator(".app-view-branch").count(),0);
  await page.keyboard.press("ArrowLeft");assert.equal(await menu.count(),0);
  assert.equal(await root.getByRole("menuitem",{name:"视图",exact:true}).evaluate(el=>el===document.activeElement),true);
  await page.keyboard.press("Escape");assert.equal(await root.count(),0);
  await openView(page);await page.mouse.click(1400,950);assert.equal(await root.count(),0);
  for(const size of [{width:1500,height:1000},{width:820,height:540},{width:390,height:280}]) {
    await page.setViewportSize(size);menu=await openView(page);
    const geometry=await page.locator(".app-tools-menu,.app-help-submenu").evaluateAll(items=>items.map(el=>({box:el.getBoundingClientRect().toJSON(),scrollHeight:el.scrollHeight,clientHeight:el.clientHeight})));
    for(const {box} of geometry)assert.ok(box.x>=0&&box.y>=0&&box.right<=size.width+1&&box.bottom<=size.height+1,JSON.stringify(box));
    await page.screenshot({path:`${out}/menu-${theme}-${size.width}.png`});
    await page.keyboard.press("Escape");await page.keyboard.press("Escape");
  }
  assert.equal(await page.evaluate(()=>window.viewPortCalls.some(({command})=>["stop_session","delete_session","browser_panel_close","launch_terminal"].includes(command))),false);
  results.push({theme,panels:5,viewportCases:3});await page.close();
 }

 const empty=await browser.newPage({viewport:{width:1000,height:720}});
 await empty.goto(`${base}/?ui-preview=1&sessions=empty`);
 let emptyMenu=await openView(empty);assert.deepEqual(await emptyMenu.getByRole("menuitem").allTextContents(),["全屏"]);
 await empty.keyboard.press("Escape");await empty.keyboard.press("Escape");
 // Browser preview uses the real browser fullscreen API rather than CSS maximization.
 emptyMenu=await openView(empty);await emptyMenu.getByRole("menuitem",{name:"全屏",exact:true}).click();
 await empty.waitForFunction(()=>Boolean(document.fullscreenElement));
 emptyMenu=await openView(empty);await emptyMenu.getByRole("menuitem",{name:"退出全屏",exact:true}).click();
 await empty.waitForFunction(()=>!document.fullscreenElement);await empty.close();

 const chat=await browser.newPage({viewport:{width:1100,height:800}});
 await chat.goto(`${base}/?ui-preview=1&harness=empty`);
 const chatMenu=await openView(chat);assert.deepEqual(await chatMenu.getByRole("menuitem").allTextContents(),["全屏"]);
 assert.equal(await chatMenu.getByRole("menuitemradio").count(),0);
 await chat.close();

 // Real Tauri API calls, with an isolated native-window port in the existing App fixture.
 const native=await browser.newPage({viewport:{width:1440,height:900}});
 native.on("pageerror",error=>errors.push(error.message));
 await native.goto(`${base}/tests/fixtures/app-session-async-ui.html?app-help=1`);
 let menu=await openView(native);
 await native.evaluate(()=>window.appSessionMock.fullscreen.hold=true);
 await menu.getByRole("menuitem",{name:"全屏",exact:true}).click();
 await native.waitForFunction(()=>window.appSessionMock.fullscreen.pending.length===1);
 menu=await openView(native);assert.equal(await menu.getByRole("menuitem",{name:"全屏",exact:true}).isDisabled(),true);
 assert.equal(await native.evaluate(()=>window.appSessionMock.fullscreen.calls.length),1);
 await native.evaluate(()=>window.appSessionMock.fullscreen.complete());
 await menu.getByRole("menuitem",{name:"退出全屏",exact:true}).waitFor();
 await native.evaluate(()=>window.appSessionMock.fullscreen.hold=false);
 await menu.getByRole("menuitem",{name:"退出全屏",exact:true}).click();
 await native.waitForFunction(()=>!window.appSessionMock.fullscreen.value);
 assert.deepEqual(await native.evaluate(()=>window.appSessionMock.fullscreen.calls),[{label:"main",value:true},{label:"main",value:false}]);
 await native.evaluate(()=>window.appSessionMock.fullscreen.external(true));
 menu=await openView(native);await menu.getByRole("menuitem",{name:"退出全屏",exact:true}).waitFor();
 await native.evaluate(()=>window.appSessionMock.fullscreen.external(false));
 await menu.getByRole("menuitem",{name:"全屏",exact:true}).waitFor();
 await native.evaluate(()=>window.appSessionMock.fullscreen.fail=true);
 await menu.getByRole("menuitem",{name:"全屏",exact:true}).click();
 await native.getByRole("alert").filter({hasText:"切换全屏失败"}).waitFor();
 assert.equal(await native.evaluate(()=>window.appSessionMock.fullscreen.value),false);
 await native.close();
 assert.deepEqual(errors,[]);writeFileSync(`${out}/ui-results.json`,JSON.stringify({results,errors},null,2));
 console.log("PASS: duplicate view menu entries removed, original sidebar/panel/session controls preserved, dark/light, keyboard/outside click, 3 viewport sizes and native fullscreen IPC/state/failure/in-flight guard");
} finally {await browser.close();}
