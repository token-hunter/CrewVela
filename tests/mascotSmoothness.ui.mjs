import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';

const dir=process.env.MASCOT_QA_DIR||'.scratch/mascot-smoothness-20261117';
mkdirSync(dir,{recursive:true});
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const browser=await chromium.launch({channel:'chrome',headless:true});
const page=await browser.newPage({viewport:{width:1440,height:1000},deviceScaleFactor:2});
const errors=[];
page.on('pageerror',e=>errors.push(e.message));
try {
  await page.goto(`${base}/tests/fixtures/native-mascot-ui.html`);
  await page.locator('[data-renderer="flow"]').nth(2).waitFor();
  // Measure live production components, including their shader output. Pixel
  // readback is sampled separately at 20Hz so this is a conservative density
  // bound, not a claim that a timer firing means a new picture was rendered.
  const report=process.env.MASCOT_QUALITY_ONLY ? null : await page.evaluate(async()=>{
    const nodes=[...document.querySelectorAll('.native-sprite-mascot')];
    const series=nodes.map(()=>[]),pixels=nodes.map(()=>[]),last=nodes.map(()=>''),lastHash=nodes.map(()=>0);
    const buffers=nodes.map(node=>new Uint8Array(node.querySelector('canvas').width*node.querySelector('canvas').height*4));
    const contexts=nodes.map(node=>node.querySelector('canvas').getContext('webgl'));
    const assets=nodes.map(()=>({resident:0,pending:0,bytes:0}));
    await new Promise(resolve=>setTimeout(resolve,700));
    const begin=performance.now(),frames=[],longTasks=[];
    let lastPixel=0;
    const observer=new PerformanceObserver(list=>list.getEntries().forEach(e=>{if(e.startTime>=begin)longTasks.push(e.duration);}));
    observer.observe({entryTypes:['longtask']});
    await new Promise(resolve=>{
      function sample(time){
        frames.push(time);
        nodes.forEach((node,i)=>{
          assets[i].resident=Math.max(assets[i].resident,Number(node.dataset.actionTextures||0));
          assets[i].pending=Math.max(assets[i].pending,Number(node.dataset.actionPending||0));
          assets[i].bytes=Math.max(assets[i].bytes,Number(node.dataset.actionBytes||0));
          const key=node.dataset.interpolation;
          if(key!==last[i]){series[i].push({time:time-begin,motion:node.dataset.motion,pose:key});last[i]=key;}
        });
        if(time-lastPixel>=45){
          lastPixel=time;
          nodes.forEach((node,i)=>{
            const canvas=node.querySelector('canvas'),gl=contexts[i],data=buffers[i];
            gl.readPixels(0,0,canvas.width,canvas.height,gl.RGBA,gl.UNSIGNED_BYTE,data);
            let hash=2166136261;for(let p=0;p<data.length;p+=4)hash=Math.imul(hash^data[p]^data[p+1]^data[p+2]^data[p+3],16777619);
            if(hash!==lastHash[i]){pixels[i].push({time:time-begin,motion:node.dataset.motion});lastHash[i]=hash;}
          });
        }
        if(time-begin<27000)requestAnimationFrame(sample);else resolve();
      }requestAnimationFrame(sample);
    });
    observer.disconnect();
    const percentile=(list,q)=>[...list].sort((a,b)=>a-b)[Math.min(list.length-1,Math.floor(list.length*q))]??0;
    const gaps=frames.slice(1).map((time,i)=>time-frames[i]);
    const targets=['敲键盘','摆尾','节奏踏步']; // DOM order: Clawd, whale girl, ZCode.
    return {durationMs:performance.now()-begin,raf:{count:frames.length,median:percentile(gaps,.5),p95:percentile(gaps,.95),max:Math.max(...gaps),over50:gaps.filter(x=>x>50).length},longTasks,
      characters:nodes.map((node,i)=>{
        const selected=series[i].filter(s=>s.motion===targets[i]),px=pixels[i].filter(s=>s.motion===targets[i]);
        const intervals=selected.slice(1).map((s,n)=>s.time-selected[n].time).filter(x=>x<1000);
        const pixelGaps=px.slice(1).map((s,n)=>s.time-px[n].time).filter(x=>x<1000);
        return {name:node.className,target:targets[i],renderer:node.dataset.renderer,css:node.getBoundingClientRect().width,canvasPixels:node.querySelector('canvas').width,
          actionNames:[...new Set(series[i].map(s=>s.motion))],poseUpdates:selected.length,medianPoseMs:percentile(intervals,.5),p95PoseMs:percentile(intervals,.95),
          uniquePixelSamples:px.length,medianPixelChangeMs:percentile(pixelGaps,.5),p95PixelChangeMs:percentile(pixelGaps,.95),peakActionTextures:assets[i]};
      })};
  });
  if(report)writeFileSync(`${dir}/after-measurement.json`,JSON.stringify(report,null,2));
  for(const character of report?.characters??[]){
    assert.equal(character.renderer,'flow');
    assert.equal(character.actionNames.length,8,'all authored actions still play');
    assert.ok(character.medianPoseMs<40,JSON.stringify(character));
    assert.ok(character.uniquePixelSamples>20,'real changing pixels, not just frame callbacks');
    assert.ok(character.medianPixelChangeMs<90,JSON.stringify(character));
    assert.equal(character.canvasPixels,Math.round(character.css*2),'device pixel ratio respected');
    assert.ok(character.peakActionTextures.resident<=2&&character.peakActionTextures.pending<=2,'HD action residency and decode concurrency bounded');
  }
  if(report)assert.ok(report.raf.p95<40,`refresh p95 ${report.raf.p95}`);
  await page.screenshot({path:`${dir}/all-mascots-after.png`,fullPage:true});

  // Inspect the exact renderer at 21 positions across each action's most
  // different pair. Endpoints match, intermediate pixels differ from an
  // unaligned dissolve, and adjacent samples cannot jump like a flipbook.
  const quality=await page.evaluate(async()=>{
    const {createSpriteMascotRenderer}=await import('/src/spriteMascotRenderer.ts');
    const {CLAWD_HD,WHALE_HD,ZCODE_HD}=await import('/src/spriteMascotHdAssets.ts');
    const kinds=[['claude-programmer','Clawd',CLAWD_HD],['deepseek-whale-girl','鲸鱼娘',WHALE_HD],['zcode-arcade','ZCode',ZCODE_HD]];
    const quality=[];
    document.body.innerHTML='<main id="quality" style="padding:20px;background:#141413;color:white"></main>';
    for(const [stem,label,assets] of kinds){
      const section=document.createElement('section');section.dataset.quality=stem;
      section.innerHTML=`<h2>${label}：原姿态 / 连续中间姿态 / 原姿态</h2><div class="strips" style="display:flex;gap:6px"></div>`;
      document.querySelector('#quality').append(section);
      const canvas=document.createElement('canvas');
      const renderer=await createSpriteMascotRenderer(canvas,assets.atlas,assets.flow);
      const gl=canvas.getContext('webgl'),size=160;
      const read=()=>{const data=new Uint8Array(size*size*4);gl.readPixels(0,0,size,size,gl.RGBA,gl.UNSIGNED_BYTE,data);return data;};
      const difference=(a,b)=>{let sum=0;for(let i=0;i<a.length;i++)sum+=Math.abs(a[i]-b[i]);return sum/a.length;};
      const hash=data=>{let h=2166136261;for(let i=0;i<data.length;i++)h=Math.imul(h^data[i],16777619);return h;};
      for(let row=0;row<8;row++){
        const readyPose={from:row*4+1,to:row*4+2,mix:.5};
        renderer.prepare(readyPose,row);
        const loading=performance.now();
        while(!renderer.readyFor(readyPose)){
          if(performance.now()-loading>10000)throw Error('HD action decode timeout');
          await new Promise(resolve=>setTimeout(resolve,10));
        }
        let best=null;
        const keys=[];for(let frame=0;frame<4;frame++){renderer.draw({from:row*4+frame,to:row*4+frame,mix:0},80,80);keys.push(read());}
        for(let a=0;a<4;a++)for(let b=a+1;b<4;b++){
          const distance=difference(keys[a],keys[b]);if(!best||distance>best.distance)best={a,b,distance};
        }
        const from=row*4+best.a,to=row*4+best.b,seen=new Set();let previous=null,maxStep=0,midpoint;
        const strip=document.createElement('div');strip.style.width='164px';strip.innerHTML=`<div>动作 ${row+1}</div>`;section.querySelector('.strips').append(strip);
        let endpoints=[];
        for(let step=0;step<=20;step++){
          renderer.draw({from,to,mix:step/20},80,80);
          const data=read();seen.add(hash(data));if(previous)maxStep=Math.max(maxStep,difference(previous,data));previous=data;
          if(step===10)midpoint=data;
          if(step===0||step===20)endpoints.push(data);
          if([0,10,20].includes(step)){
            const copy=document.createElement('canvas');copy.width=copy.height=size;copy.style.cssText='display:block;width:160px;height:160px';
            copy.getContext('2d').drawImage(canvas,0,0);strip.append(copy);
          }
        }
        const dissolve=new Uint8Array(midpoint.length);for(let p=0;p<dissolve.length;p++)dissolve[p]=Math.round((endpoints[0][p]+endpoints[1][p])/2);
        quality.push({stem,row,from,to,keyDistance:best.distance,uniqueSamples:seen.size,maxAdjacentDifference:maxStep,dissolveDifference:difference(midpoint,dissolve),
          endpointErrors:[difference(endpoints[0],keys[best.a]),difference(endpoints[1],keys[best.b])]});
      }
      renderer.dispose();
    }
    return quality;
  });
  writeFileSync(`${dir}/interpolation-quality.json`,JSON.stringify(quality,null,2));
  for(const stem of ['claude-programmer','deepseek-whale-girl','zcode-arcade'])await page.locator(`[data-quality="${stem}"]`).screenshot({path:`${dir}/intermediates-${stem}.png`});
  for(const q of quality){
    assert.ok(q.uniqueSamples>=19,`${q.stem} action ${q.row+1}: real in-between pixels`);
    assert.ok(q.endpointErrors.every(e=>e===0),'authored keyframes unchanged');
    assert.ok(q.maxAdjacentDifference<q.keyDistance*.45+.5,JSON.stringify(q));
    assert.ok(q.dissolveDifference>.1,`${q.stem} ${q.row}: not a whole-image crossfade`);
  }

  // A failed GPU/texture must leave original artwork and functional commands.
  const fallback=await browser.newPage();
  const warnings=[];fallback.on('console',m=>{if(m.type()==='warning')warnings.push(m.text());});
  await fallback.addInitScript(()=>{const original=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(type,...args){return type==='webgl'?null:original.call(this,type,...args);};});
  await fallback.goto(`${base}/tests/fixtures/native-mascot-ui.html?kind=claude_chat`);
  await fallback.locator('.native-clawd[data-renderer="static-fallback"]').waitFor();
  assert.match(warnings.join('\n'),/Continuous mascot animation unavailable/);
  assert.notEqual(await fallback.locator('.native-clawd > span').evaluate(e=>getComputedStyle(e).backgroundImage),'none');
  await fallback.getByRole('button',{name:'查看命令',exact:true}).click();
  assert.match(await fallback.locator('output').textContent(),/claude_chat 命令入口已响应/);
  await fallback.close();
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({report,qualityChecks:quality.length,fallback:true,errors},null,2));
}finally{await browser.close();}
