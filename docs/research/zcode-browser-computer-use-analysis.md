# ZCode「浏览器控制 / 电脑控制」功能完整实现架构分析

> 调研日期：2026-10-05
> 调研问题：ZCode desktop 的 browser-use（浏览器控制）与 computer-use（电脑控制）两个功能的端到端实现架构是什么？能否/如何移植到 CrewVela（Tauri + ACP 接入 claude/codex/kimi CLI）？
> 方法：只采用一手来源——ZCode 源码（REFERENCE_ROOT/ZCode，含源内中文注释）+ 本机实际安装的插件实体（%USERPROFILE%\.zcode\cli\plugins\cache\zcode-plugins-official\...）。所有结论标注来源路径；插件缓存路径下简写 `CACHE = %USERPROFILE%\.zcode\cli\plugins\cache\zcode-plugins-official`。

---

## 1. 结论速览（TL;DR）

1. **两个功能都不是独立 MCP server**，而是共用一个 ZCode 专有的 `node_repl` MCP 宿主（单工具 `js`，"fresh Node kernel per call"）。插件本体只携带 **skills（提示词工程）+ 模型侧 SDK 门面脚本 + 文档**；真正执行浏览器/桌面动作的 runtime 全部在 ZCode 核心里（Electron 桌面端 + CLI bootstrap）。
2. **架构是四层**：skill 文本（教模型怎么写 JS）→ `mcp__node_repl__js`（每次调用起一个一次性 `node:worker_threads` Worker）→ Worker globals 上以 `Symbol.for("zcode.node-repl.*-bridge")` 注入的 bridge → 本地 socket broker（named pipe / unix socket，带 token 鉴权）→ ZCode 核心的 `BrowserControlPort` / CUA 权限 broker → Electron WebContentsView（IAB）或闭源 CUA Helper。
3. **browser-use 的执行端在开源仓库里**：IAB = Electron `<webview>` guest webContents，通过 `webContents.debugger`（CDP）+ **playwright-core 1.59.1 的 injected script**（ARIA snapshot）驱动；CLI `--browser-use=headless` 时用 playwright-core 直接 `chromium.launch()` 复用**系统已装的 Chrome/Edge/Chromium**（不强制下载）。
4. **computer-use 的执行端不在开源仓库里**：`packages/zcode-cua` 是 Apache-2.0 的**占位包**（"every runtime surface reports unavailable and fails closed"，见 NOTICE.md 与 package.json description）。真实的 AX 树读取、截屏、输入注入由**闭源 Helper**（Windows: `resources/tools/cua-helper` 下 entry + koffi 原生 addon，经 `ELECTRON_RUN_AS_NODE=1` fork；macOS: 独立 app bundle 需 TCC Accessibility/Screen Recording 授权）提供，只随 ZCode 桌面发行版分发。
5. **耦合判定**：skills/SDK 门面脚本是"MIT 声明的插件资产"，可复制可改写；但它们只有配合 `node_repl` 宿主 + bridge 注入 + broker 才能跑——移植等于要重写宿主与 bridge 的全部基础设施。Computer Use 则**没有可移植的实现**（核心 runtime 闭源且 fail-closed）。
6. **无云依赖**：两条链路都不调额外云上多模态模型/API key；截图以 MCP image content block 回给会话主模型。资源上 IAB 复用桌面端自带 Chromium（无额外下载），computer-use 插件缓存约 22MB（主要是 koffi FFI + sharp/libvips 原生二进制）。

---

## 2. 组件清单（谁在哪里、各自职责）

### 2.1 插件实体（实际安装、实际运行的东西）

