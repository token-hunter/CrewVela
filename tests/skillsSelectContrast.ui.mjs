// Actual Skills scope/select controls with production theme attributes; fixture IPC only.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1433';
const out='.scratch/skills-select-contrast-20261116';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'}),rows=[],errors=[];
const luminance=color=>color.match(/[\d.]+/g).slice(0,3).map(Number).map(c=>{c/=255;return c<=.04045?c/12.92:((c+.055)/1.055)**2.4;}).reduce((sum,c,i)=>sum+c*[.2126,.7152,.0722][i],0);
const contrast=(a,b)=>{const values=[luminance(a),luminance(b)].sort((x,y)=>y-x);return (values[0]+.05)/(values[1]+.05);};
try{
 for(const theme of ['dark','light']){
  const page=await browser.newPage({viewport:{width:1487,height:1058}});page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=skills&skills=cards&open=1&section=skills&theme=${theme}`);await page.locator('.skill-library-card').first().waitFor();
  const scope=page.getByLabel('作用域',{exact:true});
  const styles=await scope.evaluate(select=>({scheme:getComputedStyle(select).colorScheme,rootScheme:getComputedStyle(document.documentElement).colorScheme,options:[...select.options].map(option=>({text:option.text,color:getComputedStyle(option).color,background:getComputedStyle(option).backgroundColor}))}));
  rows.push({theme,...styles});
  // Render the actual native options as a list for readable evidence; do not alter production UI.
  await scope.evaluate(select=>{select.size=3;select.style.height='auto';});
  await page.evaluate(()=>document.activeElement?.blur());await page.locator('.settings-dialog').screenshot({path:`${out}/options-${theme}.png`});
  await scope.evaluate(select=>{select.removeAttribute('size');select.style.removeProperty('height');});
  await scope.selectOption('project');assert.equal(await scope.inputValue(),'project');
  await scope.selectOption('task');await page.getByLabel('任务 ID',{exact:true}).waitFor();
  await scope.selectOption('global');assert.equal(await scope.inputValue(),'global');
  await page.close();
 }
 writeFileSync(`${out}/metrics.json`,JSON.stringify({rows,errors},null,2));
 assert.deepEqual(errors,[]);
 for(const row of rows){
  assert.equal(row.scheme,row.theme,`${row.theme}: native dropdown must use the matching theme`);
  for(const option of row.options){
   assert.notEqual(option.background,'rgba(0, 0, 0, 0)',`${row.theme} ${option.text}: native option needs an explicit surface, not the OS white fallback`);
   assert.ok(contrast(option.color,option.background)>=4.5,`${row.theme} ${option.text}: option text contrast >= 4.5`);
  }
 }
 console.log('PASS: Skills scope options use matching native scheme and explicit readable surfaces; global/project/task switching preserved');
}finally{await browser.close();}
