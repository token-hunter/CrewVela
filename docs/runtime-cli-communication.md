# Runtime 会话通信

项目协作的 Claude Code、Codex、Kimi Code、Pi、OpenCode 共用 `CREWVELA_CLI sessions` / `send --to <会话 ID> --body <内容>`。PTY 和项目 ACP 子进程均注入 `CREWVELA_CLI`、`CREWVELA_SESSION_ID`；每个子进程的身份独立。

Codex 工具命令会独立构造环境，因此受管 Codex 启动时还通过 `shell_environment_policy.set` 显式传入这两个变量；ACP 使用 `CODEX_CONFIG` 传入相同配置，保留其他配置。旧的运行中进程需要在新构建中重新启动或恢复才能获得更新。用户设置的工具环境白名单仍适用，若显式配置了白名单，需要允许这两个变量。

`send` 成功表示消息已持久化排队，并不表示目标已经处理或回信。`sessions` 的 `deliveryMode` 为 `terminal` 或 `scheduled_turn`，`processRunning` 说明是否有终端进程，`runnable` 包含可通过调度执行消息轮次的成员。

Pi以外的交互式终端等待输入就绪后，以 bracketed paste 写入完整多行消息，再延迟发送回车。忙碌、权限/登录提示、未识别的输入状态不投递。人工提前回车会取消待补发的回车。

文本写入后若出现权限、登录或更新菜单，延迟回车会暂停，等待新的输入就绪信号；状态仍显示 Running 的 TUI 菜单也受此规则保护。多行粘贴和菜单中的人工 Enter 不能确认消息送达。旧 PTY 的迟到输出不能改变替代进程的输入状态。直接协作唤醒保留会话原有权限，不因发消息自动升级为 YOLO。

Codex 紧凑输入框在首轮之后不依赖启动标题栏；已识别的终端帧会保留，尾随光标、颜色和分块页脚不撤销就绪。忙碌提示仍阻止收件及完成兜底。Codex 首次绑定或切换原生 thread 时，以精确 rollout 的 ID、交互来源和工作目录校验通知；后台标题、子任务不能覆盖主会话。已注册任务的完成通知必须匹配当前输入，校验先于身份和完成状态写入。

项目中一次性任务和 ACP 会话使用独立消息任务：与同成员的业务 Run 串行，在隔离消息目录处理，回复可写 CrewVela outbox。不会重跑原业务任务，也不更改原任务状态。项目为草稿、运行中或已完成时均可交流；暂停、取消、成员禁用时消息保留等待。消息任务不参与业务审核和项目完成判定。执行中断的消息不会自动重试，避免重复回复。

尚未开始的消息任务可在项目恢复后执行；已开始的消息任务在暂停、确认停止或应用恢复时进入失败状态，不自动重放。确认进程停止不等于批准再次发送。Claude/Codex 消息轮次保留强制只读；其他受支持 Runtime 使用独立消息目录和成员原权限，不能宣称它们具备强制只读边界。消息任务目录拒绝路径逃逸，Auto 的隔离判断校验真实路径并拒绝与业务目录重叠。

定向私信的可见范围同时应用于 `cc_thread_read`、`cc_knowledge_search`、`cc_memory_search` 和任务初始记忆注入。私信任务产生的任务描述、执行结果、产物及记忆不会通过搜索公开给其他成员；验证记忆内容不改变原私信范围。用户管理界面仍可检索整个项目。

直接协作、自动化消息、广播先完成整批落盘，再发布到可投递队列。写盘失败返回错误，不会留下部分可投递消息；已接受消息的历史投影写入失败，会返回明确警示而不伪装成发送失败。

受管 Pi 交互会话通过启动时的私有 `-e` 扩展读取独立收件箱，并调用 Pi 官方 `sendUserMessage` API 向原会话发送；不会写入 TUI 或补发回车，内置 `/model`、`/settings` 等选择器不会接收消息键盘输入。接收器仅在启动或 `agent_settled` 后且空闲、无待续消息、无阻塞扩展 UI 时开始投递。扩展使用每启动 token 和进程单调序号上报状态，`/reload` 不重置有效事件顺序。消息原队列在 Pi 会话 entries 确认接收后才提交；这表示 Pi 已接收，不表示模型已处理、回复或记录已写入 Pi 磁盘文件。API 调用前落下 processing 副本，未确认时不会自动重发，30秒后显示未确认错误；重载或重新启动会检查原处理记录而不重复调用。不会解析 Pi 屏幕文字，也不安装用户全局扩展。旧进程无法热附加扩展，需要用新构建恢复或重新启动原会话；等待的消息留在原目标。Oh My Pi 不支持执行。

