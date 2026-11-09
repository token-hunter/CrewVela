# 总控批量信任与启动模式实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 总控批量创建 Claude Code 会话后，用户可按一次 Enter 安全确认本批次全部工作区信任提示，并可在自然语言启动指令中为每组会话指定 Default、Auto 或 YOLO。

**Architecture:** PTY 层使用独立检测器识别完整工作区信任提示，并保存每个 PTY 的受限等待状态；总控后端保存最近一次总控启动批次，只允许确认该批次中确实处于等待状态的 PTY。启动模式作为 `ControlLaunch` 的强类型字段贯穿 Claude 结构化计划、Rust 标准化和 React 执行层。

**Tech Stack:** Rust 1.77、Tauri 2、portable-pty、React 18、TypeScript 5.6、Vite 5、Cargo 内置测试。

## Global Constraints

- 一次统一确认只发送给当前总控启动批次中实时处于工作区信任页的 Claude 会话。
- 工作区信任检测必须同时匹配 `Accessing workspace`、`Yes, I trust this folder`、`Enter to confirm`，并忽略 ANSI 控制序列和大小写。
- 总控输入框非空时，Enter 保持发送总控消息的现有行为。
- 启动模式仅允许 `default | auto | yolo`；Auto 与 YOLO 互斥。
- YOLO 不再自动确认工作区信任，必须进入与其他模式相同的批次确认流程。
- 不修改手动多开、恢复会话、普通终端、广播 Prompt 和统一 Mode 的行为。
- 不提交现有 `src/components/ControlSidebar.tsx` 中与本功能无关的建议文案改动。

---

### Task 1: 为总控启动计划增加强类型模式

**Files:**
- Modify: `src-tauri/src/control.rs`
- Modify: `src/types.ts`
- Modify: `src/components/ControlSidebar.tsx`

**Interfaces:**
- Produces: Rust `ControlLaunchMode::{Default, Auto, Yolo}`。
- Produces: `ControlLaunch { provider_id: String, count: u32, mode: ControlLaunchMode }`。
- Produces: TypeScript `ControlLaunchMode = "default" | "auto" | "yolo"`。
- Consumes: 现有 Tauri `launch_multi(providerId, cwds, count, autoMode, yolo)`。

- [ ] **Step 1: 编写失败的 Rust 解析测试**

在 `control.rs` 测试模块中把结构化输出样例扩展为：

```rust
{"providerId": "deepseek", "count": 1, "mode": "auto"},
{"providerId": "glm", "count": 1, "mode": "yolo"},
{"providerId": "kimi", "count": 1, "mode": "default"}
```

并断言：

```rust
assert_eq!(plan.launches[0].mode, ControlLaunchMode::Auto);
assert_eq!(plan.launches[1].mode, ControlLaunchMode::Yolo);
assert_eq!(plan.launches[2].mode, ControlLaunchMode::Default);
```

再增加兼容性测试，确认旧计划缺少 `mode` 时反序列化为 `Default`。

- [ ] **Step 2: 运行定向测试并确认按预期失败**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml claude_structured_output_preserves_launch_modes
```

Expected: FAIL，原因是 `ControlLaunchMode` 或 `ControlLaunch.mode` 尚不存在。

- [ ] **Step 3: 实现 Rust 模式类型和结构化输出约束**

在 `control.rs` 增加：

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlLaunchMode {
    #[default]
    Default,
    Auto,
    Yolo,
}
```

给 `ControlLaunch.mode` 添加 `#[serde(default)]`。更新 Claude Code 系统提示，明确模式映射、单个启动项作用域和冲突时回退 Default；更新 JSON schema，使 `mode` 的枚举为 `["default", "auto", "yolo"]` 且列入启动项 `required`。

- [ ] **Step 4: 运行 Rust 测试并确认通过**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml control::tests
```

Expected: control 模块全部通过。

- [ ] **Step 5: 连接 TypeScript 执行层**

在 `src/types.ts` 增加：

```ts
export type ControlLaunchMode = "default" | "auto" | "yolo";
```

并给 `ControlLaunch` 增加 `mode`。在 `ControlSidebar.executePlan` 中把每个启动项映射为：

```ts
const autoMode = launch.mode === "auto";
const yolo = launch.mode === "yolo";
```

调用 `launch_multi` 时传入这两个值；成功结果保留 `mode`，显示 `Provider ×N（Auto）`、`Provider ×N（YOLO）` 或 `Provider ×N（Default）`。

- [ ] **Step 6: 运行前端编译检查**

Run:

```powershell
npm run build
```

Expected: TypeScript 和 Vite 构建成功。

- [ ] **Step 7: 提交启动模式改动**

```powershell
git add src-tauri/src/control.rs src/types.ts
git add -p src/components/ControlSidebar.tsx
git commit -m "feat(control): support launch modes"
```

交互式暂存 `ControlSidebar.tsx` 时只选择启动模式相关 hunk，不选择任务开始前已经存在的建议文案 hunk。

---

### Task 2: 精确识别工作区信任提示

**Files:**
- Create: `src-tauri/src/workspace_trust.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/pty.rs`

**Interfaces:**
- Produces: `WorkspaceTrustDetector::push(&mut self, bytes: &[u8]) -> bool`。
- Produces: `PtyManager::workspace_trust_state(app_session_id: &str) -> WorkspaceTrustState`。
- Produces: `PtyManager::confirm_workspace_trust(app_session_ids: &HashSet<String>) -> WorkspaceTrustConfirmation`。

- [ ] **Step 1: 编写检测器失败测试**

在新文件 `workspace_trust.rs` 的测试模块覆盖：

```rust
#[test]
fn detects_fragmented_workspace_trust_prompt() {
    let mut detector = WorkspaceTrustDetector::default();
    assert!(!detector.push(b"\x1b[33mAccessing work"));
    assert!(!detector.push(b"space:\x1b[0m\r\n1. Yes, I trust "));
    assert!(detector.push(b"this folder\r\nEnter to confirm"));
}

