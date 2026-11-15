---
name: browser-control
description: 通过 browser-control MCP 驱动用户本机 Chrome 完成网页打开、阅读、点击、填表、检索与截图。需要访问网页、查资料、操作 Web 界面或验证页面效果时使用。
---

# 浏览器控制（browser-control）

你可以通过 MCP 服务器 `browser-control` 驱动浏览器。**默认在实际调用浏览器工具时自动打开并接管 CrewVela 右侧栏的内置浏览器面板**：导航、点击、输入会实时显示在用户看得到的面板里。普通聊天、MCP 初始化和查询工具列表不会打开浏览器；Windows 面板连接失败时使用外部 Chrome 窗口。Windows 与外部浏览器基于官方 @playwright/mcp，macOS 内置面板使用下面的专用适配器。

### macOS 面板

macOS 面板通过内置 WKWebView MCP 适配器操作，Windows 与外部浏览器继续使用 @playwright/mcp。macOS 支持导航、可见文本与控件 DOM 快照（带 ref）、点击、文本和批量表单填写、下拉选择、后退、等待、同步 JavaScript 与可见区域 PNG 截图。以当前 `tools/list` 为准，不调用未列出的工具。

- macOS 快照是 DOM 文本与控件表示，不是完整系统无障碍树。开放 Shadow DOM 与同源 iframe 可读取；跨源 iframe 不可读取。
- 点击、悬停、按键采用 DOM 合成事件，不保证接受可信硬件事件的网站兼容。按键仅支持 Enter、Tab、Shift+Tab、Escape；文本使用 `browser_type`。
- 原生文件选择器、浏览器原生对话框、下载管理、网络调试和新标签页暂未接入；不得把未执行的操作报告为成功。
- macOS 面板故障会明确报错，不自动转到其他浏览器重复操作；需要外部模式时请用户关闭「优先内置浏览器面板」、保存并重启会话。外部模式与面板登录态独立。
- 旧 ref 在刷新快照后失效。提交或导航报错时先读取当前快照，不能盲目重试可能已经完成的操作。

## 内置面板模式（默认）

- 你的工具直接作用于右侧栏浏览器面板里的**当前页面**，用户实时可见——这是本能力的首选形态。
- **单页模型**：接管模式下不要用 `browser_tabs new` 新建标签页（新页面没有可见窗口，用户看不到）；需要换站点时直接 `browser_navigate`。
- 面板未打开时直接调用所需浏览器工具，CrewVela 会自动打开右侧栏；无需要求用户提前手动打开。面板被隐藏后，再次实际调用工具会重新显示当前页面。
- 面板有独立的持久登录态（一次登录跨重启保留）；外部兜底模式是另一个独立配置，两边登录态不互通。
- 兜底到外部浏览器时会弹出独立窗口，提前告知用户。

## 核心原则

1. **无障碍快照优先，截图慎用**。`browser_snapshot` 返回页面的无障碍树（文本，含元素 ref），信息密度高、消耗小，是你理解页面的首选。`browser_take_screenshot` 返回图片，仅在布局/视觉信息（图表、画布、样式问题）无法从快照获得时使用。
2. **小步走**。一次工具调用做一件事：导航 → 快照 → 点击 → 快照确认 → 下一步。不要在未快照的情况下盲点元素。
3. **用 ref，不猜坐标**。快照里每个可交互元素都有 `ref`，点击/输入一律通过 ref 定位；不要凭记忆或猜测填写坐标。
4. **页面变化后重新快照**。导航、点击、提交之后页面结构可能已变，旧 ref 可能失效；失效就重新 `browser_snapshot`。

## 标准工作流

```
browser_navigate      打开目标 URL
browser_snapshot      读取无障碍树,找到目标元素的 ref
browser_click         按 ref 点击 / browser_type 按 ref 输入
（重复 snapshot → 操作,直到完成）
browser_tabs          需要多页面并行时管理标签页
```

表单批量填写用 `browser_fill_form`（一次传多个字段）；等待页面加载/特定文本用 `browser_wait_for`；页内查找用 `browser_find`；需要检查网络请求时用 `browser_network_requests`。

## 工具速查

| 工具 | 用途 |
|---|---|
| `browser_navigate` | 打开 URL（自动等待加载） |
| `browser_snapshot` | 无障碍树快照（首选的"看页面"方式） |
| `browser_click` / `browser_drag` | 按 ref 点击 / 拖拽 |
| `browser_type` | 按 ref 输入文本（可带 submit 提交） |
| `browser_press_key` | 按键（Enter/Tab/Escape 等） |
| `browser_fill_form` | 一次填写多个字段 |
| `browser_select_option` | 下拉框选择 |
| `browser_hover` / `browser_focus` | 悬停 / 聚焦 |
| `browser_tabs` | 标签页 list/new/close/select |
| `browser_wait_for` | 等待文本出现或元素状态 |
| `browser_find` | 页内查找并高亮 |
| `browser_take_screenshot` | 截图（仅在视觉信息必要时） |
| `browser_network_requests` | 查看网络请求（调试接口/失败） |
| `browser_handle_dialog` | 处理 alert/confirm/dialog |
| `browser_file_upload` | 文件上传 |
| `browser_evaluate` | 执行 JS（最后手段，慎用） |

## 注意事项

- **登录态**：内置面板模式自带独立持久配置；外部兜底模式以隔离配置启动（`--isolated`），不携带用户的历史 Cookie 与登录态。需要登录的站点：先尝试用快照找到登录表单完成输入；涉及密码/验证码时把页面停在输入处，**请用户手动完成敏感步骤**，再继续操作。
- **凭据安全**：绝不主动索要或输入密码、支付信息；这类操作交给用户手动完成。
- **下载与文件**：触发下载后向用户确认保存位置；不要静默下载可执行文件。
- **超时**：单个工具调用超时会报错，重试一次仍失败就把已看到的快照内容汇报给用户再决定。
- **浏览器找不到**（`Chromium distribution ... is not found`）：提示用户安装 Chrome，或在浏览器控制设置中指定自定义路径后重试。
- 工具不可用（列表里没有 browser_* 工具）说明本会话未启用浏览器控制，提示用户到 CrewVela 设置 → 浏览器控制 打开。
