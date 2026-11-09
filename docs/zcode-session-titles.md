# ZCode 会话简短标题接入

本次先接入 ZCode Desktop / ZCode CLI 共用的原生会话链路。标题由 ZCode 自己生成，CrewVela 接收权威会话元数据并保存到已有会话记录，不新增模型配置或摘要服务。会话列表沿用已有 `zcode-session-update` → `onSessionRecord` 更新链路；未修改 CLI 布局或 CSS。

## 源码分析

参考本机 `REFERENCE_ROOT/ZCode` 的以下实现：

- `apps/zcode-cli/packages/core/src/runtime/methods/session-title.ts`：交互主会话首轮输入触发一次后台标题任务；普通输入至少 10 个字符；子会话不生成。保存前重新读取会话，检查用户自定义名称及首条消息编辑，避免旧请求写回。
- 同目录 `title-generation-sidecar.ts`：复用当前会话模型，用用户主要语言概括任务；输入最多 1200 字符，标题最多 100 字符，默认超时 60 秒。后台异常被记录，不中止主消息执行。
- `apps/zcode-cli/packages/bootstrap/src/zcode-protocol/server-operations.ts`：`session/create.titleGenerationEnabled` 控制运行时配置；未显式关闭时启用原生标题功能。
- `apps/zcode-cli/packages/bootstrap/src/zcode-protocol-v4/product-projection.ts`：`SessionTitleUpdated` 映射到 `state.updated.patch.meta = {title, titleSource}`；快照同样包含 `meta`。v4 来源为 `default/generated/custom`；上游 `first_input` 也归入 `generated`，客户端不能仅凭该字段区分首条输入兜底与模型摘要。

## 接入规则

- 真实主会话创建开启 `titleGenerationEnabled: true`；恢复时只用于读取模型目录的无消息临时草稿继续关闭标题生成。
- 读取主会话快照与增量的 `meta.title/titleSource`，局部 patch 保留未提供的字段；完整快照清除上一份元数据。
- 只接收来源 `generated/custom` 的非空、无控制字符标题。默认、空值、未知来源不修改当前名称。上游原生首条输入兜底保留其 v4 兼容语义，不自行推断摘要是否完成。
- 复用 `SessionManager.apply_summary_title` 保存；本地 `title_locked` 始终优先，原生自定义名称不自动设置本地锁。同名更新不重复写盘；保存失败回滚内存记录，保留原名称。
- 初始恢复和工作线程每轮同步一次观察到的新标题，并发布更新后的会话记录；覆盖命令内部已经消费增量、外层轮询暂无新帧的情况。
- 标题保存失败记录固定诊断日志，不终止聊天，不在每次轮询重复尝试；新标题或重新连接时可再次尝试。
- 只从主投影读取名称。子会话、辅助会话和退订后的旧流由现有订阅身份校验隔离；水位回退在修改投影前拒绝。

## 范围与验证边界

既有历史会话恢复时若 ZCode 已保存标题，会同步回来；没有原生摘要的旧会话不会自动重新调用模型补生成。其他 Agent 暂未接入这一原生元数据同步机制。

Rust 回归覆盖主会话生成配置、快照/增量标题、局部元数据、空/default 值、旧水位/订阅拒绝、子会话隔离、本地手动锁、保存失败回滚与重新连接、会话记录跨实例保存。协议集成使用 `tests/fixtures/native-zcode-agent.mjs`，不是模型端到端验收。

验证命令：

```powershell
rtk cargo test --manifest-path src-tauri/Cargo.toml zcode_ --lib
rtk cargo test --manifest-path src-tauri/Cargo.toml apply_summary_title --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml
rtk cargo test --manifest-path src-tauri/Cargo.toml zcode_installed_control_surface_without_model_call --lib -- --ignored --exact multi_agent::zcode::tests::zcode_installed_control_surface_without_model_call
```

真实账号模型生成、原生桌面会话列表点击与视觉显示需单独验收，不能由 Mock 协议测试或前端构建代替。

2026-10-13 检查结果：ZCode Rust 回归 44 通过、9 忽略；摘要标题回归 1 通过；`cargo check` 0 错误、33 个既有警告；差异空白检查通过。额外单独执行的已安装 ZCode 无模型控制面测试 1 通过：在临时隔离目录导入历史、执行原生 `renameSession`，真实 v4 流返回 `meta.title` 与 `titleSource: custom`，随后关闭并跨进程恢复会话。该检查证明安装版标题元数据增量兼容，不证明模型摘要质量，也未断言恢复后的标题文本；标题跨实例保存和快照恢复由上述 Mock 回归覆盖。