| 路径 | 内容 | 职责 |
|---|---|---|
| `CACHE\browser-use\0.5.1\.zcode-plugin\plugin.json` | `{"name":"browser-use","version":"0.5.1","author":{"name":"Z.ai"},"license":"MIT","skills":"skills"}` | 插件清单：**只有 skills，没有声明 MCP server** |
| `CACHE\browser-use\0.5.1\skills\control-browser\SKILL.md` | 182 行英文技能 | 教模型：每次 `js` 调用先 bootstrap（从 `process.env.ZCODE_PLUGIN_ROOT` 拼 `file://` URL import `scripts/browser-client.mjs`）、后端选择（iab/extension/cdp）、tab 恢复协议、domSnapshot 优先/screenshot 慎用等 |
| `CACHE\browser-use\0.5.1\skills\web-gui-tester\SKILL.md` | GUI 黑盒测试方法论 | 纯提示词，不含代码；明确"以会话里实际存在的浏览器工具为准" |
| `CACHE\browser-use\0.5.1\scripts\browser-client.mjs` | 2235 行 esbuild bundle | 模型侧 SDK 门面：`setupBrowserRuntime({globals})` → 读 `globals[Symbol.for("zcode.node-repl.browser-control-bridge")]`，把 bridge 包装成 `agent.browsers.* / tab.playwright.* / tab.cua / tab.dom_cua` 对象模型（注释注明与 Codex IAB 对象模型兼容） |
| `CACHE\browser-use\0.5.1\docs\*.md + api.json + documents.json` | API 指南 | 供运行时 `agent.documentation.get(name)` 按需读取 |
| `CACHE\computer-use\0.6.3\.zcode-plugin\plugin.json` | `{"name":"computer-use","version":"0.6.3","license":"MIT","skills":"skills"}` | 同上，纯 skills 插件 |
| `CACHE\computer-use\0.6.3\skills\computer-use\SKILL.md` | 307 行 | bootstrap（`ZCODE_CUA_PLUGIN_ROOT ?? ZCODE_PLUGIN_ROOT ?? CLAUDE_PLUGIN_ROOT`）+ "accessibility first" 原则 + Target/App API + 错误码语义 |
| `CACHE\computer-use\0.6.3\scripts\computer-use-client.mjs` | 1208 行 | 模型侧 SDK 门面。文件头自述："与 Codex 的 `cua` 逐字同构（去掉 browser 半边），逆向基线：@oai/cua@0.2.4"；经 `globals[Symbol.for("zcode.node-repl.computer-use-bridge")].call(method, args)` 发 MCP-envelope 形态的调用 |
| `CACHE\computer-use\0.6.3\node_modules\{koffi,sharp/@img,detect-libc,semver}` | ~22MB 原生二进制 | 属于 CUA 插件自身的原生闭包（见其 `scripts/sync-cache.mjs` 注释："CUA screenshot/zoom 的 sharp native closure 属于 CUA plugin 自身"），供闭源 Helper/运行时用 |
| `CACHE\node-repl-host\0.6.0\dist\mcp\server.js` | 4.9MB bundle | **共享 node_repl MCP server**（下节详述）。`.zcode-plugin\plugin.json` 自述："Not user-facing: it carries no skill and appears in no marketplace listing" |

`.zcode-plugin-seed.json` 记录 hash/marketplace/版本，是 ZCode 从桌面发行版（`source: "filesystem"`）seed 出来的缓存。

### 2.2 ZCode 源码侧（REFERENCE_ROOT/ZCode，pnpm workspace，主程序 Apache-2.0）

| 路径（相对 REFERENCE_ROOT/ZCode） | 职责 |
|---|---|
| `apps/zcode-cli/packages/bootstrap/src/app/official-plugin-definitions.ts` | 官方插件注册表：browser-use `defaultEnabled: true`；**computer-use 明确不设 defaultEnabled**（注释："产品决策：电脑控制回退为默认关闭…默认开启意味着每个新用户首启即注入整套工具集并拉起 Helper"）；两者都声明 `hostMcpServerNames: ["node_repl"]`；node-repl-host 是独立 always-on seed 单元（"它不是面向用户的插件…唯一职责是携带 dist/mcp/server.js"） |
| `apps/zcode-cli/packages/bootstrap/src/app/built-in-node-repl.ts` | 注册判据："browser-use 或 computer-use 任一启用"才注册 `node_repl` MCP；env 注入 `ZCODE_PLUGIN_ROOT`（browser-use root）与 `ZCODE_CUA_PLUGIN_ROOT`（CUA root）；`timeoutMs: 600_000` |
| `apps/zcode-cli/packages/node-repl-host/src/*.ts` | node_repl MCP 宿主源码（server/browser-bridge/cua-bridge/cua-broker/result/tool-contract） |
| `apps/zcode-cli/packages/bootstrap/src/app/node-repl-browser-broker.ts` | 浏览器 broker：本地 net server（Win: `\\.\pipe\zcode-node-repl-<uuid>`；Unix: `$TMPDIR/znr-<uuid>.sock`），32 字节随机 token + `timingSafeEqual` 鉴权，请求上限 1MiB、响应上限 32MiB，把 `list/execute` 转发给 `BrowserControlPort`；**subagent 一律拒绝** |
| `apps/zcode-cli/packages/bootstrap/src/zcode-protocol/browser-control-broker.ts` | 桌面会话下 `BrowserControlPort` 的实现：把 `BrowserCommand` 变成 **ZCode Protocol 反向请求** `interaction/browserExecute` / `interaction/browserList` 发给桌面 host |
| `apps/zcode-cli/packages/adapters/src/browser/*.ts` | CLI headless 后端：`createManagedCdpBrowserRuntime` 用 playwright-core `chromium.launch()`；`executable.ts` 依次找 playwright 自带 chromium → 系统 Chrome/Chromium/Edge 路径，找不到报 "No installed Chrome or Chromium executable was found. Install Chromium or pass --browser-executable <absolute-path>." |
| `apps/zcode-cli/packages/core/src/repl/node-repl-session.ts` | JS kernel：`node:vm` context + async IIFE 顶层 await；`nodeRepl.write/emitImage/setResponseMeta/emitStructuredResult` 收集到 `NodeReplRunResult`；注释明说"vm 不是安全沙箱" |
| `packages/desktop/src/main/browserView/*`（29 个文件） | IAB 执行端：guest webContents 注册表、CDP 命令执行器、Playwright locator 执行器、DOM snapshot（注入 playwright injectedScript 到 isolated world `zcode-playwright-dom-snapshot`）、截图、WebM 录屏、虚拟剪贴板、tab 驻留/恢复 |
| `packages/desktop/src/main/browserView/playwrightInjectedScriptSource.ts` | 从 `playwright-core/lib/generated/injectedScriptSource.js` 提取注入脚本（注释："Playwright 1.59 的 generated injected script 未作为 public export 暴露…DOM snapshot 使用 Apache-2.0 许可的 Playwright runtime"），调用 `incrementalAriaSnapshot(root, { mode: "ai" })` |
| `packages/services/src/cua-permission-broker/*` | CUA 宿主编排：Windows Helper fork（`ELECTRON_RUN_AS_NODE=1` + `ZCODE_CUA_HELPER_ADDON` env + `--socket`/`--parent-pid` argv）、权限状态服务、PiP（画中画）会话。`index.ts` 注释：broker server（51 个文件）**已合并进闭源 `@zcode/zcode-cua`** |
| `packages/zcode-cua/*` | **占位包**。package.json description："API-compatible placeholder package for Computer Use. This build ships without Computer Use: every runtime surface reports unavailable and fails closed."，license **Apache-2.0** |
| `packages/ui/src/settings/{ComputerUseSection,BrowserSettingsSection}.tsx` | 设置页开关（见 §4） |
| `packages/ui/src/settings/computerUseAvailability.ts` | 平台判定：仅 `local-macos` / `local-windows` supported；remote（ssh/wsl/docker/server）、web、`local-linux` 均 unsupported |

