Status: ready-for-human

# 修复已打开的 Claude Code 与 Codex 双向通信

## 需求

通过通信 Skill 在用户已打开的原会话之间发送和回复，不伪造对话，不把消息改发给新 Agent，不扩大项目沙箱。

## 已复现

- 本机共享 Skill 仍引用已退役环境变量，当前进程实际提供新环境变量。备份后同步新版，Claude 使用自身身份向原 Codex 成功发送消息。
- 原 Codex 收到消息，但默认只读沙箱拒绝 CLI 写入 outbox，回信停在权限审批。此前启动热修复只恢复启动，没有完成双向通信。

## 实施与验收

- 为受管 Codex 添加仅支持会话发现与消息发送的 stdio MCP；沿用原队列及就绪投递，发送方身份绑定该启动会话，不提供文件或 shell 操作。
- Skill 优先使用该会话工具，CLI 保持兼容；更新已安装的标准指南，保留用户自定义版本。
- 隔离测试覆盖真实队列、错误输入、身份伪造、旧身份失效、MCP 握手与帧长度限制。
- 在原 Claude/Codex 窗口确认双向消息和实际回复，回归默认 Codex 启动及原有只读通信。

## Comments

- 已完成：受管 Codex 的 `crewvela_sessions` stdio MCP、共享标准指南更新和回归验证。真实 Codex 于本地时间 14:46:30 通过 MCP 发出一次测试消息，原 Claude 于 14:47:58 前通过自身 CLI 身份回信，原 Codex 实际收到 ACK 并确认结束。应用会话 ID 保持不变，不以排队回执替代送达证明。
- Rust 1009 通过/36 忽略、Node 393 通过/1 跳过、CLI 入口 1 通过、实际只读沙箱 2 通过，MCP SDK 与 Windows release 构建通过。细节与既有限制见 `docs/session-skill-communication-fix.md`。
