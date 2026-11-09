import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readiness, { registerCollaborationMcp } from '../src-tauri/src/pi-readiness.mjs';

test('Pi collaboration MCP registration stays run scoped and exposes no raw credential', () => {
  const registrations = [];
  const pi = { registerMcpServer: (...args) => registrations.push(args) };
  const environment = { CREWVELA_PI_MCP_URL: 'http://127.0.0.1:58123/mcp', CREWVELA_AGENT_TOKEN: 'fixture-secret-token' };
  const original = structuredClone(environment);
  assert.equal(registerCollaborationMcp(pi, environment), true);
  assert.deepEqual(registrations, [['crewvela', {
    url: environment.CREWVELA_PI_MCP_URL,
    headers: { Authorization: 'Bearer ${CREWVELA_AGENT_TOKEN}' },
    exposure: 'direct',
    description: 'CrewVela project-scoped collaboration, messages and shared knowledge.',
  }]]);
  assert.equal(JSON.stringify(registrations).includes(environment.CREWVELA_AGENT_TOKEN), false);
  assert.deepEqual(environment, original);
  for (const missing of [{}, { CREWVELA_AGENT_TOKEN: 'fixture' }, { CREWVELA_PI_MCP_URL: environment.CREWVELA_PI_MCP_URL }]) {
    assert.equal(registerCollaborationMcp(pi, missing), false);
  }
  assert.equal(registrations.length, 1);
});

test('Pi collaboration refuses foreign or ambiguous endpoint registrations', () => {
  for (const endpoint of ['not a URL', 'https://127.0.0.1:58123/mcp', 'http://memory.example/mcp',
    'http://127.0.0.1:58123/other', 'http://user:password@127.0.0.1:58123/mcp',
    'http://127.0.0.1:58123/mcp?other=scope', 'http://127.0.0.1:58123/mcp#other', 'http://127.0.0.1:0/mcp']) {
    assert.throws(() => registerCollaborationMcp({ registerMcpServer() { assert.fail('invalid endpoint must not register'); } },
      { CREWVELA_PI_MCP_URL: endpoint, CREWVELA_AGENT_TOKEN: 'fixture-secret-token' }), /managed local MCP endpoint/);
  }
  const errors = [], oldError = console.error;
  console.error = (...args) => errors.push(args.join(' '));
  try {
    assert.equal(registerCollaborationMcp({}, { CREWVELA_PI_MCP_URL: 'http://127.0.0.1:58123/mcp', CREWVELA_AGENT_TOKEN: 'fixture-secret-token' }), false);
  } finally { console.error = oldError; }
  assert.equal(errors.length, 1);
  assert.equal(errors[0].includes('fixture-secret-token'), false);
});

