# 每会话独立目录 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 多开 N 个 claude 时，每个会话窗口可选不同项目目录。

**Architecture:** 后端 `launch_multi` 的 `cwd: String` 改 `cwds: Vec<String>`，抽 `pick_cwd(cwds, i)` 纯函数（越界/空串→None=home）；前端新增 `MultiDirPrompt`（N 行各选 + 全部相同快捷），替换 `App.tsx` 现有单次 `open`。

**Tech Stack:** Rust（launcher.rs / lib.rs）、React+TS（MultiDirPrompt.tsx / App.tsx）、cargo test、tsc。

---

## File Structure

- `src-tauri/src/launcher.rs` — 加 `pick_cwd` 纯函数 + 单测。
- `src-tauri/src/lib.rs` — `launch_multi` 签名 `cwd: String` → `cwds: Vec<String>`，循环里用 `pick_cwd`。
- `src/components/MultiDirPrompt.tsx` — 新建：N 行目录选择 + 全部相同快捷。
- `src/App.tsx` — 接入 MultiDirPrompt，删单次 `open`。
- `README.md` — 补一句每会话独立目录。

---

## Task 1: 后端 pick_cwd 纯函数 + launch_multi 改 cwds

**Files:**
- Modify: `src-tauri/src/launcher.rs`（加 `pick_cwd` + 单测）
- Modify: `src-tauri/src/lib.rs`（`launch_multi` 签名 + 循环）

- [ ] **Step 1: 写 pick_cwd 的失败测试**

在 `src-tauri/src/launcher.rs` 的 `tests` 模块加：

```rust
    #[test]
    fn pick_cwd_returns_some_for_valid_index() {
        let cwds = vec!["a".to_string(), "b".to_string()];
        assert_eq!(pick_cwd(&cwds, 0), Some("a"));
        assert_eq!(pick_cwd(&cwds, 1), Some("b"));
    }

    #[test]
    fn pick_cwd_out_of_bounds_is_none() {
        let cwds = vec!["a".to_string()];
        assert_eq!(pick_cwd(&cwds, 5), None);
    }

    #[test]
    fn pick_cwd_empty_string_is_none() {
        let cwds = vec!["".to_string(), "b".to_string()];
        assert_eq!(pick_cwd(&cwds, 0), None);
        assert_eq!(pick_cwd(&cwds, 1), Some("b"));
    }
```

- [ ] **Step 2: 运行测试，确认编译失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml launcher::tests --message-format short`
Expected: 编译错误 `cannot find function pick_cwd`。

- [ ] **Step 3: 实现 pick_cwd**

在 `src-tauri/src/launcher.rs`（`validate_provider` 之后）加：

```rust
/// 从 cwds 取第 i 个有效目录；越界或空串返回 None（调用方据此用 home）。
pub fn pick_cwd<'a>(cwds: &'a [String], i: usize) -> Option<&'a str> {
    cwds.get(i)
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml launcher::tests --message-format short`
Expected: PASS（含 3 个新 pick_cwd 测试 + 原 build_env / validate_provider 测试）。

- [ ] **Step 5: launch_multi 改 cwds: Vec<String> + 用 pick_cwd**

把 `src-tauri/src/lib.rs` 的 `launch_multi` 改为：

```rust
#[tauri::command]
fn launch_multi(
    provider_id: String,
    cwds: Vec<String>,
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
    let mut ids = Vec::new();
    for i in 0..count {
        let cwd_opt = launcher::pick_cwd(&cwds, i as usize);
        ids.push(state.open(&provider, &key, cwd_opt).map_err(to_err)?);
    }
    Ok(ids)
}
```

- [ ] **Step 6: cargo check 确认编译**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --message-format short`
Expected: `Finished`。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/launcher.rs src-tauri/src/lib.rs
git commit -m "feat(backend): launch_multi 支持每会话独立 cwd (cwds:Vec + pick_cwd)"
```

---

## Task 2: 前端 MultiDirPrompt 组件

**Files:**
- Create: `src/components/MultiDirPrompt.tsx`

- [ ] **Step 1: 新建 MultiDirPrompt**

`src/components/MultiDirPrompt.tsx`：

```tsx
import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  count: number;
  onConfirm: (cwds: string[]) => void;
  onCancel: () => void;
}

