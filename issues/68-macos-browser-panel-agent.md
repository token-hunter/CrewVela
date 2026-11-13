Status: ready-for-agent
Execution: completed

# macOS 内置浏览器面板 Agent 接管

在 feat/macos-browser-panel-agent 实现 WKWebView 专用控制服务与 MCP 工具；保留 Windows CDP 接管实现。

使用、设计边界与可重复验收步骤见 `docs/macos-browser-panel.md`。

## 实施结果

已完成，执行验证通过；使用与边界见 `docs/macos-browser-panel.md`。

- Node 480通过/5跳过；Rust浏览器相关28通过。
- 双平台设置保存、布局、多面板及首次懒挂载回归通过。
- 独立真实WKWebView与stdio MCP验收通过，未调用真实模型。
- Windows原桥接脚本保持字节一致；无Windows设备实测。
- 分支测试 DMG 已构建，完整性、签名、ARM64 架构与新适配器嵌入检查通过；用户安装后确认测试没有问题。
- 复杂网站及所有 Agent 客户端未逐一实测。
