import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

const code=ts.transpileModule(readFileSync(new URL('../src/spriteMascotTransitions.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText;
const exports={};new Function('exports',code)(exports);
const {spriteMascotTransition,spriteMascotRigidTransform}=exports;
const data={routes:{'1:0:1':[0,3,4,1],'1:1:2':[1,4,3,2],'1:0:2':[0,2],'1:0:3':[0,0],'1:1:3':[1,4,3,0],'1:2:3':[2,0]},
  samples:[0,.25,.5,.75,1],layouts:Array.from({length:8},()=>({columns:2,rows:2,count:4,framePixels:887}))};

test('a large hand gesture crosses real authored intermediate poses without snapping or cross-action ambiguity',()=>{
  const resolve=t=>spriteMascotTransition({from:4,to:5,mix:t},data);
  assert.deepEqual([resolve(0).from,resolve(0).mix],[0,0]);
  assert.deepEqual([resolve(1).to,resolve(1).mix],[1,1]);
  for(const boundary of[1/3,2/3]){
    const before=resolve(boundary-1e-8),after=resolve(boundary);
    assert.equal(before.to,after.from,'neighboring spans share exactly one authored pose');
    assert.ok(before.mix>1-1e-6);assert.equal(after.mix,0);
  }
  const middle=resolve(.5);assert.deepEqual([middle.from,middle.to],[3,4]);assert.ok(Math.abs(middle.mix-.5)<1e-12);
});

test('reverse/recovery uses the exact same geometry, temporal samples and endpoint artwork',()=>{
  for(const [a,b]of[[0,1],[1,2],[0,2],[1,3],[2,3]])for(const t of[.01,.123,.25,.49,.5,.75,.999]){
    const forward=spriteMascotTransition({from:4+a,to:4+b,mix:t},data);
    const reverse=spriteMascotTransition({from:4+b,to:4+a,mix:1-t},data);
    const distribution=result=>{
      const weights={};weights[result.from]=(weights[result.from]||0)+1-result.mix;weights[result.to]=(weights[result.to]||0)+result.mix;
      return weights;
    };
    const f=distribution(forward),r=distribution(reverse);
    for(const key of new Set([...Object.keys(f),...Object.keys(r)]))assert.ok(Math.abs((f[key]||0)-(r[key]||0))<1e-10);
    assert.equal(forward.fieldKey,reverse.fieldKey);
    assert.ok(Math.abs(forward.sample+forward.sampleMix-reverse.sample-reverse.sampleMix)<1e-10);
  }
  assert.equal(spriteMascotTransition({from:7,to:7,mix:.7},data).from,0,'logical rest frame 3 is the shared original rest');
  assert.throws(()=>spriteMascotTransition({from:0,to:1,mix:.5},data),/Missing mascot pose route/);
});

test('distance timing gives larger movements more time while reverse playback samples identical geometry',()=>{
  const timed={...data,routeStops:{'1:0:1':[0,.2,.8,1]},segmentStops:{'1:3:4':[0,.1,.4,.9,1]}};
  const forward=spriteMascotTransition({from:4,to:5,mix:.5},timed);
  const reverse=spriteMascotTransition({from:5,to:4,mix:.5},timed);
  assert.deepEqual([forward.from,forward.to],[3,4]);assert.ok(Math.abs(forward.mix-.55)<1e-10);
  assert.deepEqual([reverse.from,reverse.to],[4,3]);assert.ok(Math.abs(reverse.mix-.45)<1e-10);
  assert.equal(forward.fieldKey,reverse.fieldKey);assert.equal(forward.sample,reverse.sample);
  assert.ok(Math.abs(forward.sampleMix-reverse.sampleMix)<1e-10);
  assert.equal(spriteMascotTransition({from:4,to:5,mix:0},timed).from,0);
  assert.equal(spriteMascotTransition({from:4,to:5,mix:1},timed).to,1);
});

test('production authored assets describe native 887px cells and have all live and recovery routes',()=>{
  for(const stem of['claude-programmer','zcode-arcade','deepseek-whale-girl']){
    const manifest=JSON.parse(readFileSync(new URL(`../src/assets/mascots-hd/${stem}/motion-manifest.json`,import.meta.url),'utf8'));
    const {transitions}=manifest;
    assert.equal(manifest.algorithm,'rife4.25-keyframe-paths-v1');assert.equal(transitions.layouts.length,8);
    assert.ok(manifest.width<=4096&&manifest.height<=4096,'motion maps also fit the portable GPU allocation');
    assert.deepEqual(transitions.samples,[0,.25,.5,.75,1]);
    for(const [key,stops]of Object.entries(transitions.routeStops??{})){
      assert.equal(stops.length,transitions.routes[key].length);assert.equal(stops[0],0);assert.equal(stops.at(-1),1);
      assert.ok(stops.every((value,i)=>Number.isFinite(value)&&(i===0||value>stops[i-1])));
    }
    for(const stops of Object.values(transitions.segmentStops??{})){
      assert.equal(stops.length,transitions.samples.length);assert.equal(stops[0],0);assert.equal(stops.at(-1),1);
      assert.ok(stops.every((value,i)=>Number.isFinite(value)&&(i===0||value>stops[i-1])));
    }
    for(let row=0;row<8;row++){
      const layout=transitions.layouts[row];assert.equal(layout.framePixels,887);assert.ok(layout.columns*887<=4096&&layout.rows*887<=4096);
      for(let a=0;a<4;a++)for(let b=0;b<4;b++)for(const t of[0,.125,.25,.5,.75,.875,1]){
        const pose=spriteMascotTransition({from:row*4+a,to:row*4+b,mix:t},transitions);
        assert.ok(pose.from>=0&&pose.from<=layout.count&&pose.to>=0&&pose.to<=layout.count);
        assert.ok(pose.mix>=0&&pose.mix<=1&&pose.sampleMix>=0&&pose.sampleMix<=1);
        if(pose.from!==pose.to){
          const entry=manifest.pairs[pose.fieldKey];assert.ok(entry,`${stem} ${pose.fieldKey}`);
          const capacity=(manifest.width/(manifest.tile+2*manifest.padding))*(manifest.height/(manifest.tile+2*manifest.padding));
          for(let offset=0;offset<entry.length;offset+=2)assert.ok(entry[offset]+pose.sample+1<capacity,'both temporal samples and rigid layers are in bounds');
        }
      }
    }
  }
});

test('rigid prop coordinates preserve all four corners through a rotating/scaling plane and reject degenerate data',()=>{
  const a=[[.6,.4],[.9,.35],[.92,.47],[.62,.52]],b=[[.57,.28],[.89,.27],[.895,.4],[.575,.41]];
  for(const t of[0,.1,.5,.9,1]){
    const points=a.map((p,i)=>p.map((v,j)=>v*(1-t)+b[i][j]*t)),matrix=spriteMascotRigidTransform(points);
    points.forEach(([x,y],i)=>{
      const mapped=[matrix[0]*x+matrix[3]*y+matrix[6],matrix[1]*x+matrix[4]*y+matrix[7]];
      const expected=[[0,0],[1,0],[1,1],[0,1]][i];
      assert.ok(mapped.every((v,j)=>Math.abs(v-expected[j])<1e-5));
    });
  }
  assert.throws(()=>spriteMascotRigidTransform([[0,0],[0,0],[0,0],[0,0]]),/Invalid mascot rigid prop plane/);
});
