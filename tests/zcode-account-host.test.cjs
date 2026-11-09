const {test}=require('node:test');
const assert=require('node:assert/strict');
const {createCipheriv,createHash}=require('node:crypto');
const {mkdtemp,writeFile,mkdir,rm}=require('node:fs/promises');
const {tmpdir}=require('node:os');
const {join}=require('node:path');
const host=require('../src-tauri/resources/zcode-account-host.cjs');
test('native encrypted credentials and corrupt authentication tag',()=>{
  const secret='isolated-secret',iv=Buffer.alloc(12,1);
  const cipher=createCipheriv('aes-256-gcm',createHash('sha256').update(secret).digest(),iv);
  const body=Buffer.concat([cipher.update('synthetic-key'),cipher.final()]);
  const encrypted=`enc:v1:${iv.toString('base64url')}.${cipher.getAuthTag().toString('base64url')}.${body.toString('base64url')}`;
  assert.equal(host.decrypt(encrypted,{ZCODE_CREDENTIAL_SECRET:secret}),'synthetic-key');
  assert.throws(()=>host.decrypt(encrypted,{ZCODE_CREDENTIAL_SECRET:'wrong'}),/无法解密/);
});
test('native account scoped key never substitutes another account',()=>{
  const credentials={'oauth:bigmodel:user_info':JSON.stringify({id:'account-one'}),'oauth:active_provider':'bigmodel','account-provider:coding-plan:account:provider:account:account-one:api-key':'synthetic-key'};
  const local={settings:{providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}},load:key=>credentials[key]||''};
  const access={type:'zhipu-account',accountType:'bigmodel',mode:'individual-coding-plan'};
  assert.equal(host.material(local,'account:provider',access).key,'synthetic-key');
  credentials['oauth:bigmodel:user_info']=JSON.stringify({id:'account-two'});
  assert.equal(host.material(local,'account:provider',access),null);
  assert.equal(host.material(local,'account:provider',{...access,mode:'team-coding-plan'}),null);
});
test('strict personal entitlement and official endpoint restrictions',()=>{
  assert.equal(host.personalEntitled({data:[{productName:'Coding plan',status:'VALID',inCurrentPeriod:true}]}),true);
  assert.equal(host.personalEntitled({data:[{productName:'Coding plan',status:'EXPIRED',inCurrentPeriod:true}]}),false);
  assert.throws(()=>host.officialOrigin('https://attacker.invalid','https://bigmodel.cn',['https://bigmodel.cn']),/官方地址/);
});
test('start plan empty whitelist, expiry, pending, and canonical model casing',()=>{
  const plan={plan_id:'zcode-v3-start-plan',status:'active',ends_at:200};
  assert.deepEqual(host.startEntitlement({data:{plans:[plan],balances:[]}},100),{entitled:true,models:[]});
  assert.equal(host.startEntitlement({data:{plans:[plan],balances:[]}},201).entitled,false);
  assert.equal(host.startEntitlement({data:{plans:[{...plan,starts_at:150}],balances:[]}},100).entitled,false);
  assert.deepEqual(host.startEntitlement({data:{plans:[plan],balances:[{plan_id:plan.plan_id,capabilities:['model:GLM-4.7']}]}},100).models,['GLM-4.7']);
});

