import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const code=ts.transpileModule(readFileSync(new URL('../src/spriteMascotActionTextures.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText;
const exports={};new Function('exports',code)(exports);
const {SpriteMascotActionTextures}=exports;

test('HD action loading cancels obsolete decodes and holds at most two GPU pairs; failed uploads and disposal release resources',async()=>{
  const original=globalThis.Image,images=[],warnings=[];
  const warn=console.warn;
  class FakeImage{
    width=1774;height=887;onload=null;onerror=null;src='';
    constructor(){images.push(this);}
    finish(){return this.onload?.();}
  }
  globalThis.Image=FakeImage;console.warn=(...args)=>warnings.push(args);
  let count=0,peak=0,allocated=0,deleted=0,error=0;
  const gl={TEXTURE3:3,TEXTURE_2D:2,UNPACK_PREMULTIPLY_ALPHA_WEBGL:4,UNPACK_COLORSPACE_CONVERSION_WEBGL:5,NONE:0,TEXTURE_MIN_FILTER:6,TEXTURE_MAG_FILTER:7,LINEAR:8,TEXTURE_WRAP_S:9,TEXTURE_WRAP_T:10,CLAMP_TO_EDGE:11,RGBA:12,UNSIGNED_BYTE:13,MAX_TEXTURE_SIZE:14,NO_ERROR:0,
    createTexture(){count++;allocated++;peak=Math.max(peak,count);return{};},deleteTexture(){count--;deleted++;},activeTexture(){},bindTexture(){},pixelStorei(){},texParameteri(){},texImage2D(){},getError(){return error;},getParameter(){return 4096;}};
  try{
    const pool=new SpriteMascotActionTextures(gl,Array.from({length:8},(_,i)=>`action-${i}`));
    pool.prepare([0,1]);assert.equal(pool.stats.pending,2);
    const obsolete=images[1].onload;
    pool.prepare([0,2]);assert.equal(images[1].src,'');assert.equal(pool.stats.pending,2);
    await obsolete();assert.equal(allocated,0,'late completion from cancelled work cannot upload');
    await images[0].finish();assert.equal(pool.stats.resident,1);assert.equal(pool.stats.pending,1);
    pool.prepare([2,3]);assert.equal(count,0,'old GPU action evicted');assert.equal(pool.stats.pending,2);
    await images[2].finish();await images[3].finish();
    assert.equal(pool.stats.resident,2);assert.equal(pool.stats.bytes,2*1774*887*4);
    pool.prepare([3,4]);images[4].width=1024;images[4].height=512;await images[4].finish();
    assert.throws(()=>pool.get(4),/native HD/);assert.equal(count,1,'invalid low resolution data is rejected');
    pool.prepare([3,5]);error=99;await images[5].finish();error=0;
    assert.throws(()=>pool.get(5),/upload failed/);assert.equal(count,1,'failed upload deletes its allocation');
    pool.prepare([6,7]);assert.equal(count,0);assert.equal(pool.stats.pending,2);
    pool.dispose();assert.equal(pool.stats.pending,0);assert.equal(count,0);assert.equal(allocated,deleted);
    assert.ok(peak<=2);assert.equal(warnings.length,2);assert.equal(images.every(image=>image.src===''),true);
  }finally{globalThis.Image=original;console.warn=warn;}
});

test('authored paths keep native frame dimensions and reject a GPU limit without allocating or downscaling',async()=>{
  const original=globalThis.Image,warn=console.warn,images=[];let allocated=0,deleted=0,limit=4096;
  class FakeImage{width=2661;height=1774;src='';onload=null;onerror=null;constructor(){images.push(this);}finish(){return this.onload?.();}}
  globalThis.Image=FakeImage;console.warn=()=>{};
  const gl={NO_ERROR:0,createTexture(){allocated++;return{};},deleteTexture(){deleted++;},activeTexture(){},bindTexture(){},pixelStorei(){},texParameteri(){},texImage2D(){},getError(){return 0;},getParameter(){return limit;}};
  const layouts=Array.from({length:8},()=>({columns:3,rows:2,count:6,framePixels:887}));
  try{
    const pool=new SpriteMascotActionTextures(gl,Array.from({length:8},(_,i)=>`motion-${i}`),layouts);
    pool.prepare([0,1]);await images[0].finish();await images[1].finish();
    assert.equal(pool.stats.resident,2);assert.equal(pool.stats.bytes,2*2661*1774*4);
    pool.prepare([2]);limit=2048;await images[2].finish();
    assert.throws(()=>pool.get(2),/GPU texture limit 2048/);assert.equal(allocated,2,'oversize source is not allocated/downscaled');
    pool.dispose();assert.equal(deleted,allocated);assert.equal(pool.stats.resident,0);
  }finally{globalThis.Image=original;console.warn=warn;}
});

test('asynchronous Blob bitmaps are premultiplied, obsolete requests abort, and every bitmap closes',async()=>{
  const originalImage=globalThis.Image,originalBitmap=globalThis.createImageBitmap,originalFetch=globalThis.fetch,images=[],jobs=[],closed=[],requests=[];let allocated=0,deleted=0;
  class FakeImage{width=1774;height=887;src='';onload=null;onerror=null;constructor(){images.push(this);}decode(){return Promise.resolve();}finish(){return this.onload?.();}}
  globalThis.Image=FakeImage;
  globalThis.fetch=async(url,options)=>{requests.push({url,...options});return {ok:true,blob:async()=>({encoded:true,url})};};
  globalThis.createImageBitmap=(blob,options)=>{assert.equal(blob.encoded,true);assert.deepEqual(options,{premultiplyAlpha:'premultiply',colorSpaceConversion:'none'});return new Promise(resolve=>jobs.push({blob,resolve}));};
  const gl={NO_ERROR:0,createTexture(){allocated++;return{};},deleteTexture(){deleted++;},activeTexture(){},bindTexture(){},pixelStorei(){},texParameteri(){},texImage2D(...args){assert.equal(args.at(-1).bitmap,true);},getError(){return 0;},getParameter(){return 4096;}};
  try{
    const pool=new SpriteMascotActionTextures(gl,Array.from({length:8},(_,i)=>`motion-${i}`));pool.prepare([0,1]);
    await new Promise(resolve=>setImmediate(resolve));
    assert.equal(jobs.length,2);pool.prepare([0,2]);assert.equal(requests[1].signal.aborted,true);
    jobs.slice(0,2).forEach((job,i)=>job.resolve({bitmap:true,width:1774,height:887,close(){closed.push(i);}}));
    await new Promise(resolve=>setImmediate(resolve));assert.equal(allocated,1);assert.equal(pool.stats.resident,1);assert.deepEqual(closed.sort(),[0,1]);
    pool.dispose();assert.equal(deleted,allocated);
    jobs[2].resolve({bitmap:true,width:1774,height:887,close(){closed.push(2);}});await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(closed.sort(),[0,1,2]);
  }finally{globalThis.Image=originalImage;globalThis.fetch=originalFetch;if(originalBitmap===undefined)delete globalThis.createImageBitmap;else globalThis.createImageBitmap=originalBitmap;}
});
