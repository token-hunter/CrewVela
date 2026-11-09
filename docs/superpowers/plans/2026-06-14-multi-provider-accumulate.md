# 多 provider 累加并存 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 不同 provider 的会话在同一网格累加并存，pane 头显示 provider 名。

**Architecture:** 纯前端：`sessionIds: number[]` → `sessions: {id, providerName}[]`，`handleDirsConfirm` 累加（不替换）；provider 名经 MultiTerminal 传到 TerminalPane 的 pane 头。后端不变。

**Tech Stack:** React+TS（App.tsx / MultiTerminal.tsx / TerminalPane.tsx）、tsc。

---

## File Structure

- `src/components/TerminalPane.tsx` — props 加 `providerName`，pane 头渲染。
- `src/components/MultiTerminal.tsx` — props 从 `sessionIds: number[]` 改 `sessions: {id, providerName}[]`。
- `src/App.tsx` — `sessionIds` → `sessions`，`handleDirsConfirm` 累加，`closePane`/`closeAll`/顶栏适配。

---

## Task 1: TerminalPane 加 providerName

**Files:**
- Modify: `src/components/TerminalPane.tsx`

- [ ] **Step 1: props 加 providerName + pane 头显示**

把 `src/components/TerminalPane.tsx` 的 `Props` 和 pane 头改为：

```tsx
interface Props {
  sessionId: number;
  providerName: string;
  onClose: (id: number) => void;
}
```

（在原 `Props` 里 `sessionId` 下加一行 `providerName: string;`。）

pane 头渲染（`<div className="pane-head">` 内）：

```tsx
      <div className="pane-head">
        <span className="pane-id">#{sessionId} · {providerName}</span>
        <button className="pane-close" title="关闭" onClick={() => onClose(sessionId)}>
          ×
        </button>
      </div>
```

（原 `<span className="pane-id">#{sessionId}</span>` 改为带 ` · {providerName}`。）

函数签名同步：

```tsx
export function TerminalPane({ sessionId, providerName, onClose }: Props) {
```

- [ ] **Step 2: tsc**

Run: `npx tsc --noEmit`
Expected: 报错——`MultiTerminal` 传给 `TerminalPane` 的 props 缺 `providerName`（Task 2 才修）。这是预期的中间态错误，先记下。

（若想此时保持绿，可先跳过 tsc，Task 2/3 一起验。但记录此中间态。）

- [ ] **Step 3: 暂不单独提交**

Task 1 单独会让 tsc 红（MultiTerminal 还没传 providerName），与 Task 2 合并提交。

---

## Task 2: MultiTerminal props 改 sessions

**Files:**
- Modify: `src/components/MultiTerminal.tsx`

- [ ] **Step 1: Props 改 sessions + map 传 providerName**

把 `src/components/MultiTerminal.tsx` 的 `Props` 和渲染改为：

```tsx
interface SessionInfo {
  id: number;
  providerName: string;
}

interface Props {
  sessions: SessionInfo[];
  onClose: (id: number) => void;
}

function gridCols(n: number): number {
  if (n <= 0) return 1;
  return Math.ceil(Math.sqrt(n));
}

export function MultiTerminal({ sessions, onClose }: Props) {
  useEffect(() => {
    let stopped = false;
    const interval = setInterval(async () => {
      if (stopped) return;
      try {
        const all = await invoke<Record<string, unknown>>("pty_poll_all");
        window.dispatchEvent(new CustomEvent("pty:chunk", { detail: all }));
      } catch (e) {
        console.error("poll_all 失败:", e);
      }
    }, 30);
    return () => {
      stopped = true;
      clearInterval(interval);
    };
  }, []);

  const cols = gridCols(sessions.length);

  return (
    <div className="grid" style={{ gridTemplateColumns: `repeat(${cols}, 1fr)` }}>
      {sessions.map((s) => (
        <TerminalPane
          key={s.id}
          sessionId={s.id}
          providerName={s.providerName}
          onClose={onClose}
        />
      ))}
    </div>
  );
}
```

（`useEffect` / `invoke` / `window` 那段不变，保持原样。）

- [ ] **Step 2: tsc**

