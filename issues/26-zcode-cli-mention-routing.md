Status: ready-for-human

# 修复 ZCode CLI 提及被错误路由到 Desktop

## 验收

- `@ZCodeCLI @Codex zcode和codex打个招呼` 仅选择 ZCode CLI 与 Codex，不选择 ZCode Desktop。
- 前端传递明确的 Runtime 群组身份；明确的实例和群组优先于正文推断，旧调用兼容。
- 复用已有 CLI 会话，支持无项目 CLI 成员时的运行时选择；保留具体实例、@All 和单向通信行为。
- 启动和投递失败反馈到当前对话，排队状态不得表示送达成功。
- 验证前后端回归、构建及实际界面交互；保留此前未提交改动。

## Comments

- 修改前独立生产函数复现：截图原句误选 Codex + ZCode Desktop；前端请求缺少 Runtime 身份。
- 已补齐 CLI 别名与明确 Runtime 身份传递；明确目标优先于正文，保留旧调用和具体实例，复用既有 CLI 会话。
- 后台失败按本次持久消息对应的准确任务 ID 反馈，不重放无关历史失败；立即失败沿用现有错误路径，排队不表示送达。
- 同步修正目标数量提示；轮询不擦除失败原因。本次追踪限当前窗口组件生命周期，尚未扩展跨重启追踪。
- Node 411 通过 / 1 跳过，Rust library 1038 通过 / 38 忽略；类型检查、前端构建、发布资源内嵌检查通过。
- CUA 实际运行真实组件与隔离 IPC，验证截图原句、具体 CLI 会话、两类失败及轮询保留。真实模型互发、账号和其他平台尚未重新验收。
- 按指定 `npm run tauri -- build --no-bundle` 打包成功，默认产物 `src-tauri/target/release/CrewVela.exe`；需退出旧进程后重开。未停止用户会话，未提交或推送。
- 详细证据：`.scratch/zcode-cli-routing/qa.md`。与修改前补丁比较，已有改动只在本次所需的 5 个源文件继续修改，其余原有改动保持一致。
