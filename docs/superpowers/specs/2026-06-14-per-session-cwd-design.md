# 每会话独立目录 设计文档

> 多开时每个会话窗口可选不同项目目录
> 日期：2026-06-14

## 1. 目标

多开 N 个 claude（同一 provider）时，每个会话窗口各自选一个项目目录，而非共享一个 cwd。典型场景：同时开 3 个项目目录的 claude 并行干活。

## 2. 非目标

- 不改 provider 配置本身（仍同 provider）。
- 不做运行中改某格目录（目录是启动时定的；要换就关了重开）。
- 不做目录历史/收藏（YAGNI，后续可加）。

## 3. 现状

当前多开流程（见 `multi-session-grid` spec）：provider → 数量 N → 选**一次**目录 → `launch_multi(providerId, cwd, count)`，N 个 session 共享同一 cwd。

## 4. 新流程

provider → 数量 N → **目录列表面板**（N 行各选）→ 开始 → 网格 N 个 claude（同 provider，各自目录）。

## 5. 新组件 `MultiDirPrompt`

选完数量后弹出（替换现有单次目录选择）：

- **N 行**，每行：一个「选目录」按钮 + 路径显示。空显示「home（默认）」。
- 顶部「**全部使用同一目录**」快捷按钮：点一次弹系统目录选择器，结果填满所有 N 行（想全同时一键搞定）。
- 每行可单独点「选目录」改自己的目录；再点清空回 home。
- 底部「**开始 N 个**」按钮 → 收集 N 个路径（空=home）调 `launch_multi`。
- 「取消」回 provider 列表。

## 6. 后端改动

`launch_multi` 签名：`cwd: String` → `cwds: Vec<String>`：

```rust
#[tauri::command]
fn launch_multi(
    provider_id: String,
    cwds: Vec<String>,
    count: u32,
    state: State<'_, PtyManager>,
) -> Result<Vec<u32>, String>
```

建第 i 个 session 时：`let cwd_i = cwds.get(i).map(|s| s.as_str()).filter(|s| !s.is_empty());`（None / 空串 → home）。即 cwds 长度 < count 时缺的位置用 home；含空串也用 home。不做硬性长度校验，按 index 取，缺则 home。

## 7. 前端改动

- `App.tsx`：
  - 删 `handleCountConfirm` 里现在的单次 `open({ directory })`。
  - `handleCountConfirm(count)` 改为 `setDirPromptFor({ provider: p, count })`，渲染 `MultiDirPrompt`。
  - `MultiDirPrompt` 的「开始 N 个」回调：`invoke("launch_multi", { providerId, cwds, count })` → 拿 ids → `setSessionIds`。
- 新增 `src/components/MultiDirPrompt.tsx`。

## 8. 测试

- 单测 `pty`/`launcher`：`launch_multi` 的 cwds 处理——抽一个纯函数 `pick_cwd(cwds: &[String], i: usize) -> Option<&str>`（空串/越界 → None），单测覆盖：正常、越界、空串。
- 手动：开 3 个 glm，3 行各选不同项目目录；3 个 claude 分别在 3 个目录启动（`pwd` 各不同）；「全部相同」快捷填满；某行留空 → 该格 home。

## 9. 实现简述

1. 后端 `launch_multi` 改 cwds: Vec<String>，抽 `pick_cwd` 纯函数 + 单测。
2. 前端 `MultiDirPrompt` 组件（N 行 + 全部相同快捷 + 各行选目录）。
3. `App.tsx` 接入 MultiDirPrompt，删单次 open。
4. 端到端验收 + README 补一句。