test('unsupported Start Plan is absent even with valid credentials and advertised models; old grants cannot authorize it',async()=>{
  const root=await mkdtemp(join(tmpdir(),'zcode-start-disabled-'));
  const previous=process.env.ZCODE_DATA_BASE_DIR;
  try{
    process.env.ZCODE_DATA_BASE_DIR=root;
    const dir=join(root,'.zcode','v2');await mkdir(dir,{recursive:true});
    const providerId='account:bigmodel-start-plan',access={type:'zhipu-account',accountType:'bigmodel',mode:'start-plan'};
    const credentials={'oauth:bigmodel:user_info':JSON.stringify({id:'one'}),'oauth:active_provider':'bigmodel',zcodejwttoken:'synthetic-start-key'};
    const settings={providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'start-plan'}}};
    await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));await writeFile(join(dir,'setting.json'),JSON.stringify(settings));
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId,config:{access,builtinModelIds:['GLM-5.3-Flash']}}]}}}));
    let calls=0;
    const request=async()=>{calls++;return new Response(JSON.stringify({data:{plans:[{plan_id:'zcode-start-plan',status:'active'}],balances:[{plan_id:'zcode-start-plan',capabilities:['model:glm-5.3-flash']}]}}));};
    const result=await host.snapshot({builtin,version:'test'},request);
    assert.equal(result.config.states[providerId].unavailableReason,'not-entitled');
    assert.equal(result.config.states[providerId].availability,'unavailable');
    assert.equal(result.startPlanUnavailable,true,'integration notice stays outside the native wire config');
    assert.deepEqual(result.config.providers[providerId].builtinModelIds,[]);assert.equal(result.scopes[providerId],undefined);
    assert.equal(calls,0,'do not query or retry an unsupported plan');
    const scope=host.material({settings,load:key=>credentials[key]||''},providerId,access).scope;
    await assert.rejects(host.auth({builtin,providerId,grant:{scope,access:{...access,entitled:true}}},request),/Start Plan.*暂不支持/);
    assert.equal(await require('node:fs/promises').readFile(join(dir,'credentials.json'),'utf8'),JSON.stringify(credentials));
    assert.equal(await require('node:fs/promises').readFile(join(dir,'setting.json'),'utf8'),JSON.stringify(settings));
  }finally{if(previous===undefined)delete process.env.ZCODE_DATA_BASE_DIR;else process.env.ZCODE_DATA_BASE_DIR=previous;await rm(root,{recursive:true,force:true});}
});
test('private auth reload rejects changed identity and mismatched provider',async()=>{
  const root=await mkdtemp(join(tmpdir(),'zcode-account-host-'));
  const previous=process.env.ZCODE_DATA_BASE_DIR;
  try{
    process.env.ZCODE_DATA_BASE_DIR=root;
    const dir=join(root,'.zcode','v2');await mkdir(dir,{recursive:true});
    const access={type:'zhipu-account',accountType:'bigmodel',mode:'individual-coding-plan'};
    const settings={providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}};
    const credentials={'oauth:bigmodel:user_info':JSON.stringify({id:'one'}),'oauth:active_provider':'bigmodel','account-provider:coding-plan:account:provider:account:one:api-key':'synthetic-key'};
    await writeFile(join(dir,'setting.json'),JSON.stringify(settings));await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId:'account:provider',config:{access}}]}}}));
    const scope=host.material({settings,load:key=>credentials[key]||''},'account:provider',access).scope;
    const input={builtin,providerId:'account:provider',grant:{scope,access:{...access,entitled:true}}};
    assert.equal((await host.auth(input)).requestAuth.apiKey,'synthetic-key');
    await assert.rejects(host.auth({...input,providerId:'account:other'}),/身份已变化/);
    credentials['oauth:bigmodel:user_info']=JSON.stringify({id:'two'});await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));
    await assert.rejects(host.auth(input),/登录或套餐连接已变化/);
  }finally{if(previous===undefined)delete process.env.ZCODE_DATA_BASE_DIR;else process.env.ZCODE_DATA_BASE_DIR=previous;await rm(root,{recursive:true,force:true});}
});

test('temporary entitlement failures remain closed and recover on a fresh snapshot',async()=>{
  const root=await mkdtemp(join(tmpdir(),'zcode-account-reload-'));
  const previous=process.env.ZCODE_DATA_BASE_DIR;
  try{
    process.env.ZCODE_DATA_BASE_DIR=root;
    const dir=join(root,'.zcode','v2');await mkdir(dir,{recursive:true});
    const providerId='account:fixture';
    const access={type:'zhipu-account',accountType:'bigmodel',mode:'individual-coding-plan'};
    await writeFile(join(dir,'setting.json'),JSON.stringify({providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}}));
    await writeFile(join(dir,'credentials.json'),JSON.stringify({'oauth:bigmodel:user_info':JSON.stringify({id:'one'}),'oauth:active_provider':'bigmodel',[`account-provider:coding-plan:${providerId}:account:one:api-key`]:'synthetic-key',zcodejwttoken:'synthetic-start-key'}));
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId,config:{access}},{providerId:'account:start-fixture',config:{access:{...access,mode:'start-plan'}}}]}}}));
    const input={builtin,version:'test'};
    const failed=await host.snapshot(input,async()=>{throw Error('synthetic network failure');});
    assert.equal(failed.config.states[providerId].unavailableReason,'credential-failed');
    assert.deepEqual(failed.scopes,{});
    const invalid=await host.snapshot(input,async()=>new Response(JSON.stringify({data:{}})));
    assert.equal(invalid.config.states[providerId].unavailableReason,'credential-failed');
    assert.deepEqual(invalid.scopes,{});
    const expired=await host.snapshot(input,async()=>new Response(JSON.stringify({data:[{productName:'Coding plan',status:'EXPIRED',inCurrentPeriod:true}]})));
    assert.equal(expired.config.states[providerId].unavailableReason,'not-entitled');
    assert.deepEqual(expired.scopes,{});
    const recovered=await host.snapshot(input,async url=>String(url).includes('subscription/list')?new Response(JSON.stringify({data:[{productName:'Coding plan',status:'VALID',inCurrentPeriod:true}]})):new Response('',{status:400}));
    assert.equal(recovered.config.states[providerId].availability,'available');
    assert.ok(recovered.scopes[providerId]);
    assert.equal(recovered.config.states['account:start-fixture'].unavailableReason,'not-entitled');
    assert.equal(recovered.scopes['account:start-fixture'],undefined);
  }finally{if(previous===undefined)delete process.env.ZCODE_DATA_BASE_DIR;else process.env.ZCODE_DATA_BASE_DIR=previous;await rm(root,{recursive:true,force:true});}
});

