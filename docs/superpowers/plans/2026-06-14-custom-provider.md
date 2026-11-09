# Custom Provider 模板 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在内置 provider 列表加一个空的 `custom` 模板，用户编辑填自己的 API 端点；启动空 base_url 的 provider 时拦截报错。

**Architecture:** 复用现有 ProviderConfig + load 迁移机制。custom 作为第 5 个 builtin（不可删）。base_url 空校验抽到 `launcher::validate_provider` 便于单测。前端徽章按 base_url 是否为空区分「未配置」/「未设 Key」。

**Tech Stack:** Rust（providers.rs / launcher.rs / lib.rs）、React+TS（ProviderList.tsx）、cargo test、tsc。

---

## File Structure

- `src-tauri/src/providers.rs` — 修改：`default_providers` 加 custom；改/加单测。
- `src-tauri/src/launcher.rs` — 修改：加 `validate_provider` 函数 + 单测。
- `src-tauri/src/lib.rs` — 修改：`launch` 开头调 `validate_provider`。
- `src/components/ProviderList.tsx` — 修改：徽章按 base_url 空否区分。

---

## Task 1: providers.rs 加 custom 空模板

**Files:**
- Modify: `src-tauri/src/providers.rs`（`default_providers` 末尾，deepseek 之后）
- Test: `src-tauri/src/providers.rs`（`tests` 模块）

- [ ] **Step 1: 改现有四内置断言为五（含 custom），并加 custom 空模板断言**

把 `src-tauri/src/providers.rs` 的 `tests` 模块里 `default_providers_include_four_builtins` 改名并加 custom，并新增 custom 空模板测试：

```rust
    #[test]
    fn default_providers_include_five_builtins() {
        let providers = default_providers();
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        for id in ["glm", "kimi", "minimax", "deepseek", "custom"] {
            assert!(ids.contains(&id), "缺少内置 provider: {id}");
        }
        assert!(providers.iter().all(|p| p.builtin));
    }

    #[test]
    fn custom_provider_is_empty_template() {
        let custom = default_providers()
            .into_iter()
            .find(|p| p.id == "custom")
            .expect("应有 custom");
        assert!(custom.base_url.is_empty(), "custom base_url 应为空");
        assert!(custom.env.is_empty(), "custom env 应为空");
        assert!(custom.builtin);
        assert_eq!(custom.auth_token_ref, "custom");
    }
```

- [ ] **Step 2: 运行测试，确认新断言失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml providers::tests --message-format short`
Expected: `default_providers_include_five_builtins` 和 `custom_provider_is_empty_template` FAIL（custom 不存在）。

- [ ] **Step 3: 在 `default_providers` 末尾（deepseek 块之后）加 custom**

```rust
        // 空白自定义模板：用户编辑填自己的 API 端点
        ProviderConfig {
            id: "custom".into(),
            name: "自定义".into(),
            base_url: String::new(),
            auth_token_ref: "custom".into(),
            env: HashMap::new(),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
        },
```

- [ ] **Step 4: 运行测试，确认全部通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml providers::tests --message-format short`
Expected: 4 个 providers 测试 PASS（原 deepseek 2 个 + 新 2 个）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/providers.rs
git commit -m "feat(providers): 加 custom 空模板内置 provider"
```

---

## Task 2: launcher.rs 加 validate_provider + launch 校验

**Files:**
- Modify: `src-tauri/src/launcher.rs`（加 `validate_provider` + 单测）
- Modify: `src-tauri/src/lib.rs`（`launch` 开头调 `validate_provider`）

- [ ] **Step 1: 写 validate_provider 的失败测试**

在 `src-tauri/src/launcher.rs` 的 `tests` 模块加：

```rust
    #[test]
    fn validate_provider_rejects_empty_base_url() {
        let mut p = sample();
        p.base_url = String::new();
        let err = validate_provider(&p).unwrap_err();
        assert!(err.contains("base_url"), "错误应提及 base_url: {err}");
    }

    #[test]
    fn validate_provider_accepts_filled_base_url() {
        let p = sample();
        assert!(validate_provider(&p).is_ok());
    }
