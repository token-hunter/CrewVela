const { test } = require('node:test');
const assert = require('node:assert/strict');
const { workflowPolicy } = require('../src-tauri/resources/zcode-workflow-policy.cjs');
test('public remote modes preserve native semantics and never send credentials',async()=>{
  for (const mode of ['disabled','onDemand','alwaysOn']) {
    const value=await workflowPolicy({version:'test'},async(url,options)=>{
      assert.equal(url.pathname,'/api/v1/client/configs');
      assert.equal(url.searchParams.get('app_version'),'test');
      assert.equal(url.searchParams.get('platform'),`${process.platform}-${process.arch}`);
      assert.equal(options.headers,undefined);assert.equal(options.method,'GET');assert.equal(options.redirect,'error');
      return new Response(JSON.stringify({data:{configs:{dynamicWorkflow:{mode}}}}));
    });
    assert.equal(value.mode,mode);assert.equal(value.enabled,mode!=='disabled');
  }
});
test('missing, invalid, rejected and unreachable configs fail closed with a visible reason',async()=>{
  const responses=[new Response('{}'),new Response(JSON.stringify({data:{configs:{dynamicWorkflow:{mode:'invalid'}}}})),new Response('{}',{status:500}),new Response(JSON.stringify({success:false}))];
  for (const response of responses) { const value=await workflowPolicy({version:'test'},async()=>response);assert.equal(value.enabled,false);assert.ok(value.notice); }
  const value=await workflowPolicy({version:'test'},async()=>{throw Error('private transport detail');});assert.equal(value.enabled,false);assert.ok(!value.notice.includes('private'));
});
