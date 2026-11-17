import { useState } from "react";
import { createRoot } from "react-dom/client";
import { CliLaunchDialog } from "../../src/components/CliLaunchDialog";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
function Fixture() {
  const [open, setOpen] = useState(true);
  return <>
    <button onClick={() => setOpen(true)}>打开新建会话</button>
    {open && <CliLaunchDialog initialRuntime="claude_code" projects={[]} onLaunch={async () => { throw Error("No directory selected; must not launch"); }} onClose={() => setOpen(false)} />}
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
