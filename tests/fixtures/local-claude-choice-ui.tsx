import { useState } from "react";
import { createRoot } from "react-dom/client";
import { NewSessionDialog } from "../../src/components/NewSessionDialog";
import type { ProviderConfig } from "../../src/types";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
const providers: ProviderConfig[] = params.get("providers") === "none" ? [] : [{
  id: "fixture-glm", name: "GLM 示例", baseUrl: "https://example.invalid/anthropic", authTokenRef: "",
  env: {}, timeoutMs: null, disableNonessentialTraffic: false, builtin: false,
}];
function Fixture() {
  const [open, setOpen] = useState(true);
  const [selected, setSelected] = useState("");
  return <>
    <button onClick={() => setOpen(true)}>打开新建会话</button>
    <p role="status">{selected}</p>
    {open && <NewSessionDialog providers={providers} keyStates={{}}
      onSelect={id => { setSelected(`Provider：${id}`); setOpen(false); }}
      onLocalClaudeSelect={() => { setSelected("本地 Claude Code：选择工作目录"); setOpen(false); }}
      onRuntimeSelect={runtime => { setSelected(`Runtime：${runtime}`); setOpen(false); }}
      onOpenSettings={() => { setSelected("打开模型设置"); setOpen(false); }} onClose={() => setOpen(false)} />}
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
