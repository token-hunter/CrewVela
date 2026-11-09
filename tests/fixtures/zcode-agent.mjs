import readline from 'node:readline';
import assert from 'node:assert/strict';
const scenario = process.env.ZCODE_TEST_SCENARIO || 'roundtrip';
const sid = 'fixture-session';
const sub = 'fixture-sub';
let seq = 0, ordinal = 0, command, decisions = 0;
let pendingCreate;
let pendingSubscribe;
const emit = value => process.stdout.write(JSON.stringify(value) + '\n');
const response = (id, result) => emit({jsonrpc:'2.0',id,result});
const ack = envelope => ({commandId:envelope.commandId,status:'accepted',revisionAtDecision:0});
const row = (id, kind, extra = {}) => ({rowId:id,kind,turnId:'turn',createdAt:0,createdAtSeq:0,...extra});
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let n = 0; n < 8; n++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return ((crc ^ 0xffffffff) >>> 0).toString(16).padStart(8,'0');
}
function frame(payload, fragment = false) {
  const inner = {topic:`conversation/${sid}`,subscriptionId:sub,fromSeq:seq,toSeq:++seq,sentAt:0,payload};
  const wire = {wireVersion:3,deliveryKind:ordinal === 0 ? 'initial':'online',logicalFrameId:`logical-${ordinal}`,logicalFrameOrdinal:++ordinal,topic:inner.topic,subscriptionId:sub};
  if (!fragment) return emit({method:'v4/conversation/frame',params:{...wire,kind:'complete',frame:inner}});
  const data = Buffer.from(JSON.stringify(inner));
  const middle = Math.floor(data.length/2);
  for (const [fragmentIndex, bytes] of [data.subarray(0,middle),data.subarray(middle)].entries()) {
    emit({method:'v4/conversation/frame',params:{...wire,kind:'fragment',fragmentIndex,fragmentCount:2,logicalBytes:data.length,checksum:{algorithm:'crc32',value:crc32(data)},dataBase64:bytes.toString('base64')}});
  }
}
function complete() {
  frame({kind:'deltas',deltas:[{op:'row.appended',row:row(2,'assistantText',{text:'fixture completed'})},{op:'row.upserted',row:row(1,'turnHeader',{origin:'userInput',startedAt:0,state:'completedSuccess',sourceCommandId:command})}]},true);
}
function subscribed(value) {
  response(value.id,{ack:{subscriptionId:sub,mode:'snapshot',logEpoch:'epoch'}});
  frame({kind:'snapshot',snapshot:{sessionId:sid,logEpoch:'epoch',seq:1,rows:{window:[row(9,'turnHeader',{state:'completedSuccess',sourceCommandId:'historical'})]}}});
}
function officialMcpRequest(id) {
  emit({id,method:'interaction/requestOfficialMcpAuthHeaders',params:{mcpKey:'official-test',pluginId:'fixture-plugin',requestId:id,targetOrigin:'https://example.invalid',workspace:process.cwd()}});
}
for await (const line of readline.createInterface({input:process.stdin})) {
  const value = JSON.parse(line);
  if (!value.method) {
    if (value.id === 'startup-preferences') {
      assert.equal(value.error.code,-32601);
      if (scenario === 'official-mcp-startup') {
        officialMcpRequest('startup-official-mcp');
        continue;
      }
      response(pendingCreate.id,{...ack(pendingCreate.params),result:{sessionId:sid}});
      pendingCreate = undefined;
      continue;
    }
    if (value.id === 'startup-official-mcp' || value.id === 'after-create-official-mcp') {
      assert.deepEqual(value.result,{ok:false,reason:'official_auth_unavailable'});
      assert.equal(value.error,undefined);
      if (value.id === 'startup-official-mcp') {
        response(pendingCreate.id,{...ack(pendingCreate.params),result:{sessionId:sid}});
        pendingCreate = undefined;
      } else {
        subscribed(pendingSubscribe);
        pendingSubscribe = undefined;
      }
      continue;
    }
    if (scenario === 'account') {assert.equal(value.result.headersApplied,false);frame({kind:'deltas',deltas:[{op:'row.upserted',row:row(1,'turnHeader',{state:'failed',sourceCommandId:command})}]});}
    else {
      assert.equal(value.result.decision,scenario === 'deny' ? 'deny':'allow');
      assert.equal(value.result.permissionUpdates,undefined);
      if (++decisions === 2) complete();
    }
    continue;
  }
  const p = value.params;
  switch(value.method) {
    case 'v4/commands/query': response(value.id,{results:[{key:p.commands[0],result:'unknown'}]}); break;
    case 'session/resume':
      assert.equal(p.sessionId,sid);
      assert.equal(p.mcpServers[0].headers[0].value,'Bearer new-run');
      response(value.id,{sessionId:sid}); break;
    case 'session/close': response(value.id,{closed:true}); break;
    case 'v4/conversation/subscribe':
      if (scenario === 'official-mcp-after-create') {
        pendingSubscribe = value;
        officialMcpRequest('after-create-official-mcp');
      } else subscribed(value);
      break;
    case 'v4/command':
      if (p.type === 'createSession') {
        assert.equal(p.payload.config.mode,'build');
        assert.equal(p.payload.mcpServers[0].type,'http');
        pendingCreate = value;
        emit({id:'startup-preferences',method:'session/requestRuntimePreferences',params:{sessionId:sid,scope:'create'}});
      } else if (p.type === 'sendText') {
        assert.equal(p.sessionId,sid);
        if (scenario === 'missing-model' || scenario === 'unknown-rejection' || scenario === 'wrong-rejected-ack' || scenario === 'unknown-status') {
          response(value.id,{commandId:scenario === 'wrong-rejected-ack'?'wrong':p.commandId,status:scenario === 'unknown-status'?'unexpected':'failed',reasonCode:'fault.command.executionFailed',message:scenario === 'missing-model'?'Session model must be provider-qualified: ':'do not leak fixture-secret'});
          continue;
        }
        command=p.commandId;
        response(value.id,scenario === 'bad-ack' ? {...ack(p),commandId:'wrong'} : ack(p));
        frame({kind:'deltas',deltas:[{op:'row.appended',row:row(1,'turnHeader',{origin:'userInput',startedAt:0,state:'running',sourceCommandId:command})}]});
        if (scenario === 'cancel') continue;
        if (scenario === 'account') emit({id:value.id,method:'interaction/requestProviderRuntimeHeaders',params:{sessionId:sid}});
        else for (const id of [value.id,`retry-${value.id}`]) emit({id,method:'interaction/requestPermission',params:{requestId:'business-request',sessionId:scenario === 'wrong-session'?'other':sid,toolName:'Bash',toolCallId:'tool',input:{command:'echo fixture'},options:[]}});
      } else if(p.type === 'stop') {
        response(value.id,ack(p));
      } else assert.fail(`unknown command ${p.type}`);
      break;
    default: assert.fail(`unknown method ${value.method}`);
  }
}
