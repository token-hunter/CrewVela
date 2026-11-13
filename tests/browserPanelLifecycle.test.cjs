const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

// 执行真实组件的 hooks 和 effect，IPC 用可控 Promise 放大创建期间的竞态。
function mountPanel(initialUrl = 'https://example.com', mac = false, existingNativeUrl = null) {
  const calls = [];
  const slots = [];
  const effects = [];
  const observers = [];
  const intervals = [];
  const listeners = new Map();
  let cursor = 0;
  let modal = false;
  let deferred;
  let boundsFailure = false;
  let nativeExists = !!existingNativeUrl;
  const host = { getBoundingClientRect: () => ({ left: 100, top: 50, width: 600, height: 400 }) };
  const react = {
    memo: (component) => component,
    useCallback: (callback) => callback,
    useState(initial) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
      return [slots[index], (value) => { slots[index] = value; }];
    },
    useRef(initial) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = { current: initial };
      return slots[index];
    },
    useEffect: (effect) => effects.push(effect),
  };
  const invoke = (command, args) => {
    calls.push({ command, args });
    if (command === 'browser_panel_open') {
      nativeExists = true;
      return new Promise((resolve, reject) => { deferred = { resolve, reject }; });
    }
    if (command === 'browser_panel_set_bounds' && boundsFailure) {
      boundsFailure = false;
      return Promise.reject(new Error('temporary bounds failure'));
    }
    if (command === 'browser_panel_current_url') return nativeExists ? Promise.resolve(existingNativeUrl || 'https://example.com') : Promise.reject(new Error('no panel'));
    return Promise.resolve();
  };
  const jsx = (type, props) => {
    if (props?.ref && type === 'div') props.ref.current = host;
    return { type, props };
  };
  const module = { exports: {} };
  const source = fs.readFileSync(path.join(__dirname, '../src/components/BrowserPanel.tsx'), 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX },
  }).outputText;
  vm.runInNewContext(output, {
    exports: module.exports,
    require(name) {
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return { jsx, jsxs: jsx };
      if (name === '@tauri-apps/api/core') return { invoke };
      if (name === '@tauri-apps/api/event') return { listen: (name, callback) => { listeners.set(name, callback); return Promise.resolve(() => {}); } };
      if (name === '@tauri-apps/plugin-opener') return { openUrl: () => Promise.resolve() };
      if (name === '../platform') return { IS_MAC: mac };
      if (name.endsWith('browserPanelState')) return {
        loadBrowserPanelState: () => ({ url: initialUrl }),
        saveBrowserPanelState: () => {},
        normalizeAddressInput: (value) => value,
      };
      if (name.endsWith('modalStack')) return { hasOpenModal: () => modal };
      if (name.startsWith('@phosphor-icons/')) return {};
      throw new Error(`Unexpected import: ${name}`);
    },
    ResizeObserver: class {
      constructor(callback) { observers.push(callback); }
      observe() {}
      disconnect() {}
    },
    requestAnimationFrame: (callback) => { queueMicrotask(callback); return 1; },
    cancelAnimationFrame: () => {},
    window: {
      setInterval: (callback) => { intervals.push(callback); return intervals.length; },
      clearInterval: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
    },
  });
  function render(active) {
    cursor = 0;
    effects.length = 0;
    module.exports.BrowserPanel({ active });
    for (const effect of effects) effect();
  }
  render(true);
  return {
    calls,
    failNextBounds: () => { boundsFailure = true; },
    render,
    observer: () => observers[0](),
    tick: () => intervals[0](),
    setModal: (value) => { modal = value; },
    navigate: (url) => listeners.get('browser-panel-navigated')({ payload: { url } }),
    event: (name) => { if (name === 'browser-panel-agent-closed') nativeExists = false; listeners.get(name)?.({ payload: {} }); },
    resolve: () => deferred.resolve(),
    reject: () => deferred.reject(new Error('temporary WebView failure')),
    error: () => slots[4],
  };
}

