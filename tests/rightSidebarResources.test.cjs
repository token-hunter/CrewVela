const { test } = require('node:test');
const assert = require('node:assert/strict');
const { isRetiredRuntime } = require('../src/runtimeTaxonomy.ts');
const fs = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');
const path = require('node:path');
const app = fs.readFileSync(path.join(__dirname, '../src/App.tsx'), 'utf8');
function platformModule(platform = 'Win32') {
  const source = fs.readFileSync(path.join(__dirname, '../src/platform.ts'), 'utf8');
  const module = { exports: {} };
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(js, { exports: module.exports, navigator: { platform } });
  return module.exports;
}

for (const platform of ['Win32', 'MacIntel']) test(`${platform} 开始页完整显示五个入口，图标更新且分别调用对应工具`, () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/components/RightSidebarActivityBar.tsx'), 'utf8');
  const componentAst = ts.createSourceFile('sidebar.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const start = componentAst.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'RightSidebarStart');
  assert.ok(start);
  const opened = [];
  const scope = {
    ...platformModule(platform),
    React: { createElement: (type, props, ...children) => ({ type, props: props ?? {}, children }) },
    FolderSimpleIcon: 'folder', TerminalWindowIcon: 'terminal-window', GlobeIcon: 'globe',
    GitBranchIcon: 'git-branch', RobotIcon: 'robot', HouseIcon: 'house', CaretRightIcon: 'caret-right',
  };
  const declarations = ['RightSidebarStart', 'ResourceIcon', 'sidebarTabKind'].map(name => componentAst.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === name).getText(componentAst).replace(/^export\s+/, '')).join('\n');
  const js = ts.transpileModule(declarations, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React },
  }).outputText;
  vm.createContext(scope);
  vm.runInContext(js, scope);
  const render = root => scope.RightSidebarStart({ root, onOpenTool: tool => opened.push(tool) });
  const cards = tree => tree.children.find(child => child?.props.className === 'right-sidebar-start-tools').children;
  const entries = cards(render('C:/example/crewvela'));
  assert.equal(entries.length, 5);
  assert.deepEqual(entries.map(entry => entry.children[1].children[0]),
    ['文件', '终端', '浏览器', 'Git', '多智能体']);
  assert.deepEqual(entries.map(entry => entry.children[0].type), ['folder', 'terminal-window', 'globe', 'git-branch', 'robot']);
  for (const [index, id] of ['files:one', 'terminal:one', 'browser:one', 'git:one', 'multi_agent:one'].entries()) {
    assert.equal(scope.ResourceIcon({ id }).type, entries[index].children[0].type, `${id}: tab and menu icon must match start entry`);
  }
  entries.forEach(entry => entry.props.onClick());
  assert.deepEqual(opened, ['files', platform === 'MacIntel' ? 'zsh' : 'powershell', 'browser', 'git', 'multi_agent']);
  assert.deepEqual(cards(render(null)).map(entry => entry.props.disabled === true), [true, true, false, false, false]);
});
const ast = ts.createSourceFile('App.tsx', app, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
function callback(name, context) {
  let expression;
  function walk(node) {
    if (ts.isVariableDeclaration(node) && node.name.getText(ast) === name) {
      expression = ts.isCallExpression(node.initializer) ? node.initializer.arguments[0] : node.initializer;
    }
    ts.forEachChild(node, walk);
  }
  walk(ast); assert.ok(expression, `missing ${name}`);
  const js = ts.transpileModule(`result = (${expression.getText(ast)});`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const scope = { ...platformModule(), ...context }; vm.runInNewContext(js, scope); return scope.result;
}
function selectionHarness(status) {
  const id = 'sidebar-1'; const state = { sessions: [{ id, kind: 'terminal', shell: 'cmd', status, ptyId: status === 'running' ? 1 : null }], tabs: [], mounted: new Set() };
  const calls = [];
  const context = {
    isRetiredRuntime,
    sessions: state.sessions, sidebarTerminalIds: [id], UI_PREVIEW: false, sidebarTabKind: tab => tab.split(':', 1)[0],
    invoke: async (command, args) => { calls.push({ command, args }); return { ...state.sessions[0], status: 'running', ptyId: 2 }; },
    isSessionActive: session => session.status === 'running',
    setSessions: fn => { state.sessions = fn(state.sessions); },
    setOpenRightSidebarTabs: fn => { state.tabs = fn(state.tabs); },
    setMountedRightSidebarTabs: fn => { state.mounted = fn(state.mounted); },
    setControlOpen: value => { state.control = value; }, setGitOpen: value => { state.git = value; },
    setFilesOpen: value => { state.files = value; }, setBrowserOpen: value => { state.browser = value; },
    setExtraRightSidebarTab: value => { state.active = value; }, setSidebarMenuOpen: value => { state.menu = value; },
    setLastRightSidebarTab: value => { state.last = value; }, showError: error => { throw new Error(error); },
  };
  return { select: callback('selectSession', context), state, calls };
}
test('从主会话列表选择正在运行的侧栏终端，重开其真实标签而不启动第二进程', async () => {
  const h = selectionHarness('running'); await h.select('sidebar-1');
  assert.equal(h.calls.length, 0); assert.equal(h.state.active, 'terminal:sidebar-1');
  assert.ok(h.state.mounted.has('terminal:sidebar-1')); assert.equal(h.state.tabs[0], 'terminal:sidebar-1');
  assert.equal(h.state.browser, false);
});
test('关闭后从主会话列表恢复侧栏终端，同时恢复 TerminalPane 挂载', async () => {
  const h = selectionHarness('stopped'); await h.select('sidebar-1');
  assert.equal(h.calls[0].command, 'resume_session'); assert.equal(h.calls[0].args.sessionId, 'sidebar-1');
  assert.equal(h.state.sessions[0].ptyId, 2); assert.equal(h.state.active, 'terminal:sidebar-1');
  assert.ok(h.state.mounted.has('terminal:sidebar-1'));
});
test('主 CLI 网格在原生聊天期间保活，避免侧栏轮询消费未挂载终端的输出', () => {
  assert.match(app, /\{anyLivePtySessions && \(\s*<div className="main-terminal-host" hidden=/);
  assert.match(app, /sessions=\{displaySessions\.filter\(item => !sidebarTerminalIds\.includes\(item\.id\)\)\}/);
});
test('新建菜单挂起原生浏览器，文件和浏览器资源保留挂载', () => {
  assert.match(app, /MemoBrowserPanel panelId=\{tab === "browser" \? undefined : tab\.slice\(8\)\} active=\{!guideRequest && activeRightSidebarTab === tab && !sidebarMenuOpen\}/);
  assert.match(app, /\[\.\.\.mountedRightSidebarTabs\]\.filter\(tab => sidebarTabKind\(tab\) === "browser"\)\.map/);
  assert.match(app, /\[\.\.\.mountedRightSidebarTabs\]\.filter\(tab => sidebarTabKind\(tab\) === "files"\)\.map/);
});
test('两个停止 IPC 同一批完成时并发关闭不会复活已关闭标签', async () => {
  const tabs = ['start', 'terminal:one', 'terminal:two'];
  const state = { tabs, mounted: new Set(tabs), sessions: [{ id: 'one' }, { id: 'two' }] };
  const pending = new Map(); const active = { current: 'terminal:two' }; const currentTabs = { current: tabs };
  const close = callback('closeSidebarResource', {
    UI_PREVIEW: false, sidebarTabKind: tab => tab.split(':', 1)[0],
    invoke: (command, args) => new Promise(resolve => pending.set(args.sessionId, resolve)),
    setSessions: fn => { state.sessions = fn(state.sessions); },
    setMountedRightSidebarTabs: fn => { state.mounted = fn(state.mounted); },
    setOpenRightSidebarTabs: fn => { state.tabs = fn(state.tabs); },
    openRightSidebarTabsRef: currentTabs, activeRightSidebarTabRef: active,
    selectRightSidebarTab: async tab => { active.current = tab; },
    showError: error => { throw new Error(error); },
  });
  const one = close('terminal:one'), two = close('terminal:two');
  pending.get('one')({ id: 'one', status: 'stopped' }); pending.get('two')({ id: 'two', status: 'stopped' });
  await Promise.all([one, two]);
  assert.deepEqual(Array.from(state.tabs), ['start']);
  assert.equal(active.current, 'start'); assert.equal(state.mounted.has('terminal:one'), false); assert.equal(state.mounted.has('terminal:two'), false);
});
test('停止终端失败时保留标签、挂载和活跃视图', async () => {
  const errors = []; let changed = false;
  const close = callback('closeSidebarResource', {
    UI_PREVIEW: false, invoke: async () => { throw new Error('stop failed'); },
    showError: error => errors.push(error), setSessions: () => { changed = true; },
    setMountedRightSidebarTabs: () => { changed = true; }, setOpenRightSidebarTabs: () => { changed = true; },
  });
  await close('terminal:one'); assert.equal(changed, false); assert.equal(errors.length, 1); assert.match(errors[0], /stop failed/);
});
for (const platform of ['Win32', 'MacIntel']) test(`${platform} 新建菜单自动聚焦，Escape 关闭并还原加号焦点；无工作区禁用文件与终端`, () => {
  const source = fs.readFileSync(path.join(__dirname, '../src/components/RightSidebarActivityBar.tsx'), 'utf8');
  const effects = [], focus = [], menuChanges = [];
  const jsx = (type, props) => {
    if (props?.ref) props.ref.current = type === 'div'
      ? { querySelector: () => ({ focus: () => focus.push('first-enabled') }) }
      : { focus: () => focus.push('add') };
    return { type, props };
  };
  const module = { exports: {} };
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  vm.runInNewContext(js, { exports: module.exports, require: name => name === 'react'
    ? { useRef: () => ({ current: null }), useEffect: effect => effects.push(effect) }
    : name === 'react/jsx-runtime' ? { jsx, jsxs: jsx } : name === '../platform' ? platformModule(platform) : {}, document: {} });
  const root = module.exports.RightSidebarActivityBar({ activeTab: 'start', tabs: [{ id: 'start', label: '开始' }], menuOpen: true,
    hasWorkspace: false, onMenuChange: open => menuChanges.push(open) });
  effects.forEach(effect => effect()); assert.deepEqual(focus, ['first-enabled']);
  root.props.onKeyDown({ key: 'Escape', preventDefault() {}, stopPropagation() {} });
  assert.deepEqual(menuChanges, [false]); assert.deepEqual(focus, ['first-enabled', 'add']);
  const items = [];
  function walk(node) {
    if (!node) return; if (Array.isArray(node)) { node.forEach(walk); return; }
    if (node.props?.role === 'menuitem') items.push(node);
    walk(node.props?.children);
  }
  walk(root);
  assert.equal(items.length, 6);
  assert.deepEqual(items.slice(1, 3).map(item => item.props.children[1]), platform === 'MacIntel' ? ['新建 zsh 终端', '新建 bash 终端'] : ['新建 PowerShell 终端', '新建 CMD 终端']);
  assert.deepEqual(items.map(item => item.props.disabled), [true, true, true, false, false, false]);
});
test('无持久化状态首次打开开始页，既有有效标签保持恢复', () => {
  let loader;
  function walk(node) {
    if (ts.isFunctionDeclaration(node) && node.name?.text === 'loadRightSidebarTab') loader = node.getText(ast);
    ts.forEachChild(node, walk);
  }
  walk(ast); assert.ok(loader);
  const js = ts.transpileModule(`${loader}\nresult = loadRightSidebarTab;`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  for (const [stored, expected] of [[null, 'start'], ['files', 'files'], ['browser', 'browser'], ['git', 'git'], ['multi_agent', 'multi_agent'], ['start', 'start']]) {
    const context = { RIGHT_SIDEBAR_TAB_STORAGE_KEY: 'test-sidebar', window: { localStorage: { getItem: () => stored, setItem() {} } },
      normalizeRightSidebarTab: value => ['files', 'browser', 'git'].includes(value) ? value : 'multi_agent' };
    vm.runInNewContext(js, context); assert.equal(context.result(), expected);
  }
});


test('关闭一个浏览器实例只关闭对应原生面板，保留兄弟实例和活跃标签', async () => {
  for (const [closed, panelId] of [['browser:first', 'first'], ['browser', null]]) {
    const sibling = 'browser:second';
    const state = { tabs: [closed, sibling], mounted: new Set([closed, sibling]) };
    const calls = [];
    const close = callback('closeSidebarResource', {
      UI_PREVIEW: false, sidebarTabKind: tab => tab.split(':', 1)[0],
      invoke: async (command, args) => { calls.push([command, args]); },
      setMountedRightSidebarTabs: update => { state.mounted = update(state.mounted); },
      setOpenRightSidebarTabs: update => { state.tabs = update(state.tabs); },
      openRightSidebarTabsRef: { current: state.tabs }, activeRightSidebarTabRef: { current: sibling },
      selectRightSidebarTab: async () => { throw new Error('closing an inactive browser must not change selection'); },
      showError: error => { throw new Error(error); },
    });
    await close(closed);
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], 'browser_panel_close');
    assert.equal(calls[0][1].panelId, panelId);
    assert.deepEqual(Array.from(state.tabs), [sibling]);
    assert.deepEqual(Array.from(state.mounted), [sibling]);
  }
});

test('关闭浏览器原生面板失败时保留实例标签与挂载', async () => {
  const errors = []; let changed = false;
  const close = callback('closeSidebarResource', {
    UI_PREVIEW: false, sidebarTabKind: tab => tab.split(':', 1)[0],
    invoke: async () => { throw new Error('browser close failed'); },
    showError: error => errors.push(error),
    setMountedRightSidebarTabs: () => { changed = true; }, setOpenRightSidebarTabs: () => { changed = true; },
  });
  await close('browser:first');
  assert.equal(changed, false);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /browser close failed/);
});
