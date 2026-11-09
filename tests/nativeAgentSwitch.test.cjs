const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/nativeAgentSwitch.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);
const record = (id, patch = {}) => ({ id, kind: 'terminal', agentRuntimeKind: 'zcode', providerId: 'local-runtime-zcode', cwd: 'F:\\project\\one', updatedAt: 1, ...patch });

test('switch reuses newest independent native conversation only in the same workspace and directory', async () => {
  const { nativeAgentCounterpart: find } = await modulePromise;
  const original = record('source', { agentRuntimeKind: 'deepseek_harness', providerId: 'local-runtime-deepseek_harness' });
  const matches = [record('old'), record('new', { cwd: 'f:/PROJECT/one/', updatedAt: 2 })];
  const excluded = [record('other-tab', { updatedAt: 999 }), record('other-dir', { cwd: 'F:/project/two', updatedAt: 999 }),
    record('archived', { archivedAt: 123, updatedAt: 999 }), record('team', { multiAgentProjectId: 'p', updatedAt: 999 }),
    record('agent', { multiAgentAgentId: 'a', updatedAt: 999 }), record('pty', { providerId: 'external', updatedAt: 999 }),
    record('shell', { kind: 'claude', updatedAt: 999 })];
  const all = [original, ...matches, ...excluded];
  const before = all.slice();
  assert.strictEqual(find(all, original, 'zcode', 'w1', id => id === 'other-tab' ? 'w2' : 'w1'), matches[1]);
  assert.deepEqual(all, before, 'lookup must not sort or mutate the session list');
  assert.equal(find(excluded, original, 'zcode', 'w1', id => id === 'other-tab' ? 'w2' : 'w1'), undefined);
});

test('Windows UNC case and separators normalize while POSIX paths remain case sensitive', async () => {
  const { nativeAgentCounterpart: find } = await modulePromise;
  const target = record('target', { cwd: '//SERVER/Share/Project/' });
  assert.strictEqual(find([target], record('source', { cwd: '\\\\server\\share\\project' }), 'zcode', 'w', () => 'w'), target);
  assert.equal(find([record('t', { cwd: '/home/User/project' })], record('s', { cwd: '/home/user/project' }), 'zcode', 'w', () => 'w'), undefined);
});

test('navigation selects an existing independent chat without creating or handing off work', async () => {
  const { openNativeAgentConversation: open } = await modulePromise;
  const source = record('source', { agentRuntimeKind: 'deepseek_harness' });
  const target = record('existing');
  const selected = [];
  await open([source, target], source, 'zcode', 'workspace', () => 'workspace',
    async () => { throw new Error('existing chat must be reused'); },
    async (...args) => { selected.push(args); });
  assert.deepEqual(selected, [[target, false]]);
  assert.equal(source.id, 'source');
});

test('navigation creates each runtime in source cwd/project when no scoped counterpart exists', async () => {
  const { openNativeAgentConversation: open, NATIVE_AGENT_CREATE_COMMANDS: commands } = await modulePromise;
  for (const [kind, command] of Object.entries(commands)) {
    const source = record('source', { agentRuntimeKind: 'codex', projectId: 'project-one' });
    const outOfScope = record('other-workspace', { agentRuntimeKind: kind, providerId: `local-runtime-${kind}` });
    const target = record('new', { agentRuntimeKind: kind });
    const calls = [];
    await open([source, outOfScope], source, kind, 'w1', id => id === outOfScope.id ? 'w2' : 'w1',
      async (...args) => { calls.push(['create', ...args]); return target; },
      async (...args) => { calls.push(['select', ...args]); });
    assert.deepEqual(calls, [['create', command, { cwd: source.cwd, projectId: source.projectId }], ['select', target, true]]);
  }
});

test('current agent is a no-op and creation failures propagate without selecting a chat', async () => {
  const { openNativeAgentConversation: open } = await modulePromise;
  const source = record('source');
  const unexpected = async () => { throw new Error('must not be called'); };
  await open([], source, 'zcode', 'w', () => 'w', unexpected, unexpected);
  await assert.rejects(open([], source, 'claude_chat', 'w', () => 'w',
    async () => { throw new Error('runtime unavailable'); }, unexpected), /runtime unavailable/);
});

