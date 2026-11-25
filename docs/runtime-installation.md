# CLI 安装与版本管理（Windows）

入口为「设置 → 协作中心 → 管理 Runtime → 安装与版本」。

## 安装范围

- Windows x64 / ARM64 下，Claude Code、Codex、OpenCode、Kimi Code、Pi 和 Antigravity CLI 有 CrewVela 托管安装入口。可安装版本取决于上游提供的对应平台发布包。
- ZCode CLI 使用本机已有安装。可以检测 PATH 中的命令，或点击「选择已有 ZCode CLI」并验证所选入口。ZCode Desktop 的聊天适配器单独管理。
- Windows x64 的官方包验证记录见下文，ARM64 尚未实机验收。macOS 可按各 Agent 的官方方式安装，然后在 CrewVela 中使用。

CLI 安装在当前用户的 CrewVela 配置目录 `runtime-installations/<kind>/versions/` 中，仅改变 CrewVela 使用的程序入口。外部终端继续使用原有全局 CLI，既有 CLI 安装及其运行进程保留。

安装的是官方原生程序或完整原生平台包，不依赖 Node/npm，不执行 npm 安装脚本。原生聊天适配器可能仍有独立的 Node/适配器依赖；本功能不宣称消除了这些依赖。

## 安装流程

1. 检查平台、官方版本和当前 CLI 状态。
2. 下载官方文件，按响应字节显示真实进度；无法取得总大小时不显示百分比。
3. 按运行时校验发行包。Claude Code 使用官方清单的 SHA-256，Codex 和 OpenCode 使用 npm 平台包的 SHA-512，Antigravity CLI 使用 Google 官方发行清单的 SHA-512。安装时保留需要的辅助程序及包目录结构。
4. 在新目录验证版本和 `--help`。通过后原子切换 `current.json`，旧入口备份为 `previous.json`。失败或取消时不切换；更高的本地版本不会被自动降级。
5. 根据需要打开原生 CLI 登录，完成后重新检查。登录成功与模型额度可用分开显示。

关闭管理面板后下载可继续，重新打开可查看同一操作。退出整个应用不会继续执行下载；原安装入口保留，可再次发起安装。安装不修改本地 Agent 的模型、账号或 MCP 配置；用户在原生 CLI 中主动完成的登录仍由该 CLI 管理。

## 验证边界

- UI 中的错误、取消、重试、登录引导复查等流程已用隔离 IPC 运行验证。
- 真实官方包在独立临时目录完成校验及启动自检；没有替换日常运行实例。
- 新用户隔离测试模式仍禁止真实 Agent、账号及安装操作。实际安装请直接运行正式 EXE，不带 `--test-data-dir`。
- 未自动提交登录、未调用模型测试额度；跨 Agent 模型互聊仍按此前约定留待后续验收。

官方安装来源：[Claude Code 安装文档](https://code.claude.com/docs/en/setup)、[Codex CLI 官方说明](https://developers.openai.com/codex/cli)。
