// Real App/hook with isolated native-window IPC. Native geometry is additionally
// verified by .scratch/fullscreen-black-strip-20261116/measure-window.ps1.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const browser=await chromium.launch({channel:'chrome'}),errors=[];
async function fresh(maximized=true,platform='Win32'){
  const page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(10000);
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(value=>Object.defineProperty(navigator,'platform',{get:()=>value}),platform);
  await page.goto(`${base}/tests/fixtures/app-session-async-ui.html?app-help=1`);
  await page.getByRole('button',{name:'更多工具',exact:true}).waitFor();
  await page.evaluate(value=>window.appSessionMock.fullscreen.maximized=value,maximized);
  return page;
}
async function menu(page){
  await page.getByRole('button',{name:'更多工具',exact:true}).click();
  await page.getByRole('menuitem',{name:'视图',exact:true}).click();
  return page.getByRole('menu',{name:'视图',exact:true});
}
async function toggle(page,name='全屏'){
  const m=await menu(page);await m.getByRole('menuitem',{name,exact:true}).click();
}
const state=page=>page.evaluate(()=>({...window.appSessionMock.fullscreen,pending:window.appSessionMock.fullscreen.pending.length}));
try{
  const maximized=await fresh();await toggle(maximized);await maximized.waitForFunction(()=>window.appSessionMock.fullscreen.value);
  let current=await state(maximized);
  assert.equal(current.maximized,false,'Regression: maximized Win32 client area must not remain taskbar-sized in fullscreen');
  assert.deepEqual(current.transitions,['unmaximize','fullscreen:true']);
  await toggle(maximized,'退出全屏');await maximized.waitForFunction(()=>!window.appSessionMock.fullscreen.value&&window.appSessionMock.fullscreen.maximized);
  assert.deepEqual((await state(maximized)).transitions,['unmaximize','fullscreen:true','fullscreen:false','maximize']);
  await maximized.close();

  const normal=await fresh(false);await toggle(normal);await normal.waitForFunction(()=>window.appSessionMock.fullscreen.value);
  await toggle(normal,'退出全屏');await normal.waitForFunction(()=>!window.appSessionMock.fullscreen.value);
  assert.equal((await state(normal)).maximized,false);assert.deepEqual((await state(normal)).transitions,['fullscreen:true','fullscreen:false']);await normal.close();

  const rejected=await fresh();await rejected.evaluate(()=>window.appSessionMock.fullscreen.fail=true);await toggle(rejected);
  await rejected.getByRole('alert').filter({hasText:'切换全屏失败'}).waitFor();
  current=await state(rejected);assert.equal(current.value,false);assert.equal(current.maximized,true,'Failed entry restores previous maximized state');
  await rejected.evaluate(()=>window.appSessionMock.fullscreen.fail=false);await toggle(rejected);await rejected.waitForFunction(()=>window.appSessionMock.fullscreen.value);
  assert.equal((await state(rejected)).maximized,false);await rejected.close();

  const unmaximizeFailed=await fresh();await unmaximizeFailed.evaluate(()=>window.appSessionMock.fullscreen.unmaximizeFails=true);await toggle(unmaximizeFailed);
  await unmaximizeFailed.getByRole('alert').filter({hasText:'切换全屏失败'}).waitFor();current=await state(unmaximizeFailed);
  assert.equal(current.value,false);assert.equal(current.maximized,true);assert.equal(current.calls.length,0,'Do not enter broken fullscreen when unmaximize fails');await unmaximizeFailed.close();

  const inflight=await fresh();await inflight.evaluate(()=>window.appSessionMock.fullscreen.hold=true);await toggle(inflight);
  await inflight.waitForFunction(()=>window.appSessionMock.fullscreen.pending.length===1);
  const pendingMenu=await menu(inflight);assert.equal(await pendingMenu.getByRole('menuitem',{name:'全屏',exact:true}).isDisabled(),true);
  assert.equal((await state(inflight)).calls.length,1);await inflight.evaluate(()=>window.appSessionMock.fullscreen.complete());
  await pendingMenu.getByRole('menuitem',{name:'退出全屏',exact:true}).waitFor();await inflight.close();

  const rollbackFailed=await fresh();await rollbackFailed.evaluate(()=>{window.appSessionMock.fullscreen.fail=true;window.appSessionMock.fullscreen.maximizeFails=true;});
  await toggle(rollbackFailed);await rollbackFailed.getByRole('alert').filter({hasText:'切换全屏失败'}).waitFor();
  assert.match(await rollbackFailed.getByRole('alert').filter({hasText:'切换全屏失败'}).innerText(),/恢复最大化失败/);await rollbackFailed.close();
  for(const platform of ['MacIntel','Linux x86_64']){
    const other=await fresh(true,platform);await toggle(other);await other.waitForFunction(()=>window.appSessionMock.fullscreen.value);
    await toggle(other,'退出全屏');await other.waitForFunction(()=>!window.appSessionMock.fullscreen.value);
    assert.deepEqual((await state(other)).transitions,['fullscreen:true','fullscreen:false']);await other.close();
  }
  assert.deepEqual(errors,[]);console.log('PASS: maximized/normal fullscreen transitions, restoration, failure/retry/rollback errors, in-flight guard and unchanged macOS/Linux routing');
}finally{await browser.close();}
