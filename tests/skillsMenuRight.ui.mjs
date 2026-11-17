import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420',dir='.scratch/skills-menu-right-20261117';
mkdirSync(dir,{recursive:true});
const browser=await chromium.launch({channel:'chrome',headless:true}),errors=[],results=[];
try{
  for(const theme of ['dark','light'])for(const size of [{width:1600,height:1120},{width:820,height:540},{width:687,height:903}]){
    const page=await browser.newPage({viewport:size});page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?mock=skills&open=1&section=skills&theme=${theme}&skills=cards`);
    await page.locator('.skill-tools-menu').waitFor();
    const trigger=page.getByLabel('更多 Skill 操作',{exact:true});
    for(const tab of ['我的','发现']){
      await page.getByRole('tab',{name:tab,exact:true}).click();await trigger.click();
      const geometry=await page.locator('.skill-tools-menu').evaluate(details=>{
        const trigger=details.querySelector('summary').getBoundingClientRect(),menu=details.querySelector('.skill-center-actions').getBoundingClientRect(),panel=details.closest('.skill-card-library').getBoundingClientRect();
        return {trigger:trigger.toJSON(),menu:menu.toJSON(),panel:panel.toJSON(),width:innerWidth};
      });
      const edge=Math.min(geometry.width,geometry.panel.right)-8;
      assert.ok(geometry.menu.right<=edge+1,`menu stays in the visible content ${JSON.stringify({theme,size,tab,geometry})}`);
      assert.ok(geometry.menu.left>=Math.max(0,geometry.panel.left)+7);
      if(geometry.trigger.left+geometry.menu.width<=edge)assert.ok(Math.abs(geometry.menu.left-geometry.trigger.left)<1,'menu opens rightward from trigger left edge');
      results.push({theme,size,tab,left:geometry.menu.left,triggerLeft:geometry.trigger.left,right:geometry.menu.right});
      if(size.width===1600)await page.screenshot({path:`${dir}/${theme}-${tab}.png`});
      await page.getByRole('heading',{name:'Skills',exact:true}).click();
      assert.equal(await page.locator('.skill-tools-menu').evaluate(e=>e.open),false,'outside click closes menu');
      await trigger.click();await trigger.press('Escape');
      assert.equal(await page.locator('.skill-tools-menu').evaluate(e=>e.open),false);
      assert.equal(await trigger.evaluate(e=>e===document.activeElement),true,'Escape restores focus');
      assert.equal(await page.locator('.settings-dialog').isVisible(),true);
      await trigger.press('Enter');
      assert.equal(await page.locator('.skill-tools-menu').evaluate(e=>e.open),true,'Enter opens the native disclosure');
      await trigger.press('Space');
      assert.equal(await page.locator('.skill-tools-menu').evaluate(e=>e.open),false,'Space toggles the native disclosure');
    }
    await trigger.click();await page.getByRole('button',{name:'配置集',exact:true}).click();
    await page.getByRole('heading',{name:'保存当前选择为配置集',exact:true}).waitFor();
    assert.equal(await page.locator('.skill-tools-menu').evaluate(e=>e.open),false,'selection closes menu');
    await page.getByRole('button',{name:'返回我的 Skills',exact:true}).click();
    await page.locator('.skill-import-menu>summary').click();
    assert.equal(await page.getByRole('button',{name:'导入目录',exact:true}).isVisible(),true,'import menu unaffected');
    await page.locator('.skill-import-menu>summary').press('Escape');
    if(size.width===1600){
      await trigger.click();await page.setViewportSize({width:820,height:540});
      const right=await page.locator('.skill-tools-menu>.skill-center-actions').evaluate(e=>e.getBoundingClientRect().right);
      assert.ok(right<=820,'open menu repositions after resize');
    }
    await page.close();
  }
  assert.deepEqual(errors,[]);writeFileSync(`${dir}/results.json`,JSON.stringify({results,errors},null,2));
  console.log('PASS: rightward menu, clipping bounds, both tabs/themes, three sizes, outside/Escape/item close and import entry');
}finally{await browser.close();}
