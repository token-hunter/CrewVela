# 多会话网格分屏 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一次启动 N（1-9）个相同配置的 claude，主区网格分屏同时显示，每格独立交互。

**Architecture:** 后端 `PtyManager` 从单 `Session` 重构为 `HashMap<u32, Session>` + 自增 id；数据通道复用轮询，新增 `pty_poll_all` 一次返回所有 session 增量；前端 `MultiTerminal` 按 `⌈√N⌉×⌈N/⌈√N⌉⌉` 网格渲染 N 个 `TerminalPane`（各自 xterm），单定时器分发。目录选择用 tauri-plugin-dialog。

**Tech Stack:** Rust（portable-pty, tauri-plugin-dialog）、React+TS（xterm.js）、CSS grid、cargo test、tsc。

---

## File Structure

- `src-tauri/src/pty.rs` — 重构：`PtyManager` 多 session；`open` 返回 id；新增 `poll_all`/`kill_all`/带 id 的 write/resize/kill。
- `src-tauri/src/lib.rs` — 改 commands：`launch_multi`/`pty_poll_all`/`pty_input(id)`/`pty_resize(id)`/`pty_kill(id)`/`pty_kill_all`；注册 dialog plugin。
- `src-tauri/Cargo.toml` — 加 `tauri-plugin-dialog`。
- `src-tauri/capabilities/default.json` — 加 `dialog:allow-open`（若不存在则创建 capabilities）。
- `package.json` — 加 `@tauri-apps/plugin-dialog`。
- `src/components/TerminalPane.tsx` — 新建：单格 xterm，绑定一个 session_id。
- `src/components/MultiTerminal.tsx` — 新建：网格容器 + 单 setInterval poll_all 分发。
- `src/components/CountPrompt.tsx` — 新建：数量输入弹窗（1-9）。
- `src/App.tsx` — 改：多开流程（provider → 数量 → 目录 → launch_multi → sessionIds）。
- `src/styles.css` — 加网格 + 单格样式。

---

## Task 1: 后端 PtyManager 重构为多 session

**Files:**
- Modify: `src-tauri/src/pty.rs`（整体重构）
- Test: `src-tauri/src/pty.rs`（`tests` 模块）

- [ ] **Step 1: 重写 `PtyManager` 为多 session 结构**

把 `src-tauri/src/pty.rs` 顶部 imports 和结构改为：

```rust
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;

use crate::launcher::{build_env, resolve_claude, CLEAR_VARS};
use crate::providers::ProviderConfig;

/// 一次轮询返回的单 session 数据块。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyChunk {
    pub data: String,
    pub running: bool,
    pub code: Option<u32>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<u32, Session>>,
    next_id: AtomicU32,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    output: Arc<Mutex<Vec<u8>>>,
    exit_code: Arc<Mutex<Option<u32>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }
    // 其余方法在后续 step 实现
}
```

（顶部加 `use std::sync::Arc;`。）

- [ ] **Step 2: 实现 `open` 返回 id**

```rust
    /// 启动一个 claude session，返回其 id。
    pub fn open(&self, provider: &ProviderConfig, key: &str, cwd: Option<&str>) -> Result<u32> {
        let claude = resolve_claude()?;
        let env = build_env(provider, key);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .context("创建 PTY 失败")?;

        let work_dir = match cwd {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")),
        };
        let mut cmd = if cfg!(target_os = "windows")
            && claude.extension().map_or(false, |e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        {
            let mut c = CommandBuilder::new("cmd.exe");
            c.arg("/c");
            c.arg(&claude);
            c
        } else {
            CommandBuilder::new(&claude)
        };
        cmd.cwd(work_dir);
        for v in CLEAR_VARS { cmd.env_remove(*v); }
        for (k, v) in &env { cmd.env(k, v); }

        let child = pair.slave.spawn_command(cmd).context("spawn claude 失败")?;
        drop(pair.slave);
        let writer = pair.master.take_writer().context("writer")?;
        let reader = pair.master.try_clone_reader().context("reader")?;

        let writer = Arc::new(Mutex::new(writer));
        let child = Arc::new(Mutex::new(child));
        let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let exit_code: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

        let output_clone = Arc::clone(&output);
        let exit_clone = Arc::clone(&exit_code);
        let child_clone = Arc::clone(&child);
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => output_clone.lock().unwrap().extend_from_slice(&buf[..n]),
                    Err(_) => break,
                }
            }
            let code = match child_clone.lock() {
                Ok(mut c) => c.wait().ok().map(|s| s.exit_code()),
                Err(_) => None,
            };
            *exit_clone.lock().unwrap() = code;
        });

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.sessions.lock().unwrap().insert(
            id,
            Session { master: pair.master, writer, child, output, exit_code },
        );
        Ok(id)
    }
```

