// Locked adapter bridge. Never modify the installed package: preserve its module
// URL so createRequire and bundled Codex discovery still resolve beside it.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

// npm exec exposes its pinned installation through PATH, so installed builds
// can use this same bridge without embedding the developer's checkout path.
export async function resolveAdapterEntry(pathValue = process.env.PATH ?? '') {
  for (const directory of pathValue.split(path.delimiter).filter(Boolean)) {
    const roots = [path.join(directory, 'node_modules')];
    if (path.basename(directory) === '.bin') roots.unshift(path.dirname(directory));
    roots.push(path.resolve(directory, '../lib/node_modules'));
    for (const root of roots) {
      const entry = path.join(root, '@agentclientprotocol/codex-acp/dist/index.js');
      try {
        const pkg = JSON.parse(await readFile(path.join(root, '@agentclientprotocol/codex-acp/package.json'), 'utf8'));
        if (pkg.version === '1.11.0') { await readFile(entry); return entry; }
      } catch (error) {
        if (!['ENOENT', 'ENOTDIR'].includes(error.code)) throw error;
      }
    }
  }
  throw new Error('Pinned codex-acp 1.11.0 was not found on the runtime PATH');
}

export function patchAdapter(source, originalUrl) {
  const replaceOnce = (before, after) => {
    if (source.split(before).length !== 2) throw new Error(`Unsupported codex-acp 1.11.0 source marker: ${before.slice(0, 60)}`);
    source = source.replace(before, after);
  };
  replaceOnce('  let codex;\n  if (codexPath) {',
    `  let codex;
  if (process.env.CREWVELA_CODEX_SQLITE_HOME !== undefined) {
    codex = crewvelaStartIsolatedAppServer(codexPath, process.env.CREWVELA_CODEX_SQLITE_HOME, spawnEnv);
  } else if (codexPath) {`);
  replaceOnce('function startCodexConnection(codexPath, env) {', `${crewvelaStartIsolatedAppServer.toString()}
function startCodexConnection(codexPath, env) {`);
  replaceOnce('return { type: "text", text: formatUriAsLink(block.name, block.uri), text_elements: [] };',
    'if (block.uri?.startsWith("plugin://")) { if (!/^plugin:\\/\\/[A-Za-z0-9._-]+@[A-Za-z0-9._-]+$/.test(block.uri)) throw new Error("Invalid plugin reference"); return { type: "mention", name: block.name, path: block.uri }; } return { type: "text", text: formatUriAsLink(block.name, block.uri), text_elements: [] };');
  replaceOnce('goal: {\n          version: GOAL_EXTENSION_VERSION,', 'crewvelaPlugins: { version: 1, catalogMethod: "_crewvela/plugins", nativeMentions: true },\n        crewvelaGoal: { version: 1, nativeSubscriptions: true },\n        crewvelaSteering: { version: 1, activeTurnOnly: true },\n        goal: {\n          version: GOAL_EXTENSION_VERSION,');
  replaceOnce('.onRequest(methods.agent.session.prompt, (ctx) => getAgent().prompt(ctx.params, ctx.signal))',
    '.onRequest(methods.agent.session.prompt, async (ctx) => { await crewvelaValidatePlugins(ctx.params); return getAgent().prompt(ctx.params, ctx.signal); })');
  // Goal set/resume in 1.11.0 can cause native turns before any session event
  // subscription exists. Enter its existing prompt/command pipeline FIRST so
  // messages, approvals, goal state and later autonomous turns use that pipe.
  replaceOnce('.onRequest(GOAL_CONTROL_METHOD, goalControlParamsParser, (ctx) => getAgent().extMethod(GOAL_CONTROL_METHOD, ctx.params))',
    '.onRequest(GOAL_CONTROL_METHOD, goalControlParamsParser, (ctx) => crewvelaGoalControl(ctx.params, ctx.signal))');
  replaceOnce('const commandPromise = this.availableCommands.tryHandleCommand(params.prompt, sessionState, {',
    'const commandPromise = this.availableCommands.tryHandleCommand(params.prompt, sessionState, { crewvelaGoal: params._meta?.crewvelaGoal, crewvelaGoalSync: () => crewvelaSyncGoalSettings(this, sessionState), crewvelaGoalIsCurrent: () => (this.goalControlGenerations.get(params.sessionId) ?? 0) === (params._meta?.crewvelaGoal?.controlGeneration ?? crewvelaGoalGeneration) && this.getSessionGeneration(params.sessionId) === (params._meta?.crewvelaGoal?.sessionGeneration ?? crewvelaSessionGeneration) && !this.sessionIsClosing(params.sessionId),');
  replaceOnce('const activePrompt = this.trackActivePrompt(params.sessionId);', 'const activePrompt = this.trackActivePrompt(params.sessionId); const crewvelaGoalGeneration = this.goalControlGenerations.get(params.sessionId) ?? 0; const crewvelaSessionGeneration = this.getSessionGeneration(params.sessionId);');
  replaceOnce('async runGoalCommand(sessionState, rest, options) {',
    'async runGoalCommand(sessionState, rest, options) { if (!options.crewvelaGoalIsCurrent()) return { handled: true }; if (options.crewvelaGoal?.action === "set" || (rest.trim() && !["pause", "clear"].includes(rest.trim().toLowerCase()))) { await options.crewvelaGoalSync(); if (!options.crewvelaGoalIsCurrent()) return { handled: true }; }');
  replaceOnce('switch (argument.toLowerCase()) {', 'switch (options.crewvelaGoal?.action === "set" ? "__set_objective__" : argument.toLowerCase()) {');
  replaceOnce('.connect(acpJsonStream);', '.onRequest("_crewvela/plugins", emptyExtensionParamsParser, (ctx) => crewvelaPluginCatalog(ctx.params.sessionId)).onRequest("_crewvela/steerActive", sessionSteerParamsParser, (ctx) => crewvelaSteerActive(ctx.params)).connect(acpJsonStream);');
  replaceOnce('  agent({ name: package_default.name }).onConnect', `
  async function crewvelaSteerActive(params) {
    const agent = getAgent();
    const state = agent.getSessionState(params.sessionId);
    const turnId = state.currentTurnId;
    if (!turnId || !agent.activePrompts.has(params.sessionId) || agent.sessionIsClosing(params.sessionId)) return { injected: false };
    agent.assertSteerInputSupported(params, state);
    return { injected: await agent.injectSteerIntoActiveTurn(params, turnId, state) };
  }
  async function crewvelaGoalControl(p, signal) {
    const a = getAgent();
    if (p.action !== "set" && p.action !== "resume") {
      const result = await a.extMethod(GOAL_CONTROL_METHOD, p);
      const native = await a.runWithProcessCheck(() => a.codexAcpClient.codexClient.threadRead({ threadId: p.sessionId, includeTurns: false }));
      const session = new ACPSessionConnection(a.connection, p.sessionId);
      await session.update({ sessionUpdate: "session_info_update", _meta: { codex: { threadStatus: native.thread.status } } });
      return result;
    }
    const s = a.getSessionState(p.sessionId);
    const g = a.bumpGoalControlGeneration(p.sessionId);
    return a.prompt({ sessionId: p.sessionId, prompt: [{ type: "text", text: "/goal " + (p.action === "resume" ? "resume" : p.objective) }], _meta: { crewvelaGoal: { ...p, controlGeneration: g, sessionGeneration: a.getSessionGeneration(p.sessionId) } } }, signal);
  }
  async function crewvelaSyncGoalSettings(a, s) {
    const model = ModelId.fromString(s.currentModelId);
    await a.runWithProcessCheck(() => a.codexAcpClient.codexClient.threadSettingsUpdate({
      threadId: s.sessionId,
      model: model.model,
      effort: model.effort,
      collaborationMode: createCodexCollaborationMode(s.collaborationMode, s.currentModelId),
      approvalPolicy: s.agentMode.approvalPolicy,
      approvalsReviewer: s.agentMode.approvalsReviewer,
      sandboxPolicy: addAdditionalDirectoriesToSandboxPolicy(s.agentMode.sandboxPolicy, s.additionalDirectories),
      summary: s.account?.type === "apiKey" || (s.supportedReasoningEfforts.length > 0 && s.supportedReasoningEfforts.every(e => e.reasoningEffort === "none")) ? "none" : "auto",
      serviceTier: resolveFastServiceTier(s.fastModeEnabled, s.currentModelSupportsFast)
    }));
  }
  async function crewvelaPluginCatalog(sessionId) {
    const agent = getAgent();
    const session = agent.sessions.get(sessionId);
    if (!session) throw RequestError.invalidParams("Unknown session");
    return await agent.codexAcpClient.codexClient.connection.sendRequest("plugin/installed", { cwds: [session.cwd] });
  }
  async function crewvelaValidatePlugins(params) {
    const refs = (params.prompt ?? []).filter(block => block.type === "resource_link" && block.uri?.startsWith("plugin://"));
    if (!refs.length) return;
    const catalog = await crewvelaPluginCatalog(params.sessionId);
    const allowed = new Set((catalog.marketplaces ?? []).flatMap(m => (m.plugins ?? []).filter(p => p.installed === true && p.enabled === true && p.availability === "AVAILABLE").map(p => "plugin://" + p.id)));
    if (refs.some(ref => !/^plugin:\\/\\/[A-Za-z0-9._-]+@[A-Za-z0-9._-]+$/.test(ref.uri) || !allowed.has(ref.uri))) throw RequestError.invalidParams("Plugin unavailable or disabled; refresh the plugin list");
  }
  agent({ name: package_default.name }).onConnect`);
  return source.replaceAll('import.meta.url', JSON.stringify(originalUrl));
}

