import readline from 'node:readline';
import assert from 'node:assert/strict';
const scenario = process.env.ZCODE_NATIVE_SCENARIO || 'normal';
const sid = 'native-fixture', sub = 'subscription';
let childSerial=0;
let canonicalAttachments=[];
let seq = 0, ordinal = 0, command, turn = 0, pending, pendingCreate, pendingSubscribe;
let selection = scenario === 'unselected' ? null : {providerId:'personal-fixture',modelId:'model'};
let workflowEnabled=false, mode="build";
let resumed=false, titleNumber=0, titleGenerationEnabled;
const queueScenario = scenario.startsWith("queue");
let queue=[], autoDrain=true, phase="completedSuccess", revision=17;
function state() { return {revision,control:{phase},queue:{items:queue,autoDrain},inputRouting:{mode:phase==="running"?"enqueue":queue.length&&!autoDrain?"choice":"startNow"}}; }
function queueState() { frame({kind:"deltas",deltas:[{op:"state.updated",patch:state()}]}); }
function drain() {
  while(autoDrain && queue.length) {
    const item=queue.shift();command=item.sourceCommandId;turn++;phase="running";queueState();
    deltas([row(turn*10+1,"turnHeader",{state:"running",sourceCommandId:command}),row(turn*10+2,"userInput",{text:item.text,attachments:item.attachments,sourceCommandId:command})]);
    end();phase="completedSuccess";queueState();
  }
}
const settings = () => ({mode:{current:mode},model:{current:selection,available:[{ref:{providerId:'personal-fixture',modelId:'model'},label:'Fixture'},{ref:{providerId:'account:fixture',modelId:'account-model'},label:'Account'},{ref:{providerId:'personal-fixture',modelId:'disabled'},label:'Disabled',disabledReason:'disabled'},...(scenario==='model_rejected'?[{ref:{providerId:'personal-fixture',modelId:'rejected-model'},label:'Rejected'}]:[])]}});
const emit = x => process.stdout.write(JSON.stringify(x) + '\n');
const reply = (id, result) => emit({id, result});
const row = (rowId, kind, extra = {}) => ({rowId, kind, turnId: String(turn), ...extra});
function frame(payload) {
  const fromSeq = seq;
  emit({method:'v4/conversation/frame',params:{wireVersion:3,deliveryKind:ordinal ? 'online':'initial',logicalFrameId:String(++ordinal),logicalFrameOrdinal:ordinal,kind:'complete',topic:`conversation/${sid}`,subscriptionId:sub,frame:{topic:`conversation/${sid}`,subscriptionId:sub,fromSeq,toSeq:++seq,payload}}});
}
function deltas(rows) {frame({kind:'deltas',deltas:rows.map(row => ({op:'row.upserted',row}))});}
function end(state = 'completedSuccess') {
  deltas([row(turn*10+1,'turnHeader',{state,sourceCommandId:command}),row(turn*10+3,'reasoning',{text:'thinking',state:state==='completedInterrupted'?'interrupted':'complete'}),row(turn*10+4,'assistantText',{text:'answer',state:state==='completedInterrupted'?'interrupted':'complete'})]);
}
for await (const line of readline.createInterface({input:process.stdin})) {
  const value=JSON.parse(line), p=value.params;
  if (!value.method) {
    if (value.id === 'official-create' || value.id === 'official-subscribe') {
      assert.deepEqual(value.result,{ok:false,reason:'official_auth_unavailable'});
      assert.equal(value.error,undefined);
      if (value.id === 'official-create') {reply(pendingCreate,{session:{sessionId:sid},settings:settings()});pendingCreate=undefined;}
      else {subscribe(pendingSubscribe);pendingSubscribe=undefined;}
      continue;
    }
    if (value.id === 'reply-language') {
      assert.deepEqual(value.result, {language:null});
      assert.equal(value.error, undefined);
      end();
      continue;
    }
    assert.equal(value.result.permissionUpdates,undefined);
    if (scenario==='account') {assert.equal(value.result.headersApplied,false);end('failed');continue;}
    assert.ok(['allow','deny'].includes(value.result.decision));
    deltas([row(turn*10+5,'toolCall',{toolName:'Bash',inputText:'echo fixture',status:value.result.decision==='allow'?'success':'cancelled'})]);
    pending=false;end();continue;
  }
  switch(value.method) {
    case 'workspace/updateDynamicWorkflowPolicy':
      if(scenario==='policy_unsupported') {emit({id:value.id,error:{code:-32601}});break;}
      workflowEnabled=p.enabled;
      reply(value.id,scenario==='policy_reset_bad'||(scenario==='policy_bad_ack'&&p.enabled)?{}:{workspace:p.workspace,enabled:p.enabled});break;
    case 'fixture/workflowState':reply(value.id,{enabled:workflowEnabled});break;
    case 'skills/referenceCatalog':
      assert.equal(p.sessionId,sid);assert.ok(p.workspace.workspacePath);reply(value.id,scenario==='catalog_error'?{authority:'session',skills:null}:{authority:'session',skills:[{id:'skill',name:'fixture',path:'F:\\fixture\\SKILL.md',scope:'workspace',enabled:true,description:'fixture'}]});break;
    case 'plugins/referenceCatalog':
      assert.equal(p.sessionId,sid);assert.ok(p.workspace.workspacePath);if(scenario==='catalog_error')emit({id:value.id,error:{code:-32601,message:'not supported'}});else reply(value.id,{authority:'session',plugins:[]});break;
    case 'workspace/readPresentation':reply(value.id,{workspace:p.workspace,mode:'build',slashCommands:[{name:'init',description:'fixture init'},{name:'workflow',description:'fixture workflow'}]});break;
    case 'v4/commands/query':reply(value.id,{results:[]});break;
    case 'session/create':assert.equal(p.persistence,'deferred');assert.equal(p.mode,'build');assert.equal(typeof p.titleGenerationEnabled,'boolean');if(resumed && scenario.startsWith('resume_') && scenario!=='resume_lineage_missing')assert.equal(p.titleGenerationEnabled,false,'must not recreate an existing or unverified session');titleGenerationEnabled=p.titleGenerationEnabled;pendingCreate=value.id;official('official-create',p.workspace);break;
    case 'session/close':reply(value.id,{closed:true});break;
    case 'session/setMode':
      assert.ok(['plan','build','edit','yolo'].includes(p.mode));
      if(scenario==='mode_rejected') {emit({id:value.id,error:{code:-32000}});break;}
      if(scenario==='mode_unconfirmed') {reply(value.id,{settings:settings()});break;}
      mode=p.mode;reply(value.id,{settings:settings()});break;
    case 'session/setModel':
      assert.equal(p.persistAsWorkspaceLastUsed,false);assert.equal(p.model.providerId,'personal-fixture');
      if(scenario==='model_rejected') {assert.equal(p.model.modelId,'rejected-model');emit({id:value.id,error:{code:-32000,message:'fixture model unavailable'}});break;}
      if(scenario==='model_disconnect') {process.exit(0);}
      if(scenario==='model_bad_frame') {process.stdout.write('invalid fixture frame\n');break;}
      selection=p.model;reply(value.id,{settings:settings()});break;
    case 'session/resume':
      assert.equal(p.sessionId,sid);assert.equal(titleGenerationEnabled,false,'catalog-only draft must disable generation');resumed=true;
      if(scenario==='missing')emit({id:value.id,error:{code:-32004}});
      else if(scenario.startsWith('resume_lineage_'))emit({id:value.id,error:{code:-32603,message:'project.session-lineage-invalid'}});
      else if(scenario==='resume_internal')emit({id:value.id,error:{code:-32603,message:'fixture internal failure'}});
      else reply(value.id,{sessionId:sid,settings:settings()});break;
    case 'session/list':
      assert.deepEqual(p.sessionIds,[sid]);assert.equal(p.workspace,undefined,'workspace filtering cannot prove global session absence');
      if(scenario==='resume_lineage_missing')reply(value.id,{sessions:[]});
      else if(scenario==='resume_lineage_existing')reply(value.id,{sessions:[{sessionId:sid}]});
      else if(scenario==='resume_lineage_list_error')emit({id:value.id,error:{code:-32603,message:'fixture list failure'}});
      else if(scenario==='resume_lineage_malformed')reply(value.id,{});
      else assert.fail('unexpected absence probe');break;
    case 'v4/conversation/unsubscribe':
      assert.ok(p.topic.startsWith('conversation/child'));assert.ok(p.connectionId);assert.ok(p.subscriptionId.startsWith('child-sub-'));reply(value.id,{});childFrame(p.topic.slice(13),'late',2,p.subscriptionId);emit({method:'v4/conversation/resyncRequired',params:{topic:p.topic,subscriptionId:p.subscriptionId}});break;
    case 'v4/conversation/subscribe':
      if(p.topic.startsWith('conversation/child')) {assert.ok(p.workspace.workspacePath);const subscription=`child-sub-${++childSerial}`;reply(value.id,{ack:{subscriptionId:subscription,mode:'snapshot',logEpoch:'child-epoch',...(scenario==='subagents_bad_ack'?{mode:'online'}:{})}});childFrame(p.topic.slice(13),'child answer',1,subscription);break;}
      pendingSubscribe=value.id;official('official-subscribe',p.workspace);break;
    case 'v4/command':
      if (p.type === 'editUserQuery') {
        assert.equal(p.baseRevision, revision);
        assert.equal(p.baseLogEpoch, 'epoch');
        assert.deepEqual(p.payload.target, {rowId:12,entityId:'user-1'});
        assert.equal(p.payload.workspaceMode, 'preserve');
        assert.equal(p.payload.attachments, undefined, 'missing attachments preserves original canonical attachments');
        if (p.payload.newText === 'reject-edit') { reply(value.id,{commandId:p.commandId,status:'stale',reasonCode:'proto.staleRevision'}); break; }
        assert.ok(p.payload.newText === '' || p.payload.newText.includes('# userselect:'));
        reply(value.id,{commandId:p.commandId,status:'accepted'});
        command=p.commandId;revision++;phase='running';
        // Publish the new CAS revision with the rewind, before edited rows can
        // become visible and trigger the next command from the test client.
        frame({kind:'deltas',deltas:[{op:'state.updated',patch:state()},{op:'row.removed',fromRowId:13}]});
        deltas([row(11,'turnHeader',{state:'running',sourceCommandId:command}),row(12,'userInput',{text:p.payload.newText,entityId:'user-1',attachments:canonicalAttachments})]);
        if (p.payload.newText) {
          phase='completedSuccess';
          deltas([row(11,'turnHeader',{state:phase,sourceCommandId:command}),row(16,'assistantText',{text:'edited answer',state:'complete'})]);
        }
        queueState();break;
      }
      if(queueScenario && ['deleteQueueItem','setAutoDrain'].includes(p.type)) assert.equal(p.baseRevision,revision,'queue CAS must carry the authoritative revision, not frame sequence');
      if(scenario==='queue_cas_conflict' && ['deleteQueueItem','setAutoDrain'].includes(p.type)) {reply(value.id,{commandId:p.commandId,status:'stale',reasonCode:'proto.staleRevision'});break;}
      if (queueScenario && p.type==='sendText' && p.payload.text==='rpc-reject') {emit({id:value.id,error:{code:-32000,message:'secret=fixture-secret'}});break;}
      if (p.type==='sendText' && ((scenario==='rejected'&&p.payload.text==='rejected')||(queueScenario&&p.payload.text==='reject')))  {reply(value.id,{commandId:p.commandId,status:'failed',reasonCode:'fault.command.executionFailed',message:'do not leak secret=fixture-secret'});break;}
      reply(value.id,{commandId:p.commandId,status:'accepted'});
      if(p.type==='stop'){if(command&&(!queueScenario||phase==='running'))end('completedInterrupted');if(queueScenario||scenario==='edit'){phase='completedInterrupted';autoDrain=false;queueState();}break;}
      if(queueScenario && p.type==='deleteQueueItem') {queue=queue.filter(item=>item.queueItemId!==p.payload.queueItemId);revision++;queueState();break;}
      if(queueScenario && p.type==='setAutoDrain') {autoDrain=p.payload.autoDrain;revision++;queueState();if(phase!=='running')drain();break;}
      assert.equal(p.type,'sendText');assert.equal(p.payload.mode,mode);
      if(queueScenario && phase==='running') {
        assert.equal(p.payload.requestedDelivery,'queue');
        queue.push({queueItemId:p.commandId,sourceCommandId:p.commandId,text:p.payload.text,attachments:p.payload.attachments||[],dispatch:{state:'queued'}});
        if(scenario==='queue_slow')setTimeout(queueState,1500);else if(scenario==='queue_delayed')setTimeout(queueState,100);else queueState();
        break;
      }
      if(queueScenario){assert.equal(p.payload.requestedDelivery,'startNow');phase='running';queueState();}
      turn++;command=p.commandId;
      canonicalAttachments=p.payload.attachments||[];
      if(scenario==='attachments') { assert.equal(p.payload.text,''); assert.equal(p.payload.attachments.length,1); assert.equal(p.payload.attachments[0].fileName,'native attachment.txt'); assert.equal(p.payload.attachments[0].bytes,7); }
      deltas([row(turn*10+1,'turnHeader',{state:'running',sourceCommandId:command}),row(turn*10+2,'userInput',{text:p.payload.text,attachments:p.payload.attachments,entityId:`user-${turn}`}),row(turn*10+3,'reasoning',{text:'think',state:'streaming'}),row(turn*10+4,'assistantText',{text:'ans',state:'streaming'})]);
      frame({kind:'deltas',deltas:[{op:'row.delta',rowId:turn*10+3,path:'text',append:'ing'},{op:'row.delta',rowId:turn*10+4,path:'text',append:'wer'}]});
      if(scenario==='reply_language')emit({id:'reply-language',method:'session/readReplyLanguage',params:{sessionId:sid}});
      else if(scenario==='account')emit({id:'account',method:'interaction/requestProviderRuntimeHeaders',params:{sessionId:sid}});
      else if(scenario==='permission'||scenario==='subagents_permission'){pending=true;emit({id:'permission',method:'interaction/requestPermission',params:{sessionId:sid,requestId:`request-${turn}`,toolName:'Bash',input:{command:'echo fixture'}}});}
      else if(scenario!=='cancel'&&!queueScenario)end();
      if(scenario==='titles') {assert.ok(resumed||titleGenerationEnabled,'main session must enable native generation');frame({kind:'deltas',deltas:[{op:'state.updated',patch:{meta:{title:`会话摘要${++titleNumber}`,titleSource:'generated'}}}]});}
      break;
    default:assert.fail(value.method);
  }
}
function official(id,workspace) {
  emit({id,method:'interaction/requestOfficialMcpAuthHeaders',params:{requestId:id,workspace,pluginId:'official-plugin',mcpKey:'official-tools',targetOrigin:'https://fixture.invalid'}});
}
function subscribe(id) {
  frame({kind:'snapshot',snapshot:{sessionId:sid,logEpoch:'epoch',seq:1,meta:scenario==='titles'&&resumed?{title:'原生已保存摘要',titleSource:'generated'}:{title:'New Session',titleSource:'default'},...((queueScenario||scenario==="edit")?state():{}),rows:{window:scenario.startsWith('subagents')?[row(1,'toolCall',{toolName:'Agent',toolCallId:'a'}),row(2,'subagent',{parentToolCallId:'b',childSessionId:'child-b',subagentType:'Explore',summaryText:'second',status:'running'}),row(3,'subagent',{parentToolCallId:'a',childSessionId:'child-a',subagentType:'Explore',summaryText:'first',status:'success'})]:[row(1,'assistantText',{text:'restored history',state:'complete'})]}}});
  reply(id,{ack:{subscriptionId:sub,mode:'snapshot',logEpoch:'epoch'}});
}

function childFrame(child,text,ordinal,subscriptionId) {
 const topic=`conversation/${child}`;
 emit({method:'v4/conversation/frame',params:{wireVersion:3,deliveryKind:ordinal===1?'initial':'online',logicalFrameId:`child-${ordinal}`,logicalFrameOrdinal:ordinal,kind:'complete',topic,subscriptionId,frame:{topic,subscriptionId,fromSeq:0,toSeq:1,payload:{kind:'snapshot',snapshot:{sessionId:child,logEpoch:'child-epoch',seq:1,meta:{title:'子会话标题',titleSource:'generated'},rows:{window:[{rowId:1,kind:'assistantText',turnId:'child-turn',text,state:'complete'}]}}}}}});
}
