// Real SettingsDialog/ArchivedSessionsPanel. Local fixture data only; no user records are changed.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1436',out='.scratch/settings-select-contrast-20261116';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),errors=[],metrics=[];
const luminance=color=>color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((total,v,i)=>total+v*[.2126,.7152,.0722][i],0);
const contrast=(a,b)=>{const values=[luminance(a),luminance(b)].sort((a,b)=>a-b);return (values[1]+.05)/(values[0]+.05);};
async function readable(select,key){
 const colors=await select.evaluate(el=>({scheme:getComputedStyle(el).colorScheme,options:[...el.options].map(option=>({label:option.text,color:getComputedStyle(option).color,background:getComputedStyle(option).backgroundColor}))}));
 metrics.push({key,...colors});
 for(const option of colors.options){assert.notEqual(option.background,'rgba(0, 0, 0, 0)',`${key}: ${option.label} needs an opaque themed option surface`);assert.ok(contrast(option.color,option.background)>=4.5,`${key}: ${option.label} contrast >= 4.5`);}
 return colors;
}
try{
 const cases=['dark','light','ocean','purple','sand','aurora'].map(theme=>({theme,width:1487,height:1058})).concat(['dark','light'].map(theme=>({theme,width:820,height:540})));
 for(const {theme,width,height} of cases){
  const light=['light','sand','aurora'].includes(theme),key=`${theme}-${width}`;
  const p=await browser.newPage({viewport:{width,height}});p.setDefaultTimeout(10000);p.on('pageerror',e=>errors.push(e.message));
  await p.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&archive=projects&open=1&theme=${light?'light':'dark'}`);
  await p.evaluate(theme=>document.documentElement.dataset.appearanceTheme=theme,theme);
  const settings=p.getByRole('dialog',{name:'设置',exact:true}),nav=settings.getByRole('navigation',{name:'设置分类',exact:true});
  await nav.getByRole('button',{name:'归档会话',exact:true}).click();
  const select=settings.getByLabel('按项目筛选归档会话',{exact:true}),search=settings.getByLabel('搜索归档会话',{exact:true});
  assert.deepEqual(await select.locator('option').allTextContents(),['所有项目','示例项目 A','示例项目 B','未分组']);
  const colors=await readable(select,key);assert.equal(colors.scheme,light?'light':'dark');
  // Show the actual option elements inline for a screenshot; this is not an OS-popup screenshot.
  await select.evaluate(el=>{el.size=4;el.style.height='auto';});await settings.screenshot({path:`${out}/options-${key}.png`});await select.evaluate(el=>{el.removeAttribute('size');el.style.removeProperty('height');});
  assert.equal(await settings.locator('.archived-settings-row').count(),4);
  await select.selectOption('archive-project-a');assert.equal(await settings.locator('.archived-settings-row').count(),2);await search.fill('归档会话 2');assert.equal(await settings.locator('.archived-settings-row').count(),1);
  await select.selectOption('archive-project-b');await settings.getByText('没有匹配的归档会话',{exact:true}).waitFor();await search.fill('');assert.equal(await settings.locator('.archived-settings-row').count(),1);assert.match(await settings.locator('.archived-settings-row').innerText(),/归档会话 3/);
  await select.selectOption({label:'未分组'});assert.equal(await settings.locator('.archived-settings-row').count(),1);assert.match(await settings.locator('.archived-settings-row').innerText(),/归档会话 4/);
  await select.selectOption('all');assert.equal(await settings.locator('.archived-settings-row').count(),4);assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  await nav.getByRole('button',{name:'常规',exact:true}).click();for(const control of await settings.locator('select').all())await readable(control,`${key}: general`);
  await nav.getByRole('button',{name:'浏览器控制',exact:true}).click();for(const control of await settings.locator('select').all())await readable(control,`${key}: browser`);
  await p.close();
 }
 assert.deepEqual(errors,[]);writeFileSync(`${out}/metrics.json`,JSON.stringify({metrics,errors},null,2));console.log('PASS: explicit readable option surfaces across six themes and two narrow layouts; archive project/ungrouped/search filters and general/browser settings selects preserved; screenshots use inline native lists, not Windows OS popups');
}finally{await browser.close();}
