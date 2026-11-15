# Codex 正常模式自动投递与完成 Hook 异常

Status: ready-for-human

## 复现证据

2026-11-15 在正常 Windows EXE、Codex CLI 0.167.1 中，项目 `模型设置实测-20261115` 的成员通过 `@All` 启动：

1. 模型设置保存 `gpt-6.1-terra`，Codex 进程参数及原生界面均使用该模型。
2. CLI 显示更新提示，手工选择 Skip 后进入可输入状态；原有协作消息没有自动进入输入区。
3. 在同一 CLI 会话人工提交请求，真实回复成功；`turn_context.model` 为 Terra，有 `task_complete`。
4. 回复后界面出现 `Hook failed`、`hook exited with code 1`。

原生会话 `01a1dd0e-22b6-7ba1-9611-8de2fb32340d`；应用会话 `bbecacab-f47e-4c28-9ae8-aa11803116a8`。截图及去除凭据的摘录见 `.scratch/model-assignments/normal-live/`；完整过程见 `docs/model-assignments-live-verification-20261115.md`。

## 待定位

- 更新提示结束后的 readiness 是否重新识别；不是把“已排队”当作“已执行”。
- 找到准确失败 Hook、调用参数、退出码和错误输出，区分 CrewVela 注入与用户已有 Hook；目前未确定根因，也未确认两项异常是否同源。
- 修复后使用原会话链路验证自动投递、唯一提交与完成回流，保留模型参数及权限边界。

## Comments

- 当前用户请求是验证模型设置，本次仅留档，没有修改产品代码或放开保护规则。模型请求成功不抵消这两项回归问题。
- 验证结束后两个测试会话已停止、待发消息为零，默认配置恢复，项目已暂停。

## 修复与复验结果（2026-11-15）

- 原生 idle 无法解除残留更新菜单阻塞是自动投递根因；新增 Update 状态，并保留登录/权限对话框保护，识别增量更新菜单。
- Hook 原生事件定位到本机 Impeccable 4.3.1 的 CMD/PowerShell 语法不兼容。已备份修复、保留启用状态，并通过原生完整插件指纹重新审核；工具默认保留现代内容信任字段。
- 正常 EXE 连续两轮通过：工作台发送后自动提交、Terra 真实回复、完成状态回流、队列清零；未在终端补打提示词/Enter。启动更新菜单仅使用 Escape 跳过。
- 两轮原生会话一致，两个唯一标记各有一次完成事件；原生诊断请求 10 个 Hook 全部完成，正常界面未再出现 Hook failed。
- Rust 1131 通过/49 忽略、Node 495 通过/1 跳过；安装版启动就绪测试已额外通过。配置与偏好已恢复，测试会话停止、目录为空，项目完成。
- 交付与实测 EXE 哈希一致：`output/CrewVela-codex-fix-20261115/CrewVela.exe`。完整证据和适用边界见 `docs/codex-delivery-hook-verification-20261115.md`。
