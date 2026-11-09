// Private Host adapter for ZCode Desktop Account Providers. No model requests,
// disk writes, credential logging, or renderer IPC. Native contracts are from
// ZCode Desktop services credentialCipherProvider, accountProviderCredentialKey,
// accountProviderRequestAuthService and codingPlanProviderAvailability.
const {readFile,readdir} = require('node:fs/promises');
const {createHash,createDecipheriv} = require('node:crypto');
const {homedir,userInfo,platform} = require('node:os');
const {join,resolve,dirname} = require('node:path');
const sha = value => createHash('sha256').update(value).digest('hex');
function decrypt(value, env=process.env) {
  if (typeof value !== 'string') return '';
  if (!value.startsWith('enc:v1:')) return value;
  const parts=value.slice(7).split('.');
  if(parts.length!==3) throw Error('ZCode Desktop 登录凭据格式损坏，请在原生 ZCode Desktop 重新登录');
  const [iv,tag,body]=parts.map(x=>Buffer.from(x,'base64url'));
  if(iv.length!==12||tag.length!==16) throw Error('ZCode Desktop 登录凭据格式损坏，请在原生 ZCode Desktop 重新登录');
  const secret=env.ZCODE_CREDENTIAL_SECRET||`zcode-credential-fallback:${platform()}:${homedir()}:${userInfo().username}`;
  try {
    const decipher=createDecipheriv('aes-256-gcm',Buffer.from(sha(secret),'hex'),iv);
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(body),decipher.final()]).toString('utf8');
  } catch {throw Error('ZCode Desktop 登录凭据无法解密，请在原生 ZCode Desktop 重新登录');}
}
async function readJson(path, optional=false) {
  try {return JSON.parse(await readFile(path,'utf8'));}
  catch(e) {if(optional&&e.code==='ENOENT')return {};throw Error('ZCode Desktop 本地账号配置无法读取');}
}
async function readLocal(input) {
  const directory=join(process.env.ZCODE_DATA_BASE_DIR?.trim()||process.env.HOME?.trim()||homedir(),'.zcode','v2');
  const [credentials,settings,builtin]=await Promise.all([
    readJson(join(directory,'credentials.json'),true),readJson(join(directory,'setting.json'),true),readJson(input.builtin)
  ]);
  const load=key=>decrypt(credentials[key]).trim();
  // The native source-header builder reuses this existing device identity.
  // Do not create/repair telemetry state or expose its value in the snapshot.
  let deviceMid;
  try{
    const state=JSON.parse(await readFile(join(directory,'telemetry-state.json'),'utf8'));
    const value=typeof state.deviceMid==='string'?state.deviceMid.trim():'';
    if(value&&/^[\x20-\x7e]+$/.test(value))deviceMid=value;
  }catch{}
  return {settings,builtin,load,directory,deviceMid};
}
function accountIdentity(local,family) {
  const profile=local.load(`oauth:${family}:user_info`);
  if(!profile)return '';
  try {const p=JSON.parse(profile);return family==='zai'?(p.id||p.user_id||''):(p.id||'');}
  catch {return '';}
}
function material(local,providerId,access) {
  const family=access.accountType, mode=access.mode;
  if(!['bigmodel','zai'].includes(family)||!['start-plan','individual-coding-plan'].includes(mode))return null;
  if(local.settings.providerFamilyDomain!==family||local.load('oauth:active_provider')!==family)return null;
  const identity=accountIdentity(local,family);
  if(typeof identity!=='string'||!identity.trim())return null;
  const selected=local.settings.providerFamilyConnectionSelections?.[family];
  if(mode==='individual-coding-plan'&&selected?.kind!==mode)return null;
  const key=mode==='start-plan'?local.load('zcodejwttoken'):
    local.load(`account-provider:coding-plan:${providerId}:account:${encodeURIComponent(identity.trim())}:api-key`);
  const oauthToken=mode==='individual-coding-plan'?local.load(`oauth:${family}:access_token`):'';
  if(!key&&!oauthToken)return null;
  return {key,oauthToken,accountId:identity.trim(),scope:sha(JSON.stringify([identity,family,mode==='start-plan'?{kind:mode}:selected])),family,mode};
}
async function nativeProjectTokenClient(input,options){
  // Reuse the installed first-party issuer and its account/project selection.
  // Its bundle exports are minified, so identify the named class, not an alias.
  const directory=join(dirname(input.builtin),'..','..','app.asar','out','host');
  for(const file of await readdir(directory)){
    if(!/^chunk-.*\.js$/.test(file))continue;
    const entry=join(directory,file);
    if(!(await readFile(entry,'utf8')).includes('ProjectAccessTokenClient'))continue;
    const Client=Object.values(require(entry)).find(value=>typeof value==='function'&&value.name==='ProjectAccessTokenClient');
    if(Client)return new Client(options);
  }
  throw Error('ZCode Desktop 原生账号凭据组件不可用，请更新原生客户端');
}
async function personalToken(input,local,auth,request,createClient){
  const origin=auth.family==='bigmodel'?officialOrigin(process.env.BIGMODEL_API_BASE_URL,'https://bigmodel.cn',['https://bigmodel.cn']):officialOrigin(process.env.ZAI_BUSINESS_BASE_URL,'https://api.z.ai',['https://api.z.ai']);
  const locations=new Map();
  const client=await createClient(input,{
    locationStore:{load:async key=>locations.has(key)?locations.get(key):local.load(key)||null,save:async(key,value)=>{locations.set(key,value);},delete:async key=>{locations.set(key,null);}},
    request:async(url,options)=>{
      const target=new URL(url),method=options?.method||'GET';
      const existingKeys=/^\/api\/biz\/v1\/organization\/[^/]+\/projects\/[^/]+\/api_keys$/;
      const refresh=/^\/api\/biz\/v1\/organization\/[^/]+\/projects\/[^/]+\/api_keys\/[^/]+\/access_tokens$/;
      if(target.origin!==origin||target.username||target.password||target.search||target.hash||!((method==='GET'&&(target.pathname==='/api/biz/customer/getCustomerInfo'||existingKeys.test(target.pathname)))||(method==='POST'&&refresh.test(target.pathname))))throw Error('ZCode Desktop 账号凭据操作不受支持，请在原生客户端完成账号初始化');
      const response=await request(target,{...options,redirect:'error',signal:AbortSignal.timeout(15000)});
      if(!response.ok)throw Error('ZCode Desktop 当前账号凭据刷新失败');
      return response.json();
    }
  });
  const material=await client.resolve({origin,family:auth.family,accountId:auth.accountId,loginToken:auth.oauthToken});
  if(typeof material?.token!=='string'||!material.token.trim())throw Error('ZCode Desktop 当前账号凭据刷新失败');
  return material.token.trim();
}
async function getJson(url,authorization,request=fetch,headers={}) {
  // Redirects cannot forward credentials to another origin. Errors are local,
  // deliberately exclude HTTP body, URL, headers and original exception text.
  try {
    const response=await request(url,{headers:{...headers,Authorization:authorization},redirect:'error',signal:AbortSignal.timeout(15000)});
    if(!response.ok)throw Error('status');
    const value=await response.json();
    if(value.success===false||(value.code!==undefined&&![0,200].includes(value.code)))return null;
    return {value,now:Date.parse(response.headers.get('date')||'')/1000};
  } catch {return null;}
}
function personalEntitled(payload) {
  return Array.isArray(payload?.data)&&payload.data.some(item=>
    [item?.productId,item?.productName].some(v=>typeof v==='string'&&v.toLowerCase().includes('coding'))&&
    item.status==='VALID'&&item.inCurrentPeriod===true);
}
function startEntitlement(payload, responseTime) {
  const data=payload?.data;
  const now=Number.isFinite(responseTime)?responseTime:(typeof data?.server_time==='number'?data.server_time:Date.now()/1000);
  const plans=(data?.plans||[]).filter(p=>{
    const id=String(p.plan_id||'').toLowerCase(),name=String(p.name||'').toLowerCase();
    return String(p.status||'').toLowerCase()==='active'&&
      ((!id&&!name)||[id,name].some(v=>v.includes('start-plan')||v.includes('start plan')))&&
      !(Number(p.ends_at)>0&&Number(p.ends_at)<=now);
  });
  if(!plans.length)return {entitled:false,models:[]};
  const models=new Set();
  for(const b of data.balances||[]){
    const owners=(data.plans||[]).filter(p=>b.user_plan_id&&p.user_plan_id?b.user_plan_id===p.user_plan_id:b.plan_id===p.plan_id);
    if(owners.length&&!owners.some(p=>plans.includes(p)))continue;
    const caps=(b.capabilities||[]).filter(v=>typeof v==='string'&&v.toLowerCase().startsWith('model:')).map(v=>v.slice(6).trim());
    for(const id of caps.length?caps:[b.show_name||''])if(/^glm-/i.test(id))models.add(id.trim());
  }
  const times=plans.flatMap(p=>p.entitlements?.length?p.entitlements.map(e=>e.effective_at):[p.starts_at]).map(Number);
  if(!models.size&&times.length&&times.every(t=>Number.isFinite(t)&&t>now))return {entitled:false,models:[]};
  return {entitled:true,models:[...models]};
}
function builtinRules(local){return local.builtin.config?.providerConfigRules?.providerRules||[];}
function officialOrigin(value, fallback, allowed){
  const origin=new URL(value||fallback).origin;
  if(!allowed.includes(origin))throw Error('ZCode Desktop 账号端点不是受支持的官方地址，已停止鉴权');
  return origin;
}
async function snapshot(input,request=fetch,createClient=nativeProjectTokenClient) {
  if(!input.version){try{input.version=require(join(dirname(input.builtin),'..','..','app.asar','package.json')).version;}catch{}}
  const local=await readLocal(input),providers={},states={},scopes={};
  const rules=builtinRules(local).filter(r=>r.config?.access?.type==='zhipu-account');
  for(const rule of rules){
    const access=rule.config.access,providerId=rule.providerId;
    // Entitlement does not prove inference is available. The current Start Plan
    // integration fails native requests (provider 3012); keep it out of the
    // catalog until that integration can be accepted end to end again.
    if(access.mode==='start-plan'){
      providers[providerId]={access:{type:'zhipu-account',entitled:false},builtinModelIds:[]};
      // Native account-config accepts only its own unavailableReason enum.
      states[providerId]={availability:'unavailable',entitled:false,current:false,unavailableReason:'not-entitled'};
      continue;
    }
    const auth=material(local,providerId,access);
    let entitled=false,models,queryFailed=false,projectToken=false;
    if(auth){
      if(auth.mode==='individual-coding-plan'){
        // Current first-party desktop uses BigModel. ZAI custom business origin
        // must be configured explicitly, never guess where to send its token.
        const origin=auth.family==='bigmodel'?officialOrigin(process.env.BIGMODEL_API_BASE_URL,'https://bigmodel.cn',['https://bigmodel.cn']):officialOrigin(process.env.ZAI_BUSINESS_BASE_URL,'https://api.z.ai',['https://api.z.ai']);
        if(origin){
          let key=auth.key;
          if(!key){try{key=await personalToken(input,local,auth,request,createClient);projectToken=true;}catch{queryFailed=true;}}
          const res=key?await getJson(new URL('/api/biz/subscription/list',origin),key.replace(/^Bearer\s+/i,''),request):null;
          queryFailed=queryFailed||!res||!Array.isArray(res.value?.data);entitled=!queryFailed&&personalEntitled(res?.value);
        }
      }else{
        const origin=officialOrigin(process.env.ZCODE_BASE_URL||process.env.ZCODE_ENDPOINT_ORIGIN,'https://zcode.z.ai',['https://zcode.z.ai']);
        const url=new URL('/api/v1/zcode-plan/billing/balance',origin);
        if(input.version)url.searchParams.set('app_version',input.version);
        const res=input.version?await getJson(url,`Bearer ${auth.key}`,request,local.deviceMid?{'X-Device-Mid':local.deviceMid}:{}):null;
        queryFailed=!res||!Array.isArray(res.value?.data?.plans)||!Array.isArray(res.value?.data?.balances);
        const state=startEntitlement(res?.value,res?.now);entitled=!queryFailed&&state.entitled;
        const canonical=rule.config.builtinModelIds||[];
        models=state.models.map(id=>canonical.find(v=>v.toLowerCase()===id.toLowerCase())||id);
      }
    }
    providers[providerId]={access:{type:'zhipu-account',entitled},...(access.mode==='start-plan'?{builtinModelIds:models||[]}:{})};
    states[providerId]={availability:entitled?'available':'unavailable',entitled,current:!!auth,...(!entitled?{unavailableReason:auth?(queryFailed?'credential-failed':'not-entitled'):'not-authenticated'}:{})};
    if(auth)states[providerId].connectionKey=auth.scope;
    if(entitled)scopes[providerId]={scope:auth.scope,access:{type:'zhipu-account',accountType:access.accountType,mode:access.mode,entitled:true},...(projectToken?{projectToken:true,loginFingerprint:sha(auth.oauthToken)}:{})};
  }
  // Do not publish a stale identity after asynchronous entitlement queries.
  const fresh=await readLocal(input);
  for(const [id,grant] of Object.entries(scopes)){
    const current=material(fresh,id,grant.access);
    if(current?.scope!==grant.scope||(grant.projectToken&&sha(current?.oauthToken||'')!==grant.loginFingerprint))throw Error('ZCode Desktop 登录账号发生变化，请恢复会话刷新模型');
  }
  const revision=`zcode-builtin:${local.builtin.revision}:${sha(resolve(input.builtin))}`;
  const config={basedOnZCodeBuiltinRevision:revision,providers,states};
  config.revision=`account:${sha(JSON.stringify(config))}`;
  return {config,scopes,startPlanUnavailable:rules.some(rule=>rule.config.access.mode==='start-plan')};
}
async function auth(input,request=fetch,createClient=nativeProjectTokenClient){
  const local=await readLocal(input),grant=input.grant;
  const rule=builtinRules(local).find(r=>r.providerId===input.providerId);
  if(!grant||!rule||['type','accountType','mode'].some(k=>rule.config.access[k]!==grant.access[k]))throw Error('ZCode Desktop 账号模型身份已变化，请恢复会话');
  if(rule.config.access.mode==='start-plan')throw Error('ZCode Desktop Start Plan 当前暂不支持在 CrewVela 中使用，请重新加载并选择其他可用模型');
  const current=material(local,input.providerId,grant.access);
  if(!current||current.scope!==grant.scope)throw Error('ZCode Desktop 登录或套餐连接已变化，请在原生 ZCode Desktop 登录后恢复会话');
  if(grant.projectToken&&(!current.oauthToken||sha(current.oauthToken)!==grant.loginFingerprint))throw Error('ZCode Desktop 登录状态已变化，请重新加载模型');
  const key=grant.projectToken?await personalToken(input,local,current,request,createClient):current.key;
  if(!key)throw Error('ZCode Desktop 当前账号凭据不可用，请重新加载模型');
  const fresh=material(await readLocal(input),input.providerId,grant.access);
  if(fresh?.scope!==grant.scope||(grant.projectToken&&sha(fresh?.oauthToken||'')!==grant.loginFingerprint))throw Error('ZCode Desktop 登录状态已变化，请重新加载模型');
  return {headersApplied:true,requestAuth:{apiKey:key}};
}
module.exports={decrypt,material,personalEntitled,startEntitlement,snapshot,auth,officialOrigin};
if(require.main===module||process.env.CREWVELA_ZCODE_ACCOUNT_HELPER==='1'){
  let bytes='';process.stdin.setEncoding('utf8');process.stdin.on('data',s=>bytes+=s);
  process.stdin.on('end',async()=>{
    try{const input=JSON.parse(bytes);const result=await(input.operation==='auth'?auth(input):snapshot(input));process.stdout.write(JSON.stringify({result}));}
    catch(e){const message=/^ZCode Desktop |^无法确认原生 ZCode Desktop/.test(e.message)?e.message:'ZCode Desktop 账号服务暂不可用，请在原生 ZCode Desktop 重新登录后恢复会话';process.stdout.write(JSON.stringify({error:message}));}
  });
}
