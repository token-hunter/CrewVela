# 多 provider 累加并存 设计文档

> 不同 provider 的会话在同一网格并存，切换不互斥
> 日期：2026-06-14

## 1. 目标

点不同 provider 多开时，会话**累加**到网格（glm 的格子保留，再开 deepseek 追加），而非替换。pane 头显示 provider 名以区分。

## 2. 背景（当前 bug）

`App.tsx` 的 `handleDirsConfirm` 用 `setSessionIds(ids)` **替换**：开 deepseek 时 glm 的 sessionIds 被覆盖，glm 格子消失，但 glm 的 claude 进程没被 kill（孤儿进程）。用户体感"开了 B 关了 A"。

## 3. 改动（纯前端，后端不变）

### 数据结构
`sessionIds: number[]` → `sessions: { id: number; providerName: string }[]`。

### 累加逻辑
`handleDirsConfirm`：
```ts
const ids = await invoke<number[]>("launch_multi", { providerId, cwds, count });
setSessions(prev => [...prev, ...ids.map(id => ({ id, providerName: ctx.provider.name }))]);
```

### 组件链路
- `App` → `MultiTerminal sessions={sessions} onClose={closePane}`。
- `MultiTerminal` 按 `sessions.length` 算列数，每项渲染 `TerminalPane key={id} sessionId={id} providerName={name}`。
- `TerminalPane` pane 头：`#{id} · {providerName}`。

### 关闭
- `closePane(id)`：`pty_kill(id)` + `setSessions(filter id)`。
- `closeAll`：`pty_kill_all` + `setSessions([])`。

## 4. 不改动

- 后端 `launch_multi` / `pty_poll_all` / `pty_input` / `pty_resize` / `pty_kill` / `pty_kill_all` 全部不变。
- 多开的数量/目录选择流程不变。

## 5. 测试

- 手动：开 glm×2 → 网格 2 格；点 deepseek 多开 1 → 网格 3 格（glm/glm/DeepSeek），pane 头各显示 provider 名；关中间格其余不动；全部关清空，无孤儿进程。
- tsc 类型检查（sessions 结构变更波及 App/MultiTerminal/TerminalPane）。

## 6. 实现简述

1. `App.tsx`：`sessionIds` → `sessions`，`handleDirsConfirm` 累加，`closePane`/`closeAll`/顶栏状态适配。
2. `MultiTerminal.tsx`：props 改 `sessions`，map 时传 `providerName`。
3. `TerminalPane.tsx`：props 加 `providerName`，pane 头渲染。
4. tsc + 手动验收。
