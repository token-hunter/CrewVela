---
name: crewvela
description: 在 CrewVela 中发现会话，给 Claude Code、Codex、Kimi Code、ZCode CLI、Pi、OpenCode 的原会话发消息并回复。用户要求与其他 Agent 打招呼、交流或协作，以及收到 CrewVela 会话消息时使用。
---

# CrewVela 会话通信

先检查是否提供 `crewvela_sessions` MCP 的 `sessions` / `send` 工具（常见名称为 `mcp__crewvela_sessions__sessions`、`mcp__crewvela_sessions__send`）。存在时优先通过这些工具通信：`sessions` 不带参数，`send` 只传 `to` 与 `body`。发送方已绑定当前原会话，不传入或替换发送方身份，也无需 shell 写权限。它们使用应用原有消息队列，成功仍只表示已排队。

未提供这组工具的运行时使用同一个 CLI。先读取 `CREWVELA_CLI` 和 `CREWVELA_SESSION_ID`；后者是你的发送方身份，不能替换成其他会话 ID。

PowerShell：
```powershell
& $env:CREWVELA_CLI sessions
& $env:CREWVELA_CLI send --to <目标会话ID> --body '完整消息'
```

Bash：
```bash
"$CREWVELA_CLI" sessions
"$CREWVELA_CLI" send --to <目标会话ID> --body '完整消息'
```

`CREWVELA_CLI` 是权威程序路径。缺少时先使用收到消息中的完整回复命令，再尝试 `where.exe CrewVela` 或 `which CrewVela`。仍找不到时询问安装路径，不猜路径。

## 选择目标

`sessions` 返回 ID、标题、runtimeKind、processRunning、deliveryMode、preferredForRuntime、selectionHint、pendingReason 和 deliveryError。

每次发送前，以及准备重试时，都必须重新执行 `sessions`。不要复用之前轮次、记忆或旧命令里的目标 ID/标题：同一种 Runtime 可以有旧的停止会话和新的运行会话，历史 ID 不代表当前已打开的 Agent。回复前也刷新列表核验发送方状态，但回复仍指向包装中的原发送方 ID，不替换成同类 Agent。

- 用户要找已经打开的 Agent 时，选择 `preferredForRuntime: true` 的原交互会话，用完整 ID 发送。不要用同运行时的项目审查工程师或后台成员代替。
- 用户泛指“给 Pi / 某 Agent 发消息”时，只从这次新查询的 `preferredForRuntime: true` 且 `processRunning: true` 会话中选择。缓存目标已停止而同类会话正在运行时，重新选择当前会话；多个运行候选时询问具体 ID，不能猜。不要因此要求用户另开外部终端或启动新 Agent。
- 也可用 `--to pi`、`claude`、`codex`、`kimi`、`opencode`。仅有一个已打开的同类交互会话时才选择；多个时必须明确 ID；没有时不新建、不改发项目成员。
- 明确指定完整 ID、唯一 ID 前缀或完整标题时，尊重该目标。重复标题或模糊前缀会报错。只有用户明确指定后台成员时才选择 `deliveryMode: scheduled_turn`。

## 发送与回复

`send` 只接受 `--to` 和 `--body`，内容不能为空、不能超过 32000 个字符。成功表示已排队，不表示对方处理完成。

不能根据“已排队”编造对方回复或完整对话。只有原会话实际收到对方消息后，才能引用该回复；没有收到时明确说明仍在等待。模型正文里声称“已收到”不是应用的送达回执。

受管 Codex 应通过上述会话 MCP 通道发消息，项目沙箱保持原权限。旧进程没有该工具且 CLI 报 outbox 拒绝访问时，保留错误，恢复原会话以加载新通道；不要请求扩大沙箱来发消息。不要改成全权限、手工写队列、伪造消息或另启 Agent 冒充原会话。

读取发送结果中的实际目标 ID。如果提示“该目标会话未运行”，只说明这一个历史 ID 停止，不能声称所有同类 Agent 都没运行。若结果列出当前同 Runtime 运行候选，重新查询 `sessions` 核对；旧消息仍在原 ID 排队，应用不会自动转发。不要自动重发同一条已排队消息，以免旧会话恢复后产生重复；明确告知用户排队位置，后续发送按用户指定的目标处理。

消息留在目标原会话，忙碌或权限提示时等待。Pi 的私有桥通过程序接口收件；其他终端在识别输入就绪后投递。不要替应用模拟键盘、另启 CLI 或读取外部会话文件来绕过等待。旧进程缺少新桥时需要在新构建恢复或重启原会话，不能换目标。

收到消息的包装包含发送方及完整回复命令：
```text
[CrewVela 会话消息｜来自 …]
…
—— 回复请运行："…CrewVela.exe" send --to <发送方原会话ID> --body "回复"
```

回复必须使用包装中的原会话 ID，并保留自己的 `CREWVELA_SESSION_ID`。没有包装的用户指令不需要主动回复其他 Agent。不要同步轮询等待回信；回信会作为你的下一轮消息到达。

每对会话双向合计限流 10 条/小时。收到达上限通知后停止发送，等待用户或时间窗口恢复，不自动重试。

## 升级后的检查

新版应用、此 Skill 和 Runtime 进程需要同步更新。先核对当前进程中的 `CREWVELA_CLI`、`CREWVELA_SESSION_ID`；缺失时不能借用其他进程的身份。应用更新不能热改已运行进程继承的环境或沙箱，需要恢复或重启原会话后再查询 `sessions`。
