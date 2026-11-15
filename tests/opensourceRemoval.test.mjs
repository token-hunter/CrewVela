import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import ts from 'typescript';
const read = p => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8');
const load = async p => import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(read(p), { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText).toString('base64')}`);

test('removed Hermes integration has no production code, launch options, bridge or logo', () => {
  function check(directory) {
    for (const entry of readdirSync(new URL(`../${directory}`, import.meta.url), { withFileTypes: true })) {
      const path = `${directory}/${entry.name}`;
      if (entry.isDirectory()) check(path);
      else if (/\.(?:tsx?|rs|py|mjs|css)$/.test(entry.name)) assert.doesNotMatch(read(path), /hermes/i, path);
    }
  }
  check('src');
  check('src-tauri/src');
  assert.equal(existsSync(new URL('../src/assets/agent-hermes.png', import.meta.url)), false);
});

test('unknown historical runtime stays readable but cannot be enabled or offered', async () => {
  const { isRetiredRuntime, TERMINAL_RUNTIMES, CHAT_RUNTIMES, COLLABORATION_RUNTIMES } = await load('src/runtimeTaxonomy.ts');
  assert.equal(isRetiredRuntime('unsupported'), true);
  assert.equal(isRetiredRuntime('removed_runtime'), true);
  assert.equal(isRetiredRuntime('codex'), false);
  assert.equal(isRetiredRuntime(null), false);
  for (const choices of [TERMINAL_RUNTIMES, CHAT_RUNTIMES, COLLABORATION_RUNTIMES]) assert.equal(choices.includes('unsupported'), false);
});

test('retired features have no routes, native modules, startup hooks, packaged resources or dependencies', () => {
  const app = read('src/App.tsx');
  const settings = read('src/components/SettingsDialog.tsx');
  const native = read('src-tauri/src/lib.rs');
  const manifest = read('src-tauri/tauri.conf.json');
  const dependencies = JSON.parse(read('package.json')).dependencies;
  assert.doesNotMatch(settings, /"(?:voice|modelEvaluation|studio|workflowCanvas)"/);
  assert.doesNotMatch(app, /useVoiceWake|DesktopCompanion|model_eval_usage_by_provider/);
  assert.doesNotMatch(native, /\b(?:mod|use)\s+(?:livekit_voice|native_speech|local_speech|local_wake|edge_speech|studio|studio_agent|studio_tools|self_media|model_eval|workflow_canvas|media)\b/);
  assert.doesNotMatch(native, /(?:livekit_voice|native_speech|local_speech|local_wake|edge_speech|studio|model_eval|workflow_canvas)::/);
  assert.doesNotMatch(manifest, /orange-speech|orange-wake|studio-yue|livekit/);
  assert.equal(dependencies['livekit-client'], undefined);
  assert.equal(dependencies['@xyflow/react'], undefined);
  for (const path of ['src/components/DesktopCompanion.tsx', 'src/components/ModelEvaluationPanel.tsx', 'src/components/StudioSettingsPanel.tsx', 'src/components/WorkflowCanvasPanel.tsx', 'tools/livekit-agent/agent.py']) {
    assert.equal(existsSync(new URL(`../${path}`, import.meta.url)), false, path);
  }
});

test('existing shortcut preferences drop retired routes and preserve retained choices', async () => {
  const { sanitizeSidebarShortcuts } = await load('src/sidebarShortcuts.ts');
  assert.deepEqual(sanitizeSidebarShortcuts(['voice', 'modelEvaluation', 'studio', 'workflowCanvas', 'git', 'companionship', 'automation']), ['git', 'companionship', 'automation']);
});

test('shared feature styles survive removal of legacy feature stylesheets', () => {
  const notifications = read('src/styles/notifications.css');
  const integrations = read('src/styles/integrations.css');
  const entry = read('src/styles.css');
  assert.match(entry, /styles\/notifications\.css/);
  assert.match(entry, /styles\/integrations\.css/);
  for (const selector of ['notification-history-list', 'cid-account-row', 'notification-controls']) assert.ok(notifications.includes(`.${selector}`), selector);
  for (const selector of ['notification-overlay-progress', 'skill-market', 'file-preview-editor']) assert.ok(integrations.includes(`.${selector}`), selector);
  assert.doesNotMatch(notifications + integrations, /\.model-eval-|\.workflow-canvas-|\.multi-agent-voice-/);
});