---

## 3. 端到端链路（问题 a）

### 3.1 从开关到能力可用

```
设置页开关（UI）
  └─ setPluginEnabled("computer-use@zcode-plugins-official") / ("browser-use@zcode-plugins-official")
       └─ 启用/禁用官方插件（连同其 skills 注入与 node_repl MCP 注册）
            ├─ resolveBuiltInNodeReplMcpServers()：任一启用 → 注册 stdio MCP server "node_repl"
            │    command = process.execPath（桌面打包态 = Electron Helper，ELECTRON_RUN_AS_NODE=1）
            │    args = [<zcode.cjs>, __zcode-plugin-host, <node-repl-host root>/dist/mcp/server.js]
            │    env = ZCODE_PLUGIN_ROOT / ZCODE_CUA_PLUGIN_ROOT
            │         + ZCODE_NODE_REPL_BROWSER_BROKER_SOCKET/TOKEN（browser broker 凭据）
            │         + ZCODE_CUA_PERMISSION_BROKER_SOCKET / _REFRESH_MARKER（CUA broker 凭据，
            │           由 mcp-config.ts 从"进程内捕获快照"恢复，防 CLI env 清理丢失）
            └─ skills/<name>/SKILL.md 进入会话技能列表（模型可发现/可加载）
```

来源：`apps/zcode-cli/packages/bootstrap/src/app/built-in-node-repl.ts`、`.../official-plugin-runtime.ts`（`createBundledMcpRuntimeConfig`）、`.../mcp-config.ts`（`injectCuaCredentialsIntoNodeRepl`）、`.../app/create-app.ts`（`createNodeReplBrowserBroker` + `injectNodeReplBrowserBroker`，L262–281）。

### 3.2 一次浏览器/桌面动作的完整路径