const flush = () => new Promise((resolve) => setImmediate(resolve));
const opens = (panel) => panel.calls.filter((call) => call.command === 'browser_panel_open');

test('failed bounds update is retried without a further resize', async () => {
  const panel = mountPanel();
  panel.failNextBounds();
  panel.resolve();
  await flush();
  const bounds = () => panel.calls.filter(call => call.command === 'browser_panel_set_bounds');
  assert.equal(bounds().length, 1);
  panel.tick();
  await flush();
  assert.equal(bounds().length, 2);
  assert.deepEqual(bounds()[1].args, bounds()[0].args);
  panel.tick();
  assert.equal(bounds().length, 2, 'successful bounds must not be resent every tick');
});

test('effect、首帧 observer 和巡检在创建期间只发送一次 open', async () => {
  const panel = mountPanel();
  panel.observer();
  panel.tick();
  await flush();
  assert.equal(opens(panel).length, 1);
  panel.resolve();
  await flush();
  assert.equal(panel.error(), '');
  panel.tick();
  assert.equal(opens(panel).length, 1);
});

test('空恢复状态不主动创建；后台真实导航后启用生命周期同步', async () => {
  const panel = mountPanel('');
  assert.equal(opens(panel).length, 0);
  panel.navigate('https://example.org');
  panel.render(true);
  assert.equal(opens(panel).length, 1);
  assert.equal(opens(panel)[0].args.url, 'https://example.org');
  panel.resolve();
  await flush();
  const beforeHide = panel.calls.length;
  panel.render(false);
  assert.ok(panel.calls.slice(beforeHide).some((call) => call.command === 'browser_panel_set_visible' && call.args.visible === false));
});

test('创建失败释放在途状态，后续巡检可重试并清除错误', async () => {
  const panel = mountPanel();
  panel.reject();
  await flush();
  assert.match(panel.error(), /temporary WebView failure/);
  panel.tick();
  assert.equal(opens(panel).length, 2);
  panel.resolve();
  await flush();
  assert.equal(panel.error(), '');
});

for (const state of ['inactive', 'modal']) {
  test(`创建完成时 ${state} 状态隐藏原生面板`, async () => {
    const panel = mountPanel();
    if (state === 'inactive') panel.render(false);
    else panel.setModal(true); // 不等待 400ms 巡检也必须读取当前弹窗状态。
    const beforeCompletion = panel.calls.length;
    panel.resolve();
    await flush();
    const completed = panel.calls.slice(beforeCompletion);
    assert.equal(completed.filter((call) => call.command === 'browser_panel_set_visible' && call.args.visible === false).length, 1);
    assert.equal(completed.filter((call) => call.command === 'browser_panel_set_bounds').length, 0);
  });
}

test('macOS Agent激活尊重已打开弹窗，关闭后不保留失效控制器', async () => {
  const panel = mountPanel('https://example.com', true);
  panel.resolve(); await flush();
  panel.setModal(true);
  const before = panel.calls.length;
  panel.event('browser-control-panel-needed'); await flush();
  assert.ok(panel.calls.slice(before).some(call => call.command === 'browser_panel_set_visible' && !call.args.visible));
  assert.equal(opens(panel).length, 1);
  panel.event('browser-panel-agent-closed');
  panel.setModal(false); panel.render(true);
  assert.equal(opens(panel).length, 1, 'close must not immediately recreate');
  panel.navigate('https://example.org'); panel.render(true);
  assert.equal(opens(panel).length, 2, 'navigation after close recreates the view');
});

test('macOS 首次Agent事件早于懒挂载时，从已存在的native URL恢复并显示', async () => {
  const panel = mountPanel('', true, 'https://agent.example');
  assert.equal(opens(panel).length, 0);
  // No activation or navigation event is delivered to this newly-mounted component.
  await flush();
  assert.equal(opens(panel).length, 1);
  assert.equal(opens(panel)[0].args.url, 'https://agent.example');
  panel.resolve(); await flush();
  assert.ok(!panel.calls.some(call => call.command === 'browser_panel_navigate'), 'must not restore an old or empty URL over the Agent page');
});
