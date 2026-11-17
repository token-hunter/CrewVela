import { createRoot } from 'react-dom/client';
import { useState } from 'react';
import { RuntimeManagementDialog } from '../../src/components/RuntimeManagementDialog';
import { COLLABORATION_RUNTIMES, CHAT_RUNTIMES } from '../../src/runtimeTaxonomy';
import { reorderRuntimeOrder } from '../../src/runtimePreferences';
import '../../src/styles.css';
import '../../src/styles/collaboration-overview.css';
const params = new URLSearchParams(location.search);
const theme = params.get('theme') === 'light' ? 'light' : 'dark';
document.documentElement.setAttribute('data-theme', theme);
const versions: Record<string, string> = { claude_code: '2.1.325', codex: '0.169.0', opencode: '1.19.5', kimi_code: '2.6.2', pi: '1.5.2' };
const state = {
  starts: 0,
  calls: [] as Array<{ command: string; args: any }>,
  job: null as any, holdStart: false, releaseStart: null as null | (() => void),
  fail: false, unknownTotal: false, installed: params.get('installed') === '1', auth: 'unverified',
  environmentError: params.get('scenario') === 'offline', loginError: params.get('scenario') === 'login-error',
  finish(phase: 'ready' | 'failed' | 'cancelled') {
    if (!state.job) return;
    state.job = { ...state.job, phase, message: phase === 'ready' ? '安装和启动自检通过' : phase === 'failed' ? '安装未完成，原入口保持不变' : '安装已取消，原入口保持不变', error: phase === 'failed' ? '安装包完整性校验失败，未切换原安装' : null };
    if (phase === 'ready') state.installed = true;
  },
};
if (params.get('previous-job') === '1') state.job = { id:'previous-ready', kind:'claude_code', phase:'ready', message:'安装已完成', downloaded:100, total:100, version:versions.claude_code, executable:'C:/fixture/official/claude.exe', error:null };
(window as any).runtimeInstallerFixture = state;
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  state.calls.push({ command, args });
  if (command === 'get_runtime_preferences') return { disabledRuntimes: [] };
  if (command === 'set_runtime_enabled') return { disabledRuntimes: args.enabled ? [] : [args.kind] };
  if (command === 'runtime_install_environment') {
    if (params.get('slow-local') === '1') await new Promise(resolve => setTimeout(resolve, 1500));
    if (state.environmentError) { state.environmentError = false; throw new Error('fixture network offline'); }
    return { kind: args.kind, supported: params.get('scenario') !== 'dependency', executable: state.installed ? `C:/fixture/official/${args.kind}.exe` : null, version: state.installed ? versions[args.kind] : null,
      latestVersion: versions[args.kind], source: state.installed ? 'CrewVela 用户级安装' : '未安装或安装异常', installDirectory: `C:/fixture/runtime-installations/${args.kind}`,
      dependencyMessage: params.get('scenario') === 'dependency' ? 'Kimi Code 需要 Git for Windows / Git Bash；安装后重新检查' : args.kind === 'kimi_code' ? '已检测到 Git Bash，不需要 Node/npm' : '当前平台支持，不需要 Node/npm',
      auth: ['claude_code', 'codex'].includes(args.kind) ? state.auth : 'unverified', error: state.installed ? null : '未检测到命令', updateError: null };
  }
  if (command === 'runtime_install_latest_version') {
    if (params.get('slow-updates') === '1') await new Promise(resolve => setTimeout(resolve, 2500));
    if (params.get('update-fails') === '1') throw new Error('fixture official version unavailable');
    return versions[args.kind];
  }
  if (command === 'runtime_install_status') return state.job?.kind === args.kind ? state.job : null;
  if (command === 'runtime_install_start') {
    if (state.holdStart) await new Promise<void>(resolve => { state.releaseStart = resolve; });
    state.job = { id: crypto.randomUUID(), kind: args.kind, phase: 'downloading', message: '正在下载并校验官方安装文件…', downloaded: 64, total: state.unknownTotal ? null : 100, version: versions[args.kind], executable: null, error: null };
    state.starts += 1;
    if (params.get('scenario') === 'retry') window.setTimeout(() => state.finish(state.starts === 1 ? 'failed' : 'ready'), 1000);
    if (params.get('scenario') === 'success') window.setTimeout(() => state.finish('ready'), 1000);
    if (params.get('scenario') === 'unknown-size') state.job.total = null;
    return state.job;
  }
  if (command === 'runtime_install_cancel') { state.finish('cancelled'); return; }
  if (command === 'runtime_open_login') { if (state.loginError) throw Error('fixture login window failed'); state.auth = 'authenticated'; return; }
  if (command === 'plugin:event|listen') return 1;
  if (command === 'plugin:event|unlisten') return;
  throw Error(`unsupported fixture IPC: ${command}`);
} };
function Fixture() {
  const [order, setOrder] = useState(Array.from(new Set([...COLLABORATION_RUNTIMES, ...CHAT_RUNTIMES])));
  const [open, setOpen] = useState(true);
  return <main style={{ minHeight: '100vh', background: 'var(--bg)', color: 'var(--text)' }}>
    <div style={{ padding: 24 }}>CrewVela · Runtime 安装隔离回归</div>
    {!open && <button onClick={() => setOpen(true)}>重新打开管理</button>}
    {open && <div className="settings-dialog settings-native" style={{ display: 'contents' }}><div className="collab-overview-page" style={{ display: 'contents' }}><RuntimeManagementDialog order={order} onClose={() => setOpen(false)} onReorder={(kind, target, position) => setOrder(current => reorderRuntimeOrder(current, kind, target, position))} /></div></div>}
  </main>;
}
const fixtureRoot = import.meta.hot?.data.root ?? createRoot(document.getElementById('root')!);
if (import.meta.hot) import.meta.hot.data.root = fixtureRoot;
fixtureRoot.render(<Fixture />);