```
模型读 skill → 发起 MCP tools/call "js" {code, title, timeout_ms?}
  └─ node_repl host（server.ts）
       ├─ 会话串行化（sessionKey 排队；无 session_id 落 "__unscoped__"）
       ├─ AbortSignal.any([MCP 请求 signal, 自身 controller, AbortSignal.timeout(默认60s, 上限120s)])
       └─ executeJsInWorker：每次调用 new 一个一次性 Worker（连 Node 模块缓存一起销毁）
            └─ Worker 内 NodeReplSession（node:vm context）
                 ├─ injectedGlobals：Symbol.for("zcode.node-repl.browser-control-bridge")
                 │                    + Symbol.for("zcode.node-repl.computer-use-bridge")
                 └─ 模型代码 import(file:///<pluginRoot>/scripts/xxx-client.mjs) → setup(globals)
                      ├─ Browser: bridge.list/execute → 本地 socket → browser broker
                      │    → BrowserControlPort →（桌面）ZCode Protocol 反向请求 interaction/browserExecute
                      │    → Electron main 的 browserGuestManager → guest webContents（CDP）
                      │    或（CLI --browser-use=headless）→ playwright-core chromium
                      └─ CUA: bridge.call(method, input) → 本地 socket → CUA broker
                           → createComputerUseRuntime（闭源）→ Helper 进程（原生 AX/截屏/输入）
            └─ 结果 toMcpRunResult()：nodeRepl.write 的文本 + emitImage 的图片
                 （base64 → MCP image content block；CUA 结构化观察自带截图，
                  "一个结果只允许一张最终 raster"）
```

关键源码：`node-repl-host/src/server.ts`（`executeJsInWorker` L253-288；`captureComputerUseRuntimeFromEnvironment` L372-381：仅凭 `ZCODE_CUA_PERMISSION_BROKER_SOCKET` env 决定 CUA 可用性）、`browser-bridge.ts`（`sendBrokerRequest`）、`cua-bridge.ts`、`result.ts`。

### 3.3 node_repl 宿主细节（问题 3 的核心）

- **标准 MCP**：`@modelcontextprotocol/server` 2.0.0，stdio，`serveStdio`，protocolVersion "2026-07-28"，server name `node_repl`，version 0.6.0。**但工具面与 bridge 协议是 ZCode 专有**——`js` 工具描述第一句就是 "Browser Use and Computer Use only"，且 fresh-kernel + bridge 注入的设计没有任何公开规范。
- **超时**：`NODE_REPL_DEFAULT_TIMEOUT_MS = 60_000`（tool-contract.ts）；入参 `timeout_ms` 上限 `MAX_SYNC_TIMEOUT_MS = 120_000`；MCP server 配置层 `timeoutMs: 600_000`。工具描述要求模型预估总时长 >30s 时显式传 `timeout_ms`（>45s 时加 15s 余量），超了就拆多次调用。
- **emitImage 回传**：`nodeRepl.emitImage(imageLike)` 把图片收进 `NodeReplRunResult.images`（`{base64, mimeType}`），`toMcpRunResult` 转成 MCP `content` 里的 `{type:"image", data, mimeType}` block，且刻意把 image 排在 text 之前（result.ts 注释：实测 `[text,image]` 模型看不见图）。
- **与核心的耦合**：宿主本身依赖 `@zcode/core/repl`（NodeReplSession）、`@zcode/contracts`（JsInputJsonSchema）、`@zcode/shared`（broker schema）、`@zcode/zcode-cua`（CUA runtime 工厂）。**它不是通用 MCP**——没有 bridge 环境变量（broker socket/token 由 ZCode 宿主进程铸造并注入）时，SDK 门面第一步就抛 "Browser runtime bridge is unavailable. Use Browser from a ZCode desktop or shared-host session."

---

## 4. 设置开关到底做了什么（问题 4）

**电脑控制**（`packages/ui/src/settings/ComputerUseSection.tsx`）：

- 文案（`packages/ui/src/i18n/locales/zh-CN.ts` L6475-6476）：`"settings.computerUse.toggleLabel": "启用电脑控制"`、`"settings.computerUse.toggleDescription": "开启后将启用电脑控制及其 MCP 与技能。"`
- 实现（组件头注释）："顶部一个总开关：开/关 zcode-cua 插件（连带其 MCP server 与 skill 一起启用/禁用）"——`onTogglePlugin` → `setPluginEnabled(ZCODE_CUA_OFFICIAL_PLUGIN_ID, next, ...)`。**没有下载二进制**：Helper 是懒启动（official-plugin-definitions.ts 注释："Helper 由 SDK 首次调用时才拉起"）。
- macOS 额外渲染 Accessibility / Screen Recording 两行 TCC 权限状态 + 授权引导（`openCuaPermissionOnboarding`）+ Helper 重启/自愈链；macOS 版本地板 12.0（`packages/desktop/src/main/cuaOsSupport.ts`：`CUA_MINIMUM_MACOS_VERSION = "12.0"`，低于则 Helper 被 LaunchServices -10825 拒启）。**Windows 只复用插件总开关**（注释："Windows 只复用插件总开关；macOS 才具备 TCC 权限…"）。
- 另有一个"输入框常驻入口显隐"开关，只管 UI 按钮渲染，不影响能力。

**浏览器控制**（`packages/ui/src/settings/BrowserSettingsSection.tsx`）：