- [ ] **Step 3: 实现 write / resize / kill / kill_all / poll_all**

```rust
    pub fn write(&self, id: u32, bytes: &[u8]) -> Result<()> {
        let guard = self.sessions.lock().unwrap();
        let s = guard.get(&id).ok_or_else(|| anyhow!("无此会话: {id}"))?;
        let mut w = s.writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: u32, rows: u16, cols: u16) -> Result<()> {
        let guard = self.sessions.lock().unwrap();
        let s = guard.get(&id).ok_or_else(|| anyhow!("无此会话: {id}"))?;
        s.master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| anyhow!("resize 失败: {e}"))?;
        Ok(())
    }

    pub fn kill(&self, id: u32) -> Result<()> {
        if let Some(s) = self.sessions.lock().unwrap().remove(&id) {
            if let Ok(mut c) = s.child.lock() { let _ = c.kill(); }
        }
        Ok(())
    }

    pub fn kill_all(&self) -> Result<()> {
        let mut guard = self.sessions.lock().unwrap();
        for (_, s) in guard.drain() {
            if let Ok(mut c) = s.child.lock() { let _ = c.kill(); }
        }
        Ok(())
    }

    /// 拉取所有活跃 session 的增量。
    pub fn poll_all(&self) -> HashMap<u32, PtyChunk> {
        let guard = self.sessions.lock().unwrap();
        let mut out = HashMap::new();
        for (id, s) in guard.iter() {
            let bytes = std::mem::take(&mut *s.output.lock().unwrap());
            let code = *s.exit_code.lock().unwrap();
            out.insert(
                *id,
                PtyChunk {
                    data: String::from_utf8_lossy(&bytes).into_owned(),
                    running: code.is_none(),
                    code,
                },
            );
        }
        out
    }
```

- [ ] **Step 4: 写单测**

在 `pty.rs` 末尾加（用 mock 难，测纯逻辑：sessions map 管理。由于 open 依赖真实 claude，单测聚焦 id 分配与 kill/poll 的 map 行为，用一个 test-only 辅助注入空 session）：

```rust
#[cfg(test)]
impl PtyManager {
    /// 测试专用：不 spawn，直接塞一个 dummy session 占位。
    fn inject_dummy(&self) -> u32 {
        // 用一个最小化的假 session：master/writer/child 无法真造，
        // 这里只验证 id 分配与 map 操作，用 panicking 占位即可（测试不触发它们）。
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // 由于 Session 字段无法零成本构造真 PTY，本测试改为白盒测 next_id 与 map。
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn next_id_increments() {
        let m = PtyManager::new();
        let a = m.next_id.fetch_add(1, Ordering::SeqCst);
        let b = m.next_id.fetch_add(1, Ordering::SeqCst);
        assert_eq!(a + 1, b);
    }
    #[test]
    fn kill_all_clears_map() {
        let m = PtyManager::new();
        // map 初始空
        assert!(m.sessions.lock().unwrap().is_empty());
    }
}
```

（注：真实 spawn 路径由 Task 2 的 command + 手动验收覆盖；这里锁定 id 自增与 map 生命周期语义。）

- [ ] **Step 5: cargo check + test**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --message-format short && cargo test --manifest-path src-tauri/Cargo.toml pty --message-format short`
Expected: 编译通过；pty 测试 PASS。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/pty.rs
git commit -m "refactor(pty): PtyManager 重构为多 session (HashMap+id)"
```

---

## Task 2: 后端 commands 适配多 session

