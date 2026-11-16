import type { RightSidebarTab } from "./components/RightSidebarActivityBar";

export type ViewPanel = "files" | "git" | "browser" | "terminal" | "multi_agent";
export interface AppViewMenuProps {
  leftSidebarVisible: boolean;
  rightSidebarVisible: boolean;
  activePanel: ViewPanel | null;
  filesAvailable: boolean;
  terminalAvailable: boolean;
  focused: boolean;
  canFocus: boolean;
  canOverview: boolean;
  fullscreen: boolean;
  fullscreenPending: boolean;
  fullscreenAvailable: boolean;
  onToggleLeft: () => void;
  onToggleRight: () => void;
  onPanel: (panel: ViewPanel) => void;
  onFocus: () => void;
  onOverview: () => void;
  onFullscreen: () => void;
  onMenuChange: (open: boolean) => void;
}

/** 切换已有工作面板与“新建标签”分开，避免查看操作重复创建资源。 */
export function existingViewPanel(
  kind: ViewPanel,
  tabs: readonly RightSidebarTab[],
  active: RightSidebarTab | null,
  last: RightSidebarTab,
): RightSidebarTab | undefined {
  return [active, last, ...tabs].find((tab): tab is RightSidebarTab =>
    tab !== null && tabs.includes(tab) && tab.split(":", 1)[0] === kind,
  );
}
