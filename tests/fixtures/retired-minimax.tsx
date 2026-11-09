import { useState } from "react";
import { createRoot } from "react-dom/client";
import { CollaborationAgentEditor } from "../../src/components/CollaborationAgentEditor";
import { RuntimeManagementDialog } from "../../src/components/RuntimeManagementDialog";
import type { AgentProfile } from "../../src/types";
import "../../src/styles.css";
import "../../src/styles/collaboration-overview.css";

const legacy = { id: "old", name: "旧成员", runtimeKind: "minimax_code", enabled: true, description: "历史记录", role: "worker" } as AgentProfile;
function Fixture() {
  const [result, setResult] = useState("");
  const [mode, setMode] = useState(new URLSearchParams(location.search).get("mode") || "editor");
  return <><button onClick={() => setMode("manage")}>运行时管理</button>
    {mode === "editor" ? <CollaborationAgentEditor agent={legacy} runtimeLocked={false} onClose={() => {}} onSave={async draft => { setResult(JSON.stringify(draft)); }} />
      : <RuntimeManagementDialog order={["claude_code", "codex", "minimax_code", "oh_my_pi"]} onClose={() => setMode("editor")} />}
    <output>{result}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
