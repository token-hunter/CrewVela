# Beta 精简为本地 CLI 与 Agent 协作
Status: ready-for-human

## 需求
完整移除模型设置、电脑控制、插件市场；所有 CLI 使用本机配置；仅在大改分支操作。

## 验收
- 无三项导航、设置、专用 IPC 或后台模型注入。
- 旧会话与协作、Skills、MCP、浏览器及共享记忆保留。
- 旧受管桌面控制注册迁移不影响用户自建内容。
- 回归与真实 Claude Code/Codex 收发通过，重新打包 Windows EXE。
- main 和备份提交保持 239febf0e55e10bb8733663d4ac840e2f4eeea25。

## Comments
- 三项功能与启动覆盖移除完成；前端/后端完整普通回归通过。证据见 docs/beta-local-cli-verification-20261115.md。
- 真实 Codex 收件回复通过；Claude 本地 GLM 套餐到期，真实双向未通过。用户明确选择先交付 EXE，真实互聊后续再验。
