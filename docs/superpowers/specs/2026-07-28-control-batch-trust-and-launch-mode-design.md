# 总控批量信任与启动模式设计

> 日期：2026-07-28

## 1. 目标

总控一次创建多个 Claude Code 会话后，如果这些会话停留在工作区信任确认页，用户只需在总控中按一次 `Enter`，即可确认本批次里所有仍在等待该确认的会话。

总控自然语言启动指令同时支持指定启动模式：

- 未指定模式：使用 Default。
- 指定 Auto：使用 `claude --permission-mode auto`。
- 指定 YOLO：使用 `claude --dangerously-skip-permissions`。

Auto 与 YOLO 互斥，不允许同一个启动项同时启用。

## 2. 交互

### 2.1 批量信任

1. 总控执行批量启动后，为本次启动保存会话 ID 集合，形成一个独立启动批次。
2. PTY 输出识别到完整的 Claude Code 工作区信任提示后，将对应会话标记为“等待工作区信任”。
3. 总控显示批次提示，例如：`3 个新会话正在等待信任此目录，按 Enter 统一确认`。
4. 当总控输入框为空且批次提示处于激活状态时，按 `Enter` 只向本批次中仍处于该状态的会话发送 `1\r`。
5. 确认成功后显示实际确认数量；已经进入对话、已经退出或出现其他提示的会话不接收按键。
6. 如果总控输入框中已有文字，`Enter` 保持原有的发送行为，不触发批量信任。
7. 本功能不记住目录；下次批量启动再次遇到信任页时，仍需进行一次统一确认。

### 2.2 指定启动模式

总控解析以下语义：

- `打开 3 个 GLM，Auto 模式`
- `用 YOLO 打开 1 个 DeepSeek 和 1 个 Kimi`
- `打开 1 个 GLM Auto、1 个 Kimi YOLO`

启动模式属于单个启动项，因此同一条总控指令可以让不同 Provider 使用不同模式。没有修饰某个启动项时，该启动项使用 Default。

总控执行结果应明确显示每组会话的模式，例如：`已创建：GLM ×1（Auto）、Kimi ×1（YOLO）`。

## 3. 架构

### 3.1 PTY 信任状态

PTY 会话增加内存态 `awaiting_workspace_trust`，默认值为 `false`。PTY 输出检测器只在匹配完整的工作区信任提示特征后将其置为 `true`，包括：

- `Accessing workspace`
- `Yes, I trust this folder`
- `Enter to confirm`

检测时忽略 ANSI 控制序列和大小写。仅出现普通回答中的 `trust` 或 `folder` 不得触发。

PTY 管理器提供受限确认操作：输入一组应用会话 ID，只向其中 `awaiting_workspace_trust=true` 的会话写入 `1\r`，随后原子地清除状态，并返回实际确认数量。普通 PTY 输入接口不承担这一职责。

### 3.2 前后端数据流

后端总控状态保存最近一次由总控创建的批次 ID。总控通过新的 `control_workspace_trust_status()` 命令读取当前批次中等待信任、尚未出现提示和已经结束的数量；命令直接查询 PTY 实时状态，不改变现有高频终端输出轮询的数据结构。

总控完成同一条启动计划中的所有 `launch_multi` 调用后，将成功创建的会话 ID 注册为一个总控启动批次。按下空输入框的 `Enter` 时调用新的受限命令 `confirm_control_workspace_trust()`；后端读取当前批次并再次校验 PTY 实时状态，避免前端状态过期导致按键误发。

独立总控窗口与停靠总控通过 Tauri 后端共享当前批次；该状态仅驻留内存，不写入会话记录或总控历史。

### 3.3 启动计划

`ControlLaunch` 增加 `mode`：

```text
default | auto | yolo
```

Claude Code 结构化输出 schema 要求每个启动项返回 `mode`。系统提示明确模式映射、作用范围和互斥规则。计划标准化阶段对未知模式回退为 Default，并继续执行现有 Provider 白名单和最大会话数校验。

前端执行启动计划时映射为：

| mode | autoMode | yolo |
|---|---:|---:|
| default | false | false |
| auto | true | false |
| yolo | false | true |

现有 `launch_multi` 的互斥校验继续作为后端最后防线。

## 4. 安全边界

- 不向“等待工作区信任”之外的终端广播 `Enter`。
- 不自动接受 Claude Code 的权限请求、计划确认、退出确认或其他交互提示。
- 不把 YOLO 视为工作区信任授权；YOLO 会话同样进入统一批次确认流程。
- 批次确认只作用于总控本次新建的会话，不影响用户手动打开或更早创建的会话。
- 批次中部分会话尚未显示信任页时，保留这些会话的批次归属；后续检测到提示后仍可再次按一次 `Enter` 确认剩余会话。

## 5. 异常处理

- 目标数量为零时不拦截 `Enter`。
- 会话已退出或 PTY 已释放时跳过，并返回实际确认数量。
- 部分写入失败时返回成功数与失败会话，前端提示“已确认 X 个，Y 个未完成”。
- 模式解析不明确时使用 Default，并在回复中明确说明。
- 同一启动项同时被解释为 Auto 和 YOLO 时，计划标准化为 Default，并返回不可同时指定的提示，不静默选择高权限模式。

## 6. 测试与验收

1. 三个会话均显示工作区信任页时，总控空输入框按一次 `Enter`，三个会话全部进入 Claude Code 对话。
2. 两个会话等待信任、一个会话已进入对话时，只向前两个会话发送 `1\r`。
3. 终端输出普通文本包含 `trust` 和 `folder` 时，不标记为等待信任。
4. 总控输入框有内容时按 `Enter`，发送总控消息，不执行批量信任。
5. 总控启动指令指定 Auto 后，调用 `launch_multi(autoMode=true, yolo=false)`。
6. 总控启动指令指定 YOLO 后，调用 `launch_multi(autoMode=false, yolo=true)`。
7. 一条指令中的不同 Provider 可以分别使用 Default、Auto 和 YOLO。
8. Auto 与 YOLO 冲突时不以 YOLO 启动。
9. 手动多开、恢复会话、终端会话、广播 Prompt 和统一 Mode 功能保持原有行为。