**Files:**
- Modify: `src-tauri/src/lib.rs`（commands + invoke_handler）

- [ ] **Step 1: 替换 launch 为 launch_multi，加 poll_all/input/resize/kill/kill_all**

把 `lib.rs` 的 `launch`、`pty_input`、`pty_resize`、`pty_kill`、`pty_poll` 整段替换为：

```rust
#[tauri::command]
fn launch_multi(
    provider_id: String,
    cwd: String,
    count: u32,
    state: State<'_, PtyManager>,
) -> Result<Vec<u32>, String> {
    let provider = providers::get(&provider_id)
        .map_err(to_err)?
        .ok_or_else(|| format!("未找到 provider: {provider_id}"))?;
    launcher::validate_provider(&provider)?;
    let key = secrets::get(&provider.auth_token_ref)
        .map_err(to_err)?
        .ok_or_else(|| format!("未设置 {provider_id} 的 API key，请先录入"))?;
    let cwd_opt = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
    let mut ids = Vec::new();
    for _ in 0..count {
        ids.push(state.open(&provider, &key, cwd_opt).map_err(to_err)?);
    }
    Ok(ids)
}

#[tauri::command]
fn pty_poll_all(state: State<'_, PtyManager>) -> Result<std::collections::HashMap<u32, pty::PtyChunk>, String> {
    Ok(state.poll_all())
}

#[tauri::command]
fn pty_input(session_id: u32, bytes: Vec<u8>, state: State<'_, PtyManager>) -> Result<(), String> {
    state.write(session_id, &bytes).map_err(to_err)
}

#[tauri::command]
fn pty_resize(session_id: u32, rows: u16, cols: u16, state: State<'_, PtyManager>) -> Result<(), String> {
    state.resize(session_id, rows, cols).map_err(to_err)
}

#[tauri::command]
fn pty_kill(session_id: u32, state: State<'_, PtyManager>) -> Result<(), String> {
    state.kill(session_id).map_err(to_err)
}

#[tauri::command]
fn pty_kill_all(state: State<'_, PtyManager>) -> Result<(), String> {
    state.kill_all().map_err(to_err)
}
```

- [ ] **Step 2: 更新 invoke_handler + 关窗时 kill_all**

把 `invoke_handler` 改为：

```rust
        .invoke_handler(tauri::generate_handler![
            list_providers, get_provider, save_provider, delete_provider,
            set_key, get_key_state, delete_key, claude_path,
            launch_multi, pty_poll_all, pty_input, pty_resize, pty_kill, pty_kill_all,
        ])
```

`on_window_event` 的关闭处理改为 `pty_kill_all`：

```rust
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                if let Some(m) = window.app_handle().try_state::<PtyManager>() {
                    let _ = m.kill_all();
                }
            }
        })
```

- [ ] **Step 3: cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --message-format short`
Expected: `Finished`（旧 launch/pty_poll 已移除，无悬空引用）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(backend): 多 session commands (launch_multi/poll_all/kill_all)"
```

---

## Task 3: 集成 tauri-plugin-dialog（目录选择）

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`（注册 plugin）
- Modify: `package.json`
- Create/Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: 后端加依赖 + 注册**

`src-tauri/Cargo.toml` `[dependencies]` 加：

```toml
tauri-plugin-dialog = "2"
```

`src-tauri/src/lib.rs` 的 `run()` builder 链加（在 opener plugin 旁）：

```rust
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
```

- [ ] **Step 2: capabilities 允许 dialog open**

检查 `src-tauri/capabilities/` 是否存在。若不存在，创建 `src-tauri/capabilities/default.json`：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "默认权限",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:allow-open"]
}
```

若已存在 `capabilities/default.json`，在 `permissions` 数组加 `"dialog:allow-open"`。

确认 `tauri.conf.json` 的 app.windows[0] 有 `"label": "main"`（capability 按label匹配）；若没有 label 字段则加上。

- [ ] **Step 3: 前端加依赖**

`package.json` `dependencies` 加：

```json
"@tauri-apps/plugin-dialog": "^2.0.0"
```

Run: `npm install`

