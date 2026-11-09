import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { once } from 'node:events';
import { apply } from '../src-tauri/resources/harness-control.mjs';

test('companion admits only its live root and dispatches commands without model prompts', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'crewvela-control-test-'));
  const endpoint = path.join(root, 'endpoint.json');
  process.env.CREWVELA_HARNESS_CONTROL_TOKEN = 'test-control-token';
  process.env.CREWVELA_HARNESS_CONTROL_ENDPOINT = endpoint;
  let dispose, executions = [], permission = 'workspace-write';
  const session = { id: 'root', header: { cwd: 'test-workspace' } };
  const agent = { session, status: 'idle', cancel: () => { agent.status = 'idle'; } };
  const agents = new Map([['root', agent], ['subagent', { session: { header: { origin: 'subagent' } }, status: 'idle' }]]);
  let slow = false;
  const listeners = new Map();
  let streamSocket;
  apply({
    on: (name, callback) => { listeners.set(name, callback); },
    effect: (start) => { dispose = start(); },
    agents: { get: (id) => agents.get(id) },
    sessions: { get: (id) => agents.get(id)?.session },
    sessionTitle: { get: () => ({ title: '会话主题摘要', source: { kind: 'provider' } }) },
    goals: { get: () => null },
    skills: { list: async ({scope,cwd}) => { assert.equal(scope, agent); assert.equal(cwd, 'test-workspace'); return [
      {name:"demo-skill",description:"demo",invocation:{userInvocable:true,modelInvocable:false}},
      {name:"hidden",description:"hidden",invocation:{userInvocable:false,modelInvocable:true}},
      {name:"plan",description:"collision",invocation:{userInvocable:true,modelInvocable:true}},
    ]; }, get:async(name,{scope})=>{assert.equal(scope,agent);return name==="demo-skill"?{invocation:{userInvocable:true}}:undefined;} },
    permissionPresets: { catalog: () => ({ options: [{ value: 'workspace-write' }, { value: 'read-only' }] }), current: () => permission, set: (_, value) => { permission = value; } },
    commands: { list: () => [{ name: 'goal' }, { name: 'compact' }, { name: 'plan' }], execute: async (_, line, attachments, signal) => {
      executions.push({ line, attachments });
      if (slow) await new Promise((resolve) => signal.addEventListener('abort', resolve, { once: true }));
      return { commandId: 'command', result: { kind: signal.aborted ? 'error' : 'success', text: signal.aborted ? 'Cancelled' : line } };
    } },
  });
  try {
    let address;
    for (let attempt = 0; attempt < 100; attempt++) {
      try { address = JSON.parse(await fs.readFile(endpoint, 'utf8')); break; }
      catch (error) { if (error.code !== 'ENOENT') throw error; await new Promise((resolve) => setTimeout(resolve, 10)); }
    }
    assert.ok(address?.port);
    const request = async (changes, splitUnicode = false) => {
      const socket = net.connect(address.port, '127.0.0.1');
      await once(socket, 'connect');
      const bytes = Buffer.from(JSON.stringify({ token: 'test-control-token', nonce: 'nonce', sessionId: 'root', action: 'state', value: '', ...changes }) + '\n');
      if (splitUnicode) {
        const index = bytes.indexOf(Buffer.from('中')) + 1;
        socket.write(bytes.subarray(0, index));
        await new Promise((resolve) => setTimeout(resolve, 10));
        socket.write(bytes.subarray(index));
      } else socket.write(bytes);
      let response = '';
      for await (const chunk of socket) response += chunk;
      return JSON.parse(response);
    };
    assert.match((await request({ token: 'wrong' })).error, /Unauthorized/);
    assert.match((await request({ sessionId: 'subagent' })).error, /root/);
    const state = await request({});
    assert.deepEqual(state.result.commands.map((command) => command.name), ['goal', 'compact']);
    streamSocket = net.connect(address.port, '127.0.0.1');
    await once(streamSocket, 'connect');
    const frames = []; let buffered = '';
    streamSocket.setEncoding('utf8');
    streamSocket.on('data', chunk => { buffered += chunk; let newline; while((newline=buffered.indexOf('\n'))>=0) { frames.push(JSON.parse(buffered.slice(0,newline))); buffered=buffered.slice(newline+1); } });
    streamSocket.write(JSON.stringify({token:'test-control-token',nonce:'stream',sessionId:'root',action:'stream'})+'\n');
    const until = async predicate => { for(let i=0;i<100&&!predicate();i++) await new Promise(resolve=>setTimeout(resolve,10)); assert.ok(predicate()); };
    await until(()=>frames[0]?.ready);
    const stream = frame => listeners.get('agent/assistant-stream')({agent,frame});
    stream({type:'start',attemptId:'attempt-1',revision:1,turn:1,step:1});
    stream({type:'chunk',attemptId:'attempt-1',revision:2,index:0,chunk:{type:'reasoning-delta',index:0,text:'思考'}});
    await until(()=>frames.some(frame=>frame.assistant?.reasoning==='思考'));
    assert.equal(frames.at(-1).assistant.text,'');
    stream({type:'chunk',attemptId:'attempt-1',revision:3,index:1,chunk:{type:'text-delta',index:1,text:'草稿'}});
    await until(()=>frames.at(-1).assistant?.text==='草稿');
    stream({type:'end',attemptId:'attempt-1',revision:4,index:2,outcome:{kind:'abandoned'}});
    await until(()=>frames.at(-1).assistant?.streamStatus==='abandoned');
    stream({type:'start',attemptId:'attempt-2',revision:5,turn:1,step:1});
    stream({type:'chunk',attemptId:'attempt-2',revision:6,index:0,chunk:{type:'text-delta',index:0,text:'新回复'}});
    listeners.get('session/event')(session,{type:'assistant/message',data:{turn:1,step:1,message:{id:'canonical',content:[{type:'text',text:'新回复规范结果'}]}}});
    stream({type:'end',attemptId:'attempt-2',revision:7,index:1,outcome:{kind:'committed',eventType:'assistant/message',seq:12}});
    await until(()=>frames.at(-1).assistant?.streamStatus==='completed');
    assert.equal(frames.at(-1).assistant.id,'attempt:attempt-2');
    assert.equal(frames.at(-1).assistant.text,'新回复规范结果');
    assert.equal(frames.at(-1).assistant.reasoning,'');
    streamSocket.destroy();

    assert.deepEqual(state.result.skills.map(skill=>skill.name), ['demo-skill']);
    assert.match((await request({ action: 'skill', value: '/hidden' })).error, /unavailable/);
    assert.match((await request({ action: 'skill', value: '/unknown' })).error, /unavailable/);
    assert.match((await request({ action: 'skill', value: '/plan' })).error, /unavailable/);
    assert.ok((await request({ action: 'skill', value: '/demo-skill 中文参数' })).result);
    assert.match((await request({ action: 'command', value: '/demo-skill' })).error, /Unsupported/);
    assert.match((await request({ action: 'command', value: '/plan' })).error, /Unsupported/);
    assert.equal(executions.length, 0);
    const unicode = await request({ action: 'command', value: '/goal 中文跨分片' }, true);
    assert.equal(unicode.result.execution.result.text, '/goal 中文跨分片');
    assert.deepEqual(executions[0].attachments, []);
    assert.match((await request({ action: 'permission', value: 'auto' })).error, /unavailable/);
    await request({ action: 'permission', value: 'read-only' });
    assert.equal(permission, 'read-only');
    agent.status = 'running';
    assert.match((await request({ action: 'permission', value: 'workspace-write' })).error, /Pause/);
    await request({ action: 'cancel' });
    assert.equal(agent.status, 'idle');
    slow = true;
    const pending = request({ action: 'command', value: '/compact' });
    for (let i = 0; i < 100 && executions.length < 2; i++) await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal((await request({})).result.commandBusy, true);
    await request({ action: 'cancel' });
    assert.equal((await pending).result.execution.result.text, 'Cancelled');
    assert.equal((await request({})).result.commandBusy, false);
  } finally {
    streamSocket?.destroy();
    dispose();
    delete process.env.CREWVELA_HARNESS_CONTROL_TOKEN;
    delete process.env.CREWVELA_HARNESS_CONTROL_ENDPOINT;
    await fs.rm(root, { recursive: true });
  }
});
