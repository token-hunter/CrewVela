import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { RightSidebarStart } from "../../src/components/RightSidebarActivityBar";
import "../../src/styles.css";

function Fixture() {
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [workspace, setWorkspace] = useState(true);
  const [lastTool, setLastTool] = useState("");
  useEffect(() => {
    if (!open) { setExpanded(false); return; }
    // Reproduce mounting the content while the expanding sidebar is still zero-width.
    const timer = setTimeout(() => setExpanded(true), 50);
    return () => clearTimeout(timer);
  }, [open]);
  return <>
    <button data-testid="sidebar-toggle" onClick={() => setOpen(value => !value)}>{open ? "收起" : "展开"}</button>
    <label><input type="checkbox" aria-label="有工作区" checked={workspace} onChange={event => setWorkspace(event.target.checked)} />有工作区</label>
    <output data-testid="last-tool">{lastTool}</output>
    <aside className="unified-right-sidebar" style={{ width: expanded ? 450 : 1, height: 850 }}>
      <div className="unified-right-sidebar-content">
        {open && <RightSidebarStart root={workspace ? "/fixture/workspace" : null} onOpenTool={setLastTool} />}
      </div>
    </aside>
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