```

- [ ] **Step 2: 运行测试，确认编译失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml launcher::tests --message-format short`
Expected: 编译错误 `cannot find function validate_provider`。

- [ ] **Step 3: 实现 validate_provider**

在 `src-tauri/src/launcher.rs`（`build_env` 之后）加：

```rust
/// 启动前校验 provider 配置。空 base_url 直接拒绝，避免连到无效端点。
pub fn validate_provider(provider: &ProviderConfig) -> Result<(), String> {
    if provider.base_url.trim().is_empty() {
        return Err(format!(
            "请先编辑 {} provider，填写 API 端点（base_url）",
            provider.id
        ));
    }
    Ok(())
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml launcher::tests --message-format short`
Expected: PASS。

- [ ] **Step 5: launch 开头接入校验**

把 `src-tauri/src/lib.rs` 的 `launch` 改为拿到 provider 后立即校验：

```rust
#[tauri::command]
fn launch(
    id: String,
    working_dir: Option<String>,
    state: State<'_, PtyManager>,
) -> Result<(), String> {
    let provider = providers::get(&id)
        .map_err(to_err)?
        .ok_or_else(|| format!("未找到 provider: {id}"))?;
    launcher::validate_provider(&provider)?;
    let key = secrets::get(&provider.auth_token_ref)
        .map_err(to_err)?
        .ok_or_else(|| format!("未设置 {id} 的 API key，请先录入"))?;
    state
        .open(&provider, &key, working_dir.as_deref())
        .map_err(to_err)
}
```

- [ ] **Step 6: cargo check 确认整体编译**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --message-format short`
Expected: `Finished`，无错误。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/launcher.rs src-tauri/src/lib.rs
git commit -m "feat(launcher): 启动前校验 base_url 非空"
```

---

## Task 3: ProviderList 徽章区分「未配置」

**Files:**
- Modify: `src/components/ProviderList.tsx`（徽章渲染处）

- [ ] **Step 1: 改徽章逻辑——base_url 空显示「未配置」**

把 `src/components/ProviderList.tsx` 里渲染徽章的部分（`const hasKey = ...` 附近）改为：

```tsx
          const hasKey = props.keyStates[p.id] ?? false;
          const configured = p.baseUrl.length > 0;
          return (
            <div
              key={p.id}
              className={"provider-item" + (p.id === props.activeId ? " active" : "")}
              onClick={() => props.onLaunch(p.id)}
            >
              <div className="pname">{p.name || p.id}</div>
              <div className="purl">{p.baseUrl || "（未配置端点）"}</div>
              <div className="badge-row">
                {!configured ? (
                  <span className="badge warn">未配置</span>
                ) : hasKey ? (
                  <span className="badge ok">已设 Key</span>
                ) : (
                  <span className="badge warn">未设 Key</span>
                )}
                {p.builtin && <span className="badge builtin">内置</span>}
```

（其余 JSX 保持不变，从 `<div className="spacer" />` 往下原样。）

- [ ] **Step 2: tsc 类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误。

- [ ] **Step 3: 提交**

```bash
git add src/components/ProviderList.tsx
git commit -m "feat(ui): provider 徽章按 base_url 区分未配置/未设Key"
```

---

## Task 4: 端到端验证

- [ ] **Step 1: 重启 tauri dev，确认左侧出现「自定义」项**

应用启动后，左侧 provider 列表应有 5 项，最后一项「自定义」，URL 显示「（未配置端点）」，徽章「未配置」「内置」。

- [ ] **Step 2: 点「自定义」启动，确认被拦截**

点 custom → 终端应显示 `启动失败: 请先编辑 custom provider，填写 API 端点（base_url）`，不 spawn claude。

- [ ] **Step 3: 编辑 custom 填真实端点 + key，启动成功**

点编辑 → 填 base_url（如某个 Anthropic 兼容端点）+ model + key → 保存 → 再点启动 → claude 正常拉起。