export function MultiDirPrompt({ count, onConfirm, onCancel }: Props) {
  const [dirs, setDirs] = useState<string[]>(() => Array(count).fill(""));

  const pickOne = async (i: number) => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setDirs((d) => d.map((x, idx) => (idx === i ? selected : x)));
    }
  };

  const pickAll = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setDirs(Array(count).fill(selected));
    }
  };

  return (
    <div className="editor-overlay" onClick={onCancel}>
      <div className="editor" onClick={(e) => e.stopPropagation()} style={{ width: 560 }}>
        <h2>选择各会话工作目录（共 {count} 个）</h2>
        <div className="field">
          <button className="btn" onClick={pickAll}>
            全部使用同一目录
          </button>
        </div>
        <div className="env-list">
          {dirs.map((d, i) => (
            <div className="env-row" key={i} style={{ gridTemplateColumns: "auto 1fr" }}>
              <button className="btn" onClick={() => pickOne(i)}>
                选目录
              </button>
              <div
                style={{
                  padding: "8px 10px",
                  fontSize: 12,
                  color: d ? "var(--text)" : "var(--text-dim)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {d || "home（默认）"}
              </div>
            </div>
          ))}
        </div>
        <div className="actions">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button className="btn primary" onClick={() => onConfirm(dirs)}>
            开始 {count} 个
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: tsc**

Run: `npx tsc --noEmit`
Expected: 无错误（此时 MultiDirPrompt 还没被引用，但自身类型正确）。

- [ ] **Step 3: 提交**

```bash
git add src/components/MultiDirPrompt.tsx
git commit -m "feat(ui): MultiDirPrompt 每会话目录选择面板"
```

---

## Task 3: App.tsx 接入 MultiDirPrompt

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 加 dirPromptFor state，改 handleCountConfirm，加 handleDirsConfirm**

`src/App.tsx` 顶部 import 加：

```tsx
import { MultiDirPrompt } from "./components/MultiDirPrompt";
```

（删掉不再需要的 `import { open } from "@tauri-apps/plugin-dialog";`。）

state 加（在 `countPromptFor` 旁）：

```tsx
  const [dirPromptFor, setDirPromptFor] = useState<{ provider: ProviderConfig; count: number } | null>(null);
```

把 `handleCountConfirm` 改为不再 open，改为打开 MultiDirPrompt：

```tsx
  const handleCountConfirm = (count: number) => {
    const p = countPromptFor;
    if (!p) return;
    setCountPromptFor(null);
    setDirPromptFor({ provider: p, count });
  };

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
      setSessionIds(ids);
    } catch (e) {
      console.error("launch_multi 失败:", e);
      alert("启动失败: " + String(e));
    }
  };
```

文件末尾（其他 overlay 旁，`countPromptFor &&` 块之后）加：

```tsx
      {dirPromptFor && (
        <MultiDirPrompt
          count={dirPromptFor.count}
          onConfirm={handleDirsConfirm}
          onCancel={() => setDirPromptFor(null)}
        />
      )}
```

- [ ] **Step 2: tsc**

Run: `npx tsc --noEmit`
Expected: 无错误（open 已不再使用，import 删除后无悬空引用）。

- [ ] **Step 3: 提交**

```bash
git add src/App.tsx
git commit -m "feat(ui): App 接入 MultiDirPrompt（替换单次目录选择）"
```

---

## Task 4: 端到端验收 + README

- [ ] **Step 1: 重启 tauri dev，多开 3 个不同目录**

点 glm → 数量 3 → 目录面板 3 行：第 1 行选项目 A、第 2 行选项目 B、第 3 行留空 → 开始 → 网格 3 个 claude。各自终端 `pwd` 应分别是 A、B、home。

- [ ] **Step 2: 验收「全部相同」快捷**

重开：数量 3 → 点「全部使用同一目录」→ 选项目 X → 3 行都变 X → 开始 → 3 个 claude 都在 X。

- [ ] **Step 3: README 补一句**

`README.md`「多开会话」节末尾加：

```markdown
每个会话可在「目录面板」单独选项目目录（留空则用 home）；点「全部使用同一目录」可一键填满所有行。
```

- [ ] **Step 4: 提交**

```bash
git add README.md
git commit -m "docs: README 补每会话独立目录说明"
```
