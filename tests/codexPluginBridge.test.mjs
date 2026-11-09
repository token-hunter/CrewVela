import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { patchAdapter } from '../tools/codex-acp-wrapper.mjs';

const original = (await readFile(new URL(import.meta.resolve('@agentclientprotocol/codex-acp')), 'utf8')).replaceAll('\r\n', '\n');
const patched = patchAdapter(original, 'file:///native/package/dist/index.js');
const promptStart = patched.indexOf('function buildPromptItems(prompt)');
const build = Function('formatUriAsLink', patched.slice(promptStart, patched.indexOf('function imageDataUrl', promptStart)) + '; return buildPromptItems;')((name, uri) => `[${name}](${uri})`);
test('locked bridge preserves native module URL and refuses incompatible markers', () => {
  assert(!patched.includes('import.meta.url'));
  assert(patched.includes('createRequire("file:///native/package/dist/index.js")'));
  assert.throws(() => patchAdapter(original.replace('switch (argument.toLowerCase()) {', 'changed marker'), 'file:///other'));
});
test('plugin resources become typed native mentions while ordinary links remain text', () => {
  assert.deepEqual(build([{ type: 'resource_link', name: 'Figma', uri: 'plugin://figma@openai-curated-remote' }]), [{ type: 'mention', name: 'Figma', path: 'plugin://figma@openai-curated-remote' }]);
  assert.equal(build([{ type: 'resource_link', name: 'file', uri: 'file:///test.txt' }])[0].type, 'text');
  assert.throws(() => build([{ type: 'resource_link', name: 'bad', uri: 'plugin://figma@market/../../file' }]));
});
test('actual adapter sendPrompt sends the typed mention in runTurn input', async () => {
  const start = patched.indexOf('async sendPrompt(request, agentMode, modelId');
  const method = patched.slice(start, patched.indexOf('  async runAgentFileChangeReport(', start));
  const sendPrompt = Function('buildPromptItems', 'addAdditionalDirectoriesToSandboxPolicy', `return ({${method}}).sendPrompt;`)(build, policy => policy);
  let wire;
  await sendPrompt.call({ refreshSkills: async () => {}, codexClient: { runTurn: async payload => { wire = payload; } } },
    { sessionId: 'thread', prompt: [{ type: 'resource_link', name: 'Figma', uri: 'plugin://figma@market' }] },
    { approvalPolicy: 'on-request', approvalsReviewer: 'user', sandboxPolicy: { type: 'workspaceWrite' } }, { model: 'supported', effort: 'medium' }, null, false, '/cwd', []);
  assert.deepEqual(wire.input, [{ type: 'mention', name: 'Figma', path: 'plugin://figma@market' }]);
  assert.equal(wire.approvalPolicy, 'on-request');
});
test('catalog uses the live connection and plugin validation rejects disabled/missing references', async () => {
  const start = patched.indexOf('async function crewvelaPluginCatalog');
  const methods = patched.slice(start, patched.indexOf('  agent({ name:', start));
  const requests = [];
  const agent = { sessions: new Map([['thread', { cwd: '/cwd' }]]), codexAcpClient: { codexClient: { connection: { sendRequest: async (...args) => {
    requests.push(args); return { marketplaces: [{ plugins: [{ id: 'figma@market', installed: true, enabled: true, availability: 'AVAILABLE' }, { id: 'disabled@market', installed: true, enabled: false, availability: 'AVAILABLE' }] }] };
  } } } } };
  const api = Function('getAgent', 'RequestError', methods + '; return { crewvelaPluginCatalog, crewvelaValidatePlugins };')(() => agent, { invalidParams: message => new Error(message) });
  await api.crewvelaValidatePlugins({ sessionId: 'thread', prompt: [{ type: 'resource_link', uri: 'plugin://figma@market' }] });
  assert.deepEqual(requests[0], ['plugin/installed', { cwds: ['/cwd'] }]);
  await assert.rejects(api.crewvelaValidatePlugins({ sessionId: 'thread', prompt: [{ type: 'resource_link', uri: 'plugin://disabled@market' }] }));
  await assert.rejects(api.crewvelaValidatePlugins({ sessionId: 'thread', prompt: [{ type: 'resource_link', uri: 'plugin://missing@market' }] }));
});

