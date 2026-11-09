const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/sessionCapabilities.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);
test('legacy ZCode default names display as Desktop without mutating history or custom titles', async () => {
  const { sessionDisplayTitle, sessionDisplayProvider } = await modulePromise;
  const session = { agentRuntimeKind: 'zcode', providerId: 'local-runtime-zcode', providerName: 'ZCode', title: 'ZCode' };
  assert.equal(sessionDisplayTitle(session), 'ZCode Desktop');
  assert.equal(sessionDisplayProvider(session), 'ZCode Desktop');
  assert.equal(sessionDisplayTitle({ ...session, title: 'ZCode · project' }), 'ZCode Desktop · project');
  assert.equal(sessionDisplayTitle({ ...session, title: '分析 ZCode 源码' }), '分析 ZCode 源码');
  assert.equal(sessionDisplayTitle({ ...session, agentRuntimeKind: 'codex' }), 'ZCode');
  assert.equal(session.title, 'ZCode');
  assert.equal(session.providerName, 'ZCode');
});

test('claude chat sessions count as native chat and keep their provider label', async () => {
  const { isNativeChatSession, isStandaloneClaudeChatSession, sessionDisplayTitle, sessionDisplayProvider } = await modulePromise;
  const session = { kind: 'terminal', agentRuntimeKind: 'claude_chat', providerId: 'local-runtime-claude_chat', providerName: 'Claude Code Chat', title: 'Claude Code Chat', ptyId: null, multiAgentProjectId: null, multiAgentAgentId: null, status: 'running' };
  assert.equal(isNativeChatSession(session), true);
  assert.equal(isStandaloneClaudeChatSession(session), true);
  assert.equal(sessionDisplayTitle(session), 'Claude Code Chat');
  assert.equal(sessionDisplayProvider(session), 'Claude Code Chat');
  // 画布 Agent 或 PTY 的 claude 会话不算原生聊天
  assert.equal(isNativeChatSession({ ...session, multiAgentProjectId: 'p1' }), false);
  assert.equal(isNativeChatSession({ ...session, agentRuntimeKind: 'claude_code', providerId: 'provider-1' }), false);
});

test('codex chat sessions count as native chat while codex PTY stays terminal', async () => {
  const { isNativeChatSession, isStandaloneCodexChatSession, sessionDisplayTitle, sessionDisplayProvider } = await modulePromise;
  const session = { kind: 'terminal', agentRuntimeKind: 'codex_chat', providerId: 'local-runtime-codex_chat', providerName: 'Codex Chat', title: 'Codex Chat', ptyId: null, multiAgentProjectId: null, multiAgentAgentId: null, status: 'running' };
  assert.equal(isNativeChatSession(session), true);
  assert.equal(isStandaloneCodexChatSession(session), true);
  assert.equal(sessionDisplayTitle(session), 'Codex Chat');
  assert.equal(sessionDisplayProvider(session), 'Codex Chat');
  // codex PTY 会话（Codex CLI）不走原生聊天视图
  assert.equal(isNativeChatSession({ ...session, agentRuntimeKind: 'codex', providerId: 'local-runtime-codex', providerName: 'Codex CLI', title: 'Codex CLI' }), false);
  assert.equal(isNativeChatSession({ ...session, multiAgentProjectId: 'p1' }), false);
});

test('kimi chat sessions count as native chat while kimi PTY stays terminal', async () => {
  const { isNativeChatSession, isStandaloneKimiChatSession, sessionDisplayTitle, sessionDisplayProvider } = await modulePromise;
  const session = { kind: 'terminal', agentRuntimeKind: 'kimi_chat', providerId: 'local-runtime-kimi_chat', providerName: 'Kimi Code Chat', title: 'Kimi Code Chat', ptyId: null, multiAgentProjectId: null, multiAgentAgentId: null, status: 'running' };
  assert.equal(isNativeChatSession(session), true);
  assert.equal(isStandaloneKimiChatSession(session), true);
  assert.equal(sessionDisplayTitle(session), 'Kimi Code Chat');
  assert.equal(sessionDisplayProvider(session), 'Kimi Code Chat');
  // kimi_code PTY 会话（Kimi Code 终端）不走原生聊天视图；画布 Agent 也不算
  assert.equal(isNativeChatSession({ ...session, agentRuntimeKind: 'kimi_code', providerId: 'local-runtime-kimi_code', providerName: 'Kimi Code', title: 'Kimi Code' }), false);
  assert.equal(isNativeChatSession({ ...session, multiAgentProjectId: 'p1' }), false);
});

test('opencode chat sessions count as native chat while opencode PTY stays terminal', async () => {
  const { isNativeChatSession, isStandaloneOpenCodeChatSession, sessionDisplayTitle, sessionDisplayProvider } = await modulePromise;
  const session = { kind: 'terminal', agentRuntimeKind: 'opencode_chat', providerId: 'local-runtime-opencode_chat', providerName: 'OpenCode Chat', title: 'OpenCode Chat', ptyId: null, multiAgentProjectId: null, multiAgentAgentId: null, status: 'running' };
  assert.equal(isNativeChatSession(session), true);
  assert.equal(isStandaloneOpenCodeChatSession(session), true);
  assert.equal(sessionDisplayTitle(session), 'OpenCode Chat');
  assert.equal(sessionDisplayProvider(session), 'OpenCode Chat');
  // opencode PTY 会话（OpenCode CLI 终端）不走原生聊天视图；画布 Agent 也不算
  assert.equal(isNativeChatSession({ ...session, agentRuntimeKind: 'opencode', providerId: 'local-runtime-opencode', providerName: 'OpenCode', title: 'OpenCode' }), false);
  assert.equal(isNativeChatSession({ ...session, multiAgentProjectId: 'p1' }), false);
});
