// Run against Vite: CREWVELA_UI_URL=http://127.0.0.1:1420 node qa-kimi-computer-use.mjs
// 验证电脑控制面板三组开关（内置/kimi-computer-use/open-computer-use）：
// 渲染、保存三命令、状态展示、错误路径、窄屏。
import { chromium } from 'playwright';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const baseUrl = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const errors = [];
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 900, height: 1100 } });
page.setDefaultTimeout(10000);
page.on('pageerror', e => errors.push(e.message));
await page.route('**/__cc-qa', r => r.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><style>body{margin:0;padding:32px;background:#1e1e1e;color:#ddd}#root{max-width:720px;margin:auto}</style></head><body><div id="root"></div><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><script type="module">import '/@vite/client';import React from '/node_modules/.vite/deps/react.js';import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';import {ComputerControlSettingsPanel} from '/src/components/ComputerControlSettingsPanel.tsx';import '/src/styles.css';(window.__qaRoot=ReactDOM.createRoot(document.getElementById('root'))).render(React.createElement(ComputerControlSettingsPanel));</script></body></html>` }));
await page.addInitScript(() => {
  window.__qaState = {
    cc: { enabled: false },
    kcu: { enabled: sessionStorage.getItem('qaKcuEnabled') === '1' },
    ocu: { enabled: sessionStorage.getItem('qaOcuEnabled') === '1' },
    calls: [],
    kcuSaveError: sessionStorage.getItem('qaKcuError'),
  };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      window.__qaState.calls.push({ cmd, args: JSON.parse(JSON.stringify(args)) });
      const s = window.__qaState;
      if (cmd === 'computer_control_get')
        return { settings: s.cc, runtime: 'PowerShell 运行时（C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe）', warning: null };
      if (cmd === 'computer_control_save') {
        s.cc = args.settings;
        return { settings: s.cc, runtime: 'PowerShell 运行时已就绪（C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe）', warning: null };
      }
      if (cmd === 'kimi_computer_use_get')
        return { settings: s.kcu, runtime: s.kcu.enabled ? 'KimiCU 运行时（C:\\Users\\demo\\AppData\\Local\\KimiCU\\kimi-cu.exe）' : '已关闭', warning: null };
      if (cmd === 'kimi_computer_use_save') {
        if (s.kcuSaveError) throw new Error(s.kcuSaveError);
        s.kcu = args.settings;
        return { settings: s.kcu, runtime: 'KimiCU 运行时已就绪（C:\\Users\\demo\\AppData\\Local\\KimiCU\\kimi-cu.exe，stdio MCP）', warning: null };
      }
      if (cmd === 'open_computer_use_get')
        return { settings: s.ocu, runtime: s.ocu.enabled ? 'Open Computer Use 运行时（C:\\npm\\open-computer-use.exe）' : '已关闭', warning: null };
      if (cmd === 'open_computer_use_save') {
        s.ocu = args.settings;
        return { settings: s.ocu, runtime: 'Open Computer Use 运行时已就绪（C:\\npm\\open-computer-use.exe，stdio MCP）', warning: null };
      }
      throw new Error('Unexpected ' + cmd);
    },
  };
});
try {
  await page.goto(new URL('/__cc-qa', baseUrl).href);

  // 1) 三个开关都渲染，初始都关闭，状态区展示三块运行时信息。
  const switches = page.locator('.browser-control-toggle');
  await switches.first().waitFor();
  if ((await switches.count()) !== 3) throw new Error(`应有 3 个开关，实际 ${await switches.count()}`);
  const bodyText = await page.locator('.browser-control-panel').innerText();
  if (!bodyText.includes('启用电脑控制')) throw new Error('缺少内置电脑控制开关文案');
  if (!bodyText.includes('启用 kimi-computer-use')) throw new Error('缺少 kimi-computer-use 开关文案');
  if (!bodyText.includes('启用 open-computer-use')) throw new Error('缺少 open-computer-use 开关文案');
  if (!bodyText.includes('KimiCU 运行时')) throw new Error('状态区缺少 KimiCU 运行时行');
  if (!bodyText.includes('Open Computer Use 运行时')) throw new Error('状态区缺少 Open Computer Use 运行时行');
  console.log('PASS: 三开关渲染且初始关闭');

  // 2) 打开 kimi 与 open 开关，保存一次应同时调用三个 save 命令。
  await switches.nth(1).click();
  await switches.nth(2).click();
  await page.getByRole('button', { name: '保存并应用' }).click();
  await page.waitForFunction(() => document.body.innerText.includes('电脑控制设置已应用'));
  const calls = await page.evaluate(() => window.__qaState.calls);
  for (const cmd of ['computer_control_save', 'kimi_computer_use_save', 'open_computer_use_save']) {
    const call = calls.find(c => c.cmd === cmd);
    if (!call) throw new Error(`保存未调用 ${cmd}`);
    if (cmd !== 'computer_control_save' && call.args.settings.enabled !== true)
      throw new Error(`${cmd} 收到的 enabled 不为 true`);
  }
  const panelText = await page.locator('.browser-control-panel').innerText();
  if (!panelText.includes('KimiCU 运行时已就绪')) throw new Error('保存后未显示 KimiCU 已就绪');
  if (!panelText.includes('Open Computer Use 运行时已就绪')) throw new Error('保存后未显示 Open Computer Use 已就绪');
  console.log('PASS: 单次保存同时应用三方案且状态更新');

  // 3) kimi 保存失败路径：错误可见且明确（开关当前为开，关掉它触发保存失败）。
  await page.evaluate(() => { window.__qaState.kcuSaveError = '未找到 kimi-cu.exe（C:\\Users\\demo\\AppData\\Local\\KimiCU\\kimi-cu.exe），请先安装 KimiCU 后再开启'; });
  await page.locator('.browser-control-toggle').nth(1).click();
  await page.getByRole('button', { name: '保存并应用' }).click();
  await page.waitForFunction(() => document.body.innerText.includes('保存失败'));
  if (!(await page.locator('.browser-control-panel').innerText()).includes('未找到 kimi-cu.exe'))
    throw new Error('保存失败未透出 kimi-cu.exe 缺失错误');
  console.log('PASS: kimi 保存失败路径透出错误');

  // 4) 窄屏无横向溢出 + 截图（重载为 kimi/open 均开启态）。
  await page.evaluate(() => { sessionStorage.setItem('qaKcuEnabled', '1'); sessionStorage.setItem('qaOcuEnabled', '1'); sessionStorage.removeItem('qaKcuError'); });
  await page.reload();
  await page.locator('.browser-control-toggle').nth(1).waitFor();
  await page.waitForFunction(() => document.body.innerText.includes('KimiCU 运行时（'));
  await page.setViewportSize({ width: 560, height: 1100 });
  await page.waitForTimeout(300);
  if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth))
    throw new Error('窄屏横向溢出');
  await page.screenshot({ path: join(tmpdir(), 'cc-three-computer-use-narrow.png'), fullPage: true });
  await page.setViewportSize({ width: 900, height: 1100 });
  await page.screenshot({ path: join(tmpdir(), 'cc-three-computer-use.png'), fullPage: true });
  console.log('PASS: 窄屏无溢出并已截图');

  if (errors.length) throw new Error(errors.join('\n'));
  console.log('PASS: 无页面错误');
} finally {
  await browser.close();
}
