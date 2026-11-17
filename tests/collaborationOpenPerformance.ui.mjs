// Production settings UI with slow isolated IPC; no native CLI or model calls.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const out='.scratch/collaboration-open-perf-20261117';mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:'chrome'});const times=[],navTimes=[],errors=[];
try {for(let index=0;index<5;index++) {
 const p=await browser.newPage({viewport:{width:1280,height:900}});p.on('pageerror',e=>errors.push(e.message));
 await p.goto(base+'/tests/fixtures/collaboration-conversation-ui.html?slow-open=1');
 await p.getByRole('button',{name:'查看任务 · 3',exact:true}).waitFor();
 await p.getByRole('button',{name:'查看任务 · 3',exact:true}).evaluate(button=>{window.perfStarted=performance.now();button.click();});
 await p.locator('.collab-task-row').first().waitFor();
 const elapsed=await p.evaluate(()=>performance.now()-window.perfStarted);times.push(Math.round(elapsed));
 if(index===0)await p.screenshot({path:out+'/opened.png'});
 assert.ok(elapsed<=500,'Real project rows must appear within 500ms while runtime/skill/health calls take 2500ms: '+elapsed);
 await p.getByRole('button',{name:'常规',exact:true}).click();
 await p.getByRole('button',{name:'协作中心',exact:true}).evaluate(button=>{window.perfStarted=performance.now();button.click();});
 await p.locator('.collab-task-row').first().waitFor();
 const navElapsed=await p.evaluate(()=>performance.now()-window.perfStarted);navTimes.push(Math.round(navElapsed));
 assert.ok(navElapsed<=500,'Sidebar navigation must show real project tasks within 500ms: '+navElapsed);
 await p.locator('.collaboration-settings-panel').getByRole('tab',{name:/^团队/}).click();
 assert.equal(await p.getByRole('button',{name:'新增成员',exact:true}).isEnabled(),true,'member actions stay responsive while probes are pending');

 await p.getByRole('button',{name:'关闭设置',exact:true}).click();await p.getByRole('button',{name:'查看任务 · 3',exact:true}).click();
 await p.locator('.collab-task-row').first().waitFor();
 await p.getByRole('button',{name:'关闭设置',exact:true}).click();await p.close();
 }
 const failure=await browser.newPage();failure.on('pageerror',e=>errors.push(e.message));
 await failure.goto(base+'/tests/fixtures/collaboration-conversation-ui.html?slow-open=1&fail-aux=1');
 await failure.getByRole('button',{name:'查看任务 · 3',exact:true}).click();
 await failure.locator('.collab-task-row').first().waitFor({timeout:500});
 await failure.getByRole('alert').filter({hasText:'运行环境检测失败'}).waitFor();
 await failure.getByRole('alert').filter({hasText:'Skill 配置失败'}).waitFor();
 assert.equal(await failure.locator('.collab-task-row').count(),3,'auxiliary failures preserve real project content');
 await failure.close();
 const slow=await browser.newPage();slow.on('pageerror',e=>errors.push(e.message));
 await slow.goto(base+'/tests/fixtures/collaboration-conversation-ui.html?slow-data=1');
 await slow.getByRole('button',{name:'查看任务 · 3',exact:true}).waitFor();
 await slow.getByRole('button',{name:'查看任务 · 3',exact:true}).evaluate(button=>{window.perfStarted=performance.now();button.click();});
 await slow.locator('.collaboration-settings-panel').waitFor({timeout:500});
 assert.ok(await slow.evaluate(()=>performance.now()-window.perfStarted)<500,'panel opens without waiting for slow project data');
 await slow.getByText('正在加载项目任务…',{exact:true}).waitFor();
 assert.equal(await slow.getByText('暂无协作任务',{exact:true}).count(),0,'pending snapshot is not reported as an empty project');
 await slow.getByRole('button',{name:'关闭设置',exact:true}).click();
 await slow.waitForTimeout(2800);
 assert.equal(await slow.locator('.collaboration-settings-panel').count(),0,'late reads cannot reopen an unmounted panel');
 await slow.close();
 assert.deepEqual(errors,[]);writeFileSync(out+'/timings.json',JSON.stringify({times,navTimes,maximum:Math.max(...times,...navTimes),slowAuxiliaryIpcMs:2500,errors},null,2));console.log('PASS collaboration open <500ms:',{times,navTimes});
}finally{await browser.close();}
