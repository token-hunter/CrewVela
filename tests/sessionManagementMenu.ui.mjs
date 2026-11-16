// Actual App/Sidebar with isolated IPC. No real sessions are stopped or deleted.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {chromium} from 'playwright';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1434',out='.scratch/session-menu-minimal-20261116/ui';
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[];
const menuFor=p=>p.getByRole('menu',{name:'会话管理',exact:true});
const confirmFor=p=>p.getByRole('dialog',{name:'删除当前工作区全部会话',exact:true});
async function fresh(mode='1',theme='dark',viewport={width:1280,height:850}) {
 const p=await browser.newPage({viewport});p.setDefaultTimeout(10000);p.on('pageerror',e=>errors.push(e.message));
 await p.addInitScript(()=>localStorage.setItem('crewvela-favorite-sessions',JSON.stringify(['one'])));
 await p.goto(`${base}/tests/fixtures/app-session-async-ui.html?bulk-actions=${mode}&app-help=1&theme=${theme}&accent=purple`);
 await p.waitForFunction(count=>document.querySelector('.session-type-filters button small')?.textContent===String(count),mode==='empty'?0:4);
 if(mode==='empty')await p.waitForFunction(()=>window.appSessionMock.bulkSessions().length===1);
 return p;
}
async function open(p) {await p.locator('.session-filter-trigger').click();await menuFor(p).waitFor();return menuFor(p);}
async function remove(p) {const m=await open(p);await m.getByRole('menuitem',{name:'删除全部会话',exact:true}).click();await confirmFor(p).waitFor();return confirmFor(p);}
const calls=(p,command)=>p.evaluate(command=>window.appSessionMock.bulkCalls.filter(item=>item.command===command),command);
async function uniformRows(menu) {
 const rows=await menu.locator('button[role^="menuitem"]').evaluateAll(nodes=>nodes.map(el=>{const box=el.getBoundingClientRect(),icon=el.querySelector('svg').getBoundingClientRect(),label=el.querySelector('span').getBoundingClientRect(),style=getComputedStyle(el);return {height:box.height,center:box.y+box.height/2,labelX:label.x,iconX:icon.x+icon.width/2,iconWidth:icon.width,iconHeight:icon.height,font:style.font,color:style.color,iconColor:getComputedStyle(el.querySelector('svg')).color,opacity:style.opacity,selected:el.getAttribute('aria-checked')==='true'};}));
 assert.equal(rows.length,6);const step=rows[1].center-rows[0].center;
 rows.forEach((row,i)=>{assert.ok(Math.abs(row.height-rows[0].height)<.5,'All rows have the same height');assert.ok(Math.abs(row.labelX-rows[0].labelX)<.5,'All labels share one left edge');assert.ok(Math.abs(row.iconX-rows[0].iconX)<.5,'All icons share one center');assert.equal(row.iconWidth,16);assert.equal(row.iconHeight,16);assert.equal(row.font,rows[0].font);assert.equal(row.opacity,'1','Disabled rows retain the same visual intensity');assert.equal(row.iconColor,row.color);if(i)assert.ok(Math.abs(row.center-rows[i-1].center-step)<.5,'Every consecutive pair has identical spacing');});
 const ordinary=rows.find(row=>!row.selected);for(const row of rows.filter(row=>!row.selected))assert.equal(row.color,ordinary.color,'Both actions match ordinary menu text');
 assert.deepEqual(await menu.locator('button[role^="menuitem"] > span').allTextContents(),['运行中','全部会话','已停止','收藏','停止运行会话','删除全部会话']);
}
try {
 for(const theme of ['dark','light'])for(const viewport of [{width:1280,height:850},{width:820,height:540}]) {
  const p=await fresh('1',theme,viewport),trigger=p.locator('.session-filter-trigger');
  await p.getByRole('button',{name:'更多工具',exact:true}).click();const tools=p.getByRole('menu',{name:'更多工具',exact:true});assert.deepEqual(await tools.getByRole('menuitem').allTextContents(),['视图','帮助']);await p.keyboard.press('Escape');
  let m=await open(p);assert.equal(await m.getByRole('group',{name:'查看范围',exact:true}).count(),1);assert.equal(await m.getByRole('group',{name:'批量操作',exact:true}).count(),1);
  assert.equal(await m.getByRole('menuitemradio').count(),4);assert.equal(await m.getByRole('menuitem').count(),2);assert.equal(await m.locator('.session-filter-group-title,.session-filter-divider,.session-filter-scope,[role=separator]').count(),0);assert.equal(await m.getByRole('group',{name:'批量操作',exact:true}).getAttribute('aria-description'),'仅作用于当前工作区「工作区 A」');
  await uniformRows(m);
  assert.equal(await m.evaluate(e=>getComputedStyle(e).position),'fixed','fresh CSS and portal positioning are active');
  const bounds=await m.boundingBox();assert.ok(bounds.x>=0&&bounds.y>=0&&bounds.x+bounds.width<=viewport.width+1&&bounds.y+bounds.height<=viewport.height+1);
  const action=m.getByRole('menuitem',{name:'删除全部会话',exact:true});await action.scrollIntoViewIfNeeded();assert.equal(await action.evaluate(el=>{const r=el.getBoundingClientRect();return document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.closest('button')===el;}),true,'menu is above resize handles and other panels');
  await m.evaluate(el=>el.scrollTop=0);await p.screenshot({path:`${out}/menu-${theme}-${viewport.width}.png`});
  await p.keyboard.press('End');assert.equal(await action.evaluate(el=>el===document.activeElement),true);await p.keyboard.press('Home');assert.equal(await m.getByRole('menuitemradio').first().evaluate(el=>el===document.activeElement),true);
  await p.keyboard.press('Escape');assert.equal(await m.count(),0);assert.equal(await trigger.evaluate(el=>el===document.activeElement),true);
  await trigger.press('ArrowDown');await m.getByRole('menuitemradio').first().waitFor();await p.keyboard.press('Tab');assert.equal(await m.count(),0);
  for(const [label,count] of [['全部会话',4],['已停止',2],['收藏',1],['运行中',2]]) {m=await open(p);await m.getByRole('menuitemradio',{name:new RegExp(`^${label}`)}).click();assert.equal(await p.locator('.session-item').count(),count);assert.match(await trigger.getAttribute('aria-label'),new RegExp(label));}
  await open(p);await p.getByPlaceholder('搜索标题 / 目录 / provider').focus();assert.equal(await m.count(),0,'focus outside dismisses menu');
  await open(p);await p.mouse.click(viewport.width-10,viewport.height-10);assert.equal(await m.count(),0,'pointer outside dismisses menu');
  const d=await remove(p);assert.match(await d.innerText(),/工作区 A/);assert.match(await d.innerText(),/全部 4 个会话/);assert.match(await d.innerText(),/包含当前筛选中未显示/);
  assert.equal(await d.getByRole('button',{name:'取消',exact:true}).evaluate(el=>el===document.activeElement),true);await d.screenshot({path:`${out}/confirm-${theme}-${viewport.width}.png`});
  await p.keyboard.press('Escape');assert.equal(await d.count(),0);assert.equal((await calls(p,'delete_session')).length,0);await p.close();
 }
 const stopping=await fresh();await stopping.getByRole('group',{name:'会话类型',exact:true}).getByRole('button',{name:/^会话/}).click();await stopping.getByPlaceholder('搜索标题 / 目录 / provider').fill('fixture-ai');
 assert.equal(await stopping.locator('.session-item').count(),1);await (await open(stopping)).getByRole('menuitem',{name:'停止运行会话',exact:true}).click();
 await stopping.waitForFunction(()=>window.appSessionMock.bulkCalls.filter(item=>item.command==='stop_session').length===2);
 assert.deepEqual((await calls(stopping,'stop_session')).map(item=>item.sessionId).sort(),['one','three']);assert.equal(await stopping.evaluate(()=>window.appSessionMock.bulkSessions().find(item=>item.id==='other').status),'running');
 await stopping.waitForFunction(()=>document.querySelector('.session-filter-trigger strong')?.textContent==='0');assert.equal(await (await open(stopping)).getByRole('menuitem',{name:'停止运行会话',exact:true}).isDisabled(),true);await stopping.close();

 const deleting=await fresh();await deleting.getByRole('group',{name:'会话类型',exact:true}).getByRole('button',{name:/^终端/}).click();await deleting.getByPlaceholder('搜索标题 / 目录 / provider').fill('fixture-one');assert.equal(await deleting.locator('.session-item').count(),1);
 let d=await remove(deleting);await d.getByRole('button',{name:'取消',exact:true}).click();assert.equal((await calls(deleting,'delete_session')).length,0);d=await remove(deleting);
 await d.getByRole('button',{name:'确认删除',exact:true}).evaluate(el=>{el.click();el.click();});await deleting.waitForFunction(()=>window.appSessionMock.bulkCalls.filter(item=>item.command==='delete_session').length===4);
 assert.deepEqual((await calls(deleting,'delete_session')).map(item=>item.sessionId).sort(),['four','one','three','two']);assert.deepEqual(await deleting.evaluate(()=>window.appSessionMock.bulkSessions().map(item=>item.id)),['other']);await deleting.close();

 const stale=await fresh();d=await remove(stale);await stale.evaluate(()=>window.appSessionMock.addBulkSession());await d.getByRole('alert').waitFor();assert.equal(await d.getByRole('button',{name:'确认删除',exact:true}).isDisabled(),true);assert.equal((await calls(stale,'delete_session')).length,0);await d.getByRole('button',{name:'取消',exact:true}).click();d=await remove(stale);assert.match(await d.innerText(),/全部 5 个会话/);
 await stale.evaluate(()=>window.appSessionMock.replaceBulkSession());await d.getByRole('alert').waitFor();assert.equal(await d.getByRole('button',{name:'确认删除',exact:true}).isDisabled(),true,'identity changes with unchanged count also require confirmation again');await stale.keyboard.press('Escape');
 d=await remove(stale);await stale.getByRole('button',{name:'工作区 B',exact:true}).evaluate(el=>el.click());await d.waitFor({state:'detached'});const next=await open(stale);assert.match(await next.getByRole('menuitem',{name:'删除全部会话',exact:true}).getAttribute('title'),/工作区 B/);assert.equal((await calls(stale,'delete_session')).length,0);await stale.close();

 for(const mode of ['empty','stopped']) {const p=await fresh(mode);const m=await open(p);const stop=m.getByRole('menuitem',{name:'停止运行会话',exact:true});assert.equal(await stop.isDisabled(),true);assert.match(await stop.getAttribute('title'),/暂无运行会话/);assert.equal(await m.getByRole('menuitem',{name:'删除全部会话',exact:true}).isDisabled(),mode==='empty');await uniformRows(m);await stop.evaluate(el=>el.click());assert.equal((await calls(p,'stop_session')).length,0,'Visually neutral disabled action still cannot run');await p.screenshot({path:`${out}/${mode}-neutral.png`});await p.keyboard.press('End');assert.equal(await p.evaluate(()=>document.activeElement?.matches(':disabled')),false);await p.close();}
 const rejected=await fresh();await rejected.evaluate(()=>window.appSessionMock.rejectBulkDelete='three');d=await remove(rejected);await d.getByRole('button',{name:'确认删除',exact:true}).click();await rejected.getByText(/删除「fixture-ai」失败/).waitFor();assert.deepEqual(await rejected.evaluate(()=>window.appSessionMock.bulkSessions().map(item=>item.id).sort()),['other','three']);await rejected.close();
 const failedStop=await fresh();await failedStop.evaluate(()=>window.appSessionMock.rejectBulkStop='one');await (await open(failedStop)).getByRole('menuitem',{name:'停止运行会话',exact:true}).click();await failedStop.getByText(/停止「fixture-one」失败/).waitFor();assert.equal(await failedStop.evaluate(()=>window.appSessionMock.bulkSessions().find(item=>item.id==='one').status),'running');assert.equal(await failedStop.evaluate(()=>window.appSessionMock.bulkSessions().find(item=>item.id==='three').status),'stopped');await failedStop.close();
 const long=await fresh(`1&workspace-name=${encodeURIComponent('长名称协作工作区'.repeat(12))}`,'light',{width:820,height:540});const longMenu=await open(long);assert.equal(await longMenu.getByRole('menuitem',{name:'删除全部会话',exact:true}).innerText(),'删除全部会话');assert.match(await longMenu.getByRole('menuitem',{name:'删除全部会话',exact:true}).getAttribute('title'),/长名称协作工作区/);await long.keyboard.press('Escape');d=await remove(long);assert.equal(await d.evaluate(el=>el.scrollWidth<=el.clientWidth),true,'long workspace names wrap within confirmation');await d.screenshot({path:`${out}/confirm-long-name.png`});await long.keyboard.press('Escape');await long.close();
 assert.deepEqual(errors,[]);console.log('PASS: actual App minimal equal-spacing session menu, keyboard/viewport, exact workspace-wide stop/delete despite filters, confirmation/cancel/staleness, empty/disabled states, visible rejections and view/help tools');
}finally {await browser.close();}
