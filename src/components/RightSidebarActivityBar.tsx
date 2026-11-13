import { FolderSimpleIcon } from "@phosphor-icons/react/dist/csr/FolderSimple";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import { isTerminalShell, terminalShells, shellLabel } from "../platform";
import type { TerminalShell } from "../types";
import { useEffect, useRef } from "react";
import { TreeStructureIcon } from "@phosphor-icons/react/dist/csr/TreeStructure";
import { StackIcon } from "@phosphor-icons/react/dist/csr/Stack";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { SquaresFourIcon } from "@phosphor-icons/react/dist/csr/SquaresFour";
import { CodeBlockIcon } from "@phosphor-icons/react/dist/csr/CodeBlock";
import { CornersOutIcon } from "@phosphor-icons/react/dist/csr/CornersOut";
import { CornersInIcon } from "@phosphor-icons/react/dist/csr/CornersIn";
import { SidebarSimpleIcon } from "@phosphor-icons/react/dist/csr/SidebarSimple";

export type RightSidebarTab = "multi_agent" | "files" | "git" | "browser" | "start" | `terminal:${string}` | `${"files" | "git" | "browser" | "multi_agent"}:${string}`;
export type SidebarTool = "files" | "browser" | "multi_agent" | "git" | TerminalShell;
export interface SidebarResourceTab { id: RightSidebarTab; label: string }
interface Props {
  activeTab: RightSidebarTab;
  tabs: SidebarResourceTab[];
  onSelect: (tab: RightSidebarTab) => void;
  onCloseTab: (tab: RightSidebarTab) => void;
  onClose: () => void;
  onToggleMaximize: () => void;
  maximized: boolean;
  menuOpen: boolean;
  onMenuChange: (open: boolean) => void;
  onOpenTool: (tool: SidebarTool) => void;
  hasWorkspace: boolean;
}
export function sidebarTabKind(id: string): string { return id.split(":", 1)[0]; }
function ResourceIcon({ id }: { id: string }) {
  id = sidebarTabKind(id);
  if (id === "files") return <StackIcon size={16} />;
  if (id === "browser") return <GlobeIcon size={16} />;
  if (id === "git") return <TreeStructureIcon size={16} />;
  if (id === "multi_agent") return <RobotIcon size={16} />;
  if (id === "terminal") return <CodeBlockIcon size={16} />;
  return <SquaresFourIcon size={16} />;
}
export function RightSidebarActivityBar(props: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (props.menuOpen) menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [props.menuOpen]);
  const closeMenu = () => { props.onMenuChange(false); addRef.current?.focus(); };
  return (
    <header className="right-sidebar-activity-bar" onKeyDown={event => {
      if (props.menuOpen && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); closeMenu(); }
    }}>
      <div className="right-sidebar-resource-tabs" role="tablist" aria-label="工作面板标签">
        {props.tabs.map(tab => (
          <div className={`right-sidebar-resource-tab${props.activeTab === tab.id ? " selected" : ""}`} key={tab.id}>
            <button type="button" role="tab" aria-selected={props.activeTab === tab.id} title={tab.label}
              onClick={() => props.onSelect(tab.id)}><ResourceIcon id={tab.id} /><span>{tab.label}</span></button>
            <button type="button" className="right-sidebar-tab-close" aria-label={`关闭${tab.label}标签`}
              title={`关闭${tab.label}标签`} onClick={() => props.onCloseTab(tab.id)}><XIcon size={12} /></button>
          </div>
        ))}
      </div>
      <div className="right-sidebar-actions">
        <button ref={addRef} type="button" aria-label="新建工作面板标签" title="新建标签" aria-expanded={props.menuOpen}
          aria-haspopup="menu" onClick={() => props.onMenuChange(!props.menuOpen)}><PlusIcon size={18} /></button>
        <button type="button" aria-label={props.maximized ? "恢复工作面板" : "最大化工作面板"}
          title={props.maximized ? "恢复" : "最大化"} onClick={props.onToggleMaximize}>
          {props.maximized ? <CornersInIcon size={17} /> : <CornersOutIcon size={17} />}</button>
        <button type="button" aria-label="收起工作面板" title="收起" onClick={props.onClose}><SidebarSimpleIcon size={17} /></button>
      </div>
      {props.menuOpen && <>
        <button className="right-sidebar-menu-backdrop" aria-label="关闭新建标签菜单" onClick={closeMenu} />
        <div ref={menuRef} className="right-sidebar-new-menu" role="menu" aria-label="新建标签" onKeyDown={event => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          const buttons = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
          const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
          buttons[(index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
        }}>
          {([
            ["files", "工作区文件"], ...terminalShells().map(shell => [shell, `新建 ${shellLabel(shell)} 终端`] as const),
            ["browser", "浏览器"], ["git", "源代码控制"], ["multi_agent", "多智能体"],
          ] as const).map(([tool, label]) => <button key={tool} role="menuitem" type="button"
            disabled={!props.hasWorkspace && (tool === "files" || isTerminalShell(tool))}
            onClick={() => { closeMenu(); props.onOpenTool(tool); }}>
            <ResourceIcon id={isTerminalShell(tool) ? "terminal:new" : tool} />{label}
          </button>)}
        </div>
      </>}
    </header>
  );
}
export function RightSidebarStart({ root, onOpenTool }: { root: string | null; onOpenTool: (tool: SidebarTool) => void }) {
  return <div className="right-sidebar-start">
    <SquaresFourIcon className="right-sidebar-start-mark" size={46} />
    <h2>工作面板</h2><p>文件、终端、网页与协作，随手切换</p>
    <div className="right-sidebar-start-tools">
      <button type="button" disabled={!root} onClick={() => onOpenTool("files")}><FolderSimpleIcon size={23} /><span><strong>工作区文件</strong><small>浏览和预览当前工作区的文件</small></span><span className="right-sidebar-start-arrow">↗</span></button>
      <button type="button" disabled={!root} onClick={() => onOpenTool(terminalShells()[0])}><TerminalWindowIcon size={23} /><span><strong>新建终端</strong><small>在当前工作区运行 {shellLabel(terminalShells()[0])}</small></span><span className="right-sidebar-start-arrow">↗</span></button>
      <button type="button" onClick={() => onOpenTool("browser")}><GlobeIcon size={23} /><span><strong>浏览器</strong><small>打开网页或搜索内容</small></span><span className="right-sidebar-start-arrow">↗</span></button>
      <button type="button" onClick={() => onOpenTool("git")}><GitBranchIcon size={23} weight="regular" /><span><strong>源代码控制</strong><small>查看 Git 更改与提交记录</small></span><span className="right-sidebar-start-arrow">↗</span></button>
      <button type="button" onClick={() => onOpenTool("multi_agent")}><RobotIcon size={23} /><span><strong>多智能体</strong><small>协调智能体任务与协作进度</small></span><span className="right-sidebar-start-arrow">↗</span></button>
    </div>
    <div className="right-sidebar-start-context" title={root ?? undefined}>{root ? `当前工作区 · ${root}` : "先打开一个工作区，即可使用文件和终端"}</div>
  </div>;
}