- 文案：`"settings.browser.control.title": "开启内置浏览器控制"`、描述："启用 Browser Use 官方插件，让新会话可以通过内置浏览器访问和操作网页。"
- 实现：同样只是 `setPluginEnabled("browser-use@zcode-plugins-official", next)`。分区里其余是 Chrome 数据导入/清理（`importChromeBrowserData` / `clearEmbeddedBrowserData`）与"忽略证书校验"设置——服务于 IAB 的登录态复用，与能力开关无关。
- browser-use `defaultEnabled: true`（新用户默认开）；computer-use 默认关（见 §2.2 注释）。

---

## 5. 底层驱动（问题 b/c）

### 5.1 Browser Use

- **后端模型**：`iab`（桌面内置浏览器）、`cdp`（CLI `--browser-use=headless` 显式启动的托管 Chromium）、`extension`（声明面存在，`unsupportedByDefaultIn: ["iab","cdp"]` 等 marker 表明按运行时广告决定；ZCode 桌面当前广告 iab，CLI headless 广告 cdp）。**Playwright 不是后端，是 Tab 的 API surface**（docs/overview.md 原文："The registry understands backend types `iab`, `extension`, and `cdp`. Playwright is a `Tab` API surface, not a backend."）。
- **IAB**：Electron `<webview>` 的 guest `webContents`（`packages/desktop/src/main/browserView/browserGuestManager.ts` 直接 import `electron` 的 `webContents`/`WebFrameMain`）。命令执行走 `webContents.debugger`（CDP）：`Runtime.evaluate` 注入 playwright-core 的 injectedScript 到 isolated world，`incrementalAriaSnapshot(root, { mode: "ai" })` 产出 ARIA 树；locator 动作由 `browserPlaywrightLocatorExecutor.ts` 在 CDP 上自实现（等价 Playwright locator 语义的子集：click/fill/press/selectOption/check/downloadMedia 等）。截图用 `capturePage()`，录屏走 `electronBrowserWebmRecorder.ts`。
- **headless CDP**：`apps/zcode-cli/packages/adapters/src/browser/index.ts` 的 `ManagedCdpBrowserControlPort` 持有一个 playwright-core `Browser` 实例，按 MCP sessionId 建 `BrowserContext` 隔离。可执行文件解析（`executable.ts`）：显式 `--browser-executable` → playwright 自带 chromium 路径 → 系统安装的 Chrome/Chromium/Edge（Win 下扫 PROGRAMFILES/LOCALAPPDATA）→ 都没有则报错。**不自动下载浏览器**；SEA 打包态则把 playwright-core 内嵌为资产并解包到缓存（`apps/zcode-cli/packages/cli/src/sea-playwright-runtime.ts`，带 sha256 清单校验）。
- **跨调用持久性**：tab 的真身由宿主（BrowserControlPort 侧）持有，JS 绑定每次调用都重建，skill 里大篇幅的 "tab 恢复协议"（先 `tabs.list()` 交模型验看、下一 cell 再 `tabs.get(id)`）就是为绕开 fresh-kernel 设计的。

### 5.2 Computer Use

- **API 形态**：与 OpenAI Codex 的 `@oai/cua`（内部名 tinysky）逐字对齐（computer-use-client.mjs 文件头注释："模型可见面与 Codex 的 cua 逐字同构…逆向基线：@oai/cua@0.2.4"），工具面 14 个方法（list_apps/list_windows/get_app_state/left_click/scroll/left_click_drag/type/set_value/select_text/key/perform_action/paste/request_access/stop_computer_control）。
- **实现分层**：开源仓库只有协议壳（broker 错误码、socket path 约定、PiP 会话契约、权限服务接口）。51 个文件的 broker server + 原生自动化后端在闭源 `@zcode/zcode-cua` 里（`packages/services/src/cua-permission-broker/index.ts` 注释）。
- **Helper（真实执行者）**：
  - Windows：`resources/tools/cua-helper`（`windowsCuaDevRuntime.ts`：`PRODUCT_RUNTIME_SEGMENTS = ["tools","cua-helper"]`，`runtime-manifest.json` + sha256 完整性校验），内容为 entry 脚本 + 原生 addon。宿主以 `childProcess.fork(command, [entry, "--socket", <path>, "--parent-pid", <pid>], { env: { ZCODE_CUA_HELPER_ADDON: <addonPath>, ELECTRON_RUN_AS_NODE: "1" } })` 拉起（windowsCuaDevHelperHost.ts L303-326），Helper 在 socket 上监听并回 `ready` 控制消息。
  - 原生能力推断（基于插件缓存依赖 + desktop staging 脚本）：**koffi 2.15.6**（FFI，`build/koffi/win32_x64/koffi.node`，用于调 Windows UI Automation / Win32 输入 API）、**sharp + @img/sharp-win32-x64**（libvips，截图缩放/处理）。`packages/desktop/scripts/koffi-package-assets.mjs` 注释："The CUA server externalizes koffi because esbuild cannot bundle its platform-dispatching .node requires."
  - macOS：独立 Helper app bundle（`helperAppBundle`，Info.plist LSMinimumSystemVersion 12.0），需 TCC Accessibility + Screen Recording。
