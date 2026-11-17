import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync,writeFileSync} from 'node:fs';

const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const dir=process.env.MASCOT_QA_DIR||'.scratch/mascot-overlap-fix/layered-regression';
mkdirSync(dir,{recursive:true});
const browser=await chromium.launch({channel:'chrome',headless:true});
const page=await browser.newPage({viewport:{width:1200,height:900},deviceScaleFactor:2});
const errors=[];page.on('pageerror',error=>errors.push(error.message));
try{
  await page.goto(`${base}/tests/fixtures/native-mascot-ui.html`);
  const report=await page.evaluate(async()=>{
    const {createSpriteMascotRenderer}=await import('/src/spriteMascotRenderer.ts');
    const {CLAWD_HD,ZCODE_HD}=await import('/src/spriteMascotHdAssets.ts');
    const {spriteMascotTransition}=await import('/src/spriteMascotTransitions.ts');
    const result=[];
    for(const [name,assets,row,physicalFrom,physicalTo]of[['clawd-hand',CLAWD_HD,1,3,4],['zcode-sign',ZCODE_HD,7,5,6]]){
      const canvas=document.createElement('canvas');
      const renderer=await createSpriteMascotRenderer(canvas,assets.atlas,assets.flow);
      const timings=assets.flow.manifest.transitions,route=timings.routes[`${row}:1:2`];
      const index=route.findIndex((value,i)=>value===physicalFrom&&route[i+1]===physicalTo);
      const stops=timings.routeStops?.[`${row}:1:2`]??route.map((_,i)=>i/(route.length-1));
      const arc=timings.segmentStops?.[`${row}:${physicalFrom}:${physicalTo}`]??timings.samples;
      const mix=stops[index]+(stops[index+1]-stops[index])*arc[2];
      const target={from:row*4+1,to:row*4+2,mix};
      const resolved=spriteMascotTransition(target,timings);
      if(assets.flow.manifest.pairs[resolved.fieldKey]?.length!==4||Math.abs(resolved.mix-.5)>1e-6)throw Error('Test must exercise an interior layered geometric pose');
      renderer.prepare(target,row);
      const begin=performance.now();
      while(!renderer.readyFor(target)){if(performance.now()-begin>10000)throw Error('Action load timeout');await new Promise(resolve=>setTimeout(resolve,10));}
      const gl=canvas.getContext('webgl'),blendStates=[],glErrors=[];
      const sample=pose=>{
        if(!renderer.draw(pose,200,200))throw Error('Expected live draw');
        blendStates.push(gl.isEnabled(gl.BLEND));glErrors.push(gl.getError());
        const pixels=new Uint8Array(canvas.width*canvas.height*4);gl.readPixels(0,0,canvas.width,canvas.height,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return pixels;
      };
      const idleBefore=sample({from:row*4,to:row*4,mix:0});
      const expected=sample(target);let visible=0;
      for(let i=3;i<expected.length;i+=4)if(expected[i]>0)visible++;
      let historyErrors=0,endpointErrors=0;
      for(const previous of[
        {from:row*4,to:row*4+1,mix:.2},
        {...target,mix:.01},{...target,mix:.99},
        {from:row*4+2,to:row*4+3,mix:.55},
        {from:row*4+3,to:row*4+3,mix:0},
      ]){
        sample(previous);const actual=sample(target);
        for(let i=0;i<actual.length;i++)if(actual[i]!==expected[i])historyErrors++;
      }
      const idleAfter=sample({from:row*4+2,to:row*4+3,mix:1});
      for(let i=0;i<idleAfter.length;i++)if(idleAfter[i]!==idleBefore[i])endpointErrors++;
      result.push({name,historyErrors,endpointErrors,visible,blendStates,glErrors,stats:renderer.stats});
      renderer.dispose();
    }
    return result;
  });
  for(const item of report){
    assert.ok(item.visible>10000,`${item.name}: real nonempty first picture`);
    assert.equal(item.historyErrors,0,`${item.name}: no previous-frame accumulation`);
    assert.equal(item.endpointErrors,0,`${item.name}: shared idle unchanged after layered rendering`);
    assert.ok(item.blendStates.every(value=>value===false),'each draw restores disabled blending');
    assert.ok(item.glErrors.every(value=>value===0),'no suppressed WebGL error');
    assert.ok(item.stats.resident<=2&&item.stats.pending<=2,'current and prefetched action cache remains bounded');
  }
  assert.deepEqual(errors,[]);
  writeFileSync(`${dir}/layered-state.json`,JSON.stringify({report,errors},null,2));
  console.log(JSON.stringify({report,errors},null,2));
}finally{await browser.close();}
