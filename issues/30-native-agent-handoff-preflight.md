Status: ready-for-human

# 修复原生 Agent 交接前检查失败

## 验收

- Claude Code Chat → Codex Chat 不再因原生 Chat 类型缺少可执行文件定义而误报未安装。
- 共享记忆明确返回项目/工作区不存在时视为无待接记录；未知 404、认证、网络及响应错误仍可见。
- 保留预览、确认、权限、来源停止和历史保留边界，不代替智能体领取记忆交接棒。
- 验证原生交接流程、实际界面与已有回归，并按指定命令重新打包。

## Comments

- 只读复查：记忆服务 2.6.0 正常，目标查询 404 正文为 project 'xiaohongshu' not found in workspace 'default'；接口并非缺失。
- 已修复原生 Chat 可执行文件解析，保持运行时身份、启用开关和权限校验独立；仅精确匹配当前作用域不存在的 404 返回空记忆，其他异常继续显示。
- 两项回归先失败后通过。完整 Rust 1044 通过 / 43 默认忽略，相关前端 22 通过；3 项本机检查另行通过，七种原生 Agent 交接链路回归通过。
- CUA 实际窗口隔离验证通过准备、预览、确认、queued；确认前来源不停止、不创建目标。真实用户会话未自动交接，真实模型回复未复测。
- 指定打包命令已成功，默认输出 src-tauri/target/release/CrewVela.exe。未提交或推送。详细证据：`.scratch/native-agent-handoff-preflight/qa.md`。