- **平台支持**：`computerUseAvailability.ts`——仅本地 macOS/Windows 桌面支持；Linux 与所有 remote（ssh/wsl/docker/server）不支持。
- **安全语义**（skill 与 SDK 大量着墨）：state_id 强校验、frame 精确栅格绑定（坐标必须来自最新 raster）、`actionSent`（possibly_sent）防重放、controller lease（`CONTROLLER_BUSY`：另一会话独占输入时拒绝）、kill switch、权限门（`request_access` / `PERMISSION_DENIED` / `SCREEN_LOCKED`）。观察默认等待 UI 稳定；AX 树按 diff 返回。

---

## 6. 与 ZCode 核心的耦合点（问题 d）

**拿来就能用 / 改动小**：
- 两个插件的 SKILL.md、docs、`.zcode-plugin/plugin.json`——纯文本资产，MIT 声明，可复制改写（它们引用的环境变量名和 bootstrap 协议是唯一"协议"）。
- browser-use 插件依赖里唯一的运行时外部件是 `playwright-core`（Apache-2.0，npm 可得）；headless CDP 后端的实现（`adapters/src/browser/`，约 1.6k 行）相对自包含，可参考甚至移植。
- `NodeReplSession`（core/src/repl，909 行）+ node_repl host（395 行 server + bridges）逻辑清晰，若 CrewVela 想复刻"JS 门面"模式，代码可读性极好（中文注释详尽）。

**绑死在 ZCode 上**：
1. **bridge 注入协议**：SDK 门面靠 `globals[Symbol.for("zcode.node-repl.browser-control-bridge")]` / `computer-use-bridge` 取 transport；这个 symbol 只存在于 ZCode 宿主构造的 Worker globals 里。脱离 ZCode 直接 import 这两个 client 脚本 → 立即抛 "bridge is unavailable. Use ... from a ZCode desktop or shared-host session."（两个 client 文件末尾均有此硬检查）。
2. **broker 凭据链**：`ZCODE_NODE_REPL_BROWSER_BROKER_SOCKET/TOKEN`、`ZCODE_CUA_PERMISSION_BROKER_SOCKET/REFRESH_MARKER` 都由 ZCode 宿主进程铸造（`randomBytes(32)` token + 进程内捕获快照），并做 `timingSafeEqual` 鉴权与 `runtime_scope === "subagent"` 拒绝。这是一整套进程间信任设计，不是环境变量巧合。
3. **ZCode Protocol**：桌面会话里 BrowserCommand 经 `interaction/browserExecute` 反向 JSON-RPC 回宿主——依赖 ZCode 的 agent↔host 协议栈（`packages/rpc`、`zcode-protocol`）。
4. **IAB 本体**：Electron `<webview>` + `webContents.debugger` + 桌面窗口管理（tab 驻留、截图面板、透明窗口 bootstrap、录屏）。CrewVela 是 Tauri（WebView2/WKWebView），**没有等价的 CDP attach 能力**，IAB 模式无法平移；能平移的只有 headless/外部浏览器 CDP 模式。
5. **Computer Use runtime 完全闭源**：开源侧 fail-closed。没有 Helper 二进制就没有任何截屏/AX/输入能力；而 Helper 只随官方桌面发行版分发（Windows 在 Electron resources 里做 manifest+sha256 校验，macOS 有代码签名/TeamID 校验逻辑骨架）。
6. **账号体系**：CUA broker 凭据与插件 authority（`ZCODE_CUA_PLUGIN_AUTHORITY_ENV_KEY`）由桌面 host 下发，`resolveTrustedOfficialCuaServerNames` 用"进程内插件注册表"防伪造——非 ZCode 宿主无法提供这个 authority。

---

## 7. 许可证结论（问题 e）

