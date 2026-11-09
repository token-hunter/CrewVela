# Control Prompt Tab Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在总控输入框中提供可见的示例指令幽灵文本，并在建议可用时支持 Tab 或点击补全。

**Architecture:** 补全状态由 `ControlSidebar` 内的固定建议和当前 `draft` 纯计算得到，不引入新的全局状态或依赖。输入区域增加一层与 textarea 字体、内边距一致的只读幽灵文本；只有补全可见时拦截 Tab，其余快捷键和浏览器焦点导航保持原样。

**Tech Stack:** React 18、TypeScript、CSS、Vite

## Global Constraints

- 默认补全文本必须为 `打开 1 个 GLM 和 2 个 Kimi 会话`。
- 补全只写入输入框，不自动发送。
- 不覆盖与默认建议不匹配的已有输入。
- 原有 `Enter`、`Shift+Enter`、`Ctrl+Shift+Enter` 行为不得改变。
- 不新增历史记录、AI 推荐、多候选列表或第三方依赖。

---

### Task 1: 总控输入框幽灵文本与 Tab 补全

**Files:**
- Modify: `src/components/ControlSidebar.tsx:39-45, 100-130, 373-395, 540-550`
- Modify: `src/styles.css:1044-1076`
- Test: production typecheck/build and focused interaction verification

**Interfaces:**
- Consumes: React `draft`、`busy`、textarea focus state，以及现有 `handleKeyDown`
- Produces: `CONTROL_PROMPT_COMPLETION: string`、`applyPromptCompletion(): void`、可点击的 `.control-prompt-completion-action`

- [x] **Step 1: 添加补全状态的纯计算**

在 `ControlSidebar.tsx` 的常量区定义：

```ts
const CONTROL_PROMPT_COMPLETION = "打开 1 个 GLM 和 2 个 Kimi 会话";
```

在组件中添加 textarea 引用和焦点状态，并计算剩余内容：

```ts
const [composerFocused, setComposerFocused] = useState(false);
const composerRef = useRef<HTMLTextAreaElement>(null);
const completionSuffix =
  CONTROL_PROMPT_COMPLETION.startsWith(draft)
    ? CONTROL_PROMPT_COMPLETION.slice(draft.length)
    : "";
const canCompletePrompt =
  composerFocused && !busy && completionSuffix.length > 0;
```

- [x] **Step 2: 实现键盘与点击补全**

定义补全函数：

```ts
const applyPromptCompletion = () => {
  if (!canCompletePrompt) return;
  setDraft(CONTROL_PROMPT_COMPLETION);
  window.requestAnimationFrame(() => {
    const textarea = composerRef.current;
    textarea?.focus();
    textarea?.setSelectionRange(
      CONTROL_PROMPT_COMPLETION.length,
      CONTROL_PROMPT_COMPLETION.length,
    );
  });
};
```

在 `handleKeyDown` 最前面加入：

```ts
if (
  event.key === "Tab" &&
  !event.shiftKey &&
  !event.ctrlKey &&
  !event.altKey &&
  !event.metaKey &&
  !event.nativeEvent.isComposing &&
  canCompletePrompt
) {
  event.preventDefault();
  applyPromptCompletion();
  return;
}
```

- [x] **Step 3: 增加输入层与幽灵提示**

把 textarea 包装在 `.control-composer-input` 中。textarea 设置 `ref`、焦点事件，并在补全可见时清空 placeholder：

```tsx
<div className="control-composer-input">
  <textarea
    ref={composerRef}
    value={draft}
    rows={3}
    placeholder={
      canCompletePrompt ? "" : `例如：${CONTROL_PROMPT_COMPLETION}`
    }
    aria-label="总控指令"
    aria-describedby={
      canCompletePrompt ? "control-prompt-completion-help" : undefined
    }
    aria-keyshortcuts={canCompletePrompt ? "Tab" : undefined}
    disabled={busy}
    onFocus={() => setComposerFocused(true)}
    onBlur={() => setComposerFocused(false)}
    onChange={(event) => setDraft(event.target.value)}
    onKeyDown={handleKeyDown}
  />
  {canCompletePrompt && (
    <>
      <div className="control-prompt-completion-ghost">
        <span aria-hidden="true">{draft}</span>
        <button
          type="button"
          aria-label={`补全剩余内容：${completionSuffix}`}
          onMouseDown={(event) => event.preventDefault()}
          onClick={applyPromptCompletion}
        >
          {completionSuffix}
        </button>
      </div>
      <button
        id="control-prompt-completion-help"
        className="control-prompt-completion-action"
        type="button"
        aria-label={`补全为：${CONTROL_PROMPT_COMPLETION}`}
        onMouseDown={(event) => event.preventDefault()}
        onClick={applyPromptCompletion}
      >
        Tab 补全
      </button>
    </>
  )}
</div>
```

- [x] **Step 4: 添加深浅主题兼容样式**

在 `src/styles.css` 的 composer 样式附近增加：

```css
.control-composer-input {
  position: relative;
}

.control-composer {
  --control-composer-font: 12px/1.55 "Segoe UI", system-ui, sans-serif;
}

.control-prompt-completion-ghost {
  position: absolute;
  top: 10px;
  right: 11px;
  left: 11px;
  pointer-events: none;
  overflow: hidden;
  color: transparent;
  font: var(--control-composer-font);
  white-space: pre-wrap;
}

.control-prompt-completion-ghost button {
  display: inline;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
  font: inherit;
  opacity: 0.72;
  pointer-events: auto;
  text-align: left;
}

.control-prompt-completion-action {
  position: absolute;
  right: 8px;
  bottom: 7px;
  padding: 3px 6px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--bg-elev);
  color: var(--text-dim);
  cursor: pointer;
  font: 700 10px/1 var(--display-font);
}

.control-prompt-completion-action:hover {
  border-color: var(--accent);
  color: var(--accent);
}
```

- [x] **Step 5: 运行验证**

运行：

```powershell
npm.cmd run build
```

预期：TypeScript 和 Vite 构建成功。

交互验收：

1. 空输入框聚焦后出现建议和 `Tab 补全`。
2. 空输入框或输入 `打开 1 个` 后按 Tab，填入完整建议且不发送。
3. 输入 `检查项目` 后按 Tab，焦点移动且内容不变。
4. 点击 `Tab 补全` 后填入完整建议并保持输入框焦点。
5. Enter、Shift+Enter、Ctrl+Shift+Enter 行为与修改前一致。

- [x] **Step 6: 提交**

```powershell
git add src/components/ControlSidebar.tsx src/styles.css docs/superpowers/plans/2026-07-28-control-prompt-tab-completion.md
git commit -m "feat(control): add prompt tab completion"
```
