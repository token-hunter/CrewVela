Status: ready-for-human

# 识别讨论真实失败并预检接管成员

见 `.scratch/discussion-runtime-diagnosis-20261116/spec.md`。以真实记录复现，补充 Codex 结构化失败和手动接管预检回归；不更改用户配置与历史。

## Comments

已接入 Codex ACP 1.11 的 version=1/sessionFailure 协商；只读取终态响应中的结构化 severity=error，不扫描回复文字。保留服务错误码和详情。手动接管检查在 begin_conversation 事务内、幂等回执检查之后完成，拒绝时无消息、轮次和任务写入。界面默认排除被接管成员，菜单标注原因，明确 @ 时阻止发送并保留草稿。

真实只读证据：项目“模型设置实测-20261115”最新轮次中 Codex 返回 400/model_not_found，却存为 succeeded；第三成员“语义复核员”处于 manual，轮次在执行到此成员时结束。项目目标及 Claude 角色本身只允许模型验收，未修改这些用户配置。

验证：先复现失败，再通过隔离 ACP 子进程回归（真实 JSON-RPC 通路）、SQLite 讨论模块 38 项、ACP 模块 29 项（10 项环境依赖测试保持忽略）、讨论可用成员暗/亮 UI、原讨论/清空/7 Chat UI、前端构建及 diff 检查。未调用真实模型；用户数据库只读，旧错误历史未改写。详情见 `.scratch/discussion-runtime-diagnosis-20261116/`。
