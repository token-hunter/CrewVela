// Explicit production appearance attributes and representative long records; no real plans/CLI.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1433';
const out='.scratch/automation-style-parity-20261116/ui';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[],failures=[],metrics=[];
try{
 const source=await browser.newPage();await source.goto(`${base}/tests/fixtures/automation-execution-ui.html?design=1`);await source.locator('.automation-card').first().waitFor();
 const {entries,sessions}=await source.evaluate(()=>({entries:window.__automationFixture.entries,sessions:window.__automationFixture.sessions}));await source.close();
 const names=['测试','test','1'];
 const details=['应用已重启或旧记录缺少执行回执，结果未确认；不会自动重发','缺少本消息的可靠回执，请检查原会话；不会自动重发','缺少本消息的可靠回执，请检查原会话；不会自动重发'];
 const unknown=entries.slice(0,3).map((item,i)=>({...item,name:names[i],enabled:true,trigger:{kind:'once',at:new Date('2026-11-15T21:49:00+08:00').getTime()+i*3600000},runCount:1,nextRunAt:null,consecutiveFailures:0,action:{...item.action,runtimeKind:'zcode_cli',cwd:'G:/tmp'},recentRuns:[{...item.recentRuns.at(-1),id:['cde6e83d-fixture','0f17dbb4-fixture','f3cfe9a9-fixture'][i],stage:'unconfirmed',detail:details[i],targets:i?item.recentRuns.at(-1).targets:[]}]}));
 for(const scenario of [
  {name:'native-unknown',theme:'dark',accent:'clay',seed:unknown,width:1538,height:929,dpr:1.25},
  ...['dark','light'].flatMap(theme=>['clay','none','purple'].flatMap(accent=>['mixed','unknown'].map(kind=>({name:kind,theme,accent,seed:kind==='mixed'?entries:unknown,width:1487,height:1058,dpr:1})))),
  {name:'narrow-unknown',theme:'dark',accent:'clay',seed:unknown,width:687,height:903,dpr:1},
 ]){
  const {name,theme,accent,seed,width,height,dpr}=scenario,key=`${name}-${theme}-${accent}`;
  const page=await browser.newPage({viewport:{width,height},deviceScaleFactor:dpr});page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(({seed,sessions})=>{window.__automationSeed=seed;window.__automationSessions=sessions;},{seed,sessions});
  await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=automation&open=1&section=automation&theme=${theme}&accent=${accent}`);await page.locator('.automation-card').first().waitFor();
  const colors=await page.evaluate(()=>{const probe=document.createElement('span');probe.style.color='var(--accent)';document.body.append(probe);const expected=getComputedStyle(probe).color;probe.remove();return {rootFill:getComputedStyle(document.documentElement).getPropertyValue('--accent-fill').trim(),expected,primary:getComputedStyle(document.querySelector('.automation-heading .automation-primary')).backgroundColor,toggle:getComputedStyle(document.querySelector('.automation-toggle input:checked')).backgroundColor};});
  const expected=colors.expected;
  if(colors.primary!==expected)failures.push(`${key}: primary ${colors.primary}, expected ${expected}`);
  if(colors.toggle!==expected)failures.push(`${key}: toggle ${colors.toggle}, expected ${expected}`);
  if(accent==='clay')assert.equal(colors.rootFill,theme==='dark'?'#ecece9':'#141413','Do not mutate the global appearance palette');
  const heights=await page.locator('.automation-card').evaluateAll(nodes=>nodes.map(n=>n.getBoundingClientRect().height));
  if(width>1000 && Math.max(...heights)>230)failures.push(`${key}: cards expanded to ${Math.max(...heights)}px`);
  metrics.push({key,width,height,dpr,...colors,heights});
  assert.equal(await page.locator('.automation-card').count(),seed.length);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  if(name.includes('unknown')){
   assert.equal(await page.locator('.execution-unconfirmed').count(),3,'Keep genuine uncertain outcomes');
   assert.equal(await page.locator('.execution-completed,.execution-running').count(),0,'No invented success or running highlights');
   assert.equal(await page.getByRole('button',{name:'停止本轮',exact:true}).count(),0,'Unknown work remains behind identity-safe controls');
   assert.equal(await page.locator('.automation-card').first().getByRole('button',{name:'打开会话',exact:true}).count(),0,'No link invented for missing sessions');
   assert.equal(await page.locator('.automation-run-description').first().getAttribute('title'),details[0],'Full explanation survives compact layout');
   assert.match(await page.locator('.automation-calendar').innerText(),/暂无待触发计划/);
  }
  await page.evaluate(()=>document.activeElement?.blur());
  await page.locator('.settings-dialog').screenshot({path:`${out}/${key}-dialog.png`});
  if(name==='mixed' && theme==='dark' && accent==='clay')await page.screenshot({path:`${out}/reference-state-full.png`});
  const history=page.locator('.automation-execution-history');await history.locator('summary').first().click();
  assert.ok((await history.innerText()).includes(seed[0].recentRuns.at(-1).detail),'Full explanation remains available in expanded execution history');
  if(name==='native-unknown'){
   await page.getByRole('button',{name:'新建自动化',exact:true}).click();await page.getByPlaceholder('例如：每天早上巡检依赖更新').waitFor();
   assert.equal(await page.getByRole('button',{name:'保存自动化',exact:true}).evaluate(el=>getComputedStyle(el).backgroundColor),'rgb(236, 236, 233)','Creation keeps the existing global theme');
   assert.equal(await page.evaluate(()=>window.__automationCalls.filter(c=>['create_automation','run_automation_now','set_automation_enabled'].includes(c.command)).length),0);
  }
  await page.close();
 }
 writeFileSync(`${out}/appearance-metrics.json`,JSON.stringify({metrics,failures,errors},null,2));assert.deepEqual(errors,[]);assert.deepEqual(failures,[]);
 console.log('PASS: list follows production clay/none/purple accent, dark/light, 125% scale, long unknown records, compact cards, full history and unchanged form palette');
}finally{await browser.close();}
