# 多会话网格分屏 设计文档

> CrewVela 支持一次开 N 个 claude，网格分屏同时可见
> 日期：2026-06-14

## 1. 目标

用户选一个 provider + 工作目录 + 数量 N（1-9），一次启动 N 个配置相同的 claude，主区以网格（`⌈√N⌉ × ⌈N/⌈√N⌉⌉`）分屏同时显示，每格独立交互。典型场景：同一模型/同一项目并行处理多个任务。

## 2. 非目标

- 不做每格独立 provider/cwd（本次全部相同）。
- 不做会话间通信或共享上下文。
- 不做拖拽调换格子顺序（关单格后留空，不重排）。
- 不做持久化（重启后网格清空）。

## 3. 架构：单 session → 多 session

### 后端 `pty.rs`

`PtyManager` 从单 session 重构为多 session：

```rust
pub struct PtyManager {
    sessions: Mutex<HashMap<u32, Session>>,
    next_id: AtomicU32,
}
```

`Session` 不变（master / writer / child / output / exit_code）。

### 前端

从单 `TerminalView` 改为：
- `MultiTerminal`：接收 `sessionIds: number[]`，按网格 CSS grid 排列，每格一个 `TerminalPane`（独立 xterm.js）。
- 单 `setInterval(30ms)` 调 `pty_poll_all`，按 session_id 分发数据到对应 xterm。

### 数据通道

复用已验证的轮询模式：`pty_poll_all()` 一次返回所有活跃 session 的增量（`HashMap<u32, PtyChunk>`），前端单定时器分发。比每 session 独立轮询高效，也避开之前 event/Channel 不可靠的问题。

## 4. 交互流程

1. 左侧 provider 列表点一个 provider。
2. 弹**数量输入框**（1-9，校验范围）。
3. 确认数量后弹**系统目录选择器**（Tauri dialog plugin）选工作目录。
4. 确认目录 → `invoke('launch_multi', { providerId, cwd, count })` → 返回 `Vec<u32>` session_ids。
5. App 把 session_ids 设入 state，主区切换为 `MultiTerminal` 网格渲染 N 格。

## 5. 后端 commands（都带 session_id）

| command | 签名 | 作用 |
|---|---|---|
| `launch_multi` | `(provider_id, cwd, count) -> Vec<u32>` | 校验 provider，建 count 个 session（各自 PTY + claude.exe spawn + reader 线程），返回 ids |
| `pty_poll_all` | `() -> HashMap<u32, PtyChunk>` | 遍历所有 session，drain 各自 output，返回 {id: {data, running, code}} |
| `pty_input` | `(session_id, bytes)` | 写指定 session 的 PTY |
| `pty_resize` | `(session_id, rows, cols)` | resize 指定 session |
| `pty_kill` | `(session_id)` | kill + 移除指定 session |
| `pty_kill_all` | `()` | kill + 清空所有 session |

`launch_multi` 内部复用现有的 `resolve_claude` + `build_env` + `validate_provider` + spawn 逻辑，只是建 count 个并各自分配 id。

## 6. 前端组件

- **数量输入弹窗**：`<input type="number" min=1 max=9>` + 确认/取消。
- **目录选择**：`@tauri-apps/plugin-dialog` 的 `open({ directory: true })`。
- **`MultiTerminal`**：props `{ sessionIds: number[] }`。按 `cols=⌈√N⌉, rows=⌈N/cols⌉` 设 CSS `grid-template-columns: repeat(cols, 1fr)`。渲染 N 个 `TerminalPane`。
- **`TerminalPane`**：props `{ sessionId }`。内部一个 xterm，`onData`/`onResize` 调对应 session_id 的 command。
- **顶栏**：「全部关闭」按钮（pty_kill_all + 清 state）。每格右上角 ×（pty_kill(id) + 从 sessionIds 移除该格，**网格不重排**，留空格）。

## 7. 关闭/管理

- 关单格：`pty_kill(id)` → 该格显示「已关闭」占位，sessionIds 不重排（避免输出跳到别的格）。剩余格子位置不变。
- 全部关：`pty_kill_all()` → sessionIds 清空 → 回 provider 列表空状态。

## 8. 网格布局规则

```
cols = ceil(sqrt(N))
rows = ceil(N / cols)
```

| N | cols×rows | 空格 |
|---|---|---|
| 1 | 1×1 | 0 |
| 2 | 2×1 | 0 |
| 3 | 2×2 | 1 |
| 4 | 2×2 | 0 |
| 5 | 3×2 | 1 |
| 6 | 3×2 | 0 |
| 7 | 3×3 | 2 |
| 8 | 3×3 | 1 |
| 9 | 3×3 | 0 |

## 9. 新增依赖

- 后端：`tauri-plugin-dialog`（目录选择）。
- 前端：`@tauri-apps/plugin-dialog`。
- capabilities：加 `dialog:allow-open`。

## 10. 测试

- 单测 `pty.rs`：
  - `open` 多次建独立 session，sessions map 大小递增，id 递增。
  - `kill(id)` 只移除指定 session。
  - `kill_all` 清空。
  - `poll_all` 返回所有活跃 session 的非空数据合并。
- 单测 `launcher.rs`：`launch_multi` 复用 `validate_provider`（空 base_url 的 custom 拒绝）。
- 手动：开 4 个 glm 网格，每格独立输入交互；关一格其余不动；全部关回空状态。

## 11. 实现简述（分阶段）

1. **P0 后端多 session**：重构 `PtyManager` 为 HashMap + next_id；`open` 返回 id 不再替换；加 `launch_multi`/`pty_poll_all`/`pty_input(id)`/`pty_resize(id)`/`pty_kill(id)`/`pty_kill_all`。单测。
2. **P1 dialog 依赖**：加 tauri-plugin-dialog（后端 + 前端 + capability）。
3. **P2 前端网格**：`MultiTerminal` + `TerminalPane` + 数量弹窗 + 目录选择 + poll_all 分发。
4. **P3 关闭管理**：单格 × + 全部关闭 + 留空不重排。
5. **P4 打磨**：错误处理、focus 切换、resize 各格、README 补多开说明。