#[test]
fn ignores_unrelated_trust_and_folder_text() {
    let mut detector = WorkspaceTrustDetector::default();
    assert!(!detector.push(
        b"I trust the folder layout, but this is ordinary assistant output"
    ));
}
```

再增加大小写差异和缓冲区超过 16 KiB 时仍保持上限的测试。

- [ ] **Step 2: 运行检测器测试并确认按预期失败**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml workspace_trust
```

Expected: FAIL，原因是模块和检测器尚不存在。

- [ ] **Step 3: 实现有界 ANSI 清理检测器**

实现 `WorkspaceTrustDetector`：

- 按字节状态机删除 CSI ANSI 序列；
- 追加小写后的可见文本；
- 缓冲区最多保留末尾 16 KiB；
- 同时包含三个完整特征时只返回一次 `true`。

在 `lib.rs` 声明 `mod workspace_trust;`。

- [ ] **Step 4: 运行检测器测试并确认通过**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml workspace_trust
```

Expected: 检测器测试全部通过。

- [ ] **Step 5: 编写 PTY 状态失败测试**

把 PTY 信任状态操作拆成可用内存 writer 测试的内部函数，测试以下行为：

```rust
assert_eq!(confirmation.confirmed_count, 2);
assert_eq!(confirmation.skipped_count, 1);
assert_eq!(writer_a.bytes(), b"1\r");
assert_eq!(writer_b.bytes(), b"1\r");
assert!(writer_active.bytes().is_empty());
```

并断言确认后两个等待标记都已清除，重复确认不会再次写入。

- [ ] **Step 6: 运行 PTY 状态测试并确认按预期失败**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml pty::tests::workspace_trust
```

Expected: FAIL，原因是 PTY 会话还没有等待状态和受限确认接口。

- [ ] **Step 7: 接入 PTY 会话状态**

在 `Session` 增加共享的 `awaiting_workspace_trust: Arc<AtomicBool>`；reader 线程使用检测器设置该状态。删除 `spawn_command` 的 `auto_trust` 参数和 YOLO 专属的宽泛 `trust + folder` 自动写入逻辑，使 Claude 与本地终端都使用无副作用的 `spawn_command`。

实现：

```rust
pub enum WorkspaceTrustState {
    Awaiting,
    Starting,
    Missing,
}

pub struct WorkspaceTrustConfirmation {
    pub confirmed_count: usize,
    pub skipped_count: usize,
    pub failed_session_ids: Vec<String>,
}
```

`confirm_workspace_trust` 必须先以 `compare_exchange(true, false, ...)` 取得确认权，再写入 `1\r`；写入失败时把状态恢复为 `true`。

- [ ] **Step 8: 运行 PTY 与完整 Rust 测试**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 全部 Rust 测试通过，原有 Auto/YOLO CLI 参数测试保持通过。

- [ ] **Step 9: 提交 PTY 信任检测改动**

```powershell
git add src-tauri/src/workspace_trust.rs src-tauri/src/lib.rs src-tauri/src/pty.rs
git commit -m "feat(pty): detect workspace trust prompts"
```

---

### Task 3: 建立总控启动批次和受限确认命令

**Files:**
- Modify: `src-tauri/src/control.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types.ts`

**Interfaces:**
- Produces: `ControlWorkspaceTrustBatch::replace(Vec<String>)`。
- Produces: `ControlWorkspaceTrustBatch::session_ids() -> Vec<String>`。
- Produces Tauri commands:
  - `register_control_launch_batch(session_ids: Vec<String>) -> ControlWorkspaceTrustStatus`
  - `control_workspace_trust_status() -> ControlWorkspaceTrustStatus`
  - `confirm_control_workspace_trust() -> ControlWorkspaceTrustReceipt`

- [ ] **Step 1: 编写总控批次失败测试**

在 `control.rs` 测试模块增加：

```rust
#[test]
fn replacing_control_launch_batch_discards_the_previous_batch() {
    let batch = ControlWorkspaceTrustBatch::new();
    batch.replace(vec!["old".into()]);
    batch.replace(vec!["new-a".into(), "new-b".into(), "new-a".into()]);
    assert_eq!(batch.session_ids(), vec!["new-a", "new-b"]);
}
```

