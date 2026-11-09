import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import {retainedSwitchSource,switchSlotId} from '../src/sessionSwitch.ts';
import {nativeChatTabSessions} from '../src/nativeAgentSwitch.ts';

function appCallback(name) {
  const file=ts.createSourceFile('App.tsx',readFileSync(new URL('../src/App.tsx',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
  let found;
  function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(file)===name&&node.initializer)found=node.initializer.getText(file);ts.forEachChild(node,visit);}
  visit(file);assert.ok(found,`missing ${name}`);
  return ts.transpileModule(`globalThis.callback=${found}`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
}

test('selecting retained CLI and native source reads history without resuming any executor',async()=>{
  for(const runtime of ['claude_code','claude_chat','zcode']){
    const session={id:'source',agentRuntimeKind:runtime,status:'stopped',ptyId:null};
    const records=[{id:'relay',source:session,targetId:'target',preserveSource:true,state:'queued'}];
    const selected=[],invocations=[];
    const context={sessions:[session],switchRecords:records,sidebarTerminalIds:[],visitedNativeSessions:[],
      retainedSwitchSource,useCallback:fn=>fn,isRetiredRuntime:()=>false,isNativeChatSession:s=>s.agentRuntimeKind!=='claude_code',isStructuredZCodeCli:()=>false,
      setSelectedSessionId:id=>selected.push(id),setMultiAgentWorkspaceProjectId(){},
      isRuntimeEnabled:()=>true,isSessionActive:()=>false,pendingSessionResumes:{current:new Set()},setLoadingSessionId(){},
      setSessions(){},showError(){},refreshSessions(){},UI_PREVIEW:false,invoke:async(...args)=>{invocations.push(args);return session;}};
    vm.runInNewContext(appCallback('selectSession'),context);
    await context.callback('source');
    assert.equal(selected.at(-1),'source');assert.deepEqual(invocations,[],`${runtime} must never resume when viewing retained history`);
  }
});

test('separate source and target chat tabs can be closed independently',()=>{
  const sessions=[{id:'source'},{id:'target'}];
  const records=[{source:sessions[0],targetId:'target',preserveSource:true,state:'queued'}];
  const slot=id=>switchSlotId(id,records);
  assert.deepEqual(nativeChatTabSessions(sessions,['source','target'],slot).map(s=>s.id),['source','target']);
  assert.deepEqual(nativeChatTabSessions(sessions,['source'],slot).map(s=>s.id),['source']);
  assert.deepEqual(nativeChatTabSessions(sessions,['target'],slot).map(s=>s.id),['target']);
});
