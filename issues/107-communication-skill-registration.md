Status: ready-for-human

# 补齐六种 CLI 的 CrewVela Skill 与 ZCode 调用入口

## 验收

- 六种 CLI 的原生 Skill 目录覆盖完整，配置根及用户自定义内容保留。
- ZCode CLI 可发现 Skill，并提供 `/crewvela` 命令入口。
- 协作诊断可检查 ZCode CLI，回归测试通过。
- 不调用模型、不发送测试消息、不修改登录或权限。

## Comments

- 范围和起始证据见 `.scratch/communication-skill-20261116/spec.md`。
- 已补齐六种原生用户目录及共享目录，保留用户自定义内容；ZCode 注册原生 `/crewvela` 命令，协作诊断支持 ZCode CLI。
- 6 项通信指南同步测试、8 项协作诊断/交接回归、17 项 Skill Center 回归通过；六种原生发现接口或加载器均识别 crewvela，ZCode 原生命令列表、指南读取和帮助通过。
- 本机配置已应用；应用自动同步的源码修复尚未打包到 EXE，现有进程需重启或恢复原会话刷新发现。未调用模型、未发送消息，不能把发现成功视作模型已调用或消息已送达。
- 详细证据见 `.scratch/communication-skill-20261116/verification.md`。