test('disabled Start Plan never sends device identity; personal entitlement and native files remain intact',async()=>{
  const root=await mkdtemp(join(tmpdir(),'zcode-account-device-'));
  const previous=process.env.ZCODE_DATA_BASE_DIR;
  try{
    process.env.ZCODE_DATA_BASE_DIR=root;
    const dir=join(root,'.zcode','v2');await mkdir(dir,{recursive:true});
    const startId='account:start-fixture',personalId='account:personal-fixture';
    const access={type:'zhipu-account',accountType:'bigmodel',mode:'start-plan'};
    await writeFile(join(dir,'setting.json'),JSON.stringify({providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}}));
    await writeFile(join(dir,'credentials.json'),JSON.stringify({'oauth:bigmodel:user_info':JSON.stringify({id:'one'}),'oauth:active_provider':'bigmodel',zcodejwttoken:'synthetic-start-key',[`account-provider:coding-plan:${personalId}:account:one:api-key`]:'synthetic-personal-key'}));
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId:startId,config:{access,builtinModelIds:['GLM-4.7']}},{providerId:personalId,config:{access:{...access,mode:'individual-coding-plan'}}}]}}}));
    const input={builtin,version:'test'};
    let personalCalls=0;
    const request=async(url,options)=>{
      assert.equal(new URL(url).pathname,'/api/biz/subscription/list','unsupported plan must not issue requests');
      assert.equal(options.headers['X-Device-Mid'],undefined,'native device ID stays private');
      personalCalls++;
      return new Response(JSON.stringify({data:[{productName:'Coding plan',status:'VALID',inCurrentPeriod:true}]}));
    };
    assert.equal((await host.snapshot(input,request)).config.states[startId].unavailableReason,'not-entitled');
    const telemetry=JSON.stringify({deviceMid:'synthetic-native-device'});
    await writeFile(join(dir,'telemetry-state.json'),telemetry);
    const loaded=await host.snapshot(input,request);
    assert.equal(loaded.config.states[startId].availability,'unavailable');
    assert.deepEqual(loaded.config.providers[startId].builtinModelIds,[]);
    assert.equal(loaded.scopes[startId],undefined);
    assert.equal(loaded.config.states[personalId].availability,'available');assert.ok(loaded.scopes[personalId]);
    assert.equal(JSON.stringify(loaded).includes('synthetic-native-device'),false,'device ID never goes to the renderer config');
    assert.equal(await require('node:fs/promises').readFile(join(dir,'telemetry-state.json'),'utf8'),telemetry,'native state remains untouched');
    for(const invalid of ['{broken',JSON.stringify({deviceMid:'bad\r\nheader'}),JSON.stringify({deviceMid:42}),JSON.stringify({deviceMid:'  '})]){
      await writeFile(join(dir,'telemetry-state.json'),invalid);
      const closed=await host.snapshot(input,request);
      assert.equal(closed.scopes[startId],undefined);
      assert.equal(closed.config.states[startId].unavailableReason,'not-entitled');
      assert.ok(closed.scopes[personalId]);
    }
    assert.equal(personalCalls,6);
  }finally{if(previous===undefined)delete process.env.ZCODE_DATA_BASE_DIR;else process.env.ZCODE_DATA_BASE_DIR=previous;await rm(root,{recursive:true,force:true});}
});

