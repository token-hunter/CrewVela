import { useEffect, useId, useRef, useState } from "react";
import { contextUsageDetails, formatContextTokens, type CodexContextUsage as Usage } from "../codexContextUsage";
import "../styles/codex-context-usage.css";

export function CodexContextUsage({ usage }: { usage?: Usage | null }) {
  const [pinned, setPinned] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const id = useId();
  const details = contextUsageDetails(usage);
  const open = !dismissed && (pinned || hovered || focused);
  useEffect(() => {
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) { setPinned(false); setFocused(false); setDismissed(true); }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") { setPinned(false); setDismissed(true); }
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", escape); };
  }, []);
  return <div className="codex-context-picker" ref={root}
    onMouseEnter={() => { setHovered(true); setDismissed(false); }} onMouseLeave={() => setHovered(false)}
    onFocus={() => { setFocused(true); setDismissed(false); }} onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false); }}>
    <button type="button" className="harness-picker codex-context-trigger" data-tone={details && details.percent >= 90 ? "high" : details && details.percent >= 75 ? "warning" : "normal"}
      aria-label={details ? `上下文已用 ${details.percent}%` : "上下文用量暂不可用"} aria-expanded={open} aria-controls={id} aria-haspopup="dialog"
      onClick={() => { setPinned(value => !value); setDismissed(pinned); }}>
      <svg width="18" height="18" viewBox="0 0 20 20" aria-hidden="true"><circle className="codex-context-ring-track" cx="10" cy="10" r="7" /><circle className="codex-context-ring-value" cx="10" cy="10" r="7" pathLength="100" strokeDasharray={`${details?.ringPercent ?? 0} 100`} transform="rotate(-90 10 10)" /></svg>
      <span>{details ? `${details.percent}%` : "—"}</span>
    </button>
    {open && <div className="codex-context-popover" id={id} role="dialog" aria-label="上下文窗口详情">
      <strong>上下文窗口</strong>
      {details ? <><div className="codex-context-headline">{details.percent}% 已用</div><div className="codex-context-progress" aria-hidden="true"><i style={{ width: `${details.ringPercent}%` }} /></div>
        <dl><div><dt>已用 Token</dt><dd title={details.used.toLocaleString("en-US")}>{formatContextTokens(details.used)}</dd></div><div><dt>总容量</dt><dd title={details.size.toLocaleString("en-US")}>{formatContextTokens(details.size)}</dd></div><div><dt>剩余比例</dt><dd>{details.remainingPercent}%</dd></div></dl><p>当前上下文用量，自动压缩后会更新。</p></> : <><div className="codex-context-headline">用量暂不可用</div><p>等待 Codex 返回上下文用量。</p></>}
    </div>}
  </div>;
}