Pi 项目任务另通过同一私有扩展的官方 `registerMcpServer` 接入当前 Run 的 `crewvela` MCP。只接受受管本机地址；`CREWVELA_PI_MCP_URL` 和 `CREWVELA_AGENT_TOKEN` 保留在子进程环境，注册配置使用环境变量引用，不把令牌写进项目或用户配置。已实测 Pi 1.1 的原生 MCP 与 Claude 互通；缺少该 API 的旧版本会明确提示，仍可使用会话 CLI 通信，因此未把原生 MCP 无条件声明为所有 Pi 版本都具备的能力。

`sessions` 的 `preferredForRuntime` 和 `selectionHint` 区分已经打开的交互会话与项目后台成员。仅指定 runtime 别名（例如 `send --to pi`）时优先唯一已打开的交互会话；多个候选明确报错，零候选不自动启动项目成员。完整会话 ID、唯一 ID 前缀、完整标题仍按明确目标发送，不自动重定向；重复标题要求使用 ID。用户要给已经打开的 Pi 打招呼时，应选该会话 ID 或 `pi`，不能自行用项目审查成员替代。

应用的常驻自动化心跳也会摄取 CLI outbox 和 Claude hooks、调度消息轮次及提交已知就绪的终端消息，无需一直打开项目终端视图。尚未识别的交互状态仍需终端轮询观察。

遗留的内部初始提示、系统通知队列助手仍有记录写盘错误但保留内存操作的路径；磁盘故障后应用崩溃可能丢失这类尚未落盘的通知。面向用户的直接协作、自动化消息、广播已改为持久化成功后才接受；CLI outbox 与后台任务表使用独立持久化路径，不能把正常回归理解为任意 I/O 故障都保证送达。

自动化回归使用隔离 outbox/mailbox、五种 sender × 五种 target 的假 PTY 投递、并发持久化及项目任务调度测试。另有显式启用的真实 Claude/Codex MCP 闭环、实际 CLI 连续四轮收件，以及 Claude/Codex/OpenCode/DeepSeek 四方全互通、Pi 与 Claude、两种 ZCode 的实际模型通信；这些测试使用独立项目和会话，Kimi 保留账号 403 阻塞。具体证据、失败记录及未验证范围见 [通信验收报告](agent-communication-validation.md)。

受管 Claude Code 原生 runtime 的身份 settings 和输入就绪 hooks 在常规权限模式也启用，独立于 YOLO；不会为通信放宽权限。发送方声明的会话 ID 不存在时拒绝，不把它静默降级成人工无来源消息。回复包装同时提供合法 PowerShell（`&` 调用与单引号转义）和 Bash 命令，始终引用发送方原 ID。

统一技能源为 `skills/crewvela/SKILL.md`。应用启动时同步到共享 `.agents/skills/crewvela` 和六种 CLI 的原生用户 Skill 目录：Claude `.claude/skills`、Codex `.codex/skills`、Kimi `.kimi-code/skills`、ZCode CLI `.zcode/skills`、Pi `.pi/agent/skills`、OpenCode `.config/opencode/skills`。尊重 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`KIMI_CODE_HOME`、`PI_CODING_AGENT_DIR`、`OPENCODE_CONFIG_DIR` / `XDG_CONFIG_HOME`，只更新内置原文或可识别的旧版，保留自定义文件及 Skill Center 管理记录。共享目录可独立存在或链接，不依赖链接才能发现。

ZCode CLI 0.16 的 Skill 发现与斜杠补全分开：`zcode --json skills list` 可检查发现，`/skill crewvela <需求>` 是原生调用方式。应用另安装 `.zcode/commands/crewvela.md`，让 `/crewvela <需求>` 出现在原生自定义命令列表，要求通过 Skill 工具加载同一指南；已有自定义同名命令保留。旧运行进程需要重新启动或恢复原会话才能刷新命令目录；发现成功不代表模型已调用 Skill，更不代表消息已送达。不替换 Kimi 已有 skill roots，不覆盖 Codex developer instructions 或放宽工具环境策略。

Pi 私有桥的启动 token 校验通过后，将原生会话位置保存到应用私有 sidecar。恢复Pi必须带官方 `--session` 指向仍存在的原会话文件。缺失或无效时明确拒绝，不静默建立新历史。尚未报告位置的旧进程不能承诺自动恢复上下文。

人工群发按成员会话分别确认入队。部分失败时可用“仅补发失败成员”重试原文；成功成员不会收到重复消息，继续编辑的新草稿会保留。跨任务定向通信保留来源任务，收件成员能在自己的任务中读取，其他成员不能读取这条私信。
