import { useState } from "react";
import { createRoot } from "react-dom/client";
import { NativeAgentWelcome } from "../../src/components/NativeAgentWelcome";
import type { NativeAgentKind } from "../../src/nativeAgentSwitch";
import "../../src/styles.css";

const kinds: NativeAgentKind[] = ["claude_chat", "codex_chat", "kimi_chat", "opencode_chat", "pi_chat", "deepseek_harness", "zcode"];
function Preview() {
  const [active, setActive] = useState(true);
  const [compact, setCompact] = useState(false);
  const [command, setCommand] = useState("");
  const [filter, setFilter] = useState<NativeAgentKind | "all">("all");
  return <main className={`mascot-qa${compact ? " compact" : ""}`}>
    <style>{`.mascot-qa{padding:24px;background:#1f1f1f;min-height:100vh;color:#eee}.mascot-qa header{display:flex;align-items:center;gap:24px;flex-wrap:wrap;margin-bottom:24px}.mascot-qa header h1{font-size:22px;margin:0}.mascot-qa-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(310px,1fr));gap:20px;max-width:1500px;margin:auto}.mascot-qa article{border:1px solid #3c3c3c;border-radius:18px;padding:18px 10px;container-type:inline-size;min-width:0}.mascot-qa .native-agent-character{width:320px;height:280px}.mascot-qa .native-agent-welcome h1{font-size:22px}.mascot-qa .native-agent-welcome p{font-size:13px}.mascot-qa.compact .mascot-qa-grid{max-width:330px;grid-template-columns:1fr}.mascot-qa output{color:#aaa}@media(max-width:380px){.mascot-qa{padding:10px}.mascot-qa-grid{grid-template-columns:1fr}}`}</style>
    <style>{`.mascot-qa [data-preview-kind="deepseek_harness"] .native-agent-character{width:min(520px,100%);height:280px}`}</style>
    <header><h1>欢迎页角色动画验收</h1>
      <label><input type="checkbox" checked={active} onChange={e => setActive(e.target.checked)} />播放动画</label>
      <label><input type="checkbox" checked={compact} onChange={e => setCompact(e.target.checked)} />窄屏布局</label>
      <label>显示角色 <select value={filter} onChange={e => setFilter(e.target.value as NativeAgentKind | "all")}><option value="all">全部</option>{kinds.map(kind => <option key={kind} value={kind}>{kind === "deepseek_harness" ? "DeepSeek Harness 海豚" : kind}</option>)}</select></label>
      <output aria-live="polite">{command}</output>
    </header>
    <div className="mascot-qa-grid">{kinds.filter(kind => filter === "all" || kind === filter).map(kind => <article key={kind} className="harness-chat native-agent-chat" data-preview-kind={kind}>
      <NativeAgentWelcome kind={kind} active={active} skillsDisabled={false} onSkills={() => setCommand(`${kind} 命令入口已响应`)} />
    </article>)}</div>
  </main>;
}
createRoot(document.getElementById("root")!).render(<Preview />);
