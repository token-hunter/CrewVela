export interface WorkspaceTab {
  id: string;
  title: string;
  projectId?: string;
}

export interface WorkspaceTabState {
  tabs: WorkspaceTab[];
  activeTabId: string;
  /** sessionId -> tabId */
  assignments: Record<string, string>;
}

export function findWorkspaceSessionBySlot<T extends { id: string }>(
  sessions: readonly T[],
  tabId: string,
  slotId: string | null | undefined,
  sessionTab: (sessionId: string) => string,
  sessionSlot: (sessionId: string) => string,
): T | null {
  if (slotId === null || slotId === undefined) return null;
  return sessions.find((session) => sessionTab(session.id) === tabId && sessionSlot(session.id) === slotId) ?? null;
}

export const WORKSPACE_TABS_STORAGE_KEY = "crewvela-workspace-tabs";

const DEFAULT_TAB: WorkspaceTab = { id: "tab-default", title: "工作区 1" };

function defaultState(): WorkspaceTabState {
  return { tabs: [DEFAULT_TAB], activeTabId: DEFAULT_TAB.id, assignments: {} };
}

export function loadWorkspaceTabs(): WorkspaceTabState {
  try {
    const raw = window.localStorage.getItem(WORKSPACE_TABS_STORAGE_KEY);
    if (!raw) return defaultState();
    const parsed = JSON.parse(raw) as Partial<WorkspaceTabState>;
    const tabs = Array.isArray(parsed.tabs)
      ? parsed.tabs.filter(
          (tab): tab is WorkspaceTab =>
            typeof tab?.id === "string" && typeof tab?.title === "string"
            && (tab.projectId === undefined || typeof tab.projectId === "string"),
        )
      : [];
    if (tabs.length === 0) return defaultState();
    const activeTabId =
      typeof parsed.activeTabId === "string" &&
      tabs.some((tab) => tab.id === parsed.activeTabId)
        ? parsed.activeTabId
        : tabs[0].id;
    const assignments: Record<string, string> = {};
    if (parsed.assignments && typeof parsed.assignments === "object") {
      for (const [sessionId, tabId] of Object.entries(parsed.assignments)) {
        if (typeof tabId === "string" && tabs.some((tab) => tab.id === tabId)) {
          assignments[sessionId] = tabId;
        }
      }
    }
    return { tabs, activeTabId, assignments };
  } catch {
    return defaultState();
  }
}

export function saveWorkspaceTabs(state: WorkspaceTabState): void {
  try {
    window.localStorage.setItem(WORKSPACE_TABS_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // 当前窗口状态仍然有效。
  }
}

export function createWorkspaceTab(existing: WorkspaceTab[]): WorkspaceTab {
  let index = 1;
  const titles = new Set(existing.map((tab) => tab.title));
  while (titles.has(`工作区 ${index}`)) index += 1;
  const id =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? `tab-${crypto.randomUUID()}`
      : `tab-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  return { id, title: `工作区 ${index}` };
}
