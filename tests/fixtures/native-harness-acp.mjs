// Keyless peer for the standalone CrewVela conversation. No files or model calls.
import readline from 'node:readline';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const send = (frame) => process.stdout.write(JSON.stringify({jsonrpc:'2.0', ...frame})+'\n');
let model = 'model-a';
let thinking = 'low';
const teamModels = process.argv.includes('--team-models');
const catalogOnly = process.argv.includes('--catalog-only');
const options = () => [{id:'model',type:'select',currentValue:model,options:[{name:'模型 A',value:'model-a'},{name:'模型 B',value:'model-b'}]},
  ...(teamModels ? [{id:'provider-effort',category:'thought_level',type:'select',currentValue:thinking,
    options:[{name:'低',value:'low'},{name:'高',value:'high'},{name:'最高',value:'max'}]}] : [])];
let promptId, permissionId, disposeBridge;
const listeners = new Map();
let messageText = '', streamFixtureRevision = 0;
const emit = (name, ...args) => { for (const callback of listeners.get(name) ?? []) callback(...args); };
const update = (payload) => {
  if(payload.sessionUpdate === 'agent_message_chunk') {
    messageText += payload.content.text;
    emit('session/event', session, {type:'assistant/message',data:{message:{id:String(promptId ?? 'goal'),content:[{type:'text',text:messageText}]}}});
  }
  send({method:'session/update',params:{sessionId:'native-fake',update:payload}});
};
let goal=null, permission='workspace-write';
const session={id:'native-fake',header:{}};
const agent={session,status:'idle',cancel:()=>{agent.status='idle';if(goal)goal={...goal,phase:'paused',activation:'disarmed'};}};
const patchIndex=process.argv.indexOf('--patch');
if(patchIndex>=0) {
  const patch=JSON.parse(fs.readFileSync(process.argv[patchIndex+1],'utf8'));
  const plugin=await import(patch.find(row=>row.insert)?.insert[0].name);
  plugin.apply({
    effect: (start) => { disposeBridge = start(); },
    on: (name, callback) => { if(!listeners.has(name)) listeners.set(name,[]);listeners.get(name).push(callback); },
    agents: {get: (id) => id==='native-fake'?agent:undefined},
    sessions: {get: (id) => id==='native-fake'?session:undefined},
    sessionTitle: {get: () => messageText ? {title:'Harness主题摘要',source:{kind:'provider'}} : undefined},
    goals: {get: () => goal},
    skills: {list:async()=>[{name:"demo-skill",description:"demo",invocation:{userInvocable:true,modelInvocable:false}}],get:async(name)=>name==="demo-skill"?{name,invocation:{userInvocable:true,modelInvocable:false}}:undefined},
    permissionPresets: {catalog: () => ({options:['read-only','workspace-write','danger-full-access'].map(value=>({value,name:value}))}),current:()=>permission,set:(_,value)=>{permission=value;}},
    commands: {list:()=>[{name:'goal',description:'设置或查看目标'},{name:'compact',description:'压缩上下文'}],execute:async(_,line)=>{
      if(line==='/compact') return {commandId:'compact',result:{kind:'success',text:'No compactable history yet.'}};
      if(line==='/goal pause') {agent.status='idle';goal={...goal,phase:'paused',activation:'disarmed'};}
      else if(line==='/goal clear') {agent.status='idle';goal=null;}
      else if(line.startsWith('/goal ') && line!=='/goal resume') {
        goal={objective:line.slice(6),phase:'active',activation:'armed',roundsStarted:1,maxGoalRounds:10};agent.status='running';
        setTimeout(()=>update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'自主目标回复'}}),300);
      }
      return {commandId:'goal',result:{kind:'success',text:goal?'Goal: '+goal.objective:'No goal is currently set.'}};
    }},
  });
}
for await (const line of readline.createInterface({input:process.stdin})) {
  const frame = JSON.parse(line);
  const reply = (result) => send({id:frame.id,result});
  if (!frame.method && frame.id===permissionId) {
    const allow=frame.result?.outcome?.optionId==='once';
    update({sessionUpdate:'tool_call_update',toolCallId:'read',status:allow?'completed':'failed'});
    update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:allow?'批准完成':'拒绝完成'}});
    send({id:promptId,result:{stopReason:'end_turn'}});
  } else if(frame.method==='initialize') {
    reply({protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{},close:{}}},agentInfo:{name:'native-fake',version:'1'}});
  } else if(['session/new','session/resume'].includes(frame.method)) {
    if (catalogOnly) {
      if (frame.method !== 'session/new' || process.env.DSH_PERMISSION_MODE !== 'read-only' || frame.params.mcpServers.length) throw Error('incorrect catalog-only boundary');
    } else if (process.env.DSH_PERMISSION_MODE !== 'workspace-write' || frame.params.mcpServers.some(server => server.type === 'http')) {
      throw Error('incorrect standalone boundary');
    }
    // Match Harness's parent-marker traversal without making any model request.
    // realpath.native on a verbatim drive root fails with EISDIR on Windows.
    if(process.platform==='win32' && frame.params.cwd.startsWith('\\\\?\\')) throw Error('verbatim ACP cwd');
    for(let cursor=frame.params.cwd;;cursor=path.dirname(cursor)) {
      fs.realpathSync.native(cursor);
      if(path.dirname(cursor)===cursor) break;
    }
    reply({...frame.method==='session/new'?{sessionId:'native-fake'}:{},configOptions:options()});
  } else if(frame.method==='session/set_config_option') {
    if(frame.params.configId==='model') model=frame.params.value;
    if(teamModels && frame.params.configId==='provider-effort' && frame.params.value!=='max') thinking=frame.params.value;
    reply({configOptions:options()});
  } else if(frame.method==='session/prompt') {
    if (catalogOnly) throw Error('catalog lookup must not send a prompt');
    promptId=frame.id;permissionId=frame.id;messageText='';
    const text=frame.params.prompt[0].text;
    if(text.startsWith('/demo-skill')) {
      if(text !== '/demo-skill 中文参数') throw Error('skill arguments changed');
      if(frame.params.prompt[1]?.type !== 'resource_link') throw Error('skill attachment missing');
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'技能参数和附件已收到'}});reply({stopReason:'end_turn'});continue;
    }
    if(text==='attachment-check') {
      const file=frame.params.prompt[1];
      if(file?.type!=='resource_link' || !file.uri.startsWith('file:') || !file.name) throw Error('missing file resource link');
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'附件已收到：'+file.name}});
      reply({stopReason:'end_turn'});continue;
    }
    if(text==='reasoning-stream') {
      agent.status='running';
      const attemptId='fake:'+promptId; let index=0;
      const stream=(body)=>emit('agent/assistant-stream',{agent,frame:{attemptId,revision:++streamFixtureRevision,...body}});
      stream({type:'start',turn:1,step:1});
      stream({type:'chunk',index:index++,chunk:{type:'reasoning-delta',index:0,text:'真实测试思考首段'}});
      setTimeout(()=>stream({type:'chunk',index:index++,chunk:{type:'text-delta',index:1,text:'回答首段'}}),350);
      setTimeout(()=>{
        stream({type:'chunk',index:index++,chunk:{type:'text-delta',index:1,text:'，第二段'}});
        const content=[{type:'reasoning',text:'真实测试思考首段'},{type:'text',text:'回答首段，第二段'}];
        emit('session/event',session,{type:'assistant/message',data:{turn:1,step:1,message:{id:'durable-answer',content}}});
        stream({type:'end',index,outcome:{kind:'committed',eventType:'assistant/message',seq:1}});
        send({method:'session/update',params:{sessionId:'native-fake',update:{sessionUpdate:'agent_thought_chunk',messageId:'durable-answer',content:content[0]}}});
        send({method:'session/update',params:{sessionId:'native-fake',update:{sessionUpdate:'agent_message_chunk',messageId:'durable-answer',content:content[1]}}});
        agent.status='idle';reply({stopReason:'end_turn'});
      },900);
      continue;
    }
    if(text==='disconnect-stream') { disposeBridge(); continue; }
    if(text==='cancel') continue;
    if(text==='slow-stream') {
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'首段'}});
      continue;
    }
    if(text==='crash'){process.exit(1);}
    update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'流式回复：'+'片段'.repeat(550)}});
    update({sessionUpdate:'tool_call',toolCallId:'read',title:'读取项目文件',status:'in_progress',rawInput:{path:'package.json'}});
    send({id:permissionId,method:'session/request_permission',params:{sessionId:'native-fake',toolCall:{toolCallId:'read'},options:[{kind:'allow_once',optionId:'once'},{kind:'reject_once',optionId:'reject'}]}});
  } else if(frame.method==='session/cancel') {
    send({id:promptId,result:{stopReason:'cancelled'}});
  } else if(frame.method==='session/close') reply({});
  else throw Error('unsupported method '+frame.method);
}
