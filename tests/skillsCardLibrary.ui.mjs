// Actual SettingsDialog + SkillCenterPanel. Mock IPC touches no Skill directories or runtimes.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1432';
const browser=await chromium.launch({channel:'chrome'}), errors=[];
const out='.scratch/skills-redesign-20261116/ui';mkdirSync(out,{recursive:true});
const url=(extra='')=>`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=skills&open=1&section=skills&${extra}`;
try {
 for(const theme of ['light','dark']) for(const size of [{width:1487,height:1058},{width:820,height:540},{width:687,height:903}]){
  const page=await browser.newPage({viewport:size});page.on('pageerror',e=>errors.push(e.message));page.setDefaultTimeout(10000);
  await page.goto(url(`theme=${theme}&skills=cards`));await page.locator('.skill-library-card').first().waitFor();
  await page.locator('.skill-runtime-tabs [data-runtime=codex]').click();
  await page.getByRole('checkbox',{name:'选择 frontend-design',exact:true}).check();
  await page.getByRole('checkbox',{name:'选择 code-review',exact:true}).check();
  assert.equal(await page.locator('.skill-library-card').count(),6);
  assert.equal(await page.locator('.skill-library-card.selected').count(),2);
  const geometry=await page.evaluate(()=>{
   const cards=[...document.querySelectorAll('.skill-library-card')].map(e=>e.getBoundingClientRect().toJSON());
   const frame=document.querySelector('.settings-dialog').getBoundingClientRect();const footer=document.querySelector('.skill-center-footer').getBoundingClientRect();const body=document.querySelector('.skill-distribution-body');
   return {cards,footerVisible:footer.bottom<=frame.bottom+1&&footer.right<=frame.right+1,bodyOverflow:body.scrollWidth>body.clientWidth+1,rootOverflow:document.documentElement.scrollWidth>innerWidth+1};
  });
  assert.equal(geometry.footerVisible,true,'Batch actions stay inside settings frame');assert.equal(geometry.bodyOverflow,false);assert.equal(geometry.rootOverflow,false);
  if(size.width===1487)assert.equal(Math.round(geometry.cards[0].y),Math.round(geometry.cards[1].y),'Desktop has two columns');
  else assert.ok(geometry.cards[1].y>geometry.cards[0].y,'Narrow panel has one readable column');
  assert.equal(await page.getByRole('button',{name:'部署到 2 个 Agent',exact:true}).isEnabled(),true);
  await page.locator('.skill-distribution-body').evaluate(e=>e.scrollTop=0);
  await page.screenshot({path:`${out}/${theme}-${size.width}x${size.height}.png`,fullPage:true});
  if(size.width===1487)await page.locator('.settings-dialog').screenshot({path:`${out}/${theme}-dialog.png`});
  if(size.width===1487){
   for(const selector of ['.skill-tools-menu > summary','.skill-filter-menu > summary']){
    await page.locator(selector).click();await page.locator(selector).press('Escape');
    assert.equal(await page.locator(selector).evaluate(e=>e.parentElement.open),false,'Escape only closes the local menu');
    assert.equal(await page.locator('.settings-dialog').isVisible(),true);
   }
   await page.getByRole('button',{name:'research 更多操作',exact:true}).click();
   const actions=page.getByRole('dialog',{name:'research 操作',exact:true});
   await actions.getByRole('button',{name:'文件预览与信任审阅',exact:true}).click();
   await page.getByText('# research',{exact:false}).waitFor();
   await page.getByRole('button',{name:'信任',exact:true}).click();await page.getByRole('status').filter({hasText:'信任状态已更新'}).waitFor();
   await page.getByRole('button',{name:'返回我的 Skills',exact:true}).click();
   const research=page.getByRole('listitem',{name:'research',exact:true});await research.getByText('已信任',{exact:true}).waitFor();
   await page.locator('.skill-tools-menu > summary').click();await page.getByRole('button',{name:'版本更新',exact:true}).click();
   await page.getByRole('heading',{name:'版本更新',exact:true}).waitFor();await page.getByRole('button',{name:'返回我的 Skills',exact:true}).click();
   await page.locator('.skill-tools-menu > summary').click();await page.getByRole('button',{name:'配置集',exact:true}).click();
   await page.getByRole('heading',{name:'保存当前选择为配置集',exact:true}).waitFor();await page.getByRole('button',{name:'返回我的 Skills',exact:true}).click();
   await page.getByRole('tab',{name:'发现',exact:true}).click();await page.getByRole('button',{name:'skills.sh',exact:true}).waitFor();
   await page.getByRole('tab',{name:'我的',exact:true}).click();assert.equal(await page.locator('.skill-library-card.selected').count(),2,'Secondary views preserve bulk selection');
  }
  await page.close();
 }
 const empty=await browser.newPage();await empty.goto(url('skills=empty'));await empty.getByText('还没有 Skills',{exact:true}).waitFor();
 assert.equal(await empty.getByRole('button',{name:'部署到 Claude Code',exact:true}).isEnabled(),false);await empty.close();
 assert.deepEqual(errors,[]);console.log('PASS: card design, 6 samples, desktop/narrow dark/light, fixed actions, preview/trust, updates/sets/discovery and empty state');
}finally{await browser.close();}