test('Pi lifecycle readiness waits for settlement and never treats agent_end as idle', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-pi-test-'));
  const keys = ['CREWVELA_PI_EVENTS_DIR', 'CREWVELA_SESSION_ID', 'CREWVELA_PI_TOKEN', 'CREWVELA_PI_INBOX_DIR'];
  const original = keys.map(key => process.env[key]);
  [dir, 'existing-pi', 'launch-token', path.join(dir, 'inbox')].forEach((value, index) => process.env[keys[index]] = value);
  try {
    const handlers = new Map();
    readiness({ on: (name, fn) => handlers.set(name, fn) });
    let idle = true, pending = false;
    const ctx = { isIdle: () => idle, hasPendingMessages: () => pending, sessionManager: { getEntries: () => [], getBranch: () => [] } };
    const emit = name => handlers.get(name)?.({}, ctx);
    const events = () => fs.readdirSync(dir).filter(name => name.endsWith('.json'))
      .map(name => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8')))
      .sort((a, b) => a.runtime_sequence - b.runtime_sequence);
    const last = () => events().at(-1);
    emit('session_start'); assert.equal(last().ready, true);
    const realNow = Date.now;
    Date.now = () => 1_700_000_000_000;
    try {
      emit('ui_prompt_start'); emit('ui_prompt_end'); emit('ui_prompt_start'); emit('ui_prompt_end');
      const frozen = events().slice(-4);
      for (let i = 1; i < frozen.length; i++) assert.ok(frozen[i]._crewvela_queued_at > frozen[i - 1]._crewvela_queued_at);
      const byFile = fs.readdirSync(dir).filter(name => name.endsWith('.json')).sort().map(name => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8')));
      assert.deepEqual(byFile.map(event => event.runtime_sequence), events().map(event => event.runtime_sequence));
    } finally { Date.now = realNow; }
    emit('input'); assert.equal(last().ready, false);
    idle = false; emit('agent_start');
    idle = true; emit('agent_end'); assert.equal(last().ready, false);
    pending = true; emit('agent_settled'); assert.equal(last().ready, false);
    pending = false; emit('agent_settled'); assert.equal(last().ready, true);
    emit('ui_prompt_start'); emit('ui_prompt_start'); assert.equal(last().ready, false);
    emit('ui_prompt_end'); assert.equal(last().ready, false);
    emit('ui_prompt_end'); assert.equal(last().ready, true);
    emit('input'); emit('agent_start'); emit('agent_settled'); assert.equal(last().ready, true);
    emit('session_shutdown'); assert.equal(last().ready, false);
    const beforeReload = last().runtime_sequence;
    readiness({ on: (name, fn) => handlers.set(name, fn) });
    emit('session_start');
    assert.equal(last().ready, true);
    assert.ok(last().runtime_sequence > beforeReload, '/reload must retain monotonic event order');
    for (const event of events()) {
      assert.equal(event.session_id, 'existing-pi');
      assert.equal(event.runtime_token, 'launch-token');
      assert.equal(event.hook_event_name, 'RuntimePromptState');
    }
  } finally {
    clearInterval(globalThis[Symbol.for('crewvela.pi-inbox-timer')]);
    delete globalThis[Symbol.for('crewvela.pi-inbox-timer')];
    keys.forEach((key, index) => original[index] === undefined ? delete process.env[key] : process.env[key] = original[index]);
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('extension is inert outside a managed Pi process', () => {
  const previous = process.env.CREWVELA_PI_TOKEN;
  delete process.env.CREWVELA_PI_TOKEN;
  try { readiness({ on() { assert.fail('registered hooks without managed launch token'); } }); }
  finally { if (previous !== undefined) process.env.CREWVELA_PI_TOKEN = previous; }
});

test('first startup readiness reconciles idle API changes without bypassing settlement or modals', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-pi-first-start-'));
  const keys = ['CREWVELA_PI_EVENTS_DIR', 'CREWVELA_SESSION_ID', 'CREWVELA_PI_TOKEN', 'CREWVELA_PI_INBOX_DIR'];
  const original = keys.map(key => process.env[key]);
  [dir, 'first-pi', 'token', path.join(dir, 'inbox')].forEach((value, i) => process.env[keys[i]] = value);
  const oldSetInterval = globalThis.setInterval, oldClearInterval = globalThis.clearInterval;
  let poll;
  globalThis.setInterval = fn => { poll = fn; return { unref() {} }; };
  globalThis.clearInterval = () => {};
  try {
    const handlers = new Map();
    readiness({ on: (name, fn) => handlers.set(name, fn) });
    let idle = false, pending = false;
    const ctx = { isIdle: () => idle, hasPendingMessages: () => pending,
      sessionManager: { getEntries: () => [], getBranch: () => [] } };
    const emit = name => handlers.get(name)?.({}, ctx);
    const events = () => fs.readdirSync(dir).filter(name => name.endsWith('.json'))
      .map(name => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8')))
      .sort((a, b) => a.runtime_sequence - b.runtime_sequence);
    const last = () => events().at(-1);
    emit('session_start'); assert.equal(last().ready, false);
    idle = true; poll(); assert.equal(last().ready, true);
    const count = events().length;
    poll(); poll(); assert.equal(events().length, count, 'idle must not flood event files');
    emit('input'); poll(); assert.equal(last().ready, true, 'preflight failure need not emit settlement');
    emit('agent_start'); poll(); assert.equal(last().ready, false, 'idle API alone cannot bypass settlement');
    emit('agent_settled'); assert.equal(last().ready, true);
    emit('ui_prompt_start'); poll(); assert.equal(last().ready, false);
    emit('ui_prompt_end'); assert.equal(last().ready, true);
    pending = true; poll(); assert.equal(last().ready, false);
    pending = false; poll(); assert.equal(last().ready, true);
    emit('session_shutdown'); poll(); assert.equal(last().ready, false);
  } finally {
    globalThis.setInterval = oldSetInterval; globalThis.clearInterval = oldClearInterval;
    delete globalThis[Symbol.for('crewvela.pi-inbox-timer')];
    keys.forEach((key, i) => original[i] === undefined ? delete process.env[key] : process.env[key] = original[i]);
    fs.rmSync(dir, { recursive: true, force: true });
  }
});


test('programmatic Pi inbox bypasses selector focus, waits for Pi session receipt, and never replays processing', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-pi-inbox-'));
  const inbox = path.join(dir, 'inbox');
  const keys = ['CREWVELA_PI_EVENTS_DIR', 'CREWVELA_SESSION_ID', 'CREWVELA_PI_TOKEN', 'CREWVELA_PI_INBOX_DIR'];
  const original = keys.map(key => process.env[key]);
  [dir, 'live-pi', 'token', inbox].forEach((value, i) => process.env[keys[i]] = value);
  const oldSetInterval = globalThis.setInterval, oldClearInterval = globalThis.clearInterval;
  let poll;
  globalThis.setInterval = fn => { poll = fn; return { unref() {} }; };
  globalThis.clearInterval = () => {};
  try {
    const handlers = new Map(), entries = [], sends = [];
    const ctx = { isIdle: () => true, hasPendingMessages: () => false,
      sessionManager: { getEntries: () => entries, getBranch: () => entries },
      ui: { getEditorText() { assert.fail('must not touch selector/editor'); } },
    };
    const api = { on: (name, fn) => handlers.set(name, fn), sendUserMessage: (text, options) => sends.push({ text, options }) };
    readiness(api); handlers.get('session_start')({}, ctx);
    // Failed manual model/auth preflight emits input but no agent_start or agent_settled.
    // Once Pi's official idle API says idle, it must not permanently block CLI reception.
    handlers.get('input')({}, ctx);
    fs.writeFileSync(path.join(inbox, 'm1.request.json'), JSON.stringify({sessionId: 'live-pi', token: 'token', messageId: 'm1', body: '/model is literal message text'}));
    poll(); assert.equal(sends.length, 1);
    assert.deepEqual(sends[0].options, { deliverAs: 'followUp', expandPromptTemplates: false });
    assert.ok(sends[0].text.includes('/model is literal'));
    assert.equal(fs.existsSync(path.join(inbox, 'm1.accepted.json')), false);
    poll(); assert.equal(sends.length, 1, 'void API invocation is not ACK and must not repeat');
    // Reload before receipt: claims survive extension instance recreation.
    readiness(api); handlers.get('session_start')({}, ctx); poll();
    assert.equal(sends.length, 1, 'reload must not replay uncertain processing');
    entries.push({ type: 'message', message: { role: 'user', content: [{ type: 'text', text: sends[0].text }] } });
    poll(); assert.equal(JSON.parse(fs.readFileSync(path.join(inbox, 'm1.accepted.json'), 'utf8')).status, 'accepted');
    assert.equal(sends.length, 1);
    handlers.get('agent_start')({}, ctx);
    entries.push({ type: 'message', message: { role: 'assistant', content: [{ type: 'text', text: 'task result' }], stopReason: 'stop' } });
    handlers.get('agent_end')?.({}, ctx);
    const events = () => fs.readdirSync(dir).filter(name => name.endsWith('.json')).map(name => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'))).sort((a, b) => a.runtime_sequence - b.runtime_sequence);
    assert.equal(events().at(-1).turn_completion, undefined);
    handlers.get('agent_settled')({}, ctx);
    assert.deepEqual(events().at(-1).turn_completion, { prompt: '/model is literal message text', summary: 'task result' });
    ctx.sessionManager.getBranch = () => [{ type: 'message', message: { role: 'user', content: 'new task' } }, { type: 'message', message: { role: 'assistant', content: 'partial', stopReason: 'error', errorMessage: 'authentication failed' } }];
    handlers.get('agent_settled')({}, ctx);
    assert.deepEqual(events().at(-1).turn_completion, { prompt: 'new task', summary: '', error: 'authentication failed' });
    // Fresh settled turn can accept the next message into the same existing Pi session.
    handlers.get('agent_settled')({}, ctx);
    fs.writeFileSync(path.join(inbox, 'm2.request.json'), JSON.stringify({sessionId: 'live-pi', token: 'token', messageId: 'm2', body: 'second'}));
    poll(); assert.equal(sends.length, 2);
    // Cancellation preserves uncertain receipt evidence but cannot poison a new native launch.
    fs.writeFileSync(path.join(inbox, 'task-cancel.processing.json'), JSON.stringify({ sessionId: 'live-pi', token: 'old-token', messageId: 'task-cancel', body: 'cancelled task' }));
    fs.writeFileSync(path.join(inbox, 'task-cancel.retired.json'), '{}');
    entries.push({ type: 'message', message: { role: 'user', content: sends[1].text } });
    poll();
    readiness(api); handlers.get('session_start')({}, ctx);
    assert.equal(events().at(-1).ready, true);
    assert.equal(fs.existsSync(path.join(inbox, 'task-cancel.processing.json')), true);
    fs.writeFileSync(path.join(inbox, 'm3.request.json'), JSON.stringify({sessionId: 'live-pi', token: 'token', messageId: 'm3', body: 'next task'}));
    poll(); assert.equal(sends.length, 3);
    // The same native process recovers from unconfirmed input without a reload.
    const realNow = Date.now;
    Date.now = () => realNow() + 31_000;
    try { poll(); } finally { Date.now = realNow; }
    assert.equal(fs.existsSync(path.join(inbox, 'm3.unconfirmed.json')), true);
    fs.writeFileSync(path.join(inbox, 'm3.retired.json'), '{}');
    poll(); assert.equal(events().at(-1).ready, true);
    fs.writeFileSync(path.join(inbox, 'm4.request.json'), JSON.stringify({sessionId: 'live-pi', token: 'token', messageId: 'm4', body: 'task after failure'}));
    poll(); assert.equal(sends.length, 4);


  } finally {
    globalThis.setInterval = oldSetInterval; globalThis.clearInterval = oldClearInterval;
    delete globalThis[Symbol.for('crewvela.pi-inbox-timer')];
    keys.forEach((key, i) => original[i] === undefined ? delete process.env[key] : process.env[key] = original[i]);
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