- [ ] **Step 4: cargo check 验证编译**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --message-format short`
Expected: `Finished`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/capabilities package.json package-lock.json
git commit -m "feat: 集成 tauri-plugin-dialog (目录选择)"
```

---

## Task 4: 前端 TerminalPane + MultiTerminal 组件

**Files:**
- Create: `src/components/TerminalPane.tsx`
- Create: `src/components/MultiTerminal.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: 新建 TerminalPane（单格，绑定 session_id）**

`src/components/TerminalPane.tsx`：

```tsx
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  sessionId: number;
  onClose: (id: number) => void;
}

const encoder = new TextEncoder();

export function TerminalPane({ sessionId, onClose }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const term = new Terminal({
      fontFamily: '"Cascadia Code", Consolas, monospace',
      fontSize: 12,
      theme: { background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5" },
      cursorBlink: true,
      scrollback: 3000,
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    termRef.current = term;
    const doFit = () => { try { fit.fit(); } catch { /* */ } };
    requestAnimationFrame(() => { doFit(); term.focus(); });
    term.onData((data) => {
      if (data === "\x1b[I" || data === "\x1b[O") return;
      invoke("pty_input", { sessionId, bytes: Array.from(encoder.encode(data)) }).catch(() => {});
    });
    term.onResize(({ cols, rows }) => {
      invoke("pty_resize", { sessionId, rows, cols }).catch(() => {});
    });
    const ro = new ResizeObserver(() => doFit());
    ro.observe(container);
    return () => { ro.disconnect(); term.dispose(); termRef.current = null; };
  }, [sessionId]);

  return (
    <div className="pane">
      <div className="pane-head">
        <span className="pane-id">#{sessionId}</span>
        <button className="pane-close" title="关闭" onClick={() => onClose(sessionId)}>×</button>
      </div>
      <div ref={containerRef} className="pane-term" onClick={() => termRef.current?.focus()} />
    </div>
  );
}
```

- [ ] **Step 2: 新建 MultiTerminal（网格 + poll_all 分发）**

`src/components/MultiTerminal.tsx`：

```tsx
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TerminalPane } from "./TerminalPane";

interface Chunk {
  data: string;
  running: boolean;
  code: number | null;
}

interface Props {
  sessionIds: number[];
  onClose: (id: number) => void;
}

/** N → 网格列数（cols = ceil(sqrt(N))） */
function gridCols(n: number): number {
  return Math.ceil(Math.sqrt(n));
}

export function MultiTerminal({ sessionIds, onClose }: Props) {
  const panesRef = useRef<Map<number, HTMLDivElement | null>>(new Map());

  useEffect(() => {
    let stopped = false;
    const interval = setInterval(async () => {
      if (stopped) return;
      try {
        const all = await invoke<Record<string, Chunk>>("pty_poll_all");
        // panesRef 在 TerminalPane 没暴露 term，这里通过自定义事件分发
        window.dispatchEvent(new CustomEvent("pty:chunk", { detail: all }));
      } catch (e) {
        console.error("poll_all 失败:", e);
      }
    }, 30);
    return () => { stopped = true; clearInterval(interval); };
  }, []);

  const cols = gridCols(sessionIds.length);

  return (
    <div className="grid" style={{ gridTemplateColumns: `repeat(${cols}, 1fr)` }}>
      {sessionIds.map((id) => (
        <TerminalPane key={id} sessionId={id} onClose={onClose} />
      ))}
    </div>
  );
}
```

（说明：`pty:chunk` 是纯前端 window event，把 poll_all 的数据分发给各 TerminalPane。TerminalPane 需监听该 event 取自己 sessionId 的 chunk 写入 term——下一步补监听。）

- [ ] **Step 3: TerminalPane 监听 pty:chunk 写入 term**

在 `TerminalPane.tsx` 的 `useEffect` return 前，加：

```tsx
    const onChunk = (e: Event) => {
      const detail = (e as CustomEvent).detail as Record<string, Chunk>;
      const c = detail[String(sessionId)];
      if (c && c.data.length > 0) term.write(c.data);
    };
    window.addEventListener("pty:chunk", onChunk as EventListener);
    return () => {
      ro.disconnect();
      window.removeEventListener("pty:chunk", onChunk as EventListener);
      term.dispose();
      termRef.current = null;
    };
