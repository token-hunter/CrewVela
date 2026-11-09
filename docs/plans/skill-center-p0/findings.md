# 发现：Skill 中心 P0

## 当前事实
- 仓库存在未提交的 Git P0 工作台变更，本任务必须在其上增量开发并保持边界清晰。
- 当前 CLI Runtime 包括：Claude Code、Codex、Kimi Code、Pi、OpenCode、Hermes。
- 当前 Cargo 没有 SHA-256 与 ZIP 依赖；P0 若完整支持内容 Hash 和 `.zip/.skill`，需要新增小型依赖。

## 模块设计方向
- `SkillCenter` 是深模块：React 和 Tauri 只学习资产、部署、配置集与批量操作接口；路径发现、所有权和 materialization 模式全部留在 Rust 实现内部。
- Runtime 身份只使用现有 `AgentRuntimeKind`，不复制 skills-manager 的 Tool registry。
- MCP 注入与 Skill 文件部署保持不同模块，不共享“启用”状态。

## 参考实现中确认的安全边界
- 目标必须区分为：不存在、指向中央源的链接、外部链接、真实目录、真实文件；只有能够证明为 CrewVela 管理的目标才允许覆盖。
- 批量分发必须先做全量 preflight，再开始写文件，避免部分成功、部分冲突。
- Windows 按目录符号链接 → Junction → Copy 降级；Copy 必须依赖数据库记录、部署 Hash 和所有权标记，不能只凭目录存在判断归属。
- 中央源与目标目录重叠、目标位于源目录内部、源位于目标内部都必须拒绝。
- Codex、Pi 的 `~/.agents/skills` 只用于发现，不作为部署根；OpenCode 使用不对称的全局/项目路径；Hermes 必须递归发现。

## 最终落地
- 中央库位于 CrewVela 配置目录的 `skill-center/`，SQLite 只保存元数据、部署所有权和配置集；Skill 内容保存在 `assets/`。
- 资产以 SHA-256 识别，Git 导入记录实际检出的 Commit；ZIP/.skill 有路径穿越、条目数、单文件和总解压体积限制。
- 全局部署写入 Runtime 用户级根；项目部署写入项目根；任务/Agent 部署强制使用独立 Worktree 绝对路径，避免“只记录任务标签、实际却让整个项目可见”。
- 部署身份使用 Skill、Runtime、作用域、工作目录、任务和 Agent 组成的唯一键，重复操作只刷新同一记录。
- 批量部署与撤下均先检查所有目标；外部目录、外部链接、修改后的 Copy 副本会使整批操作终止。
- 配置集支持项目与 Agent 角色绑定；项目协作页根据当前 Runtime 和角色显示匹配建议，但不会静默信任未审阅 Skill。