function goalCommand() {
  const start = patched.indexOf('async runGoalCommand(sessionState, rest, options)');
  const method = patched.slice(start, patched.indexOf('  handleCommandTurnStarted(', start));
  return Function(`return ({${method}}).runGoalCommand;`)();
}
test('goal command checks generation after delayed subscription and after settings preflight', async () => {
  const run = goalCommand();
  let generation = 1, nativeCalls = 0, preflightCalls = 0;
  const command = { runWithProcessCheck: operation => operation(), codexAcpClient: { setGoal: async () => { nativeCalls++; return null; }, resumeGoal: async () => { nativeCalls++; return null; } }, createGoalCommandResult: () => ({ handled: true }) };
  const options = { crewvelaGoal: { action: 'set' }, crewvelaGoalIsCurrent: () => generation === 1, crewvelaGoalSync: async () => { preflightCalls++; } };
  // Simulate prompt entering subscribeToSessionEvents, pause/clear succeeding,
  // then the existing prompt pipeline reaching runGoalCommand.
  let release;
  const subscribed = new Promise(resolve => { release = resolve; });
  const pending = (async () => { await subscribed; return run.call(command, { sessionId: 'thread' }, 'objective', options); })();
  generation++; release(); await pending;
  assert.equal(nativeCalls, 0); assert.equal(preflightCalls, 0);
  generation = 1;
  options.crewvelaGoalSync = async () => { generation++; };
  await run.call(command, { sessionId: 'thread' }, 'objective', options);
  assert.equal(nativeCalls, 0);
  // Ordinary /goal also uses the same preflight and refuses failures.
  delete options.crewvelaGoal; generation = 1;
  options.crewvelaGoalSync = async () => { throw new Error('settings rejected'); };
  await assert.rejects(run.call(command, { sessionId: 'thread' }, 'objective', options), /settings rejected/);
  assert.equal(nativeCalls, 0);
});
test('goal settings reuse selected permission/model/effort/plan/fast and preserve explicit clearing', async () => {
  const start = patched.indexOf('async function crewvelaSyncGoalSettings');
  const method = patched.slice(start, patched.indexOf('  async function crewvelaPluginCatalog', start));
  const sync = Function('ModelId', 'createCodexCollaborationMode', 'addAdditionalDirectoriesToSandboxPolicy', 'resolveFastServiceTier', method + '; return crewvelaSyncGoalSettings;')(
    { fromString: () => ({ model: 'selected-model', effort: 'high' }) }, (mode, model) => ({ mode, model }), (policy, roots) => ({ ...policy, writableRoots: roots }), enabled => enabled ? 'fast' : null);
  let payload;
  await sync({ runWithProcessCheck: operation => operation(), codexAcpClient: { codexClient: { threadSettingsUpdate: async value => { payload = value; } } } },
    { sessionId: 'thread', currentModelId: 'selected-model[high]', collaborationMode: 'plan', agentMode: { approvalPolicy: 'on-request', approvalsReviewer: 'user', sandboxPolicy: { type: 'workspaceWrite', networkAccess: false } }, additionalDirectories: ['/allowed'], fastModeEnabled: false, currentModelSupportsFast: true, supportedReasoningEfforts: [], account: { type: 'chatgpt' } });
  assert.equal(payload.model, 'selected-model'); assert.equal(payload.effort, 'high');
  assert.equal(payload.approvalPolicy, 'on-request'); assert.equal(payload.approvalsReviewer, 'user');
  assert.equal(payload.collaborationMode.mode, 'plan'); assert.equal(payload.serviceTier, null);
  assert.deepEqual(payload.sandboxPolicy, { type: 'workspaceWrite', networkAccess: false, writableRoots: ['/allowed'] });
});
test('catalog and goal controls share the original session connection and prompt pipeline', () => {
  assert(patched.includes('agent.codexAcpClient.codexClient.connection.sendRequest("plugin/installed"'));
  assert(patched.includes('await crewvelaValidatePlugins(ctx.params)'));
  assert(patched.includes('p.installed === true && p.enabled === true && p.availability === "AVAILABLE"'));
  assert(patched.includes('return a.prompt({ sessionId: p.sessionId'));
  assert(patched.includes('options.crewvelaGoal?.action === "set" ? "__set_objective__"'));
});
