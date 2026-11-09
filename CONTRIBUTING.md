# 参与 CrewVela / Contributing

优先欢迎可复现 Bug、Windows/Mac 安装验收、Agent 兼容性、首次使用说明和无障碍改进。维护语言以中文为主，也欢迎英文问题与 PR。

## 提交问题

使用 Bug 模板填写版本、平台/架构、Agent 及 CLI 版本、预期/实际结果和最小复现。日志与截图先脱敏，不附真实 Key、账号、邮箱或私有路径。安全问题按 [SECURITY.md](SECURITY.md) 处理。

## 开发与验证

准备 Node.js 24、Rust stable、平台构建工具以及本机 Chrome/Edge。运行 `npm ci`、`npm test`、`npm run build`。Rust 按变更范围运行有意义的回归，完整命令和平台步骤见 [使用手册](docs/user-guide.md) 及 [Mac 指南](MACOS-CODEX-HANDOFF.md)。

只修改当前问题所需内容。代码、测试和截图均使用合成数据；不要把个人历史或系统凭据复制进仓库。前端 Mock/浏览器预览不代表原生桌面或真实模型验收。新增平台跳过必须说明原因，不通过删断言或跳过失败测试制造通过。

## 提交 PR

从任务分支发起 PR，说明解决的问题、行为变化、实际验证及未完成项。界面变更附脱敏截图。添加依赖或素材时记录来源与许可。避免无关格式化和批量锁文件升级；破坏兼容性或改权限机制先说明设计取舍。

尚未建立远程 Issues 时，本地任务遵循 `docs/agents/issue-tracker.md`；公开后使用仓库 Issue/PR，保留能帮助复现的上下文。

## English summary

Reproducible bugs, platform testing, agent compatibility, accessibility, and docs are welcome. Include the app/CLI versions, OS/architecture, expected behavior, and sanitized reproduction steps. Keep PRs focused, test affected behavior, and state limitations. Never include credentials or private histories. Record licenses for new dependencies and artwork. Report sensitive issues through the security policy rather than a public issue.
