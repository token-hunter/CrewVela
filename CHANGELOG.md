# 更新记录

当前开发线为 **v0.1.0 Beta**。安装包内部版本为 `0.1.0`。GitHub 测试构建使用独立的 Beta 标签，例如 `v0.1.0-beta.20261118`。下载、源码提交与校验值见[发布列表](https://github.com/token-hunter/CrewVela/releases)。

## v0.1.0 Beta · 2026-11-26

[Windows x64 Beta 安装包](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0-beta.20261126)，对应源码提交 `0062a19`。

- 合入 macOS 上的 ZCode Desktop 模型目录修复，以及新建会话时 Agent 图标遮挡文字的修复。
- 使用更新后的源码重新构建 Windows x64 安装包。

## v0.1.0 Beta · 2026-11-25

[Windows x64 Beta 安装包](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0-beta.20261125)。以下是 11 月 18 日 Windows Beta 之后合入的主要变化。

- **接入与兼容性**：新增 Antigravity CLI，支持在 Windows 管理官方安装及版本。ZCode CLI 支持检测并选择现有安装。ZCode Desktop 支持读取回复语言并同步官方模型目录。
- **项目托管**：支持单人和双人成品交付，多成员开发时可由实现成员兼任联调。改进 Claude Code 待审批后的会话保留、ZCode CLI 审查预检，以及交付卡片和 Demo 操作。
- **协作聊天**：增加 Bot 模式、项目总览、成员模型和推理选项，以及图片和文档附件预览。
- **界面与工作区**：支持简体中文与 English，覆盖主要界面、高级设置及系统托盘。可搜索会话、终端和项目，组合筛选会话，并将设置入口固定到侧栏。
- **项目与工作树**：增加项目工作区、开发服务启动、工作树管理和恢复。
- **共享记忆**：加入内置本地记忆服务，扩展旧版接入选项，修复内置服务端口冲突的处理。

## v0.1.0 Beta · 2026-11-09

以下为早期 0.1.0 开发线的功能概览。当前安装包与校验文件见[下载与安装](docs/downloads.md)。

- 多 Agent CLI 和原生聊天工作台、会话通信及教程。
- 项目协作、交接预览与可见的来源历史。
- 本地共享记忆接入向导（快速安装当前限 Windows x64）。
- 独立首次启动测试目录与 Mac 专属 WebKit 存储。
- 修复 Mac 新旧品牌缓存冲突导致的普通启动崩溃。
- 增加 Windows NSIS 安装向导配置及独立 bundle 命令，统一 Windows/Mac 发行文件名。

平台验证范围、已知问题与签名状态见[版本说明](docs/releases/v0.1.0.md)和[检查清单](docs/release-checklist.md)。源码提交与安装包 SHA-256 记录在版本说明中。

后续约定：兼容修复用 `0.1.1`，新功能批次用 `0.2.0`；0.x 的不兼容行为也必须明确迁移说明。核心工作流、升级与两平台安装稳定后，再考虑 `1.0.0`。已发布版本不覆盖原二进制，任何修订发新版本。
