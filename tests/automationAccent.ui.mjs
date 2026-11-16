// Real App appearance controls, persistence and AutomationPanel with isolated IPC.
import assert from 'node:assert/strict';
import {mkdirSync,writeFileSync} from 'node:fs';
import {chromium} from 'playwright';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1435',out='.scratch/automation-accent-20261116/ui';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[],metrics=[];
const accents=[['clay','陶土'],['none','无强调色'],['purple','紫色'],['cyan','青色'],['orange','橙色'],['pink','粉色'],['coral','珊瑚色'],['gray','灰色']];
const themes=[['dark','深色'],['light','浅色'],['ocean','深海'],['purple','石墨黑'],['sand','暖沙'],['aurora','暖瓷白']];
const rgb=color=>color.match(/[\d.]+/g).slice(0,3).map(Number);
function contrast(a,b){const luminance=color=>rgb(color).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((s,v,i)=>s+v*[.2126,.7152,.0722][i],0);const values=[luminance(a),luminance(b)].sort((a,b)=>a-b);return (values[1]+.05)/(values[0]+.05);}
async function colors(page){return page.evaluate(()=>{
 const value=(name,scope=document.body)=>{const probe=document.createElement('span');probe.style.color=`var(${name})`;scope.append(probe);const result=getComputedStyle(probe).color;probe.remove();return result;};
 const panel=document.querySelector('.automation-design'),primary=panel.querySelector('.automation-heading .automation-primary'),toggle=panel.querySelector('.automation-toggle input:checked');
 return {accent:value('--accent'),hover:value('--accent-strong'),primary:getComputedStyle(primary).backgroundColor,text:getComputedStyle(primary).color,toggle:getComputedStyle(toggle).backgroundColor,knob:getComputedStyle(toggle,'::after').backgroundColor,border:getComputedStyle(panel.querySelector('.execution-running')).borderTopColor,completed:getComputedStyle(panel.querySelector('.status-completed')).color,failed:getComputedStyle(panel.querySelector('.status-failed')).color,unknown:getComputedStyle(panel.querySelector('.status-unconfirmed')).color,waiting:getComputedStyle(panel.querySelector('.tone-running .automation-state-icon')).color,success:value('--success',panel),danger:value('--danger',panel),muted:value('--text-dim',panel),warn:value('--warn',panel)};
});}
function check(c,key){
 for(const field of ['primary','toggle','border'])assert.equal(c[field],c.accent,`${key}: ${field} follows selected accent`);
 assert.equal(c.completed,c.success,`${key}: success stays semantic`);assert.equal(c.failed,c.danger,`${key}: failure stays semantic`);assert.equal(c.unknown,c.muted,`${key}: uncertain outcome stays neutral`);
 assert.equal(c.waiting,c.warn,`${key}: progress stays semantic`);
 assert.ok(contrast(c.primary,c.text)>=4.5,`${key}: button text contrast`);assert.ok(contrast(c.toggle,c.knob)>=3,`${key}: checked knob contrast`);
}
try{
 const source=await browser.newPage();await source.goto(`${base}/tests/fixtures/automation-execution-ui.html`);await source.locator('.automation-card').first().waitFor();const seed=await source.evaluate(()=>window.__automationFixture.entries);await source.close();
 const page=await browser.newPage({viewport:{width:1487,height:1058}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(seed=>{window.__automationSeed=seed;},seed);
 await page.goto(`${base}/tests/fixtures/app-session-async-ui.html?automation-accent=1`);
 await page.getByRole('button',{name:'设置',exact:true}).click();
 const settings=page.getByRole('dialog',{name:'设置',exact:true}),nav=settings.getByRole('navigation',{name:'设置分类',exact:true});
 for(const [theme,themeLabel] of themes){
  const seen=new Set();let semantic;
  for(const [accent,label] of accents){
   await nav.getByRole('button',{name:'外观',exact:true}).click();
   await settings.getByRole('button',{name:themeLabel,exact:true}).click();
   const radio=settings.getByRole('radio',{name:label,exact:true});if(accent==='purple'){await radio.focus();await radio.press('Space');}else await radio.click();
   await page.waitForFunction(({theme,accent})=>document.documentElement.dataset.appearanceTheme===theme&&document.documentElement.dataset.appearanceAccent===accent,{theme,accent});
   assert.equal(await radio.getAttribute('aria-checked'),'true');
   await nav.getByRole('button',{name:'自动化',exact:true}).click();await page.locator('.automation-card').first().waitFor();
   const key=`${theme}-${accent}`,c=await colors(page);check(c,key);seen.add(c.accent);metrics.push({key,...c});
   const current=[c.completed,c.failed,c.unknown,c.waiting];if(semantic)assert.deepEqual(current,semantic,`${key}: semantic colors do not depend on accent`);else semantic=current;
   const cta=page.getByRole('button',{name:'新建自动化',exact:true});await cta.hover();await page.waitForFunction(expected=>getComputedStyle(document.querySelector('.automation-heading .automation-primary')).backgroundColor===expected,c.hover);
   const saved=await page.evaluate(()=>JSON.parse(localStorage.getItem('crewvela-appearance')));assert.equal(saved.accent,accent);assert.equal(saved.theme,theme);
   if((theme==='dark'&&accent==='purple')||(theme==='light'&&accent==='cyan')){await page.mouse.move(1,1);await settings.screenshot({path:`${out}/${key}.png`});}
  }
  assert.equal(seen.size,8,`${theme}: every selectable accent produces a distinct palette`);
 }
 // CSS responds immediately on an already-mounted list as root theme attributes change.
 const live=await page.locator('.automation-card').first().elementHandle();
 await page.mouse.move(1,1);
 for(const accent of ['clay','purple','cyan','none']){
  await page.evaluate(accent=>document.documentElement.dataset.appearanceAccent=accent,accent);
  const expected=(await colors(page)).accent;
  await page.waitForFunction(expected=>getComputedStyle(document.querySelector('.automation-heading .automation-primary')).backgroundColor===expected&&getComputedStyle(document.querySelector('.automation-toggle input:checked')).backgroundColor===expected&&getComputedStyle(document.querySelector('.execution-running')).borderTopColor===expected,expected);
  check(await colors(page),`live-${accent}`);assert.equal(await live.evaluate(el=>el===document.querySelector('.automation-card')),true);
 }
 await page.reload();await page.getByRole('button',{name:'设置',exact:true}).click();await page.getByRole('navigation',{name:'设置分类',exact:true}).getByRole('button',{name:'自动化',exact:true}).click();await page.locator('.automation-card').first().waitFor();
 assert.deepEqual(await page.evaluate(()=>[document.documentElement.dataset.appearanceTheme,document.documentElement.dataset.appearanceAccent]),['aurora','gray']);check(await colors(page),'reload');
 await page.setViewportSize({width:687,height:903});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);await page.screenshot({path:`${out}/narrow.png`});
 const calls=await page.evaluate(()=>window.appSessionMock.automationAccentCalls);assert.equal(calls.some(c=>['create_automation','run_automation_now','set_automation_enabled','stop_automation_run','delete_automation'].includes(c.command)),false);
 assert.deepEqual(errors,[]);writeFileSync(`${out}/metrics.json`,JSON.stringify({metrics,errors},null,2));
 console.log('PASS: actual App settings, 8 accents × 6 themes, keyboard selection, immediate mounted-list updates, hover, semantic statuses, button/knob contrast, persistence/reload, narrow layout and no automation mutations');
}finally{await browser.close();}