function crewvelaStartIsolatedAppServer(codexPath, home, inherited) {
  if (!codexPath || !path.isAbsolute(codexPath) || !path.isAbsolute(home) || home.includes('\0')) {
    throw new Error('CrewVela Codex isolation requires an absolute CLI and storage path');
  }
  const env = { ...inherited, CODEX_HOME: home, CODEX_SQLITE_HOME: home };
  const override = `sqlite_home=${home}`;
  if (process.platform === 'win32' && /\.(cmd|bat)$/i.test(codexPath)) {
    // Keep the native npm shim without handing its paths to an interpolating shell.
    // cmd quoting protects spaces/metacharacters but cannot disable percent expansion.
    if (/["%!^\r\n]/.test(codexPath + home)) {
      throw new Error('CrewVela Codex isolation cannot quote these paths for a Windows batch launcher');
    }
    return spawn(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/v:off', '/c', `""${codexPath}" -c "${override}" app-server"`],
      { env, windowsVerbatimArguments: true });
  }
  return spawn(codexPath, ['-c', override, 'app-server'], { env });
}

async function main() {
  const entry = process.argv[2] ?? await resolveAdapterEntry();
  const originalUrl = pathToFileURL(entry).href;
  const pkg = JSON.parse(await readFile(new URL('../package.json', originalUrl), 'utf8'));
  const isolated = process.env.CREWVELA_CODEX_SQLITE_HOME !== undefined;
  if (pkg.version !== '1.11.0') {
    if (isolated) throw new Error('CrewVela Codex isolation requires codex-acp 1.11.0');
    process.stderr.write('CrewVela plugin bridge unavailable: unsupported codex-acp version; using native adapter.\n');
    await import(originalUrl);
    return;
  }
  let patched;
  try { patched = patchAdapter((await readFile(entry, 'utf8')).replaceAll('\r\n', '\n'), originalUrl); }
  catch (error) {
    if (isolated) throw error;
    process.stderr.write(`CrewVela plugin bridge unavailable: ${error.message}; using native adapter.\n`);
    await import(originalUrl);
    return;
  }
  await import('data:text/javascript;base64,' + Buffer.from(patched).toString('base64'));
}
if (process.env.CREWVELA_CODEX_ACP_BRIDGE === '1') await main();