Run: `npx tsc --noEmit`
Expected: 报错——`App.tsx` 还传 `sessionIds`（Task 3 才修）。中间态。

- [ ] **Step 3: 暂不单独提交**

与 Task 3 合并。

---

## Task 3: App.tsx sessions 累加

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: state sessionIds → sessions**

把 `src/App.tsx` 的：

```tsx
  const [sessionIds, setSessionIds] = useState<number[]>([]);
```

改为：

```tsx
  const [sessions, setSessions] = useState<{ id: number; providerName: string }[]>([]);
```

- [ ] **Step 2: handleDirsConfirm 累加 + 记录 provider 名**

把 `handleDirsConfirm` 改为：

```tsx
  const handleDirsConfirm = async (cwds: string[]) => {
    const ctx = dirPromptFor;
    if (!ctx) return;
    setDirPromptFor(null);
    try {
      const ids = await invoke<number[]>("launch_multi", {
        providerId: ctx.provider.id,
        cwds,
        count: ctx.count,
      });
      setSessions((prev) => [
        ...prev,
        ...ids.map((id) => ({ id, providerName: ctx.provider.name })),
      ]);
    } catch (e) {
      console.error("launch_multi 失败:", e);
      alert("启动失败: " + String(e));
    }
  };
```

（关键：`setSessions((prev) => [...prev, ...])` 累加，不再 `setSessionIds(ids)` 替换。）

- [ ] **Step 3: closePane / closeAll / 顶栏适配**

```tsx
  const closePane = async (id: number) => {
    await invoke("pty_kill", { sessionId: id }).catch(() => {});
    setSessions((arr) => arr.filter((x) => x.id !== id));
  };

  const closeAll = async () => {
    await invoke("pty_kill_all");
    setSessions([]);
  };
```

顶栏 status / 渲染里的 `sessionIds` 全改为 `sessions`：

```tsx
          <span className="status">
            {sessions.length > 0
              ? `运行 ${sessions.length} 个会话`
              : "选择左侧 provider 启动（点选后可多开）"}
          </span>
          ...
          {sessions.length > 0 && (
            <button className="btn danger" onClick={closeAll}>
              全部关闭
            </button>
          )}
        </div>
        <div className="terminal-wrap">
          {sessions.length > 0 ? (
            <MultiTerminal sessions={sessions} onClose={closePane} />
          ) : (
            <div style={{ padding: 24, color: "var(--text-dim)" }}>
              点击左侧 provider → 输入数量（1-9）→ 选工作目录，开多个 claude 网格分屏
            </div>
          )}
        </div>
```

- [ ] **Step 4: tsc 全绿**

Run: `npx tsc --noEmit`
Expected: 无错误（Task 1-3 类型对齐：sessions/{id,providerName} 贯通 App→MultiTerminal→TerminalPane）。

- [ ] **Step 5: 提交（Task 1+2+3 合并）**

```bash
git add src/components/TerminalPane.tsx src/components/MultiTerminal.tsx src/App.tsx
git commit -m "fix(ui): 多 provider 会话累加并存 + pane 头显示 provider 名"
```

---

## Task 4: 端到端验收

- [ ] **Step 1: 重启 tauri dev，混合多开**

点 glm → 数量 2 → 目录面板（全留空）→ 开始 → 网格 2 格，pane 头 `#1 · glm-5.2`、`#2 · glm-5.2`（或你改的名）。

- [ ] **Step 2: 追加 deepseek（关键：glm 不消失）**

再点 DeepSeek → 数量 1 → 目录 → 开始 → 网格变 **3 格**：glm/glm/DeepSeek 全保留，pane 头各显示对应 provider 名。glm 的 2 格不消失。

- [ ] **Step 3: 关闭管理**

关中间格 → 该格消失，其余 2 格不动；「全部关闭」→ 清空回 provider 列表，任务管理器无残留 claude.exe（无孤儿）。

- [ ] **Step 4: README 补一句（可选）**

`README.md` 多开会话节加：

```markdown
不同 provider 的会话可混合并存——开 glm 后再开 deepseek，glm 的格子保留，新 deepseek 格子追加。
```

- [ ] **Step 5: 提交**

```bash
git add README.md
git commit -m "docs: README 补多 provider 混合并存说明"
```
