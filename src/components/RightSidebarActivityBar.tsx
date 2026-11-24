import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { FolderSimpleIcon } from "@phosphor-icons/react/dist/csr/FolderSimple";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { TerminalIcon } from "@phosphor-icons/react/dist/csr/Terminal";
import { isTerminalShell, terminalShells, shellLabel } from "../platform";
import type { RightSidebarStartStyle, TerminalShell } from "../types";
import { useEffect, useRef } from "react";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { HouseIcon } from "@phosphor-icons/react/dist/csr/House";
import { CornersOutIcon } from "@phosphor-icons/react/dist/csr/CornersOut";
import { CornersInIcon } from "@phosphor-icons/react/dist/csr/CornersIn";
import { SidebarSimpleIcon } from "@phosphor-icons/react/dist/csr/SidebarSimple";
import {
  RightSidebarStartCreative,
  RightSidebarStartStyleCorner,
} from "./RightSidebarStartCreative";

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
  modeButtonRef?: (button: HTMLButtonElement | null) => void;
  maximized: boolean;
  menuOpen: boolean;
  onMenuChange: (open: boolean) => void;
  onOpenTool: (tool: SidebarTool) => void;
  hasWorkspace: boolean;
}
export function sidebarTabKind(id: string): string { return id.split(":", 1)[0]; }
function ResourceIcon({ id }: { id: string }) {
  id = sidebarTabKind(id);
  if (id === "files") return <FolderSimpleIcon size={16} />;
  if (id === "browser") return <GlobeIcon size={16} />;
  if (id === "git") return <GitBranchIcon size={16} weight="regular" />;
  if (id === "multi_agent") return <RobotIcon size={16} />;
  if (id === "terminal") return <TerminalIcon size={16} aria-hidden="true" />;
  return <HouseIcon size={16} />;
}
export function RightSidebarActivityBar(props: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  const isCollaboration = sidebarTabKind(props.activeTab) === "multi_agent";
  const maximizeLabel = isCollaboration
    ? "Bot模式"
    : (props.maximized ? "恢复工作面板" : "最大化工作面板");
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
        <button ref={props.modeButtonRef} type="button" aria-label={maximizeLabel}
          title={isCollaboration ? maximizeLabel : (props.maximized ? "恢复" : "最大化")} onClick={props.onToggleMaximize}>
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
export function RightSidebarStart({
  root,
  onOpenTool,
  startStyle = "minimal",
  onStartStyleChange,
  onOpenAppearanceSettings,
}: {
  root: string | null;
  onOpenTool: (tool: SidebarTool) => void;
  startStyle?: RightSidebarStartStyle;
  onStartStyleChange?: (style: RightSidebarStartStyle) => void;
  onOpenAppearanceSettings?: () => void;
}) {
  if (startStyle !== "minimal") {
    return (
      <RightSidebarStartCreative
        root={root}
        onOpenTool={onOpenTool}
        startStyle={startStyle}
        onStartStyleChange={onStartStyleChange}
        onOpenAppearanceSettings={onOpenAppearanceSettings}
      />
    );
  }
  return (
    <nav className="right-sidebar-start right-sidebar-start--minimal" aria-label="工作区工具" data-workspace-root={root ?? undefined} data-start-style="minimal">
      <div className="right-sidebar-start-tools">
        <button type="button" disabled={!root} title="浏览工作区文件" onClick={() => onOpenTool("files")}><i className="right-sidebar-start-icon" aria-hidden="true"><FolderSimpleIcon size={17} /></i><span>文件</span><i className="right-sidebar-start-arrow-pill" aria-hidden="true"><CaretRightIcon className="right-sidebar-start-arrow" size={14} /></i></button>
        <button type="button" disabled={!root} title={`在当前工作区运行 ${shellLabel(terminalShells()[0])}`} onClick={() => onOpenTool(terminalShells()[0])}><i className="right-sidebar-start-icon" aria-hidden="true"><TerminalIcon size={17} /></i><span>终端</span><i className="right-sidebar-start-arrow-pill" aria-hidden="true"><CaretRightIcon className="right-sidebar-start-arrow" size={14} /></i></button>
        <button type="button" title="打开网页或搜索内容" onClick={() => onOpenTool("browser")}><i className="right-sidebar-start-icon" aria-hidden="true"><GlobeIcon size={17} /></i><span>浏览器</span><i className="right-sidebar-start-arrow-pill" aria-hidden="true"><CaretRightIcon className="right-sidebar-start-arrow" size={14} /></i></button>
        <button type="button" title="查看 Git 更改与提交记录" onClick={() => onOpenTool("git")}><i className="right-sidebar-start-icon" aria-hidden="true"><GitBranchIcon size={17} weight="regular" /></i><span>Git</span><i className="right-sidebar-start-arrow-pill" aria-hidden="true"><CaretRightIcon className="right-sidebar-start-arrow" size={14} /></i></button>
        <button type="button" title="协调智能体任务与协作进度" onClick={() => onOpenTool("multi_agent")}><i className="right-sidebar-start-icon" aria-hidden="true"><RobotIcon size={17} /></i><span>多智能体</span><i className="right-sidebar-start-arrow-pill" aria-hidden="true"><CaretRightIcon className="right-sidebar-start-arrow" size={14} /></i></button>
      </div>
      {onStartStyleChange && (
        <RightSidebarStartStyleCorner
          startStyle={startStyle}
          onStartStyleChange={onStartStyleChange}
          onOpenAppearanceSettings={onOpenAppearanceSettings}
        />
      )}
    </nav>
  );
}
