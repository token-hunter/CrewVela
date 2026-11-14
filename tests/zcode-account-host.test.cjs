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
    assert.equal(loaded.startPlanUnavailable,false,'a dormant Start Plan rule must not mask the selected Individual account');
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

function encryptV2(value,key,source='os'){
  const iv=Buffer.alloc(12,3),cipher=createCipheriv('aes-256-gcm',key,iv);
  const body=Buffer.concat([cipher.update(value,'utf8'),cipher.final()]);
  const fingerprint=createHash('sha256').update(key).digest('hex').slice(0,24);
  return `enc:v2:${source}:${fingerprint}:${iv.toString('base64url')}.${cipher.getAuthTag().toString('base64url')}.${body.toString('base64url')}`;
}
async function withV2Fixture(body){
  const root=await mkdtemp(join(tmpdir(),'zcode-v2-readonly-')),directory=join(root,'.zcode/v2');
  const envKeys=['ZCODE_DATA_BASE_DIR','ZCODE_CREDENTIAL_SECRET','ZCODE_CREDENTIAL_SECRET_FILE'];
  const previous=Object.fromEntries(envKeys.map(k=>[k,process.env[k]]));
  process.env.ZCODE_DATA_BASE_DIR=root;delete process.env.ZCODE_CREDENTIAL_SECRET;delete process.env.ZCODE_CREDENTIAL_SECRET_FILE;
  try{
    await mkdir(directory,{recursive:true});
    const providerId='account:bigmodel-individual-coding-plan',key=Buffer.alloc(32,9);
    const plaintext={'oauth:active_provider':'bigmodel','oauth:bigmodel:user_info':JSON.stringify({id:'fixture-v2-account'}),
      [`account-provider:coding-plan:${providerId}:account:fixture-v2-account:api-key`]:'synthetic-private-api-key'};
    const credentials=Object.fromEntries(Object.entries(plaintext).map(([k,v])=>[k,encryptV2(v,key)]));
    const file=join(directory,'credentials.json'),builtin=join(root,'builtin.json');
    await writeFile(file,JSON.stringify(credentials));await writeFile(join(directory,'credential-source'),'os\n');
    await writeFile(join(directory,'setting.json'),JSON.stringify({providerFamilyDomain:'bigmodel',providerFamilyConnectionSelections:{bigmodel:{kind:'individual-coding-plan'}}}));
    await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[{providerId,config:{access:{type:'zhipu-account',accountType:'bigmodel',mode:'individual-coding-plan'}}},{providerId:'account:bigmodel-start-plan',config:{access:{type:'zhipu-account',accountType:'bigmodel',mode:'start-plan'}}}]}}}));
    const request=async()=>new Response(JSON.stringify({data:[{productName:'Coding Plan',status:'VALID',inCurrentPeriod:true}]}));
    await body({root,directory,file,builtin,providerId,key,plaintext,credentials,request});
  }finally{
    for(const k of envKeys)if(previous[k]===undefined)delete process.env[k];else process.env[k]=previous[k];
    await rm(root,{recursive:true,force:true});
  }
}
test('v2 OS keyring decrypts current Individual account privately without creating or changing state',async()=>withV2Fixture(async f=>{
  const fs=require('node:fs/promises'),before=await fs.readFile(f.file,'utf8');let reads=0;
  const store=()=>({get:async()=>{reads++;return f.key.toString('base64url');},set:()=>assert.fail('must not write OS keys'),delete:()=>assert.fail('must not delete OS keys')});
  const input={builtin:f.builtin,version:'3.18.2'};
  const result=await host.snapshot(input,f.request,undefined,store);
  assert.equal(result.config.states[f.providerId].availability,'available');assert.ok(result.scopes[f.providerId]);
  assert.equal(result.startPlanUnavailable,false);assert.equal(reads,1,'snapshot and identity recheck reuse one read-only key');
  const grant=result.scopes[f.providerId];
  const auth=await host.auth({...input,providerId:f.providerId,grant},f.request,undefined,store);
  assert.equal(auth.requestAuth.apiKey,'synthetic-private-api-key');
  assert.ok(!JSON.stringify(result).includes('synthetic-private-api-key'));
  assert.ok(!JSON.stringify(result).includes(f.key.toString('base64url')));
  assert.equal(await fs.readFile(f.file,'utf8'),before);
  assert.equal(await fs.readFile(join(f.directory,'credential-source'),'utf8'),'os\n');
  assert.equal((await fs.readdir(f.directory)).some(name=>name.includes('secret')),false);
}));
test('v2 missing, mismatched, locked OS key and damaged tag fail closed before any entitlement request',async()=>withV2Fixture(async f=>{
  let requested=0;const request=async()=>{requested++;throw Error('must not query without decrypted account');};
  for(const store of [()=>({get:async()=>null}),()=>({get:async()=>Buffer.alloc(32,8).toString('base64url')}),()=>({get:async()=>{throw Error('sensitive raw keyring failure');}})]){
    await assert.rejects(host.snapshot({builtin:f.builtin},request,undefined,store),error=>/安全登录存储/.test(error.message)&&!error.message.includes('sensitive'));
  }
  const corrupt={...f.credentials};corrupt['oauth:active_provider']=corrupt['oauth:active_provider'].slice(0,-1)+'!';await writeFile(f.file,JSON.stringify(corrupt));
  await assert.rejects(host.snapshot({builtin:f.builtin},request,undefined,()=>({get:async()=>f.key.toString('base64url')})),/安全登录存储/);
  assert.equal(requested,0);
}));
test('v2 managed secret and source selection follow the native contract without OS key fallback',async()=>withV2Fixture(async f=>{
  const fs=require('node:fs/promises'),secret='synthetic-managed-secret-that-is-long-enough';
  const key=createHash('sha256').update(secret).digest();
  await fs.rm(join(f.directory,'credential-source'));
  await writeFile(f.file,JSON.stringify(Object.fromEntries(Object.entries(f.plaintext).map(([k,v])=>[k,encryptV2(v,key,'env')]))));
  const secretFile=join(f.directory,'credential-secret');await writeFile(secretFile,secret,{mode:0o600});
  const store=()=>assert.fail('env credentials must not consult OS keyring');
  let result=await host.snapshot({builtin:f.builtin},f.request,undefined,store);
  assert.ok(result.scopes[f.providerId]);
  if(process.platform!=='win32'){
    await fs.chmod(secretFile,0o644);
    await assert.rejects(host.snapshot({builtin:f.builtin},f.request,undefined,store),/安全登录存储/);
    await fs.chmod(secretFile,0o600);
  }
  process.env.ZCODE_CREDENTIAL_SECRET=secret;
  result=await host.snapshot({builtin:f.builtin},f.request,undefined,store);assert.ok(result.scopes[f.providerId]);
  process.env.ZCODE_CREDENTIAL_SECRET_FILE=secretFile;
  await assert.rejects(host.snapshot({builtin:f.builtin},f.request,undefined,store),/安全登录存储/);
  delete process.env.ZCODE_CREDENTIAL_SECRET;
  result=await host.snapshot({builtin:f.builtin},f.request,undefined,store);assert.ok(result.scopes[f.providerId]);
}));
test('unknown encryption versions are never treated as plaintext login credentials',()=>{
  assert.throws(()=>host.decrypt('enc:v2:os:synthetic'),/安全解密器/);
  assert.throws(()=>host.decrypt('enc:v99:synthetic'),/安全解密器/);
});