test('missing personal cache refreshes the existing OAuth project token and keeps account binding private',async()=>{
  const root=await mkdtemp(join(tmpdir(),'zcode-account-oauth-'));
  const previous=process.env.ZCODE_DATA_BASE_DIR;
  try{
    process.env.ZCODE_DATA_BASE_DIR=root;
    const dir=join(root,'.zcode','v2');await mkdir(dir,{recursive:true});
    const providerId='account:bigmodel-individual-coding-plan';
    const access={type:'zhipu-account',accountType:'bigmodel',mode:'individual-coding-plan'};
    const credentials={'oauth:bigmodel:user_info':JSON.stringify({id:'current-account'}),'oauth:active_provider':'bigmodel','oauth:bigmodel:access_token':'synthetic-oauth'};
    await writeFile(join(dir,'setting.json'),JSON.stringify({providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}}));
    await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId,config:{access,builtinModelIds:['GLM-5.3','GLM-5.3-Flash']}}]}}}));
    const input={builtin,version:'test'},calls=[];
    const request=async(url,options)=>{
      calls.push(new URL(url).pathname);
      if(String(url).endsWith('/access_tokens')){
        assert.equal(options.headers.Authorization,'Bearer synthetic-oauth');
        return new Response(JSON.stringify({code:0,data:{token:'synthetic-project-token'}}));
      }
      assert.equal(options.headers.Authorization,'synthetic-project-token');
      return new Response(JSON.stringify({data:[{productName:'Coding plan',status:'VALID',inCurrentPeriod:true}]}));
    };
    const factory=async(_input,options)=>({resolve:async params=>{
      assert.equal(params.accountId,'current-account');assert.equal(params.family,'bigmodel');
      await options.locationStore.save('isolated-location','private-project');
      assert.equal(await options.locationStore.load('isolated-location'),'private-project');
      await options.locationStore.delete('isolated-location');
      assert.equal(await options.locationStore.load('isolated-location'),null);
      const data=await options.request('https://bigmodel.cn/api/biz/v1/organization/org/projects/p/api_keys/existing/access_tokens',{method:'POST',headers:{Authorization:`Bearer ${params.loginToken}`}});
      return {token:data.data.token};
    }});
    const snapshot=await host.snapshot(input,request,factory);
    assert.equal(snapshot.config.states[providerId].availability,'available');
    assert.ok(snapshot.scopes[providerId]);
    assert.deepEqual(snapshot.config.providers[providerId].access,{type:'zhipu-account',entitled:true});
    assert.equal(JSON.stringify(snapshot).includes('synthetic-'),false);
    assert.equal(await require('node:fs/promises').readFile(join(dir,'credentials.json'),'utf8'),JSON.stringify(credentials),'refresh never writes native credentials');
    assert.deepEqual(calls,['/api/biz/v1/organization/org/projects/p/api_keys/existing/access_tokens','/api/biz/subscription/list']);
    assert.equal((await host.auth({...input,providerId,grant:snapshot.scopes[providerId]},request,factory)).requestAuth.apiKey,'synthetic-project-token');
    for(const endpoint of ['https://attacker.invalid/steal','https://bigmodel.cn/api/biz/v1/organization/org/projects/p/api_keys']){
      const blocked=await host.snapshot(input,request,async(_input,options)=>({resolve:()=>options.request(endpoint,{method:'POST',headers:{Authorization:'synthetic-oauth'}})}));
      assert.equal(blocked.scopes[providerId],undefined,'never create a persistent API key or send OAuth to an arbitrary endpoint');
    }
    const changedDuringRefresh=async()=>({resolve:async()=>{
      await writeFile(join(dir,'credentials.json'),JSON.stringify({...credentials,'oauth:bigmodel:access_token':'changed-oauth'}));
      return {token:'must-not-return-stale-token'};
    }});
    await assert.rejects(host.snapshot(input,async()=>new Response(JSON.stringify({data:[{productName:'Coding plan',status:'VALID',inCurrentPeriod:true}]})),changedDuringRefresh),/登录账号发生变化/);
    await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));
    await assert.rejects(host.auth({...input,providerId,grant:snapshot.scopes[providerId]},request,changedDuringRefresh),/登录状态已变化/);
    credentials['oauth:bigmodel:access_token']='changed-oauth';await writeFile(join(dir,'credentials.json'),JSON.stringify(credentials));
    await assert.rejects(host.auth({...input,providerId,grant:snapshot.scopes[providerId]},request,factory),/登录|变化/);
  }finally{if(previous===undefined)delete process.env.ZCODE_DATA_BASE_DIR;else process.env.ZCODE_DATA_BASE_DIR=previous;await rm(root,{recursive:true,force:true});}
});
