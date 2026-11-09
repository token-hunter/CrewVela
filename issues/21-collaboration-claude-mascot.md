Status: ready-for-human

# 修复多智能体 Claude Code 标题栏缺少吉祥物

## 原因与改动

多智能体 Claude Code 使用 kind=terminal、agentRuntimeKind=claude_code，原标题栏仅对 kind=claude 渲染 SessionPet，而通用 CLI 吉祥物列表也不包含 Claude。改为按标题栏解析后的 runtimeKind=claude_code 复用现有 SessionPet，兼容普通 Claude 及协作会话。

## 验证

- 扩充已有 cli-session-pets-ui fixture，添加带协作关联的 Claude 终端会话。
- 实际浏览器运行 TerminalPane，验证普通/协作 Claude 的工作和空闲状态切换，实际图片可见；同时观察 Codex、其他 CLI 及普通终端的显示不变。
- Node 全量399通过、1跳过；Windows生产构建、版本检查通过，保留既有Rust/Vite及bundle identifier提示。
- 证据：output/design-qa/claude-collaboration-mascot/idle.png。预览不等于对用户当前原生窗口进行了热更新。

## Comments

- 2026-11-08：保留已有未提交的通信及首次启动引导改动。本次不更改通信代码，不提交或推送。
- 交付：output/CrewVela-20261108-claude-mascot/CrewVela.exe，包含之前的通信修复。未终止用户程序，需退出旧程序后启动新版；原生窗口未直接热更新，浏览器预览的终端输出为fixture。
