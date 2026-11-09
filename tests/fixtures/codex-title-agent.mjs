import assert from 'node:assert/strict';
import readline from 'node:readline';
const scenario = process.argv[2] || 'normal';
const emit = value => process.stdout.write(`${JSON.stringify(value)}\n`);
const reply = (id, result) => emit({ id, result });
for await (const line of readline.createInterface({ input: process.stdin })) {
  const request = JSON.parse(line);
  const p = request.params;
  switch (request.method) {
    case 'initialize': reply(request.id, {}); break;
    case 'initialized': break;
    case 'config/read': reply(request.id, { config: { model_provider: 'fixture-provider', mcp_servers: { secret: { command: 'never-run' } } } }); break;
    case 'thread/start':
      assert.equal(p.ephemeral, true);
      assert.equal(p.sandbox, 'read-only');
      assert.equal(p.config.default_permissions, ':read-only');
      assert.equal(p.modelProvider, 'fixture-provider');
      assert.equal(p.model, 'fixture-model');
      assert.equal(p.config['features.shell_tool'], false);
      assert.equal(p.config['features.hooks'], false);
      assert.deepEqual(p.config.mcp_servers.secret, { enabled: false });
      assert.deepEqual(p.dynamicTools, []);
      reply(request.id, { thread: { id: 'title-thread' } });
      break;
    case 'turn/start': {
      assert.equal(p.threadId, 'title-thread');
      assert.equal(p.outputSchema.required[0], 'title');
      // Notifications before ACK must survive request dispatch.
      const threadId = scenario === 'wrong-thread' ? 'other-thread' : 'title-thread';
      const turnId = scenario === 'wrong-turn' ? 'other-turn' : 'title-turn';
      const params = { threadId, turnId, item: { type: scenario === 'tool' ? 'commandExecution' : 'agentMessage', text: '{"title":"登录超时修复"}' } };
      emit({ method: 'item/started', params });
      emit({ method: 'item/completed', params });
      reply(request.id, { turn: { id: 'title-turn' } });
      emit({ method: 'turn/completed', params: { threadId, turn: { id: turnId, status: scenario === 'failed' ? 'failed' : 'completed' } } });
      break;
    }
    default: assert.fail(request.method);
  }
}
