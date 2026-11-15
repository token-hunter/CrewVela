# macOS 浏览器面板控制

macOS 14+ 可开启「允许 Agent 操作浏览器」和「优先内置浏览器面板」，保存后重启 Agent 会话。浏览器工具发现不会打开面板；实际导航或操作时自动激活同一个右侧面板。macOS 原生面板需要 Node.js 22+ 运行 MCP 适配器，不需要安装额外 Chromium 或配置屏幕录制权限。外部 Chrome 配置在关闭面板优先后生效。

## 实现边界

- macOS 使用独立 WKWebView 控制服务，Windows 原来的 `browser-control-bridge.mjs`、WebView2/CDP、外部兜底保持原链路。
- WKWebView 持久数据存储标识由配置目录派生，面板与应用主页面隔离。首次切换到专属存储后需重新登录网站；不迁移主页面 Cookie。
- Node 适配器只请求 loopback 服务。随机令牌写入权限 0600 的配置；拒绝网页 Origin/Referer、非本机 Host、重复长度、分块和超限请求。命令只能作用于 `browser-panel`，不能指定主应用 WebView。每次命令重新检查开关，旧会话在禁用后立即失效；首次启动隔离模式仍禁止真实浏览器接管。
- WebKit 调用分派到主线程，HTTP 工作线程等待有界回调；截图采用 `takeSnapshotWithConfiguration`，不截整个桌面。
- 旧 macOS Playwright 直连 MCP 条目在启动或读取设置时迁移到原生适配器；已有会话需重启才能重新加载 MCP。脚本资产缺失或过期会恢复。
- 显隐由 BrowserPanel 处理，设置弹窗打开时不让原生视图覆盖弹窗。关闭工具通知前端清理创建状态，后续手动导航或 Agent 导航可以重新创建。

## 支持与限制

支持：导航、DOM 文本与控件快照/ref、点击、输入、批量填写表单、选择下拉、DOM 悬停、Enter/Tab/Shift+Tab/Escape、等待文本、后退、同步 evaluate、当前可见区域 PNG、单页面板查询/关闭。

DOM 快照不等价于完整系统无障碍树。开放 Shadow DOM、同源 iframe 可访问；跨源 iframe 不可访问。DOM 合成事件不能冒充可信硬件事件；原生文件上传、对话框、下载管理、网络请求抓取、异步 evaluate、多标签和全页截图不在本适配器工具列表中。错误不会自动重放到外部浏览器；如需外部模式，关闭面板优先并重启会话。Windows 功能范围保持原样。

## 验证

- `npm test`：含 Windows 原桥接回归、新 macOS MCP 握手/取消/禁用/等待语义、前端生命周期。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-default-features browser_`：含原面板及 CDP 探活、请求认证和资产迁移。
- Vite 在 1432 端口运行时，`node tests/macosCore.ui.mjs`：Windows/macOS 平台模拟及开关交互。
- `cargo build --locked --manifest-path src-tauri/Cargo.toml --example macos_browser_panel_acceptance` 后运行 `node tests/browserControlMacos.native.mjs`：独立原生窗口复用生产面板与服务，真实 stdio MCP 驱动临时本地页面，检查点击、Shadow DOM、输入、延迟链接/提交、后退、PNG、旧 ref、关闭重开、禁用撤销与来源拒绝。

原生宿主只使用新建空临时目录，不加载真实账户或聊天数据。验收截图写入 `output/macos-browser-panel-agent/native-panel.png`。该验收不需要真实模型请求，且不代表 Windows 设备、所有复杂网站或每种 Agent 客户端均完成实机验证。