| 对象 | 许可证 | 来源 |
|---|---|---|
| ZCode 主仓库源码（含 desktop IAB 实现、node-repl-host、adapters、services） | **Apache-2.0** | `REFERENCE_ROOT/ZCode\LICENSE`（Apache License Version 2.0 全文） |
| `@zcode/zcode-cua`（占位包） | Apache-2.0 | `packages/zcode-cua/package.json` + README |
| browser-use 插件分发实体 | 插件 manifest 声明 **MIT**（author Z.ai）；源码包 `@zcode/browser-use-plugin` package.json 为 Apache-2.0 | `CACHE\browser-use\0.5.1\.zcode-plugin\plugin.json` vs `apps/zcode-cli/packages/browser-use-plugin/package.json` |
| computer-use 插件分发实体 | 同上：manifest MIT / 源码包 Apache-2.0 | 同上两处 |
| node-repl-host 插件实体 | manifest MIT；缓存 package.json 未标 license（私有包） | `CACHE\node-repl-host\0.6.0\.zcode-plugin\plugin.json` |
| playwright-core 1.59.1 | Apache-2.0 | THIRD-PARTY-NOTICES.md L1883 |
| koffi 2.15.6 | MIT | THIRD-PARTY-NOTICES.md L1521 |
| sharp/@img | Apache-2.0（libvips LGPL 组件随行，见 notices） | THIRD-PARTY-NOTICES.md |

**判断**：
- **可以合法复用**（保留版权/许可声明前提下）：SKILL.md 等提示词资产（MIT）；ZCode 源码中任何部分的** Apache-2.0 允许**复制/修改/再许可（含商用、含并入 MIT/私有项目），但需遵守 NOTICE（仓库带 NOTICE.md，Apache-2.0 §4 要求保留）并标注修改；playwright-core/koffi 各自许可证宽松。
- **不能复制**：闭源 CUA Helper 二进制及其 runtime（开源仓库中不存在，也无再分发授权）；ZCode 桌面发行版里 seed 出来的插件缓存若含未开源产物（如真实 `@zcode/zcode-cua` 运行时），不可拷贝使用。
- **注意点**：browser-use 的源码包与分发 manifest 许可证标注不一致（Apache-2.0 vs MIT），保守起见按 **Apache-2.0**（较严一方）对待从源码仓库复制的部分；插件缓存里的 skills 按 MIT 对待是发行方明示意图。
- ZCode 的 API 设计大量"与 Codex `cua` 逐字同构"（这是对 OpenAI 内部包的逆向兼容，不是抄代码——ZCode 自己实现了门面），移植时同样可以借鉴 API 形状而不必复制实现。

---

## 8. 资源占用与账号依赖（问题 f）

- **Chromium 下载体积**：IAB 模式零额外下载（复用桌面端 Electron 自带 Chromium；每个 tab 一个 guest webContents，内存随 tab 数增长，ZCode 专门有 processResourceChromiumSource 等采样模块监控）。headless 模式优先复用**系统已装**的 Chrome/Edge（不下载）；仅当想用 playwright 托管 chromium 时才有 ~130-170MB 的浏览器下载（由 playwright 机制承担，ZCode 自身不触发）。
- **插件体积**（本机实测 du）：browser-use 0.5.1 ≈ **241KB**；computer-use 0.6.3 ≈ **22MB**（koffi .node + libvips dll + sharp-win32-x64.node 占绝大头）；node-repl-host 0.6.0 ≈ 4.9MB（bundled server.js）。
- **进程模型**：常驻多出的只有 node_repl MCP 子进程（Node/Electron-as-Node）；Worker 每调用即焚。CUA Helper 懒启动、按需常驻一个子进程（Windows 下 fork 自 Electron runtime）。
- **账号/API key**：**无额外要求**。两条链路都不调用云上视觉模型——截图/AX 树通过 MCP tool result 回到当前会话的主模型（CrewVela 场景下就是 claude/codex/kimi 本身，模型需具备图像输入能力才能走视觉路径；DOM/AX 路径纯文本，任何模型可用）。Browser Use 的"登录态"来自用户主动导入的 Chrome 数据（可选）。ZCode 桌面账号体系只影响 ZCode 自己（插件 authority、更新、订阅），不属于功能本身的依赖。
- **超时/带宽**：单次 `js` 调用默认 60s、上限 120s；broker 响应上限 32MiB（截图 base64 走本地 socket，不经网络）。

---

## 9. 移植到 CrewVela 的可行性初步事实

> 本节只列硬事实，不做最终决策。