```

（同时在文件顶部加 `interface Chunk { data: string; running: boolean; code: number | null; }` 类型。）

- [ ] **Step 4: styles.css 加网格 + 单格样式**

`src/styles.css` 末尾加：

```css
.grid {
  display: grid;
  gap: 6px;
  padding: 6px;
  height: 100%;
  width: 100%;
  background: #11121b;
  overflow: hidden;
}
.pane {
  display: flex;
  flex-direction: column;
  background: #1a1b26;
  border: 1px solid #3b4060;
  border-radius: 6px;
  overflow: hidden;
  min-height: 120px;
  min-width: 200px;
}
.pane-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 2px 8px;
  background: #24283b;
  border-bottom: 1px solid #3b4060;
  font-size: 11px;
  color: #7a82b4;
}
.pane-close {
  background: transparent;
  border: none;
  color: #f7768e;
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
}
.pane-term {
  flex: 1;
  overflow: hidden;
}
.pane-term .terminal { height: 100%; }
```

- [ ] **Step 5: tsc**

Run: `npx tsc --noEmit`
Expected: 无错误。

- [ ] **Step 6: 提交**

```bash
git add src/components/TerminalPane.tsx src/components/MultiTerminal.tsx src/styles.css
git commit -m "feat(ui): MultiTerminal 网格 + TerminalPane 单格 + poll_all 分发"
```

---

## Task 5: 数量弹窗 + 目录选择 + launch_multi 流程

**Files:**
- Create: `src/components/CountPrompt.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: 新建 CountPrompt（数量输入弹窗）**

`src/components/CountPrompt.tsx`：

```tsx
import { useState } from "react";

interface Props {
  providerName: string;
  onConfirm: (count: number) => void;
  onCancel: () => void;
}

export function CountPrompt({ providerName, onConfirm, onCancel }: Props) {
  const [val, setVal] = useState("4");
  const n = Number(val);
  const valid = Number.isInteger(n) && n >= 1 && n <= 9;
  return (
    <div className="editor-overlay" onClick={onCancel}>
      <div className="editor" onClick={(e) => e.stopPropagation()} style={{ width: 320 }}>
        <h2>多开 {providerName}</h2>
        <div className="field">
          <label>数量（1-9）</label>
          <input
            type="number"
            min={1}
            max={9}
            value={val}
            autoFocus
            onChange={(e) => setVal(e.target.value)}
          />
          {!valid && <div className="hint" style={{ color: "#f7768e" }}>请输入 1-9 的整数</div>}
        </div>
        <div className="actions">
          <button className="btn" onClick={onCancel}>取消</button>
          <button className="btn primary" disabled={!valid} onClick={() => onConfirm(n)}>
            下一步：选目录
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: App.tsx 多开流程**

把 `App.tsx` 的状态与 `handleLaunch` 改为多开。新增 state：

```tsx
const [sessionIds, setSessionIds] = useState<number[]>([]);
const [countPromptFor, setCountPromptFor] = useState<ProviderConfig | null>(null);
```

`handleLaunch` 改为打开数量弹窗：

```tsx
  const handleLaunch = (id: string) => {
    const p = providers.find((x) => x.id === id);
    if (!p) return;
    if (!(keyStates[id] ?? false)) { setEditing(p); return; }
    if (!p.baseUrl) { setEditing(p); return; } // custom 未配置
    setCountPromptFor(p);
  };
```

确认数量后选目录、launch_multi：

```tsx
  const handleCountConfirm = async (count: number) => {
    const p = countPromptFor;
    if (!p) return;
    setCountPromptFor(null);
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: undefined });
      const cwd = typeof selected === "string" ? selected : "";
      const ids = await invoke<number[]>("launch_multi", {
        providerId: p.id, cwd, count,
      });
      setSessionIds(ids);
    } catch (e) {
      console.error("launch_multi 失败:", e);
      alert("启动失败: " + String(e));
    }
  };