test('selector icons navigate while only the separate handoff button starts a handoff', () => {
  const componentSource = fs.readFileSync(require('node:path').join(__dirname, '../src/components/NativeAgentWelcome.tsx'), 'utf8');
  const js = ts.transpileModule(componentSource, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const componentModule = { exports: {} };
  const runtimes = ['claude_chat', 'codex_chat', 'kimi_chat', 'zcode', 'deepseek_harness', 'opencode_chat', 'pi_chat'];
  const runtimeLabels = Object.fromEntries(runtimes.map(kind => [kind, kind]));
  const load = name => name === '../runtimeLogos' ? { RUNTIME_LABELS: runtimeLabels, RUNTIME_LOGOS: runtimeLabels }
    : name === '@phosphor-icons/react' ? { ArrowsLeftRight: () => null }
    : name.startsWith('./Native') ? {} : require(name);
  new Function('require', 'module', 'exports', js)(load, componentModule, componentModule.exports);
  const calls = [];
  const render = patch => componentModule.exports.NativeAgentSelector({ kind: 'zcode', onSwitchAgent: kind => calls.push(kind), onHandoffAgent: () => calls.push('handoff'), ...patch });
  const tree = render({});
  assert.equal(render({ managedToolbar: true }), null, 'App owns the actual session tabs and hides the legacy icon row');
  const [nav, handoff] = tree.props.children;
  const icons = nav.props.children;
  icons.find(button => button.props['aria-pressed']).props.onClick();
  assert.deepEqual(calls, [], 'current chat stays selected');
  icons.find(button => button.props['aria-label'] === '打开 codex_chat 聊天').props.onClick();
  assert.deepEqual(calls, ['codex_chat']);
  handoff.props.onClick();
  assert.deepEqual(calls, ['codex_chat', 'handoff']);
  const busyTree = render({ switching: true });
  assert.ok(busyTree.props.children[0].props.children.every(button => button.props.disabled));
  assert.equal(busyTree.props.children[1].props.disabled, true);
  const disabledTree = render({ enabledAgents: { codex_chat: false } });
  assert.equal(disabledTree.props.children[0].props.children.find(button => button.props['aria-label'] === '打开 codex_chat 聊天').props.disabled, true);
});

test('new chat always creates a distinct session, including the same Agent, with source directory and project', async () => {
  const { createNativeAgentConversation: createChat, NATIVE_AGENT_CREATE_COMMANDS: commands } = await modulePromise;
  for (const [kind, command] of Object.entries(commands)) {
    const source = record('source', { agentRuntimeKind: kind, projectId: 'project-one' });
    const created = [];
    const selected = [];
    const create = async (actualCommand, args) => {
      assert.equal(actualCommand, command);
      assert.deepEqual(args, { cwd: source.cwd, projectId: source.projectId });
      const target = record(`new-${created.length}`, { agentRuntimeKind: kind });
      created.push(target);
      return target;
    };
    await createChat(source, kind, create, target => selected.push(target));
    await createChat(source, kind, create, target => selected.push(target));
    assert.equal(created.length, 2);
    assert.notEqual(created[0].id, created[1].id);
    assert.deepEqual(selected, created);
    assert.equal(source.id, 'source', 'new chat leaves the source intact');
  }
});

test('new chat propagates failure and never selects a fictitious result', async () => {
  const { createNativeAgentConversation: createChat } = await modulePromise;
  let selected = false;
  await assert.rejects(createChat(record('source'), 'zcode', async () => { throw new Error('creation failed'); }, () => { selected = true; }), /creation failed/);
  assert.equal(selected, false);
});

test('opened tabs preserve opening order and distinct same-Agent sessions; handoff replaces its logical slot', async () => {
  const { nativeChatTabSessions: tabs } = await modulePromise;
  const first = record('first');
  const second = record('second');
  const unseen = record('unseen');
  const sessions = [unseen, second, first];
  assert.deepEqual(tabs(sessions, ['first', 'second'], id => id), [first, second]);
  const handedOff = record('target', { agentRuntimeKind: 'kimi_chat' });
  assert.deepEqual(tabs([second, handedOff, unseen], ['first', 'second'], id => id === 'target' ? 'first' : id), [handedOff, second]);
  assert.deepEqual(tabs([second, handedOff, unseen], ['second'], id => id === 'target' ? 'first' : id), [second], 'closed slot stays closed during background updates');
  assert.equal(sessions.length, 3, 'tab presentation never deletes session records');
  assert.deepEqual(tabs([second, handedOff, unseen], ['second', 'first'], id => id === 'target' ? 'first' : id), [second, handedOff], 'sidebar reopening returns the current handoff target');
  assert.deepEqual(tabs([second], ['first', 'second'], id => id), [second], 'workspace-filtered sessions exclude other workspace tabs');
});

test('new chat menu Escape consumes the global shortcut and restores focus; trigger ArrowUp chooses the last enabled Agent', () => {
  const source = fs.readFileSync(require('node:path').join(__dirname, '../src/components/NativeChatTabs.tsx'), 'utf8');
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const componentModule = { exports: {} };
  const states = [];
  let focused = null;
  const trigger = { focus: () => { focused = 'trigger'; } };
  const buttons = ['claude', 'codex', 'pi'].map(id => ({ focus: () => { focused = id; } }));
  let scrollOptions;
  const effects = [];
  let stateIndex = 0;
  const refs = [{ current: { querySelectorAll: () => buttons } }, { current: trigger }, { current: { querySelector: () => ({ closest: () => ({ scrollIntoView: options => { scrollOptions = options; } }) }) } }];
  const load = name => name === 'react' ? { useState: initial => [stateIndex++ === 0 ? true : initial, value => { if (typeof value === 'boolean') states.push(value); }], useEffect: callback => effects.push(callback), useLayoutEffect: callback => effects.push(callback), useRef: () => refs.shift() }
    : name === '../runtimeLogos' ? { RUNTIME_LABELS: {}, RUNTIME_LOGOS: {} }
    : name === '../sessionCapabilities' ? { sessionDisplayTitle: item => item.title }
    : name === '@phosphor-icons/react' ? { Plus: 'span', X: 'span', ArrowsLeftRight: 'span' } : require(name);
  new Function('require', 'module', 'exports', js)(load, componentModule, componentModule.exports);
  const tree = componentModule.exports.NativeChatTabs({ sessions: [], selectedId: null, creating: false, enabledAgents: {}, canCreate: true, onSelect: () => {}, onClose: () => {}, onCreate: () => {} });
  effects[2]();
  assert.deepEqual(scrollOptions, { block: 'nearest', inline: 'nearest' }, 'newly selected tabs scroll into the visible area without scrolling the page');
  const create = tree.props.children[0].props.children[1];
  let prevented = false;
  let stopped = false;
  create.props.onKeyDown({ key: 'Escape', preventDefault: () => { prevented = true; }, stopPropagation: () => { stopped = true; } });
  assert.equal(prevented, true);
  assert.equal(stopped, true, 'global Escape must not clear the selected chat');
  assert.deepEqual(states, [false]);
  assert.equal(focused, 'trigger');
  const previousDocument = global.document;
  global.document = { activeElement: trigger };
  try {
    create.props.onKeyDown({ key: 'ArrowUp', preventDefault: () => {} });
    assert.equal(focused, 'pi');
    create.props.onKeyDown({ key: 'ArrowDown', preventDefault: () => {} });
    assert.equal(focused, 'claude');
  } finally {
    if (previousDocument === undefined) delete global.document;
    else global.document = previousDocument;
  }
});

test('compact native chat tabs capture pointer on the tab button and retain ordinary click behavior', () => {
  const source = fs.readFileSync(require('node:path').join(__dirname, '../src/components/NativeChatTabs.tsx'), 'utf8');
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const componentModule = { exports: {} };
  const load = name => name === 'react'
    ? { useState: initial => [initial, () => {}], useEffect: () => {}, useLayoutEffect: () => {}, useRef: () => ({ current: null }) }
    : name === 'react-dom' ? { createPortal: () => null }
    : name === '../runtimeLogos' ? { RUNTIME_LABELS: {}, RUNTIME_LOGOS: { codex_chat: 'codex', pi_chat: 'pi' } }
    : name === '../sessionCapabilities' ? { sessionDisplayTitle: item => item.title }
    : name === '@phosphor-icons/react' ? { Plus: 'span', X: 'span', ArrowsLeftRight: 'span' }
    : require(name);
  new Function('require', 'module', 'exports', js)(load, componentModule, componentModule.exports);
  const sessions = [
    { id: 'codex', agentRuntimeKind: 'codex_chat', title: 'Codex Chat', cwd: '/repo', status: 'running' },
    { id: 'pi', agentRuntimeKind: 'pi_chat', title: 'Pi Chat', cwd: '/repo', status: 'running' },
  ];
  const render = (onSelect, onReorder) => componentModule.exports.NativeChatTabs({
    sessions, selectedId: 'codex', creating: false, enabledAgents: {}, canCreate: true,
    onSelect, onClose: () => {}, onCreate: () => {}, onReorder,
  });
  const wrappers = tree => tree.props.children[0].props.children[0].props.children;
  const roleTab = wrapper => wrapper.props.children.find(child => child?.props?.role === 'tab');
  const eventOrder = [];
  const reorderCalls = [];
  const selected = [];
  const tree = render(id => selected.push(id), (...args) => { eventOrder.push('reorder'); reorderCalls.push(args); });
  const [sourceWrapper, targetWrapper] = wrappers(tree);
  const sourceTab = roleTab(sourceWrapper);
  let capturedPointer = null;
  const buttonNode = {
    setPointerCapture: id => { capturedPointer = id; eventOrder.push('capture'); },
    hasPointerCapture: id => capturedPointer === id,
    releasePointerCapture: id => { eventOrder.push('release'); capturedPointer = null; },
  };
  const wrapperNode = { querySelector: selector => { assert.equal(selector, '[role="tab"]'); return buttonNode; } };
  const pointerTarget = { closest: () => null };
  sourceWrapper.props.onPointerDown({ button: 0, pointerId: 7, clientX: 237, target: pointerTarget, currentTarget: wrapperNode });
  assert.equal(capturedPointer, 7, 'capture starts on the tab button at pointerdown');

  sourceWrapper.props.onPointerDown({
    button: 0, pointerId: 8, clientX: 237,
    target: { closest: selector => selector === '.native-chat-tab-close' ? {} : null },
    currentTarget: wrapperNode,
  });
  assert.equal(capturedPointer, 7, 'the close button remains excluded from drag initiation');

  const targetNode = { dataset: { nativeTabId: 'pi' }, getBoundingClientRect: () => ({ left: 260, width: 44 }) };
  const previousDocument = global.document;
  global.document = { elementFromPoint: () => ({ closest: () => targetNode }) };
  try {
    let prevented = false;
    sourceWrapper.props.onPointerMove({ pointerId: 7, clientX: 245, currentTarget: wrapperNode, preventDefault: () => { prevented = true; } });
    assert.equal(prevented, true, 'movement past the existing 6px threshold enters the drag path');
    sourceWrapper.props.onPointerUp({ pointerId: 7, currentTarget: wrapperNode });
  } finally {
    if (previousDocument === undefined) delete global.document;
    else global.document = previousDocument;
  }
  assert.deepEqual(reorderCalls, [['codex', 'pi', false]]);
  assert.deepEqual(eventOrder, ['capture', 'reorder', 'release'], 'reorder runs before the captured button is released');
  let dragClickPrevented = false;
  sourceWrapper.props.onClickCapture({ preventDefault: () => { dragClickPrevented = true; }, stopPropagation: () => {} });
  assert.equal(dragClickPrevented, true, 'click suppression still follows a completed drag');

  const clickTree = render(id => selected.push(id), () => {});
  const clickWrapper = wrappers(clickTree)[0];
  const clickTab = roleTab(clickWrapper);
  let clickCapture = null;
  const clickButtonNode = {
    setPointerCapture: id => { clickCapture = id; },
    hasPointerCapture: id => clickCapture === id,
    releasePointerCapture: () => { clickCapture = null; },
  };
  const clickWrapperNode = { querySelector: () => clickButtonNode };
  clickWrapper.props.onPointerDown({ button: 0, pointerId: 9, clientX: 237, target: pointerTarget, currentTarget: clickWrapperNode });
  clickWrapper.props.onPointerUp({ pointerId: 9, currentTarget: clickWrapperNode });
  let ordinaryClickPrevented = false;
  clickWrapper.props.onClickCapture({ preventDefault: () => { ordinaryClickPrevented = true; }, stopPropagation: () => {} });
  clickTab.props.onClick();
  assert.equal(ordinaryClickPrevented, false, 'a pointerup below threshold does not suppress click');
  assert.deepEqual(selected, ['codex'], 'the tab button still receives and handles an ordinary click');
});

test('App capture-phase Escape lets the expanded new-chat menu handle dismissal before changing session selection', () => {
  const source = fs.readFileSync(require('node:path').join(__dirname, '../src/App.tsx'), 'utf8');
  const file = ts.createSourceFile('App.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  let handler;
  const visit = node => {
    if (ts.isVariableDeclaration(node) && node.name.getText(file) === 'onEscape') handler = node.initializer;
    ts.forEachChild(node, visit);
  };
  visit(file);
  assert.ok(handler, 'exercise the actual App capture listener');
  const js = ts.transpileModule(`exports.handle = ${handler.getText(file)};`, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
  const exports = {};
  let selection = 'current-chat';
  class Target {
    constructor(open) { this.open = open; }
    closest(selector) { return selector === '.native-chat-create' && this.open ? { querySelector: () => ({}) } : null; }
  }
  class InputTarget extends Target {
    constructor(selector, value = '') { super(false); this.selector = selector; this.value = value; this.classList = { contains: name => name === selector }; }
    closest(selector) { return selector === this.selector ? {} : super.closest(selector); }
  }
  const openMenus = new Set();
  new Function('exports', 'Element', 'HTMLInputElement', 'document', 'hasOpenModal', 'controlOpen', 'selectedSessionId', 'setSelectedSessionId', js)(exports, Target, InputTarget, { querySelector: selector => openMenus.has(selector) ? {} : null }, () => false, false, selection, value => { selection = value; });
  let stopped = false;
  const event = { key: 'Escape', defaultPrevented: false, target: new Target(true), preventDefault: () => {}, stopPropagation: () => { stopped = true; } };
  exports.handle(event);
  assert.equal(selection, 'current-chat', 'capture leaves the session selected');
  assert.equal(stopped, false, 'capture allows the event to reach the menu handler');
  exports.handle({ ...event, target: new Target(false) });
  assert.equal(selection, null, 'outside the menu the existing global shortcut still works');
  for (const selector of ['.session-action-menu', '.direct-new-menu', '.session-filter-menu', '.right-sidebar-new-menu']) {
    selection = 'current-chat';
    openMenus.add(selector);
    exports.handle({ ...event, target: new Target(false) });
    assert.equal(selection, 'current-chat', `${selector} must handle Escape before the global session shortcut`);
    openMenus.delete(selector);
  }
  for (const target of [new InputTarget('.session-search', 'query'), new InputTarget('workspace-tab-input'), new InputTarget('.browser-address'), new InputTarget('.multi-agent-composer-editor [role="combobox"][aria-expanded="true"]')]) {
    selection = 'current-chat';
    exports.handle({ ...event, target });
    assert.equal(selection, 'current-chat', `${target.selector} receives its local Escape first`);
  }
  selection = 'current-chat';
  exports.handle({ ...event, target: new InputTarget('.session-search') });
  assert.equal(selection, null, 'empty search retains the existing global escape behavior');
});
