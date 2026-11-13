// macOS-only MCP adapter. Windows keeps browser-control-bridge.mjs and CDP.
import { readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { panelScript } from './browser-panel-dom.mjs';

const string = description => ({ type: 'string', description });
const ref = { element: string('目标元素说明'), ref: string('最近 browser_snapshot 返回的 ref') };
const tool = (name, description, properties = {}, required = []) => ({ name, description,
  inputSchema: { type: 'object', properties, required, additionalProperties: false } });
export const MACOS_PANEL_TOOLS = [
  tool('browser_navigate', '在可见的内置面板打开 http/https URL', { url: string('目标 URL') }, ['url']),
  tool('browser_snapshot', '读取可见页面文本和 DOM 控件（含 ref）；跨源 iframe 不可读取'),
  tool('browser_click', '点击 DOM 元素（合成事件；不是硬件输入）', { ...ref, doubleClick: { type: 'boolean' }, button: { type: 'string', enum: ['left'] } }, ['ref']),
  tool('browser_type', '填写文本框，可选择提交', { ...ref, text: string('文本'), submit: { type: 'boolean' } }, ['ref', 'text']),
  tool('browser_fill_form', '批量填写表单字段', { fields: { type: 'array', maxItems: 100, items: { type: 'object', properties: {
    name: string('字段名称'), type: { type: 'string', enum: ['textbox','checkbox','radio','combobox'] }, ref: string('控件 ref'), value: string('字段值；勾选框用 true/false') }, required: ['ref','type','value'], additionalProperties: false } } }, ['fields']),
  tool('browser_select_option', '选择下拉选项', { ...ref, values: { type: 'array', items: { type: 'string' }, minItems: 1 } }, ['ref','values']),
  tool('browser_hover', '向元素发送 DOM 悬停事件；不模拟系统鼠标', ref, ['ref']),
  tool('browser_press_key', '向当前焦点或 ref 发送 DOM 按键；支持 Enter/Tab 的表单默认行为', { key: { type: 'string', enum: ['Enter', 'Tab', 'Shift+Tab', 'Escape'] }, ref: string('可选控件 ref') }, ['key']),
  tool('browser_wait_for', '等待可见页面文本出现/消失或等待指定秒数（最多30秒）', { text: string('等待出现的文本'), textGone: string('等待消失的文本'), time: { type: 'number', minimum: 0, maximum: 30 } }),
  tool('browser_take_screenshot', '截取内置面板当前可见区域，返回 PNG；不需要屏幕录制权限'),
  tool('browser_navigate_back', '内置面板后退'),
  tool('browser_evaluate', '同步执行页面 JavaScript 函数；不能访问本地文件或主应用窗口', { function: string('同步 JavaScript 函数'), ref: string('可选元素 ref'), element: string('元素说明') }, ['function']),
  tool('browser_scroll', '滚动内置面板', { x: { type: 'number' }, y: { type: 'number' } }),
  tool('browser_tabs', '单页面板：仅列出/选择当前页/关闭；换站点请用 browser_navigate', { action: { type: 'string', enum: ['list','select','close'] }, index: { type: 'integer', minimum: 0, maximum: 0 } }, ['action']),
  tool('browser_close', '关闭内置面板，不创建新浏览器'),
];

export async function nativeCommand(configFile, command, signal) {
  const config = JSON.parse(readFileSync(configFile, 'utf8'));
  if (!Number.isInteger(config.port) || config.port < 1 || config.port > 65535 ||
      typeof config.token !== 'string' || !/^[a-zA-Z0-9-]{32,}$/.test(config.token)) throw new Error('面板连接配置无效');
  const response = await fetch(`http://127.0.0.1:${config.port}/command`, {
    method: 'POST', redirect: 'error', headers: { Authorization: `Bearer ${config.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(command), signal: AbortSignal.any([signal ?? new AbortController().signal, AbortSignal.timeout(20000)]),
  });
  if (!response.ok) throw new Error(`面板请求被拒绝（HTTP ${response.status}）`);
  const body = await response.json();
  if (body.error) throw new Error(body.error);
  return body.value;
}

const pause = (ms, signal) => new Promise((resolve, reject) => {
  signal?.throwIfAborted();
  const abort = () => { clearTimeout(timer); reject(new Error('请求已取消')); };
  const timer = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve(); }, ms);
  signal?.addEventListener('abort', abort, { once: true });
});
export class MacOSPanelBridge {
  constructor({ command, write }) { this.command = command; this.write = write; this.initialized = false; this.ready = false;
    this.used = false; this.queue = Promise.resolve(); this.requests = new Map(); this.closed = false; }
  async evaluate(action, args, signal) {
    const result = await this.command({ action: 'eval', script: panelScript(action, args) }, signal);
    if (!result || !result.ok) throw new Error(result?.error || '面板脚本未返回有效结果');
    return result.value;
  }
  async snapshot(signal) { return this.evaluate('snapshot', {}, signal); }
  async readyPage(signal, oldDocument) {
    const deadline = Date.now() + 15000;
    do {
      const native = await this.command({ action: 'state' }, signal);
      if (!native.url || native.loading) { await pause(100, signal); continue; }
      const info = await this.evaluate('info', {}, signal);
      if (info.ready !== 'loading' && (!oldDocument || info.documentId !== oldDocument)) return info;
      await pause(100, signal);
    } while (Date.now() < deadline);
    throw new Error('等待面板页面加载超时；请先重新读取快照，勿自动重复提交');
  }
  async call(name, args, signal) {
    const text = value => ({ content: [{ type: 'text', text: typeof value === 'string' ? value : JSON.stringify(value) }] });
    if (!MACOS_PANEL_TOOLS.some(tool => tool.name === name)) throw new Error('此工具不受 macOS 内置面板支持，请使用 tools/list 中列出的工具');
    if (name === 'browser_close' || (name === 'browser_tabs' && args.action === 'close')) {
      await this.command({ action: 'close' }, signal); this.used = false; return text('内置浏览器面板已关闭');
    }
    if (name === 'browser_tabs') {
      if (!['list','select'].includes(args.action) || (args.index !== undefined && args.index !== 0)) throw new Error('内置面板仅支持当前页；请用 browser_navigate 换站点');
      if (!this.used && args.action === 'list') return text('本连接尚未打开面板');
    }
    if (name === 'browser_navigate') {
      const url = new URL(args.url); if (!['http:','https:'].includes(url.protocol)) throw new Error('仅支持 http/https URL');
      const state = await this.command({ action: 'state' }, signal);
      const previous = state.url ? await this.evaluate('info', {}, signal) : null;
      await this.command({ action: 'navigate', url: url.href }, signal);
      this.used = true;
      // Same-URL fragment navigations may preserve the document.
      await this.readyPage(signal, !previous || previous.url.split('#')[0] === url.href.split('#')[0] ? null : previous.documentId);
      return text(await this.snapshot(signal));
    }
    await this.command({ action: 'ensure' }, signal); this.used = true;
    if (name === 'browser_take_screenshot') return { content: [{ type: 'image', mimeType: 'image/png', data: await this.command({ action: 'screenshot' }, signal) }] };
    if (name === 'browser_navigate_back') { await this.command({ action: 'back' }, signal); await pause(150, signal); await this.readyPage(signal); return text(await this.snapshot(signal)); }
    if (name === 'browser_snapshot') { await this.readyPage(signal); return text(await this.snapshot(signal)); }
    if (name === 'browser_tabs') return text(await this.evaluate('info', {}, signal));
    if (name === 'browser_wait_for') {
      if (args.time !== undefined && (!Number.isFinite(args.time) || args.time < 0 || args.time > 30)) throw new Error('等待时间必须在0到30秒之间');
      if (args.time !== undefined) await pause(args.time * 1000, signal);
      const deadline = Date.now() + 15000;
      do {
        const content = await this.evaluate('text', {}, signal);
        if ((!args.text || content.includes(args.text)) && (!args.textGone || !content.includes(args.textGone))) return text(await this.snapshot(signal));
        await pause(100, signal);
      } while (Date.now() < deadline);
      throw new Error('等待页面文本超时');
    }
    const actions = { browser_click: 'click', browser_type: 'type', browser_fill_form: 'fill_form', browser_select_option: 'select',
      browser_hover: 'hover', browser_press_key: 'press', browser_evaluate: 'evaluate', browser_scroll: 'scroll' };
    const result = await this.evaluate(actions[name], args, signal);
    if (name === 'browser_evaluate') return text(result);
    await pause(100, signal); await this.readyPage(signal);
    return text(await this.snapshot(signal));
  }
  receive(message) {
    if (this.closed) return;
    if (!message || message.jsonrpc !== '2.0' || typeof message.method !== 'string') {
      this.write({ jsonrpc: '2.0', id: message?.id ?? null, error: { code: -32600, message: 'JSON-RPC 请求无效' } }); return;
    }
    if (message.id === undefined) {
      if (message.method === 'notifications/initialized' && this.initialized) this.ready = true;
      if (message.method === 'notifications/cancelled') this.requests.get(message.params?.requestId)?.abort();
      return;
    }
    const id = message.id;
    if ((typeof id !== 'string' && !Number.isSafeInteger(id)) || this.requests.has(id)) {
      this.write({ jsonrpc: '2.0', id: null, error: { code: -32600, message: '请求ID无效或重复' } }); return;
    }
    const controller = new AbortController(); this.requests.set(id, controller);
    this.queue = this.queue.then(async () => {
      try {
        controller.signal.throwIfAborted();
        let result;
        if (message.method === 'initialize' && !this.initialized) {
          this.initialized = true;
          const offered = message.params?.protocolVersion;
          const protocolVersion = ['2024-11-05','2025-03-26','2025-06-18','2025-11-25'].includes(offered) ? offered : '2024-11-05';
          result = { protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'crewvela-wkwebview', version: '1.0.0' },
            instructions: '操作可见的CrewVela单页WKWebView面板。快照是DOM文本与控件，使用最新ref；DOM合成事件不等于系统输入。跨源iframe、文件上传、原生对话框和网络调试不支持。操作报错时先读取快照，不可盲目重试提交。' };
        } else if (message.method === 'ping') result = {};
        else if (!this.ready) throw new Error('请先完成 initialize/initialized 握手');
        else if (message.method === 'tools/list') result = { tools: MACOS_PANEL_TOOLS };
        else if (message.method === 'tools/call') {
          try { result = await this.call(message.params?.name, message.params?.arguments || {}, controller.signal); }
          catch (error) { result = { isError: true, content: [{ type: 'text', text: error.message }] }; }
        } else throw new Error('不支持此 MCP 方法');
        controller.signal.throwIfAborted();
        if (!this.closed) this.write({ jsonrpc: '2.0', id, result });
      } catch (error) {
        if (!this.closed) this.write({ jsonrpc: '2.0', id, error: { code: controller.signal.aborted ? -32800 : -32600, message: error.message } });
      } finally { this.requests.delete(id); }
    });
    return this.queue;
  }
  close() { this.closed = true; for (const controller of this.requests.values()) controller.abort(); }
}

function main() {
  const configFile = process.env.CREWVELA_BROWSER_MACOS_CONFIG || fileURLToPath(new URL('./panel-macos.json', import.meta.url));
  const bridge = new MacOSPanelBridge({ command: (command, signal) => nativeCommand(configFile, command, signal),
    write: message => process.stdout.write(`${JSON.stringify(message)}\n`) });
  let buffer = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => {
    buffer += chunk;
    if (Buffer.byteLength(buffer) > 1024 * 1024) { console.error('MCP 输入超过1MiB限制'); bridge.close(); process.stdin.destroy(); process.exitCode = 1; return; }
    let newline;
    while ((newline = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1); if (!line.trim()) continue;
      try { bridge.receive(JSON.parse(line)); }
      catch { process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'JSON 请求无效' } })}\n`); }
    }
  });
  process.stdin.on('end', () => bridge.close());
  for (const signal of ['SIGTERM','SIGINT']) process.on(signal, () => { bridge.close(); process.stdin.destroy(); });
}
function isEntryPoint() {
  try { return !!process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url); }
  catch { return false; }
}
if (isEntryPoint()) main();
