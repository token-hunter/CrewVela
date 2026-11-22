import { CaretDown, CaretRight, CheckCircle, FileText, Quotes, Terminal, WarningCircle } from "@phosphor-icons/react";
import { useId, useState } from "react";
import type { ChatTurnChanges } from "../types";
import "../styles/session-evidence.css";

function numberedLines(patch: string) {
  let before = 0;
  let after = 0;
  return patch.split("\n").map((text, index) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) {
      before = Number(hunk[1]);
      after = Number(hunk[2]);
    }
    const oldLine = /^[ -]/.test(text) ? before++ : null;
    const newLine = /^[ +]/.test(text) ? after++ : null;
    return { text, index, oldLine, newLine };
  });
}

export function ChatTurnChangesCard({ changes, onQuote }: {
  changes: ChatTurnChanges;
  onQuote: (quote: string) => void;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [fileIndex, setFileIndex] = useState<number | null>(0);
  const [selection, setSelection] = useState<{ path: string; line: string; text: string } | null>(null);
  return <section className="chat-turn-changes">
    <header>
      <button type="button" className="session-evidence-heading" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <FileText size={19} /><strong>本轮改动</strong><small>{changes.files.length} 个文件</small>
        {open ? <CaretDown size={14} /> : <CaretRight size={14} />}
      </button>
      {open && <button type="button" className="btn secondary" disabled={!selection} onClick={() => {
        if (selection) onQuote(`引用 ${selection.path}:${selection.line}（本轮差异）\n> ${selection.text}`);
      }}><Quotes size={16} />引用到输入框</button>}
    </header>
    <div id={id} hidden={!open}>
      {changes.files.map((file, i) => <div className="chat-turn-file" key={file.path}>
        <button type="button" aria-expanded={fileIndex === i} onClick={() => { setFileIndex(i === fileIndex ? null : i); setSelection(null); }}>
          {fileIndex === i ? <CaretDown size={13} /> : <CaretRight size={13} />}<FileText size={15} /><code>{file.path}</code>
        </button>
        {fileIndex === i && (file.patch ? <div className="chat-turn-patch" role="group" aria-label={`${file.path} 的文件差异`}>
          {numberedLines(file.patch).map(line => {
            const label = line.newLine != null ? String(line.newLine) : line.oldLine != null ? `旧 ${line.oldLine}` : null;
            const added = line.text.startsWith("+");
            const removed = line.text.startsWith("-");
            const content = <><span>{line.oldLine}</span><span>{line.newLine}</span><code>{line.text || " "}</code></>;
            return label === null ? <div className="chat-turn-patch-line" key={line.index}>{content}</div>
              : <button type="button" className="chat-turn-patch-line" data-tone={added ? "added" : removed ? "removed" : undefined}
                aria-label={`引用第 ${label} 行 ${line.text}`} aria-pressed={selection?.path === file.path && selection.line === label}
                onClick={() => setSelection({ path: file.path, line: label, text: line.text })} key={line.index}>{content}</button>;
          })}
        </div> : <small className="session-evidence-note">文件已变更，当前没有可显示的文本差异。</small>)}
      </div>)}
      {changes.checks.length > 0 && <ul className="chat-turn-checks">{changes.checks.map((check, index) => <li key={index}>
        <Terminal size={16} /><code>{check.command}</code>
        {check.exitCode === 0 ? <><CheckCircle size={16} className="session-evidence-ok" />命令执行成功</>
          : check.exitCode == null ? <span>已记录命令，退出状态未报告</span>
            : <><WarningCircle size={16} />退出码 {check.exitCode}</>}
      </li>)}</ul>}
      {changes.limited && <small className="session-evidence-note">部分超限、非文本或无法读取的文件未纳入。</small>}
      <small className="session-evidence-note">记录该轮执行期间的工作树差异。</small>
    </div>
  </section>;
}
