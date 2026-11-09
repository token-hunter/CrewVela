// Companion for the official ACP profile. It uses public Harness services and
// never sends human commands as model prompts or mutates the user's profile.
import net from 'node:net';
import fs from 'node:fs';
export const name = 'crewvela-control-bridge';
export const inject = ['agents', 'sessions', 'commands', 'goals', 'permissionPresets', 'skills', 'sessionTitle'];
const allowed = new Set(['goal', 'compact']);

export function apply(ctx) {
  const token = process.env.CREWVELA_HARNESS_CONTROL_TOKEN;
  const endpoint = process.env.CREWVELA_HARNESS_CONTROL_ENDPOINT;
  if (!token || !endpoint) throw Error('CrewVela control launch configuration missing');
  const sockets = new Set();
  const operations = new Map();
  let owner;
  let streamRevision = 0;
  let lastFrameRevision = 0;
  const subscribers = new Map();
  const attempts = new Map();
  const currentAttempts = new Map();
  const publish = (id, message) => {
    const frame = { revision: ++streamRevision, assistant: message };
    for (const socket of subscribers.get(id) ?? []) {
      if (socket.writableLength > 1024 * 1024) { socket.destroy(Error('Harness stream consumer is too slow')); continue; }
      socket.write(JSON.stringify(frame) + '\n');
    }
  };
  const timers = new Set();
  const flush = (id, attempt) => {
    if (attempt.timer) { clearTimeout(attempt.timer); timers.delete(attempt.timer); attempt.timer = undefined; }
    publish(id, render(attempt));
  };
  const render = (attempt) => {
    const blocks = [...attempt.blocks.entries()].sort(([a], [b]) => a - b).map(([, block]) => block);
    return { id: attempt.id, turn: attempt.turn, step: attempt.step, text: blocks.filter(block => block.type === 'text').map(block => block.text).join(''), reasoning: blocks.filter(block => block.type === 'reasoning').map(block => block.text).join(''), streamStatus: attempt.status };
  };
  ctx.on('agent/assistant-stream', ({ agent, frame }) => {
    if (agent.session.id !== owner || ctx.agents.get(owner) !== agent || ctx.sessions.get(owner) !== agent.session || !subscribers.has(owner)) return;
    if (frame.revision <= lastFrameRevision) return;
    lastFrameRevision = frame.revision;
    if (frame.type === 'start') {
      const attempt = { id: 'attempt:' + frame.attemptId, turn: frame.turn, step: frame.step, blocks: new Map(), status: 'streaming', revision: frame.revision, index: 0 };
      attempts.set(frame.attemptId, attempt);
      currentAttempts.set(owner, frame.attemptId);
      publish(owner, render(attempt));
      return;
    }
    const attempt = attempts.get(frame.attemptId);
    if (!attempt || frame.revision <= attempt.revision || frame.index !== attempt.index) return;
    attempt.revision = frame.revision;
    if (frame.type === 'chunk') {
      attempt.index++;
      const chunk = frame.chunk;
      if (chunk.type === 'text-delta' || chunk.type === 'reasoning-delta') {
        const type = chunk.type === 'text-delta' ? 'text' : 'reasoning';
        const block = attempt.blocks.get(chunk.index) ?? { type, text: '' };
        block.text += chunk.text;
        attempt.blocks.set(chunk.index, block);
      } else if (chunk.type === 'block-end' && ['text', 'reasoning'].includes(chunk.block.type)) {
        attempt.blocks.set(chunk.index, { type: chunk.block.type, text: chunk.block.text });
      } else if (chunk.type === 'finish') attempt.finish = chunk.reason;
    } else if (frame.type === 'end') {
      attempt.status = frame.outcome.kind === 'abandoned' ? 'abandoned' : frame.outcome.eventType === 'assistant/message' ? attempt.interrupted ? 'interrupted' : 'completed' : attempt.finish?.kind === 'aborted' ? 'interrupted' : 'failed';
      currentAttempts.delete(owner);
      attempts.delete(frame.attemptId);
    }
    if (frame.type === 'end' || !attempt.visiblePublished && (render(attempt).text || render(attempt).reasoning)) {
      attempt.visiblePublished = true;
      flush(owner, attempt);
    } else if (!attempt.timer) {
      attempt.timer = setTimeout(() => { timers.delete(attempt.timer); attempt.timer = undefined; publish(owner, render(attempt)); }, 100);
      timers.add(attempt.timer);
    }
  });
  ctx.on('session/event', (session, event) => {
    if (session.id !== owner || ctx.sessions.get(owner) !== session || event.type !== 'assistant/message' || !subscribers.has(owner)) return;
    const active = attempts.get(currentAttempts.get(owner));
    const message = event.data.message;
    if (active && (active.turn !== event.data.turn || active.step !== event.data.step)) return;
    if (active && active.turn === event.data.turn && active.step === event.data.step) {
      active.blocks = new Map(message.content.map((block, index) => [index, block]));
      active.interrupted = event.data.interrupted === true;
      active.status = active.interrupted ? 'interrupted' : 'completed';
      flush(owner, active);
    } else {
      publish(owner, { id: 'message:' + message.id, text: message.content.filter(block => block.type === 'text').map(block => block.text).join(''), reasoning: message.content.filter(block => block.type === 'reasoning').map(block => block.text).join(''), streamStatus: event.data.interrupted ? 'interrupted' : 'completed' });
    }
  });
  const resolve = (id) => {
    const agent = ctx.agents.get(id);
    const session = ctx.sessions.get(id);
    if (!agent || !session || agent.session !== session || session.header.origin === 'subagent' || session.header.parentSession !== undefined) throw Error('Not an active root Harness session');
    if (owner === undefined) owner = id;
    if (owner !== id) throw Error('Control request belongs to another Harness session');
    return agent;
  };
  const lookup = (agent, signal) => ({ cwd: agent.session.header.cwd, scope: agent, signal });
  const skills = async (agent, signal) => {
    const reserved = new Set([...ctx.commands.list(agent).map(command => command.name), 'file', 'model', 'permission']);
    return (await ctx.skills.list(lookup(agent, signal))).filter(skill => skill.invocation.userInvocable && /^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(skill.name) && !reserved.has(skill.name)).map(({name, description, invocation}) => ({name, description, invocation}));
  };
  const state = async (agent, signal) => ({
    sessionTitle: ctx.sessionTitle.get(agent.session) ?? null,
    skills: await skills(agent, signal),
    streamRevision,
    commands: ctx.commands.list(agent).filter((command) => allowed.has(command.name)),
    permission: { ...ctx.permissionPresets.catalog(), currentValue: ctx.permissionPresets.current(agent.session) },
    goal: ctx.goals.get(agent) ?? null,
    agentStatus: agent.status,
    commandBusy: operations.has(agent.session.id),
  });
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.setEncoding('utf8');
    const cancellation = new AbortController();
    let input = '', admitted = false;
    socket.setTimeout(10000);
    socket.on('timeout', () => socket.destroy());
    socket.on('close', () => { sockets.delete(socket); cancellation.abort(); });
    socket.on('error', () => { cancellation.abort(); });
    socket.on('data', (chunk) => {
      if (admitted) { socket.destroy(); return; }
      input += chunk.toString('utf8');
      if (Buffer.byteLength(input) > 128 * 1024) { socket.destroy(); return; }
      const newline = input.indexOf('\n');
      if (newline < 0) return;
      admitted = true;
      socket.setTimeout(300000);
      void (async () => {
        let nonce;
        try {
          const request = JSON.parse(input.slice(0, newline));
          if (request.token !== token || typeof request.nonce !== 'string' || request.nonce.length > 80) throw Error('Unauthorized Harness control request');
          nonce = request.nonce;
          const agent = resolve(request.sessionId);
          if (request.action === 'stream') {
            let receivers = subscribers.get(request.sessionId);
            if (!receivers) subscribers.set(request.sessionId, receivers = new Set());
            receivers.add(socket);
            socket.setTimeout(0);
            socket.once('close', () => { receivers.delete(socket); if (!receivers.size) subscribers.delete(request.sessionId); });
            socket.write(JSON.stringify({ nonce, ready: true }) + '\n');
            for (const attempt of attempts.values()) socket.write(JSON.stringify({ revision: streamRevision, assistant: render(attempt) }) + '\n');
            return;
          }
          let execution = null;
          if (request.action === 'command') {
            const match = /^\/([a-z][a-z0-9_-]*)(?=$|\s)/u.exec(request.value);
            if (!match || !allowed.has(match[1])) throw Error('Unsupported Harness slash command');
            if (operations.has(request.sessionId)) throw Error('Harness command is already running');
            operations.set(request.sessionId, cancellation);
            try { execution = await ctx.commands.execute(agent, request.value, [], cancellation.signal); }
            finally { operations.delete(request.sessionId); }
            if (!execution) throw Error('Harness command is not registered');
          } else if (request.action === 'skill') {
            const match = /^\/([a-z0-9]+(?:-[a-z0-9]+)*)(?=$|\s)/u.exec(request.value);
            if (!match || !(await skills(agent, cancellation.signal)).some(skill => skill.name === match[1])) throw Error('Unknown or unavailable user-invocable skill');
            const skill = await ctx.skills.get(match[1], lookup(agent, cancellation.signal));
            if (!skill?.invocation.userInvocable) throw Error('Skill is no longer available for user invocation');
          } else if (request.action === 'cancel') {
            operations.get(request.sessionId)?.abort(new Error('Cancelled by CrewVela'));
            if (agent.status === 'running') agent.cancel({ kind: 'user' });
          } else if (request.action === 'permission') {
            if (agent.status === 'running' || operations.has(request.sessionId)) throw Error('Pause the active Harness turn before changing permission');
            const option = ctx.permissionPresets.catalog().options.find((option) => option.value === request.value);
            if (!option) throw Error('Harness permission preset unavailable');
            ctx.permissionPresets.set(agent.session, request.value);
          } else if (request.action !== 'state') throw Error('Unsupported Harness control action');
          socket.end(JSON.stringify({ nonce, result: { ...await state(agent, cancellation.signal), execution } }) + '\n');
        } catch (error) {
          socket.end(JSON.stringify({ nonce, error: error instanceof Error ? error.message : String(error) }) + '\n');
        }
      })();
    });
  });
  ctx.effect(() => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      fs.writeFileSync(endpoint + '.tmp', JSON.stringify({ port: address.port }), { mode: 0o600 });
      fs.renameSync(endpoint + '.tmp', endpoint);
    });
    return () => { for (const timer of timers) clearTimeout(timer); for (const socket of sockets) socket.destroy(); server.close(); };
  });
  server.on('error', (error) => {
    console.error('CrewVela Harness control server failed: ' + error.message);
    for (const socket of sockets) socket.destroy();
  });
}
