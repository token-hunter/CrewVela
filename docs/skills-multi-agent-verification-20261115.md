# Skills 多选 Agent 分发验证

日期：2026-11-15

## 修复

- 原分发页仅保存一个 runtimeId，切换 Runtime 会清空 Skill 勾选；现将 Agent 目标与 Skill 勾选分开保存，增减目标不会清空 Skill。
- 部署、配置集和批量撤下根据当前可见已选 Skill × 选中 Runtime 生成目标；没有 Agent 时禁用写操作。
- 多选时逐目标显示状态、路径和单项撤下入口。“已同步”要求选中目标全部同步；“未部署”包含任一缺失部署；“需要处理”包含任一异常部署。
- 复用现有 skill_center_deploy_batch / skill_center_undeploy_batch，不改变后端所有权、信任和冲突检查。

## 验证结果

- TypeScript 检查通过；分发选择/筛选、Skills P0 与版本更新检查共 14 项通过。
- Rust skill_center::tests 共 17 项通过。扩展既有真实临时目录测试：同一 Skill 一次部署至 Claude Code 和 Codex 两个项目目录，读取两份 SKILL.md 内容，重复部署不增加记录，批量撤下后中央副本保留；跨 Runtime 冲突预检拒绝整批且保留用户文件。
- IAB 实际 React 页面 + 隔离 IPC：双 Agent 同时选中；保留已选 Skill；部署参数包含两目标；配置集保存两目标；批量撤下两部署；清空目标禁止写操作；恢复目标保留 Skill；冲突显示“整批未写入”。
- 任务作用域中两个输入均携带同一准确 worktree/taskId；更换 taskId 后两个状态均变为未部署。取消未信任 Skill 确认后没有部署调用。方向键移动焦点不改变选中目标。
- 深浅色显示检查通过；五个目标可同时选中；687×903 窄屏分发区不横向溢出、底部操作可见，页面错误日志为空。
- 既有 tests/skillsRuntimeFirst.ui.mjs 更新为多目标回归场景。本轮交互通过 IAB 逐项执行，未单独运行该 Playwright CLI 脚本。

## 边界

没有对用户真实全局 Skill 目录执行部署或撤下。真实写入发生在独立临时项目目录；浏览器验证使用隔离数据。没有重新验收模型读取 Skill、macOS 原生界面或完整测试套件。

## Windows 交付

- `npm run tauri -- build --no-bundle` 成功，保留 50 条现有 Rust 警告。
- 输出：`output/CrewVela-skills-multi-agent-20261115/CrewVela.exe`，46,261,760 字节。
- SHA256：`C65F50D1708C96684923B779D70EFC4BE9560784D34A0AE757A18705EDB69180`。
- 复制哈希一致、`--help` 退出码 0、发布隐私扫描无发现。旧包和运行中的用户实例未覆盖。