**有利事实**：
1. **提示词资产可直接搬**：两份 SKILL.md + docs 均为 MIT 声明的纯文本，其中"fresh kernel 每次重建绑定""先 list 后 get 的 tab 协议""snapshot 优先 screenshot 慎用""AX 优先坐标兜底"等经验对任何实现都有普适价值。
2. **模型侧门面脚本是自包含 bundle**（browser-client.mjs 2235 行 / computer-use-client.mjs 1208 行），入口仅依赖 `globals` 上的 bridge symbol + `node:*` 内建。若 CrewVela 自建一个 MCP server 注入同名 symbol 的 bridge，**这两个 SDK 理论上可以原样复用**——这就是 ZCode 设计留下的接缝（symbol 是 `Symbol.for` 全局注册表键，不是模块私有）。
3. **Browser Use 的 headless/CDP 路线可完整复刻**：playwright-core 是 Apache-2.0 的 npm 包，`adapters/src/browser/`（ZCode 实现）展示了按 MCP session 建 context、abort 传播、错误分类的完整做法；CrewVela 可在侧车 Node 进程跑一个等价 MCP server（Tauri 侧只需 spawn 它）。
4. **无云依赖、无账号依赖**：对 CrewVela 的 ACP agent（claude/codex/kimi）而言，这只是一个普通 MCP server + skills 注入，只要 ACP 客户端能把 MCP 工具和 skill 文件喂给 agent 即可。
5. **许可证干净**：仓库 Apache-2.0（可合规并入私有/MIT 项目），插件资产 MIT，依赖 playwright-core(Apache-2.0)/koffi(MIT)/sharp(Apache-2.0)。

**不利事实**：
1. **Computer Use 无可移植实现**：开源侧 fail-closed 占位；截屏/AX/输入注入的真身是闭源 Helper，只随 ZCode 桌面版分发且有完整性校验。CrewVela 要做电脑控制只能自研（Windows: UIA + SendInput，可走 koffi 或 Rust 侧 crates；macOS: AX API + TCC 引导——ZCode 的 UI/权限引导代码可参考但平台工程量大）。
2. **IAB 模式不可平移**：ZCode 内置浏览器依赖 Electron webContents + CDP attach；Tauri 的 WebView2/WKWebView 没有对等调试协议。CrewVela 若要"内置浏览器"，要么嵌入独立 Chromium（重），要么只做外部/headless 浏览器（ZCode 的 cdp 后端路线）。
3. **node_repl 不是通用件**：它是"专用于这两个功能的 JS 门面"（工具描述明写 "Browser Use and Computer Use only"），fresh-Worker、bridge 注入、双 broker 凭据、subagent 拒绝、会话串行化都是 ZCode 宿主专有设施。移植 browser-use 门面 = 要在 CrewVela 侧实现等价的宿主 + broker（或简化为直接在 MCP server 进程内实现 BrowserControlPort，砍掉 socket 一跳）。
4. **skill 与门面深度互锁**：SKILL.md 里写死了 `mcp__node_repl__js`、`ZCODE_PLUGIN_ROOT`、bootstrap 代码块、`agent.browsers` 对象模型。换成自己的工具面（例如直接给一组细粒度 MCP 工具）则 skill 文本需要大改；保留 JS 门面则要连 nodeRepl API（write/emitImage/requestMeta）一起复刻。
5. **截图/多模态链路受 ACP 约束**：ZCode 靠 MCP image content block 回传截图；CrewVela 的 ACP 适配器（如 claude-code-acp）能否无损透传图片 block 给各 CLI agent，是移植前必须验证的点（本仓库另一份报告 `claude-code-acp-support.md` 可对照）。
6. **版本耦合**：插件 0.5.1/0.6.3 与 node-repl-host 0.6.0 的工具面（"升到 0.6.0：工具面收敛到只剩 js…"）互相咬合，抽取时需按当前版本快照，别混版本。

---

## 附：关键环境变量速查

| 变量 | 含义 | 注入方 |
|---|---|---|
| `ZCODE_PLUGIN_ROOT` | browser-use 插件根（docs/bootstrap 用） | built-in-node-repl.ts |
| `ZCODE_CUA_PLUGIN_ROOT` | computer-use 插件根 | 同上 |
| `ZCODE_NODE_REPL_BROWSER_BROKER_SOCKET` / `_TOKEN` | 浏览器 broker 的 named pipe/unix socket + 鉴权 token | create-app.ts（node-repl-browser-broker） |
| `ZCODE_CUA_PERMISSION_BROKER_SOCKET` | CUA 权限 broker socket（= `ZCODE_CUA_BROKER_SOCKET_ENV_KEY`，shared/src/runtimeEnv.ts） | 桌面 host → mcp-config.ts 从捕获快照恢复 |
| `ZCODE_CUA_PERMISSION_BROKER_REFRESH_MARKER` | 权限刷新 marker 文件路径 | 同上 |
| `ZCODE_CUA_HELPER_ADDON` | Windows Helper 的原生 addon 路径 | windowsCuaDevHelperHost.ts fork env |
| `ZCODE_CUA_DEV_ROOT` | 开发态覆盖 CUA runtime 根 | windowsCuaDevRuntime.ts |
| `ELECTRON_RUN_AS_NODE=1` | 让 Electron 二进制以纯 Node 跑 MCP server / Helper | official-plugin-runtime.ts / windowsCuaDevHelperHost.ts |
