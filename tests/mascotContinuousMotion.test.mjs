import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import ts from 'typescript';

function moduleAt(file) {
  const javascript = ts.transpileModule(readFileSync(new URL(file, import.meta.url), 'utf8'), {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText;
  const exports = {};
  new Function('exports', 'require', javascript)(exports, () => ({}));
  return exports;
}
const { SpriteMascotTimeline, spriteMascotPose } = moduleAt('../src/spriteMascotMotion.ts');
const { hdSpritePose } = moduleAt('../src/spriteMascotRenderer.ts');
const configs = [
  ['claude-programmer', moduleAt('../src/clawdMotion.ts').CLAWD_MOTIONS],
  ['zcode-arcade', moduleAt('../src/zcodeArcadeMotion.ts').ZCODE_ARCADE_MOTIONS],
  ['deepseek-whale-girl', moduleAt('../src/whaleGirlMotion.ts').WHALE_GIRL_MOTIONS],
];
function key(pose) { return `${Math.min(pose.from,pose.to)}:${Math.max(pose.from,pose.to)}`; }

for (const [stem,motions] of configs) {
  const manifest = JSON.parse(readFileSync(new URL(`../src/assets/${stem}-flow.json`,import.meta.url),'utf8'));
  test(`${stem}: displacement data matches the original artwork and every played/reversed/skip transition`, () => {
    const hash = createHash('sha256').update(readFileSync(new URL(`../src/assets/${stem}-atlas.png`,import.meta.url))).digest('hex');
    assert.equal(manifest.sourceSha256,hash);
    assert.equal(manifest.algorithm,'rife4.25-midpoint-backward');
    assert.ok(manifest.width <= 2048 && manifest.height <= 2048,'portable WebGL 1 texture dimensions');
    for(let row=0;row<8;row++) {
      for(let time=0;time<=motions[row].duration;time+=7) {
        const p=spriteMascotPose(motions,row,time);
        assert.ok(p.from===p.to || manifest.pairs[key(p)],`missing ${key(p)}`);
        assert.ok(p.mix>=0&&p.mix<=1);
      }
    }
    const timeline=new SpriteMascotTimeline(motions);
    for(let index=0;index<600;index++) {
      timeline.advance(index%31);
      const before=timeline.sample();
      timeline.next();
      assert.deepEqual(timeline.sample(),before,'click starts from the exact current in-between pose');
      for(const delta of [17,47,56,96,64,73]) {
        timeline.advance(delta);
        const p=timeline.sample();
        assert.ok(p.from===p.to || manifest.pairs[key(p)],`missing interrupted ${key(p)}`);
      }
    }
  });
  test(`${stem}: action boundaries are continuous and active motions produce real intermediate samples`, () => {
    for(let row=0;row<8;row++) {
      const end=spriteMascotPose(motions,row,motions[row].duration);
      const next=spriteMascotPose(motions,(row+1)%8,0);
      assert.equal(end.to,next.from);
      assert.equal(end.mix,1);
      assert.equal(next.mix,0);
    }
    // 60fps on an authored changing segment must produce many sub-poses, not
    // a 60fps timer which still chooses the same four images.
    const row=stem==='zcode-arcade'?6:1;
    const action=motions[row];
    const samples=new Set();
    for(let time=0;time<action.duration;time+=1000/60) {
      const p=spriteMascotPose(motions,row,time);
      if(p.from!==p.to) samples.add(`${p.from}:${p.to}:${p.mix.toFixed(5)}`);
    }
    assert.ok(samples.size>60,`${samples.size} continuous intermediate poses`);
  });
}

for (const [stem,motions] of configs) test(`${stem}: all native HD assets are intact and compact flow covers live/rapid-switch poses`,()=>{
  const manifest=JSON.parse(readFileSync(new URL(`../src/assets/mascots-hd/${stem}/manifest.json`,import.meta.url),'utf8'));
  assert.equal(manifest.layout,'action-pairs');assert.ok(manifest.framePixels>=768);assert.equal(manifest.sources.length,8);
  assert.equal(Object.keys(manifest.pairs).length,48);
  for(const source of manifest.sources){
    assert.ok(source.nativeFramePixels>=768);
    const path=new URL(`../src/assets/mascots-hd/${stem}/action-${source.row}.webp`,import.meta.url);
    assert.equal(createHash('sha256').update(readFileSync(path)).digest('hex'),source.assetSha256);
  }
  const check=p=>{p=hdSpritePose(p);assert.ok(p.from===p.to||manifest.pairs[key(p)],`missing HD transition ${key(p)}`);assert.equal(Math.floor(p.from/4),Math.floor(p.to/4));};
  for(let row=0;row<8;row++)for(let t=0;t<=motions[row].duration;t+=11)check(spriteMascotPose(motions,row,t));
  const timeline=new SpriteMascotTimeline(motions);
  for(let n=0;n<700;n++){timeline.advance(n%53);timeline.next();for(const delta of[17,84,57,133]){timeline.advance(delta);check(timeline.sample());}}
});

test('long blink preparation remains a deliberate hold, and elapsed wall time is independent of frame rate', () => {
  const motions=configs[0][1];
  assert.equal(spriteMascotPose(motions,0,3000).mix,0);
  const a=new SpriteMascotTimeline(motions),b=new SpriteMascotTimeline(motions);
  a.advance(60000);
  for(let i=0;i<6000;i++)b.advance(10);
  assert.equal(a.motion,b.motion); assert.equal(a.elapsed,b.elapsed); assert.deepEqual(a.sample(),b.sample());
});
