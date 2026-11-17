import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const code=ts.transpileModule(readFileSync(new URL('../src/spriteMascotActionTextures.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText;
const exports={};new Function('exports',code)(exports);
const {SpriteMascotActionTextures}=exports;

test('HD action loading cancels obsolete decodes and holds at most two GPU pairs; failed uploads and disposal release resources',()=>{
  const original=globalThis.Image,images=[],warnings=[];
  const warn=console.warn;
  class FakeImage{
    width=1774;height=887;onload=null;onerror=null;src='';
    constructor(){images.push(this);}
    finish(){this.onload?.();}
  }
  globalThis.Image=FakeImage;console.warn=(...args)=>warnings.push(args);
  let count=0,peak=0,allocated=0,deleted=0,error=0;
  const gl={TEXTURE3:3,TEXTURE_2D:2,UNPACK_PREMULTIPLY_ALPHA_WEBGL:4,UNPACK_COLORSPACE_CONVERSION_WEBGL:5,NONE:0,TEXTURE_MIN_FILTER:6,TEXTURE_MAG_FILTER:7,LINEAR:8,TEXTURE_WRAP_S:9,TEXTURE_WRAP_T:10,CLAMP_TO_EDGE:11,RGBA:12,UNSIGNED_BYTE:13,NO_ERROR:0,
    createTexture(){count++;allocated++;peak=Math.max(peak,count);return{};},deleteTexture(){count--;deleted++;},activeTexture(){},bindTexture(){},pixelStorei(){},texParameteri(){},texImage2D(){},getError(){return error;}};
  try{
    const pool=new SpriteMascotActionTextures(gl,Array.from({length:8},(_,i)=>`action-${i}`));
    pool.prepare([0,1]);assert.equal(pool.stats.pending,2);
    const obsolete=images[1].onload;
    pool.prepare([0,2]);assert.equal(images[1].src,'');assert.equal(pool.stats.pending,2);
    obsolete();assert.equal(allocated,0,'late completion from cancelled work cannot upload');
    images[0].finish();assert.equal(pool.stats.resident,1);assert.equal(pool.stats.pending,1);
    pool.prepare([2,3]);assert.equal(count,0,'old GPU action evicted');assert.equal(pool.stats.pending,2);
    images[2].finish();images[3].finish();
    assert.equal(pool.stats.resident,2);assert.equal(pool.stats.bytes,2*1774*887*4);
    pool.prepare([3,4]);images[4].width=1024;images[4].height=512;images[4].finish();
    assert.throws(()=>pool.get(4),/native HD/);assert.equal(count,1,'invalid low resolution data is rejected');
    pool.prepare([3,5]);error=99;images[5].finish();error=0;
    assert.throws(()=>pool.get(5),/upload failed/);assert.equal(count,1,'failed upload deletes its allocation');
    pool.prepare([6,7]);assert.equal(count,0);assert.equal(pool.stats.pending,2);
    pool.dispose();assert.equal(pool.stats.pending,0);assert.equal(count,0);assert.equal(allocated,deleted);
    assert.ok(peak<=2);assert.equal(warnings.length,2);assert.equal(images.every(image=>image.src===''),true);
  }finally{globalThis.Image=original;console.warn=warn;}
});