再测试空批次会清除旧批次。

- [ ] **Step 2: 运行批次测试并确认按预期失败**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml replacing_control_launch_batch
```

Expected: FAIL，原因是批次状态尚不存在。

- [ ] **Step 3: 实现内存批次状态和回执类型**

使用 `Mutex<Vec<String>>` 实现去重且保持启动顺序的批次状态，增加：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlWorkspaceTrustStatus {
    pub batch_count: usize,
    pub awaiting_count: usize,
    pub starting_count: usize,
    pub missing_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlWorkspaceTrustReceipt {
    pub confirmed_count: usize,
    pub skipped_count: usize,
    pub failed_session_ids: Vec<String>,
}
```

- [ ] **Step 4: 实现并注册三个 Tauri 命令**

`register_control_launch_batch` 只接受前端传回的成功启动会话 ID，并替换旧批次。`control_workspace_trust_status` 使用批次 ID 查询 `PtyManager`。`confirm_control_workspace_trust` 只把当前批次传给 PTY 受限确认接口。

在 Tauri builder 中 `.manage(ControlWorkspaceTrustBatch::new())`，并将三个命令加入 `generate_handler!`。

- [ ] **Step 5: 运行 Rust 全量测试**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 全部 Rust 测试通过。

- [ ] **Step 6: 增加前端回执类型**

在 `src/types.ts` 增加与 Rust camelCase 输出一致的 `ControlWorkspaceTrustStatus` 和 `ControlWorkspaceTrustReceipt`，字段必须与上一步完全一致。

- [ ] **Step 7: 提交总控批次状态改动**

```powershell
git add src-tauri/src/control.rs src-tauri/src/lib.rs src/types.ts
git commit -m "feat(control): track workspace trust batches"
```

---

### Task 4: 在总控中完成一次 Enter 的确认交互

**Files:**
- Modify: `src/components/ControlSidebar.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: Task 3 的三个 Tauri 命令。
- Consumes: `ControlWorkspaceTrustStatus`、`ControlWorkspaceTrustReceipt`。

- [ ] **Step 1: 注册每次总控启动批次**

`executePlan` 汇总所有 fulfilled 启动结果中的 `SessionRecord.id`。所有 provider 启动完成后调用：

```ts
await invoke("register_control_launch_batch", {
  sessionIds: launchedSessionIds,
});
```

只注册本条总控计划成功创建的会话；全部失败时注册空数组清除旧批次。

- [ ] **Step 2: 轮询当前批次状态**

总控打开时以 250ms 间隔调用 `control_workspace_trust_status`，组件关闭时清理 timer。只有 `awaitingCount > 0` 时展示：

```text
N 个新会话正在等待信任此目录
按 Enter 统一确认
```

`startingCount > 0` 时补充“另有 N 个会话正在启动”，避免用户误以为所有会话都已出现提示。

- [ ] **Step 3: 接入空输入框 Enter**

在 `handleKeyDown` 的普通 Enter 分支中，先判断：

```ts
const canConfirmWorkspaceTrust =
  draft.trim().length === 0 &&
  trustStatus.awaitingCount > 0 &&
  !busy &&
  !trustBusy;
```

满足时阻止默认行为并调用 `confirm_control_workspace_trust`；否则保持现有 `submit(draft)`。确认结束后立即刷新状态，并追加：

```text
已统一确认 X 个会话的工作区信任。
```

如果有失败 ID，则显示成功数和未完成数，不删除批次。

- [ ] **Step 4: 增加可访问的批次提示样式**

在 `styles.css` 增加浅色/深色主题均可辨识的 `.control-trust-notice`，使用 `role="status"` 和 `aria-live="polite"`。提示放在建议命令列表与输入框之间，不覆盖消息区或发送按钮。

- [ ] **Step 5: 运行前端构建**

Run:

```powershell
npm run build
```

Expected: TypeScript 与 Vite 构建成功，无类型错误。

- [ ] **Step 6: 运行完整验证**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

Expected: Rust 测试全通过、前端生产构建成功、diff 无空白错误。

- [ ] **Step 7: 手工验收桌面交互**

启动开发版后依次验证：

1. 总控输入“打开 1 个 DeepSeek、1 个 GLM、1 个 Kimi”，三个信任页出现后，空输入框按一次 Enter，三个会话进入对话。
2. 输入框有文字时按 Enter，只发送总控消息。
3. 输入“打开 1 个 GLM Auto、1 个 Kimi YOLO”，终端启动参数分别生效，结果文案显示模式。
4. 手动打开的会话停留在信任页时，总控统一确认不影响它。
5. 停靠与分离总控窗口都能读取同一批次并执行确认。

- [ ] **Step 8: 提交统一确认交互**

```powershell
git add -p src/components/ControlSidebar.tsx
git add src/styles.css
git commit -m "feat(control): confirm batch workspace trust"
```

交互式暂存时继续排除任务开始前已经存在的建议文案 hunk。
