import { useState } from "react";
import { createRoot } from "react-dom/client";
import { RightSidebarActivityBar, RightSidebarStart, type SidebarTool } from "../../src/components/RightSidebarActivityBar";
import { GeneralSettingsPanel } from "../../src/components/GeneralSettingsPanel";
import { BrowserControlSettingsPanel } from "../../src/components/BrowserControlSettingsPanel";
import "../../src/styles.css";
function Fixture() {
 const [menu, setMenu] = useState(false); const [tool, setTool] = useState<SidebarTool | "">("");
 return <><RightSidebarActivityBar activeTab="start" tabs={[]} onSelect={()=>{}} onCloseTab={()=>{}} onToggleMaximize={()=>{}} maximized={false} menuOpen={menu} onMenuChange={setMenu} onOpenTool={setTool} hasWorkspace={true}/><RightSidebarStart root="/project" onOpenTool={setTool}/><output>{tool}</output><ComputerControlSettingsPanel/><GeneralSettingsPanel/><BrowserControlSettingsPanel/></>;
}
createRoot(document.getElementById("root")!).render(<Fixture/>);
