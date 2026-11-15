# 模型设置正常模式实测（2026-11-15）

后续已修复本记录发现的 Codex 两项异常，并完成正常模式连续两轮复验；最新结果见 `docs/codex-delivery-hook-verification-20261115.md`。下文保留首次核验的历史结果。

结论：正常 Windows EXE 中，从模型分配表保存 Agent 默认值后，新建的 Claude Code、Codex 团队 CLI 会话使用了保存的模型参数，并完成真实模型请求。Codex 的协作自动投递及完成 Hook 有异常，单独记录，未计作通过。

## 环境与范围

- EXE：`output/CrewVela-model-settings-20261115/CrewVela.exe`，正常启动，无 `--test-data-dir`，非浏览器 Mock。
- Claude Code 2.1.319；Codex CLI 0.167.1。跳过 Codex 更新提示，没有安装新版本。
- 独立项目：`模型设置实测-20261115`，ID `684db442-fb6f-4df8-8d3e-a769c086916c`。
- 目录：`C:\Users\PC\AppData\Local\Temp\crewvela-model-live-e79907a4ca`；项目仅作回复标记验收，不承载业务代码。
- 创建 Claude Code、Codex 两名执行成员，成员模型字段均为 `null`，验证 Agent 默认值继承到新启动团队 CLI 的路径。本次没有验证运行中热切换或所有其它适配器。

## 保存与执行证据

从正式模型设置界面保存，再读取 `runtime-model-defaults.json` 确认：

| Runtime | 保存连接 | 保存模型 | 原生执行证据 | 结果 |
| --- | --- | --- | --- | --- |
| Claude Code | 现有 `glm` 连接 | `glm-5.3-flash[1m]` | 进程 `--model`、应用启动记录，以及真实 assistant 响应的 `message.model = glm-5.3-flash` | 返回 `MODEL_LIVE_afc0c443ac` |
| Codex | 本机 OpenAI 配置 | `gpt-6.1-terra` | 进程 `--model`、原生 `turn_context.model = gpt-6.1-terra`，并有 `task_complete` | 返回 `MODEL_LIVE_CODEX_afc0c443ac` |

Codex 本机配置在验收前后仍是 `gpt-6.1-sol`。执行记录却是 `gpt-6.1-terra`，因此没有退回本机默认值。原生会话元数据的 `model_provider` 为 `openai`，完成事件记录了真实回复，耗时 4932 ms。

- Claude 应用会话：`427b5790-9dea-4441-a947-4e04dd9d10b9`；原生会话：`555345f6-5a2d-40ea-a6a1-9971e9dcfb8f`。
- Codex 应用会话：`bbecacab-f47e-4c28-9ae8-aa11803116a8`；原生会话：`01a1dd0e-22b6-7ba1-9611-8de2fb32340d`。
- 原生记录按准确会话 ID、唯一项目目录匹配，未取“最新会话”代替证据，未依据模型自报身份判断。

## 未通过项

通过工作台 `@All` 向两名成员发送请求后，Claude 自动处理了请求。Codex 启动时停在更新提示；跳过后虽已显示选定的 Terra 模型，排队消息仍未自动进入输入区。因此，在**同一个应用创建、已携带保存参数的 Codex 进程**里，人工提交单独的只读请求，获取真实回复；没有另开 CLI 冒充成功。

Codex 回复后显示 `Hook failed` / `hook exited with code 1`。不能据此将协作投递与完成回调判作通过。根因尚未确定，见 `issues/74-codex-live-delivery-hook.md`。

Claude 的协作包装提示它使用通信工具，实际调用了 ToolSearch、Skill 和两次 PowerShell；其中产生一条指向本次 Codex 成员的标记消息。没有将“不要调用工具”的约束算作通过，也不把本次当作严格无工具验收。业务目录最终仍为空。

## 恢复与留档

- 从界面对两个 Runtime 执行「恢复继承」，配置最终为 `{"runtimes":{}}`，恢复测试前无默认覆盖的含义；Codex 本机配置没有改写。
- 两条测试会话均已停止、PTY 解绑，待发消息为零。
- 测试项目已暂停，记录保留；原有 79 名成员的 ID、名称、Runtime、模型逐项对比未变。
- 测试目录文件数为零。没有修改产品代码、安装依赖或升级 CLI。

以下证据保存在 `.scratch/model-assignments/normal-live/`，响应摘录不含凭据、系统提示或思维内容：

- `defaults-saved.json`：界面保存后的实际配置。
- `sessions-evidence.json`：两条会话的启动模型记录。
- `claude-response-evidence.json`：Claude 真实响应模型、文本及工具名称摘录。
- `codex-response-evidence.json`：Codex 准确原生会话、模型上下文、完成事件和回复摘录。
- `native-real-responses.png`：原生双终端结果及 Codex Hook 错误。
- `defaults-restored.png`、`cleanup-evidence.json`、`verification-summary.json`：恢复和核验结果。