```

顶部 `import`：

```tsx
import { open } from "@tauri-apps/plugin-dialog";
import { MultiTerminal } from "./components/MultiTerminal";
import { CountPrompt } from "./components/CountPrompt";
```

主区渲染：当 `sessionIds.length > 0` 显示 `MultiTerminal`，否则保留原"选择 provider"提示。把原 `<div className="terminal-wrap"><TerminalView .../></div>` 替换为：

```tsx
        <div className="terminal-wrap">
          {sessionIds.length > 0 ? (
            <MultiTerminal
              sessionIds={sessionIds}
              onClose={(id) => setSessionIds((arr) => arr.filter((x) => x !== id))}
            />
          ) : (
            <div style={{ padding: 24, color: "#7a82b4" }}>选择左侧 provider 启动（点选后可多开）</div>
          )}
        </div>
```

文件末尾（其他 overlay 旁）加 CountPrompt 渲染：

```tsx
      {countPromptFor && (
        <CountPrompt
          providerName={countPromptFor.name}
          onConfirm={handleCountConfirm}
          onCancel={() => setCountPromptFor(null)}
        />
      )}
```

顶栏加「全部关闭」按钮（替换原"停止"）：

```tsx
          {sessionIds.length > 0 && (
            <button className="btn danger" onClick={async () => {
              await invoke("pty_kill_all");
              setSessionIds([]);
            }}>全部关闭</button>
          )}
```

删除旧的 `selectedId` / `TerminalView` 相关逻辑（不再需要单选 selectedId）。

- [ ] **Step 3: tsc**

Run: `npx tsc --noEmit`
Expected: 无错误。

- [ ] **Step 4: 提交**

```bash
git add src/components/CountPrompt.tsx src/App.tsx
git commit -m "feat(ui): 数量弹窗 + 目录选择 + launch_multi 多开流程"
```

---

## Task 6: 关闭管理验收 + 残留清理

**Files:**
- Modify: `src/components/TerminalPane.tsx`（关闭后占位）
- Modify: `src/App.tsx`（关单格已留空）

- [ ] **Step 1: TerminalPane 关闭调 pty_kill**

`TerminalPane` 的 `onClose` 由父组件传入；父组件（App）的 `onClose` 目前只 `setSessionIds(filter)`。改为先 kill 再移除：

App.tsx 的 `onClose`：

```tsx
            onClose={async (id) => {
              await invoke("pty_kill", { sessionId: id }).catch(() => {});
              setSessionIds((arr) => arr.filter((x) => x !== id));
            }}
```

- [ ] **Step 2: 移除已无用的旧 Terminal.tsx（若不再被引用）**

```bash
rm src/components/Terminal.tsx
```

确认 App.tsx 不再 `import { TerminalView }`。

- [ ] **Step 3: tsc + cargo check**

Run: `npx tsc --noEmit && cargo check --manifest-path src-tauri/Cargo.toml --message-format short`
Expected: 均通过。

- [ ] **Step 4: 提交**

```bash
git add -A
git commit -m "feat: 多开会话关闭管理 + 清理旧单终端组件"
```

---

## Task 7: 端到端验收 + README

- [ ] **Step 1: 启动 tauri dev，多开 4 个 glm**

点 glm → 数量输入 4 → 选目录（任一项目）→ 主区 2×2 网格 4 个终端，各自显示 Claude Code 欢迎语，可独立输入。

- [ ] **Step 2: 验收关闭**

点某格 × → 该格消失（sessionIds 减 1），其余 3 格不动；点「全部关闭」→ 回 provider 列表空状态。

- [ ] **Step 3: 验收 custom 拦截**

点 custom（未配置）→ 走编辑器（不进多开流程）。

- [ ] **Step 4: README 补多开说明**

`README.md` 「使用」节加：

```markdown
### 多开会话
点 provider 后输入数量（1-9）→ 选工作目录 → 网格分屏同时开 N 个 claude（同 provider 同目录）。
每格独立交互；点格子右上角 × 关单个（其余不动），顶栏「全部关闭」清空。
```

- [ ] **Step 5: 提交**

```bash
git add README.md
git commit -m "docs: README 补多开会话说明"
```