test('one-shot private helper flushes its JSON and exits even if imported native modules keep handles',async()=>{
  const {spawnSync}=require('node:child_process');
  const root=await mkdtemp(join(tmpdir(),'zcode-host-exit-'));
  try{
    const builtin=join(root,'builtin.json');await writeFile(builtin,JSON.stringify({revision:1,config:{providerConfigRules:{providerRules:[]}}}));
    const entry=require('node:path').resolve(__dirname,'../src-tauri/resources/zcode-account-host.cjs');
    const script=`setInterval(()=>{},1000);require(${JSON.stringify(entry)});`;
    const child=spawnSync(process.execPath,['-e',script],{input:JSON.stringify({operation:'snapshot',builtin}),encoding:'utf8',timeout:3000,
      env:{...process.env,ZCODE_DATA_BASE_DIR:root,CREWVELA_ZCODE_ACCOUNT_HELPER:'1'}});
    assert.equal(child.error,undefined);assert.equal(child.status,0);assert.equal(child.stderr,'');
    assert.equal(JSON.parse(child.stdout).result.startPlanUnavailable,false);
  }finally{await rm(root,{recursive:true,force:true});}
});

test('v2 account change during entitlement lookup cannot publish the previous account grant',async()=>withV2Fixture(async f=>{
  const request=async()=>{
    const changed={...f.credentials,'oauth:bigmodel:user_info':encryptV2(JSON.stringify({id:'different-fixture-account'}),f.key)};
    await writeFile(f.file,JSON.stringify(changed));
    return f.request();
  };
  await assert.rejects(host.snapshot({builtin:f.builtin},request,undefined,()=>({get:async()=>f.key.toString('base64url')})),/账号发生变化/);
}));
