import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';
const dir=process.env.MASCOT_QA_DIR||'.scratch/mascot-hd-20261117',base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
mkdirSync(dir,{recursive:true});
const browser=await chromium.launch({channel:'chrome',headless:true});
const page=await browser.newPage({viewport:{width:1180,height:1000},deviceScaleFactor:2});
const errors=[];page.on('pageerror',e=>errors.push(e.message));
try{
  await page.goto(`${base}/tests/fixtures/mascot-hd-comparison.html`);
  await page.evaluate(async()=>{
    await Promise.all([...document.querySelectorAll('.native-sprite-mascot>span')].map(async span=>{
      const image=new Image();image.src=span.style.backgroundImage.slice(5,-2);await image.decode();
    }));
  });
  for(const stem of ['claude-programmer','zcode-arcade','deepseek-whale-girl'])await page.locator(`[data-comparison="${stem}"]`).screenshot({path:`${dir}/comparison-${stem}.png`});
  await page.screenshot({path:`${dir}/comparison-all.png`,fullPage:true});
  const samples=[];
  for(const kind of ['claude_chat','zcode','deepseek_harness']){
    await page.goto(`${base}/tests/fixtures/native-mascot-ui.html?kind=${kind}`);
    await page.locator('.native-sprite-mascot[data-renderer="flow"]').waitFor();
    await page.waitForFunction(()=>document.querySelector('.native-sprite-mascot').dataset.actionTextures==='2');
    const result=await page.evaluate(async kind=>{
      const button=document.querySelector('.native-sprite-mascot');
      const image=new Image();image.src=button.querySelector('span').style.backgroundImage.slice(5,-2);await image.decode();
      const {CLAWD_HD,ZCODE_HD,WHALE_HD}=await import('/src/spriteMascotHdAssets.ts');
      const assets={claude_chat:CLAWD_HD,zcode:ZCODE_HD,deepseek_harness:WHALE_HD}[kind];
      const layouts=assets.flow.manifest.transitions.layouts;
      const expectedActionBytes=layouts.slice(0,2).reduce((sum,layout)=>sum+layout.columns*layout.rows*layout.framePixels**2*4,0);
      return {readyWithPrefetchMs:performance.now(),nativeFramePixels:image.naturalWidth,displayPixels:button.getBoundingClientRect().width,
        canvasPixels:button.querySelector('canvas').width,actionTextures:Number(button.dataset.actionTextures),actionBytes:Number(button.dataset.actionBytes),expectedActionBytes};
    },kind);
    assert.equal(result.nativeFramePixels,887);assert.equal(result.actionTextures,2);
    assert.equal(result.canvasPixels,Math.round(result.displayPixels*2));assert.equal(result.actionBytes,result.expectedActionBytes,'only current+next native authored atlases are resident');
    // Only current+next image requests are made on initial load, not all eight.
    const loaded=await page.evaluate(()=>performance.getEntriesByType('resource').filter(r=>['img','fetch'].includes(r.initiatorType)&&/(?:action|motion)-\d\.webp/.test(r.name)).length);
    assert.equal(loaded,2);
    samples.push({kind,...result,loadedActions:loaded});
    // Responsive layout still uses the native HD source.
    await page.setViewportSize({width:390,height:780});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    await page.screenshot({path:`${dir}/narrow-${kind}.png`});
    await page.setViewportSize({width:1180,height:1000});
  }
  const pixelScales=[];
  for(const dpr of [1,1.25,2]){
    const scaled=await browser.newPage({viewport:{width:1180,height:1000},deviceScaleFactor:dpr});
    await scaled.goto(`${base}/tests/fixtures/native-mascot-ui.html?kind=deepseek_harness`);
    await scaled.locator('.native-whale-girl[data-renderer="flow"]').waitFor();
    const result=await scaled.locator('.native-whale-girl').evaluate(button=>({css:button.getBoundingClientRect().width,pixels:button.querySelector('canvas').width,rendering:getComputedStyle(button.querySelector('canvas')).imageRendering}));
    assert.equal(result.pixels,Math.round(result.css*dpr));assert.equal(result.rendering,'auto','fractional scales must not force uneven nearest-neighbor blocks');
    await scaled.locator('.native-whale-girl').screenshot({path:`${dir}/whale-dpr-${dpr}.png`});
    pixelScales.push({dpr,...result});await scaled.close();
  }
  assert.deepEqual(errors,[]);
  writeFileSync(`${dir}/hd-ui-results.json`,JSON.stringify({samples,pixelScales,errors},null,2));console.log({samples,pixelScales,errors});
}finally{await browser.close();}
