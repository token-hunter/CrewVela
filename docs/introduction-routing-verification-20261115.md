# 自我介绍分流与重复启动修复验证（2026-11-15）

## 结果

截图原句 `@ClaudeCode @Codex 你两自我介绍一下` 已在新版 Windows EXE 的正常模式中通过：Claude Code 和 Codex 各通过自己的通信工具发送一次介绍，对方原生会话均实际收到完整正文。没有创建工程任务、语义复核任务或工程运行。既有 24 条任务及失败历史保留，不将旧失败改为成功。

## 修复范围

- `direct_message.rs` 识别自我介绍、互相介绍、互相认识等意图，复用现有直接通信链路。仍排除开发、实现、重构等工程指令；成员名称或引文中的介绍字样不触发。
- `lib.rs` 对多成员介绍明确要求向其他参与者真实发送一次介绍；收到介绍后本地查看，不再发送确认或问候，避免循环。
- `executor.rs` 将已注册且正常准备或运行的调度器重复启动处理为成功复用，不重复准备成员会话。取消中的注册仍报“正在停止”，关闭检查继续生效。
- 未改动 Git/worktree 限制，也未修改成员职责、模型或旧任务。

## 正常模式实测

执行文件：`output/CrewVela-introduction-fix-20261115/CrewVela.exe`，主进程 PID 18048，无命令行参数，使用正式 `%APPDATA%/CrewVela` 数据；没有 Mock 或测试数据目录参数。

项目：模型设置实测-20261115，ID `684db442-fb6f-4df8-8d3e-a769c086916c`。目录为原非 Git 测试目录 `C:/Users/PC/AppData/Local/Temp/crewvela-model-live-e79907a4ca`。

在实际界面只发送一次原句，没有附加验收指令或标记。启动 Codex 时按 Escape 跳过官方更新提示；没有向成员终端手工粘贴提示或代按提交。

| 成员 | 应用会话 | 原生会话 | 实际模型 |
| --- | --- | --- | --- |
| Claude Code | c7b87a67-63b1-4756-98db-0de8cd4440e3 | 2e37ac6d-ab4d-4206-adb1-5f695c803fd0 | glm-5.3-flash |
| Codex | f8aa1d07-2109-47fb-9474-bd687ab0d40b | 01a1de2f-cac5-7800-bd7c-3ef5721cc5db | gpt-6.1-terra |

Claude 通过自己的 CREWVELA_CLI 发送，Codex 通过绑定会话的 crewvela_sessions MCP 发送。两条消息分别具有成功的真实工具调用及返回，数据库记录的发送/接收身份与原生会话一致，接收方转录包含完整匹配正文：

- Claude → Codex：`outbox-4a7df10501a9e0a44d25a1a2a11733cb2833d8de5d6abd389a48ef99977b57cf`。
- Codex → Claude：`outbox-0de4185aae2dc6622861eb3577781cd3594a28777d8eb75816f7f28c1183a4ce`。

最终各发送一次，双方本地确认收到，无后续问候循环；任务和运行 ID 与测试前一致，两方队列均为零。证据是实际收件，不以排队成功代替。

验证快照保存在 `.scratch/introduction-routing/verified.json`、`live-evidence.json`、`normal-process.json`；`audit.py`、`verify.py` 保留核对过程。快照仅代表本次发送完成时状态，之后用户继续操作产生的数据不属于本次结果。

![正常模式双向介绍](../.scratch/introduction-routing/normal-introduction-passed.png)

## 自动回归与构建

先观察到新增测试的真实失败：原句分流返回空，重复启动未接受，正在停止的注册未拒绝。修复后，意图测试同时覆盖介绍变体及自我介绍页面开发等反例；启动测试调用实际 start 入口，断言复用同一注册对象、只有一个调度器且未准备会话或新增任务，并覆盖停止中的拒绝。

首次全量测试 1137 通过、1 失败、49 忽略；失败是新增测试夹具清理 SQLite 文件时遇到 Windows 共享占用（通信桥线程仍持有文件），业务断言已通过。将该测试夹具改为内存 SQLite，保留全部断言后重新运行：**1138 通过、0 失败、49 忽略**。日志：`.scratch/introduction-routing/rust-final.log`。

按约定执行 `npm run tauri -- build --no-bundle`，构建成功；已有编译警告仍保留。交付 EXE 隐私扫描通过，未发现禁止路径。未在 macOS 运行验证。

- 交付：`output/CrewVela-introduction-fix-20261115/CrewVela.exe`。
- 大小：46,385,152 字节。
- SHA256：`B6A9EC775FFC6B034C4BD186362F7C3C9692743DC2CE4258683DF6223FE74BB5`。

新版窗口保持打开，方便继续测试。退出旧版本时临时调整的关闭到托盘选项已通过界面恢复为原来的启用状态并保存；未提交用户草稿。ZCodeCLI 按用户明确要求暂不处理，不将其列为通过。
