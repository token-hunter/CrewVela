// Keyless fake of @agentclientprotocol/claude-agent-acp for CrewVela tests.
// No files or model calls; mirrors the adapter's ACP surface used by the
// native Claude chat flavor: session/new|load (replay after reply),
// set_config_option, prompt with permission requests, session/cancel,
// available_commands_update, and resume assigning a NEW session id.
import readline from 'node:readline';
const send = (frame) => process.stdout.write(JSON.stringify({jsonrpc:'2.0', ...frame})+'\n');
let model = 'sonnet';
let mode = 'default';
let sessionId = 'claude-fake-1';
let loadCount = 0;
let promptId, permissionId;
const configOptions = () => [
  {id:'model',type:'select',currentValue:model,options:[{name:'Sonnet',value:'sonnet'},{name:'Opus',value:'opus'}]},
  {id:'mode',type:'select',currentValue:mode,options:[{name:'默认',value:'default'},{name:'计划',value:'plan'}]},
];
const availableModes = [{id:'default',kind:'permission'},{id:'plan',kind:'permission'}];
const commands = () => [{name:'model',description:'Switch model',input:{hint:'model name'}},{name:'clear',description:'Clear context'}];
const update = (payload) => send({method:'session/update',params:{sessionId,update:payload}});
for await (const line of readline.createInterface({input:process.stdin})) {
  const frame = JSON.parse(line);
  const reply = (result) => send({id:frame.id,result});
  if (!frame.method && frame.id===permissionId) {
    const allow = frame.result?.outcome?.optionId === 'allow';
    update({sessionUpdate:'tool_call_update',toolCallId:'read',status:allow?'completed':'failed'});
    update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:allow?'批准完成':'已拒绝'}});
    send({id:promptId,result:{stopReason:'end_turn'}});
  } else if (frame.method==='initialize') {
    reply({protocolVersion:1,agentCapabilities:{loadSession:true},agentInfo:{name:'claude-fake',version:'1'}});
  } else if (frame.method==='session/new') {
    reply({sessionId,configOptions:configOptions(),modes:{currentModeId:mode,availableModes}});
    update({sessionUpdate:'available_commands_update',availableCommands:commands()});
  } else if (frame.method==='session/load') {
    if (frame.params.sessionId !== sessionId) throw Error('unknown session to load: '+frame.params.sessionId);
    loadCount += 1;
    sessionId = `claude-fake-${loadCount + 1}`; // Claude Code resume opens a new session file
    reply({sessionId,configOptions:configOptions(),modes:{currentModeId:mode,availableModes}});
    update({sessionUpdate:'available_commands_update',availableCommands:commands()});
    // The adapter replays full history AFTER the load response. Delayed past
    // the spawn drain window so the replay guard is what drops these.
    setTimeout(() => {
      update({sessionUpdate:'user_message',content:{type:'text',text:'历史用户消息'}});
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'历史重放内容'}});
      update({sessionUpdate:'tool_call',toolCallId:'replayed',title:'重放工具',status:'in_progress'});
    }, 1000);
  } else if (frame.method==='session/set_config_option') {
    if (frame.params.configId==='model') model = frame.params.value;
    else if (frame.params.configId==='mode') mode = frame.params.value;
    else throw Error('unknown config id '+frame.params.configId);
    reply({configOptions:configOptions()});
  } else if (frame.method==='session/prompt') {
    promptId = frame.id; permissionId = frame.id;
    const text = frame.params.prompt[0]?.text ?? '';
    if (text.startsWith('/')) {
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'命令已执行：'+text}});
      reply({stopReason:'end_turn'});
      continue;
    }
    if (text==='slow-stream') {
      update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'首段'}});
      continue;
    }
    if (text==='cancel') continue;
    update({sessionUpdate:'agent_message_chunk',content:{type:'text',text:'流式回复：Claude'}});
    update({sessionUpdate:'tool_call',toolCallId:'read',title:'读取项目文件',status:'in_progress',rawInput:{path:'package.json'}});
    send({id:permissionId,method:'session/request_permission',params:{sessionId,toolCall:{toolCallId:'read',title:'读取项目文件',kind:'read',rawInput:{path:'package.json'}},options:[{kind:'allow_once',optionId:'allow'},{kind:'reject_once',optionId:'reject'}]}});
  } else if (frame.method==='session/cancel') {
    send({id:promptId,result:{stopReason:'cancelled'}});
  } else if (frame.method==='session/set_mode') {
    mode = frame.params.modeId;
    reply({});
  } else if (frame.method==='session/close') {
    reply({});
  } else {
    throw Error('unsupported method '+frame.method);
  }
}
