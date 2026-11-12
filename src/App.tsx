import { attentionOpenMode, type AttentionKind } from "./sidebarAttention";
import { isTerminalShell, shellLabel } from "./platform";
import { sidebarTabKind } from "./components/RightSidebarActivityBar";
import "./components/native-chat-tab-sort.css";
import { useRuntimeAvailability } from "./runtimeAvailability";
import { isRetiredRuntime } from "./runtimeTaxonomy";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { FolderSimpleIcon } from "@phosphor-icons/react/dist/csr/FolderSimple";
import orangeMascot from "./assets/notification-orange-mascot.png";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  memo,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppAppearance,
  AppTheme,
  LaunchPreset,
  Project,
  ProviderConfig,
  SessionRecord,
  SessionTypeFilter,
  SessionUsage,
  TerminalShell,
  InterventionRecord,
  AgentRuntimeKind,
} from "./types";
import {
  loadAppearance,
  resolveAppearanceTheme,
  resolveBaseTheme,
  saveAppearance,
} from "./appearance";
import { AppSidebar } from "./components/AppSidebar";
import { CommunicationDiscoveryHint, CommunicationGuide } from "./components/CommunicationGuide";
import { COMMUNICATION_GUIDE_EVENT, communicationSessions, markCommunicationGuideSeen, resolveCommunicationGuideRequest, type CommunicationGuideRequest, type CommunicationScope } from "./communicationGuide";
import { TerminalPane } from "./components/TerminalPane";
import { usePtyPolling } from "./ptyPolling";
import { keepIfUnchanged } from "./stablePollState";
import { MultiTerminal } from "./components/MultiTerminal";
import { ZCodeChat } from "./components/ZCodeChat";
import { ClaudeChat } from "./components/ClaudeChat";
import { CodexChat } from "./components/CodexChat";
import { KimiChat } from "./components/KimiChat";
import { OpenCodeChat } from "./components/OpenCodeChat";
import { PiChat } from "./components/PiChat";
import { DeepSeekHarnessChat } from "./components/DeepSeekHarnessChat";
import { createNativeAgentConversation, nativeChatTabSessions, type NativeAgentKind } from "./nativeAgentSwitch";
import { loadNativeChatTabOrders, orderedNativeChatSlots, reorderNativeChatSlots, saveNativeChatTabOrders, type NativeChatTabOrders } from "./nativeChatTabOrder";
import { NativeChatTabs } from "./components/NativeChatTabs";
import { HandoffSourceView } from "./components/HandoffSourceView";
import "./styles/test-profile.css";
import { RUNTIME_LABELS } from "./runtimeLogos";
import { MultiAgentWorkspace } from "./components/MultiAgentWorkspace";
import { ProviderEditor } from "./components/ProviderEditor";
import { CountPrompt } from "./components/CountPrompt";
import { hasOpenModal } from "./components/ui/Modal";
import { MultiDirPrompt } from "./components/MultiDirPrompt";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { AddProjectDialog } from "./components/AddProjectDialog";
import { TranscriptViewer } from "./components/TranscriptViewer";
import { SessionSwitchDialog } from "./components/SessionSwitchDialog";
import { SWITCH_RUNTIME_LABELS, isStructuredZCodeCli, needsSwitchRecovery, projectSwitchSessions, retainedSwitchSource, switchSlotId, switchSelectionAfterSnapshot, switchWorkspaceAssignment, switchedHistory, type SessionSwitchRecord } from "./sessionSwitch";
import { isPtySession, isSessionActive, isNativeChatSession, isStandaloneHarnessSession, isStandaloneClaudeChatSession, isStandaloneCodexChatSession, isStandaloneKimiChatSession, isStandaloneOpenCodeChatSession, isStandalonePiChatSession, isStandaloneZCodeSession } from "./sessionCapabilities";
import {
  SettingsDialog,
  type SettingsSection,
} from "./components/SettingsDialog";
import { ControlSidebar } from "./components/ControlSidebar";
import { GitSourceControlPanel } from "./components/GitSettingsPanel";
import { FileExplorerPanel } from "./components/FileExplorerPanel";
import { BrowserPanel } from "./components/BrowserPanel";
import { saveBrowserPanelState } from "./browserPanelState";
import { WorkspaceTabs } from "./components/WorkspaceTabs";
import {
  RightSidebarActivityBar,
  RightSidebarStart,
  type SidebarTool,
  type RightSidebarTab,
} from "./components/RightSidebarActivityBar";
import { normalizeRightSidebarTab } from "./teamMessages";
import { ToastStack, type ToastItem } from "./components/Toast";
import { DesktopPet } from "./components/DesktopPet";
import { SkinAtmosphere } from "./components/SkinAtmosphere";
import { DynamicSkinLayer } from "./components/DynamicSkinLayer";
import {
  loadPetSettings,
  savePetSettings,
  type PetSettings,
} from "./petSettings";
import {
  loadCompanionSettings,
  saveCompanionSettings,
  type CompanionSettings,
} from "./companionSettings";
import { findDeepSkin } from "./deepSkins";
import { loadCustomSkins } from "./customSkins";
import {
  loadControlSurface,
  saveControlSurface,
  type ControlSurface,
} from "./controlSurface";
import { useSessionData } from "./hooks/useSessionData";
import { notifySessionTransitions } from "./sessionNotifications";
import {
  initializeNotificationActions,
  deliverDesktopNotice,
} from "./notificationCenter";
import {
  loadNotificationSettings,
  saveNotificationSettings,
  type NotificationSettings,
} from "./notificationSettings";
import {
  createWorkspaceTab,
  findWorkspaceSessionBySlot,
  loadWorkspaceTabs,
  saveWorkspaceTabs,
  type WorkspaceTab,
  type WorkspaceTabState,
} from "./workspaceTabs";

const MemoControlSidebar = memo(ControlSidebar);
const MemoGitSourceControlPanel = memo(GitSourceControlPanel);
const MemoFileExplorerPanel = memo(FileExplorerPanel);
const MemoBrowserPanel = memo(BrowserPanel);
const MemoAppSidebar = memo(AppSidebar);
const MemoMultiTerminal = memo(MultiTerminal);
const MemoWorkspaceTabs = memo(WorkspaceTabs);

const UI_PREVIEW =
  (window.location.hostname === "localhost" ||
    window.location.hostname === "127.0.0.1") &&
  new URLSearchParams(window.location.search).get("ui-preview") === "1";
const RIGHT_SIDEBAR_TAB_STORAGE_KEY = "crewvela-right-sidebar-tab";
const RIGHT_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-right-sidebar-width";
const FILE_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-file-sidebar-width";
const CONTROL_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-control-sidebar-width";
const GIT_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-git-sidebar-width";
const BROWSER_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-browser-sidebar-width";
const LEFT_SIDEBAR_WIDTH_STORAGE_KEY = "crewvela-left-sidebar-width";
const DEFAULT_LEFT_SIDEBAR_WIDTH = 292;
const DEFAULT_RIGHT_SIDEBAR_WIDTH = 380;

function parseRightSidebarWidth(raw: string | null, maxWidth: number): number | null {
  if (raw === null || raw.trim() === "") return null;
  const width = Number(raw);
  if (!Number.isFinite(width)) return null;
  return Math.max(0, Math.min(width, maxWidth));
}

function loadRightSidebarWidth(): number {
  let fallbackWidth = DEFAULT_RIGHT_SIDEBAR_WIDTH;
  try {
    const sharedWidth = parseRightSidebarWidth(
      window.localStorage.getItem(RIGHT_SIDEBAR_WIDTH_STORAGE_KEY),
      window.innerWidth,
    );
    if (sharedWidth !== null) return sharedWidth;

    const tabKind = sidebarTabKind(loadRightSidebarTab());
    const legacyStorageKey =
      tabKind === "files"
        ? FILE_SIDEBAR_WIDTH_STORAGE_KEY
        : tabKind === "git"
          ? GIT_SIDEBAR_WIDTH_STORAGE_KEY
          : tabKind === "browser"
            ? BROWSER_SIDEBAR_WIDTH_STORAGE_KEY
            : CONTROL_SIDEBAR_WIDTH_STORAGE_KEY;
    fallbackWidth =
      parseRightSidebarWidth(
        window.localStorage.getItem(legacyStorageKey),
        window.innerWidth,
      ) ?? DEFAULT_RIGHT_SIDEBAR_WIDTH;
  } catch {
    // 持久化不可用时仍使用统一默认宽度，并继续尝试保存该回退值。
  }
  saveRightSidebarWidth(fallbackWidth);
  return fallbackWidth;
}

function clampSidebarWidth(width: number, maxWidth: number): number {
  return Math.max(0, Math.min(maxWidth, width));
}

function saveRightSidebarWidth(width: number): void {
  try {
    window.localStorage.setItem(RIGHT_SIDEBAR_WIDTH_STORAGE_KEY, String(width));
  } catch {
    // 当前窗口仍可继续使用调整后的宽度。
  }
}

function loadLeftSidebarWidth(): number {
  try {
    const raw = window.localStorage.getItem(LEFT_SIDEBAR_WIDTH_STORAGE_KEY);
    if (raw === null) return DEFAULT_LEFT_SIDEBAR_WIDTH;
    const width = Number(raw);
    if (!Number.isFinite(width)) return DEFAULT_LEFT_SIDEBAR_WIDTH;
    return Math.max(0, Math.min(width, window.innerWidth * 0.6));
  } catch {
    return DEFAULT_LEFT_SIDEBAR_WIDTH;
  }
}

function loadRightSidebarTab(): RightSidebarTab {
  try {
    const value = window.localStorage.getItem(RIGHT_SIDEBAR_TAB_STORAGE_KEY);
    if (value === null) return "start";
    const tab = value === "start" ? "start" : normalizeRightSidebarTab(value);
    if (value === "group_chat") window.localStorage.setItem(RIGHT_SIDEBAR_TAB_STORAGE_KEY, sidebarTabKind(tab));
    return tab;
  } catch {
    // 当前窗口仍可使用默认页签。
  }
  return "start";
}

const PREVIEW_PROVIDERS: ProviderConfig[] = [
  {
    id: "kimi-k2",
    name: "Kimi K3",
    baseUrl: "https://api.kimi.com/coding/",
    authTokenRef: "KIMI_API_KEY",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
  },
  {
    id: "glm-5.2",
    name: "GLM-5.2",
    baseUrl: "https://open.bigmodel.cn/api/anthropic",
    authTokenRef: "GLM_API_KEY",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
  },
  {
    id: "deepseek-v4",
    name: "DeepSeek-V4",
    baseUrl: "https://api.deepseek.com/anthropic",
    authTokenRef: "DEEPSEEK_API_KEY",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
  },
  {
    id: "minimax-m3",
    name: "MiniMax-M3",
    baseUrl: "https://api.minimaxi.com/anthropic",
    authTokenRef: "MINIMAX_API_KEY",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
  },
  {
    id: "custom-preview",
    name: "自定义",
    baseUrl: "",
    authTokenRef: "",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: false,
  },
];

const PREVIEW_KEY_STATES: Record<string, boolean> = {
  "kimi-k2": true,
  "glm-5.2": true,
  "deepseek-v4": true,
  "minimax-m3": true,
};

const PREVIEW_SESSIONS: SessionRecord[] = [
  {
    id: "preview-deepseek-1",
    claudeSessionId: "preview-deepseek-1",
    kind: "claude",
    shell: null,
    providerId: "deepseek-v4",
    providerName: "DeepSeek-V4",
    cwd: "C:\\Users\\demo\\project-alpha",
    title: "需求拆解",
    status: "running",
    ptyId: 9101,
    yolo: false,
    autoMode: false,
    permissionMode: "default",
    titleLocked: true,
    exitCode: null,
    lastError: null,
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: "preview-kimi-1",
    claudeSessionId: "preview-kimi-1",
    kind: "claude",
    shell: null,
    providerId: "kimi-k2",
    providerName: "Kimi-K2",
    cwd: "C:\\Users\\demo\\project-beta",
    title: "接口检查",
    status: "running",
    ptyId: 9102,
    yolo: false,
    autoMode: false,
    permissionMode: "default",
    titleLocked: true,
    exitCode: null,
    lastError: null,
    createdAt: 2,
    updatedAt: 2,
  },
  {
    id: "preview-powershell-1",
    claudeSessionId: "",
    kind: "terminal",
    shell: "powershell",
    providerId: "local-terminal",
    providerName: "PowerShell",
    cwd: "D:\\projects\\crewvela",
    title: "PowerShell",
    status: "running",
    ptyId: 9103,
    yolo: false,
    autoMode: false,
    permissionMode: "default",
    titleLocked: true,
    exitCode: null,
    lastError: null,
    createdAt: 3,
    updatedAt: 3,
  },
  {
    id: "preview-cmd-1",
    claudeSessionId: "",
    kind: "terminal",
    shell: "cmd",
    providerId: "local-terminal",
    providerName: "CMD",
    cwd: "D:\\projects\\crewvela",
    title: "CMD · 构建输出",
    status: "running",
    ptyId: 9104,
    yolo: false,
    autoMode: false,
    permissionMode: "default",
    titleLocked: true,
    exitCode: null,
    lastError: null,
    createdAt: 4,
    updatedAt: 4,
  },
  {
    id: "preview-harness-session",
    claudeSessionId: "",
    kind: "terminal",
    shell: null,
    agentRuntimeKind: "deepseek_harness",
    providerId: "local-runtime-deepseek_harness",
    providerName: "DeepSeek Harness",
    cwd: "C:\\example\\crewvela",
    title: "DeepSeek Harness · ACP 预览",
    status: "running",
    ptyId: null,
    yolo: false,
    autoMode: false,
    permissionMode: null,
    titleLocked: true,
    exitCode: null,
    lastError: null,
    createdAt: 5,
    updatedAt: 5,
  },
];

const PREVIEW_PROJECTS: Project[] = [
  {
    id: "preview-crewvela",
    name: "crewvela",
    path: "C:\\example\\crewvela",
    badgeColor: "#8b5cf6",
    createdAt: 1,
    updatedAt: 1,
  },
];

export default function App() {
  const [testProfile, setTestProfile] = useState<{dataDir: string} | null>(null);
  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed=false;
    void invoke<{dataDir:string} | null>("test_profile_info").then(value => { if (!disposed) setTestProfile(value); }).catch(error => console.error("读取测试模式信息失败",error));
    return () => { disposed=true; };
  }, []);
  const { isRuntimeEnabled } = useRuntimeAvailability();
  const [providers, setProviders] = useState<ProviderConfig[]>(
    UI_PREVIEW ? PREVIEW_PROVIDERS : [],
  );
  const [editing, setEditing] = useState<ProviderConfig | null>(null);
  const [keyStates, setKeyStates] = useState<Record<string, boolean>>(
    UI_PREVIEW ? PREVIEW_KEY_STATES : {},
  );
  const [sessions, setSessions] = useState<SessionRecord[]>(
    UI_PREVIEW && new URLSearchParams(window.location.search).get("sessions") !== "empty" ? new URLSearchParams(window.location.search).get("communication-preview") === "1" ? [
      { ...PREVIEW_SESSIONS[0], id: "communication-claude", title: "前端助手", providerName: "Claude Code", cwd: "C:/projects/demo-project" },
      { ...PREVIEW_SESSIONS[0], id: "communication-codex", title: "代码审查员", kind: "terminal", providerId: "local-runtime-codex", providerName: "Codex CLI", agentRuntimeKind: "codex", ptyId: 9105, cwd: "C:/projects/demo-project" },
    ] : PREVIEW_SESSIONS : [],
  );
  const [archivedSessions, setArchivedSessions] = useState<SessionRecord[]>([]);
  const [switchRecords, setSwitchRecords] = useState<SessionSwitchRecord[]>([]);
  const [switchRecordsReady, setSwitchRecordsReady] = useState(UI_PREVIEW);
  const [switchFor, setSwitchFor] = useState<SessionRecord | null>(null);
  const [switchTarget, setSwitchTarget] = useState<NativeAgentKind | undefined>();
  const [switchDraft, setSwitchDraft] = useState<SessionSwitchRecord | undefined>();
  const displaySessions = useMemo(() => projectSwitchSessions(sessions, switchRecords), [sessions, switchRecords]);
  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    let pending = false;
    const load = async () => {
      if (pending) return;
      pending = true;
      try {
        const items = await invoke<SessionSwitchRecord[]>("session_switch_list");
        if (!disposed) { setSwitchRecords((current) => keepIfUnchanged(current, items)); setSwitchRecordsReady(true); }
      } catch (error) {
        console.error("加载 Agent 接力记录失败:", error);
      } finally { pending = false; }
    };
    void load(); const timer = window.setInterval(() => void load(), 2000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, []);
  const openSwitch = useCallback((source: SessionRecord) => {
    setSwitchTarget(undefined); setSwitchDraft(retainedSwitchSource(source.id, switchRecords) ?? undefined); setSwitchFor(source);
  }, [switchRecords]);
  const sourceHistory = useCallback((session: SessionRecord) => switchedHistory(session.id, switchRecords), [switchRecords]);
  const logicalSessionId = useCallback((id: string) => switchSlotId(id, switchRecords), [switchRecords]);
  const openRelayDetails = useCallback((session: SessionRecord) => {
    const record = retainedSwitchSource(session.id, switchRecords) ?? switchRecords.find((item) => item.targetId === session.id);
    if (record) { setSwitchDraft(record); setSwitchFor(record.source); }
  }, [switchRecords]);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(
    UI_PREVIEW && new URLSearchParams(window.location.search).get("harness") === "empty" ? "preview-harness-session" : null,
  );
  const [loadingSessionId, setLoadingSessionId] = useState<string | null>(null);
  const pendingSessionResumes = useRef(new Set<string>());
  const [visitedNativeSessions, setVisitedNativeSessions] = useState<string[]>([]);
  const [nativeTabOrderError, setNativeTabOrderError] = useState<string | null>(null);
  const [nativeTabOrders, setNativeTabOrders] = useState<NativeChatTabOrders>(() => {
    try { return loadNativeChatTabOrders(window.localStorage); }
    catch { return {}; }
  });
  useEffect(() => {
    try { loadNativeChatTabOrders(window.localStorage); }
    catch { setNativeTabOrderError("无法读取聊天标签顺序，当前使用默认顺序。"); }
  }, []);
  const [openNativeSlots, setOpenNativeSlots] = useState<string[]>([]);
  const restoredHandoffTabs = useRef(false);
  useEffect(() => {
    if (!selectedSessionId || !sessions.some(item => item.id === selectedSessionId && isNativeChatSession(item))) return;
    const slot = switchSlotId(selectedSessionId, switchRecords);
    setOpenNativeSlots(current => current.includes(slot) ? current : [...current, slot]);
  }, [selectedSessionId, sessions, switchRecords]);
  const [switchingNativeAgent, setSwitchingNativeAgent] = useState(false);
  const nativeAgentNavigationPending = useRef(false);
  useEffect(() => {
    setVisitedNativeSessions(previous => {
      const next = previous.filter(id => sessions.some(item => item.id === id && isNativeChatSession(item)));
      if (selectedSessionId && sessions.some(item => item.id === selectedSessionId && isNativeChatSession(item)) && !next.includes(selectedSessionId)) next.push(selectedSessionId);
      return next.length === previous.length && next.every((id, index) => id === previous[index]) ? previous : next;
    });
  }, [sessions, selectedSessionId]);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sessionTypeFilter, setSessionTypeFilter] =
    useState<SessionTypeFilter>("all");
  const [newSessionOpen, setNewSessionOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [guideRequest, setGuideRequest] = useState<(CommunicationGuideRequest & { scope: CommunicationScope; nonce: number }) | null>(null);
  const guideNonce = useRef(0);
  const [collaborationInitialTab, setCollaborationInitialTab] = useState<"overview" | "diagnostics">("overview");
  const [collaborationDiagnosticGlobal, setCollaborationDiagnosticGlobal] = useState(false);
  const [collaborationInitialProjectId, setCollaborationInitialProjectId] = useState<string | undefined>();
  const [settingsTarget, setSettingsTarget] = useState<SettingsSection>("providers");
  const [addProjectOpen, setAddProjectOpen] = useState(false);
  const [projects, setProjects] = useState<Project[]>(
    UI_PREVIEW ? PREVIEW_PROJECTS : [],
  );
  const [presets, setPresets] = useState<LaunchPreset[]>([]);
  const [usages, setUsages] = useState<Record<string, SessionUsage>>({});
  const [transcriptFor, setTranscriptFor] = useState<SessionRecord | null>(
    null,
  );
  const [controlOpen, setControlOpen] = useState(false);
  const [multiAgentWorkspaceProjectId, setMultiAgentWorkspaceProjectId] =
    useState<string | null>(UI_PREVIEW && new URLSearchParams(window.location.search).get("workspace-preview") === "1" ? "preview-collaboration-project" : null);
  const [controlDetached, setControlDetached] = useState(false);
  const [controlSurface, setControlSurface] =
    useState<ControlSurface>(loadControlSurface);
  const [gitOpen, setGitOpen] = useState(false);
  const [filesOpen, setFilesOpen] = useState(false);
  const [browserOpen, setBrowserOpen] = useState(false);
  const [leftSidebarWidth, setLeftSidebarWidth] = useState(loadLeftSidebarWidth);
  const [rightSidebarDesiredWidth, setRightSidebarDesiredWidth] = useState(loadRightSidebarWidth);
  const leftSidebarResizeRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const rightSidebarResizeRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [lastRightSidebarTab, setLastRightSidebarTab] =
    useState<RightSidebarTab>(loadRightSidebarTab);
  const [mountedRightSidebarTabs, setMountedRightSidebarTabs] = useState(
    new Set<RightSidebarTab>(),
  );
  const [openRightSidebarTabs, setOpenRightSidebarTabs] = useState<RightSidebarTab[]>(["start"]);
  const [extraRightSidebarTab, setExtraRightSidebarTab] = useState<RightSidebarTab | null>(null);
  const [sidebarTerminalIds, setSidebarTerminalIds] = useState<string[]>([]);
  const [sidebarMenuOpen, setSidebarMenuOpen] = useState(false);
  const [sidebarMaximized, setSidebarMaximized] = useState(false);
  const sidebarTerminalLaunchRef = useRef(false);
  const workspaceRootRef = useRef<string | null>(null);
  const activeRightSidebarTab: RightSidebarTab | null = gitOpen
      ? "git"
      : filesOpen
        ? "files"
        : browserOpen
          ? "browser"
          : controlOpen && !controlDetached
              ? "multi_agent"
              : extraRightSidebarTab;
  const activeRightSidebarTabRef = useRef(activeRightSidebarTab);
  activeRightSidebarTabRef.current = activeRightSidebarTab;
  const openRightSidebarTabsRef = useRef(openRightSidebarTabs);
  openRightSidebarTabsRef.current = openRightSidebarTabs;
  const rightSidebarOpen = activeRightSidebarTab !== null;
  const [workspaceAvailableWidth, setWorkspaceAvailableWidth] = useState(window.innerWidth);
  const rightSidebarWidth = Math.min(rightSidebarDesiredWidth, workspaceAvailableWidth);
  const rightSidebarStyle = {
    "--right-sidebar-width": `${rightSidebarWidth}px`,
  } as CSSProperties;
  useEffect(() => {
    const host = document.querySelector<HTMLElement>(".main-workspace");
    if (!host) return;
    const update = () => setWorkspaceAvailableWidth(host.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);
  const appShellStyle = {
    "--left-sidebar-width": `${leftSidebarWidth}px`,
  } as CSSProperties;
  const openSettingsSection = useCallback(
    (section: SettingsSection) => {
      setCollaborationInitialTab("overview");
      setCollaborationDiagnosticGlobal(false);
      setCollaborationInitialProjectId(undefined);
      setSettingsTarget(section);
      setSettingsOpen(true);
    },
    [],
  );
  const toastIdRef = useRef(0);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const showError = useCallback((message: string) => {
    toastIdRef.current += 1;
    const id = toastIdRef.current;
    setToasts((current) => [...current, { id, message }]);
  }, []);
  const dismissToast = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);
  useEffect(() => {
    const openGuide = (event: Event) => {
      const detail = (event as CustomEvent<CommunicationGuideRequest>).detail ?? {};
      setGuideRequest({ ...resolveCommunicationGuideRequest(detail, multiAgentWorkspaceProjectId), nonce: ++guideNonce.current });
      setSettingsOpen(false);
      try { markCommunicationGuideSeen(); } catch { showError("无法保存通信提示偏好，本次已收起提示，下次启动可能再次显示。"); }
    };
    window.addEventListener(COMMUNICATION_GUIDE_EVENT, openGuide);
    return () => window.removeEventListener(COMMUNICATION_GUIDE_EVENT, openGuide);
  }, [multiAgentWorkspaceProjectId, showError]);
  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    const subscription = listen<string>("app-shutdown-error", event => {
      if (!disposed) showError(`无法安全退出：${event.payload}`);
    }).catch(error => {
      console.error("退出状态监听失败:", error);
      return () => {};
    });
    return () => {
      disposed = true;
      void subscription.then(unlisten => unlisten()).catch(error => console.error("退出状态监听失败:", error));
    };
  }, [showError]);

  const {
    sessionsReady,
    refreshProviders,
    refreshSessions,
    refreshProjects,
    refreshPresets,
  } = useSessionData({
    uiPreview: UI_PREVIEW,
    setProviders,
    setKeyStates,
    setSessions,
    setArchivedSessions,
    setProjects,
    setPresets,
    setControlDetached,
    setControlSurface,
    setLastRightSidebarTab,
    setFilesOpen,
    setControlOpen,
  });

  useEffect(() => {
    if (restoredHandoffTabs.current || !sessionsReady || !switchRecordsReady) return;
    restoredHandoffTabs.current = true;
    const retained = switchRecords.filter(item => item.preserveSource && !["preview", "cancelled"].includes(item.state));
    const slots = retained.flatMap(item => [item.source.id, item.targetId]).filter((id): id is string => !!id && sessions.some(session => session.id === id && isNativeChatSession(session))).map(id => switchSlotId(id, switchRecords));
    setOpenNativeSlots(current => [...new Set([...current, ...slots])]);
  }, [sessionsReady, switchRecordsReady, switchRecords, sessions]);

  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    const unlisten = listen<{ projectId: string }>(
      "multi-agent-workspace-open",
      (event) => {
        if (!disposed && event.payload?.projectId)
          setMultiAgentWorkspaceProjectId(event.payload.projectId);
      },
    ).catch(error => {
      if (!disposed) showError(`协作工作区事件监听失败：${String(error)}`);
    });
    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose?.()).catch(error => console.error("释放协作工作区监听失败:", error));
    };
  }, [showError]);
  const [pendingLaunchProviderId, setPendingLaunchProviderId] = useState<
    string | null
  >(null);
  const [appearance, setAppearance] = useState<AppAppearance>(loadAppearance);
  const [petSettings, setPetSettings] = useState<PetSettings>(loadPetSettings);
  const [companionSettings, setCompanionSettings] = useState<CompanionSettings>(
    loadCompanionSettings,
  );
  const [notificationSettings, setNotificationSettings] =
    useState<NotificationSettings>(loadNotificationSettings);
  const [systemPrefersDark, setSystemPrefersDark] = useState(
    () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true,
  );
  const resolvedAppearanceTheme = resolveAppearanceTheme(
    appearance,
    systemPrefersDark,
  );
  const theme: AppTheme = resolveBaseTheme(resolvedAppearanceTheme);

  useEffect(() => {
    try { savePetSettings(petSettings); }
    catch (reason) { showError(`桌宠设置未能保存，当前修改仅在本次使用中生效：${String(reason)}`); }
  }, [petSettings, showError]);

  // 定时自动化心跳：pty_poll_all 只在有多终端会话时被调用，调度器挂在
  // 它上面会在零会话时停摆；这里常驻轻量 tick 驱动时间型自动化。
  useEffect(() => {
    const timer = window.setInterval(() => {
      void invoke("automation_tick").catch(() => {});
    }, 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    try { saveCompanionSettings(companionSettings); }
    catch (reason) { showError(`陪伴设置未能保存，当前修改仅在本次使用中生效：${String(reason)}`); }
  }, [companionSettings, showError]);

  useEffect(() => {
    try { saveNotificationSettings(notificationSettings); }
    catch (reason) { showError(`通知设置未能保存，当前修改仅在本次使用中生效：${String(reason)}`); }
  }, [notificationSettings, showError]);

  useEffect(() => {
    if (UI_PREVIEW) return;
    void initializeNotificationActions();
  }, []);

  const notifiedInterventionsRef = useRef(new Set<string>());
  useEffect(() => {
    if (
      UI_PREVIEW ||
      !notificationSettings.enabled ||
      (!notificationSettings.waitingForUser && !notificationSettings.taskFailed)
    )
      return;
    let disposed = false;
    const poll = async () => {
      try {
        const items = await invoke<InterventionRecord[]>(
          "ma_list_interventions",
        );
        if (disposed) return;
        const openIds = new Set(
          items.filter((item) => item.status === "open").map((item) => item.id),
        );
        for (const id of [...notifiedInterventionsRef.current])
          if (!openIds.has(id)) notifiedInterventionsRef.current.delete(id);
        for (const item of items) {
          if (
            item.status !== "open" ||
            notifiedInterventionsRef.current.has(item.id)
          )
            continue;
          notifiedInterventionsRef.current.add(item.id);
          const isFailure =
            item.kind === "retry_exhausted" || item.kind === "semantic_review";
          if (isFailure && !notificationSettings.taskFailed) continue;
          if (!isFailure && !notificationSettings.waitingForUser) continue;
          if (notificationSettings.suppressWhenFocused && document.hasFocus())
            continue;
          void deliverDesktopNotice({
            id: `intervention:${item.id}`,
            title: isFailure
              ? `CrewVela · ${item.kind === "semantic_review" ? "审核需要处理" : "任务重试失败"}`
              : "CrewVela · 等待人工处理",
            body: `${item.projectName}：${item.title}`,
            category: "intervention",
          });
        }
      } catch {
        // 后台轮询失败时等待下一轮，不影响会话。
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 5000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [
    notificationSettings.enabled,
    notificationSettings.waitingForUser,
    notificationSettings.taskFailed,
    notificationSettings.suppressWhenFocused,
  ]);
  const [countPromptFor, setCountPromptFor] = useState<ProviderConfig | null>(
    null,
  );
  // 项目下新建会话流程：记录目标项目，目录选择随之锁定为项目路径。
  const [launchProject, setLaunchProject] = useState<Project | null>(null);
  const [dirPromptFor, setDirPromptFor] = useState<{
    provider: ProviderConfig;
    count: number;
    autoMode: boolean;
    yolo: boolean;
    initialPrompt: string;
    project: Project | null;
  } | null>(null);
  const [workspace, setWorkspace] =
    useState<WorkspaceTabState>(loadWorkspaceTabs);
  const closingWorkspaceRef = useRef(false);
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const selectedSessionByWorkspace = useRef<Record<string, string | null>>({});

  useEffect(() => {
    saveWorkspaceTabs(workspace);
  }, [workspace]);
  useEffect(() => {
    selectedSessionByWorkspace.current[workspace.activeTabId] = selectedSessionId === null
      ? null
      : switchSlotId(selectedSessionId, switchRecords);
  }, [selectedSessionId, switchRecords, workspace.activeTabId]);
  const nativeViewLocation = useRef({ sessionId: selectedSessionId, tabId: workspace.activeTabId, tabIds: workspace.tabs.map(tab => tab.id) });
  nativeViewLocation.current = { sessionId: selectedSessionId, tabId: workspace.activeTabId, tabIds: workspace.tabs.map(tab => tab.id) };

  // 会话状态跃迁 → 系统通知（waiting/error 总通知，completed 仅窗口失焦时）。
  const prevSessionsRef = useRef<SessionRecord[]>([]);
  useEffect(() => {
    if (UI_PREVIEW) return;
    notifySessionTransitions(prevSessionsRef.current, sessions);
    prevSessionsRef.current = sessions;
  }, [sessions]);

  // token 用量：低频轮询（增量解析 transcript，5s 一轮足够跟手）。
  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    const tick = async () => {
      const [sessionResult] = await Promise.allSettled([
        invoke<Record<string, SessionUsage>>("session_usages"),
      ]);
      if (!disposed) {
        if (sessionResult.status === "fulfilled") {
          const next = sessionResult.value;
          setUsages((current) => keepIfUnchanged(current, next));
        }
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 5000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, []);

  // 等待首次会话和接力快照后再清理持久化归属，避免启动时的空数组抹掉工作区。
  useEffect(() => {
    if (!sessionsReady || !switchRecordsReady) return;
    setWorkspace((current) => {
      const known = new Set([...sessions.map((session) => session.id), ...switchRecords.filter((item) => item.state !== "cancelled").map((item) => item.source.id)]);
      const tabIds = new Set(current.tabs.map((tab) => tab.id));
      let changed = false;
      const assignments: Record<string, string> = {};
      for (const [sessionId, tabId] of Object.entries(current.assignments)) {
        if (known.has(sessionId) && tabIds.has(tabId)) {
          assignments[sessionId] = tabId;
        } else {
          changed = true;
        }
      }
      for (const session of sessions) {
        if (!(session.id in assignments)) {
          const logicalId = switchSlotId(session.id, switchRecords);
          assignments[session.id] = switchWorkspaceAssignment(logicalId, current.assignments, switchRecords, current.activeTabId);
          changed = true;
        }
      }
      return changed ? { ...current, assignments } : current;
    });
  }, [sessions, switchRecords, sessionsReady, switchRecordsReady]);

  const tabOf = useCallback(
    (sessionId: string) =>
      switchWorkspaceAssignment(sessionId, workspace.assignments, switchRecords, workspace.tabs[0].id),
    [workspace, switchRecords],
  );
  const rememberedWorkspaceSession = useCallback((tabId: string) => {
    return findWorkspaceSessionBySlot(
      displaySessions,
      tabId,
      selectedSessionByWorkspace.current[tabId],
      tabOf,
      (sessionId) => switchSlotId(sessionId, switchRecords),
    );
  }, [displaySessions, switchRecords, tabOf]);

  const selectTab = useCallback(
    (tabId: string) => {
      if (tabId === workspace.activeTabId) return;
      selectedSessionByWorkspace.current[workspace.activeTabId] = selectedSessionId === null
        ? null
        : switchSlotId(selectedSessionId, switchRecords);
      const restoredSession = rememberedWorkspaceSession(tabId);
      setWorkspace((current) => ({ ...current, activeTabId: tabId }));
      setSelectedSessionId(restoredSession?.id ?? null);
      if (restoredSession && (isNativeChatSession(restoredSession) || isStructuredZCodeCli(restoredSession, switchRecords))) {
        setMultiAgentWorkspaceProjectId(null);
      }
    },
    [rememberedWorkspaceSession, selectedSessionId, switchRecords, workspace.activeTabId],
  );

  const addTab = useCallback(() => {
    const tab = createWorkspaceTab(workspace.tabs);
    setWorkspace((current) => ({
      ...current,
      tabs: [...current.tabs, tab],
      activeTabId: tab.id,
    }));
    setSelectedSessionId(null);
  }, [workspace.tabs]);

  const closeTab = useCallback(async (tab: WorkspaceTab) => {
    if (!sessionsReady || !switchRecordsReady) { showError("会话数据正在加载，请稍后再关闭工作区。"); return; }
    if (closingWorkspaceRef.current || workspaceRef.current.tabs.length <= 1) return;
    if (!workspaceRef.current.tabs.some(item => item.id === tab.id)) return;
    const owned = sessions.filter((session) => tabOf(session.id) === tab.id);
    if (
      owned.length > 0 &&
      !window.confirm(
        `关闭页签「${tab.title}」将停止并删除其 ${owned.length} 个会话。`,
      )
    ) {
      return;
    }
    closingWorkspaceRef.current = true;
    const removed = new Set<string>();
    let failed = false;
    try {
    for (const session of owned) {
      try {
        if (!UI_PREVIEW) await invoke("delete_session", { sessionId: session.id });
        removed.add(session.id);
      } catch (error) {
        failed = true;
        showError(`删除「${session.title}」失败：${String(error)}`);
      }
    }
    setSessions(current => current.filter(session => !removed.has(session.id)));
    const newSessions = sessionsRef.current.some(session =>
      !owned.some(original => original.id === session.id) &&
      workspaceRef.current.assignments[session.id] === tab.id);
    if (failed || newSessions) {
      if (newSessions) showError("该工作区出现了新会话，已保留页签；请确认后再次关闭。");
      await refreshSessions();
      return;
    }
    delete selectedSessionByWorkspace.current[tab.id];
    const activeTabId = nativeViewLocation.current.tabId;
    const activeTabIds = nativeViewLocation.current.tabIds;
    const closingActiveTab = activeTabId === tab.id;
    const closingIndex = activeTabIds.indexOf(tab.id);
    const nextActiveTabId = closingActiveTab
      ? activeTabIds.filter((id) => id !== tab.id)[Math.max(0, closingIndex - 1)]
      : undefined;
    const restoredSession = nextActiveTabId
      ? rememberedWorkspaceSession(nextActiveTabId)
      : null;
    setWorkspace((current) => {
      if (current.tabs.length <= 1 || !current.tabs.some(item => item.id === tab.id)) return current;
      const index = current.tabs.findIndex((item) => item.id === tab.id);
      const tabs = current.tabs.filter((item) => item.id !== tab.id);
      const assignments = Object.fromEntries(
        Object.entries(current.assignments).filter(
          ([, tabId]) => tabId !== tab.id,
        ),
      );
      const activeTabId =
        current.activeTabId === tab.id
          ? tabs[Math.max(0, index - 1)].id
          : current.activeTabId;
      return { tabs, activeTabId, assignments };
    });
    if (closingActiveTab) {
      setSelectedSessionId(restoredSession?.id ?? null);
      if (restoredSession && (isNativeChatSession(restoredSession) || isStructuredZCodeCli(restoredSession, switchRecords))) {
        setMultiAgentWorkspaceProjectId(null);
      }
    }
    await refreshSessions();
    } finally { closingWorkspaceRef.current = false; }
  }, [
    refreshSessions,
    sessions,
    sessionsReady,
    switchRecordsReady,
    showError,
    rememberedWorkspaceSession,
    tabOf,
    workspace.activeTabId,
    workspace.tabs.length,
  ]);

  const renameTab = useCallback((tab: WorkspaceTab, title: string) => {
    setWorkspace((current) => ({
      ...current,
      tabs: current.tabs.map((item) =>
        item.id === tab.id ? { ...item, title } : item,
      ),
    }));
  }, []);

  // 派生集合（useMemo 保持引用稳定，配合下方 memo 组件跳过重渲染）。
  const tabSessions = useMemo(
    () =>
      displaySessions.filter(
        (session) => tabOf(session.id) === workspace.activeTabId,
      ),
    [displaySessions, tabOf, workspace.activeTabId],
  );
  const liveSessions = useMemo(
    () => tabSessions.filter(item => !sidebarTerminalIds.includes(item.id) && isSessionActive(item)),
    [tabSessions, sidebarTerminalIds],
  );
  const anyLivePtySessions = sessions.some(item => !sidebarTerminalIds.includes(item.id) && isPtySession(item));
  const hiddenSessionIds = useMemo(
    () =>
      new Set(
        sessions
          .filter((session) => tabOf(session.id) !== workspace.activeTabId)
          .map((session) => session.id),
      ),
    [sessions, tabOf, workspace.activeTabId],
  );


  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) return;
    const update = (event: MediaQueryListEvent) =>
      setSystemPrefersDark(event.matches);
    setSystemPrefersDark(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.appearanceTheme = resolvedAppearanceTheme;
    document.documentElement.dataset.appearanceBackground =
      appearance.background;
    document.documentElement.dataset.appearanceAccent = appearance.accent;
    document.documentElement.dataset.terminalTransparent =
      appearance.terminalTransparent === true ? "true" : "false";
    document.documentElement.dataset.globalTransparent =
      appearance.terminalTransparent === true ? "true" : "false";
    document.documentElement.style.setProperty(
      "--terminal-pane-opacity",
      `${Math.round((appearance.terminalOpacity ?? 0.65) * 100)}%`,
    );
    const deepSkin = findDeepSkin(companionSettings.customSkinId);
    const customSkin = loadCustomSkins().find(
      (skin) => skin.id === companionSettings.customSkinId,
    );
    if (deepSkin) {
      document.documentElement.dataset.deepSkin = deepSkin.id;
      document.documentElement.style.setProperty(
        "--deep-skin-background",
        `url("${deepSkin.background}")`,
      );
    } else if (customSkin?.media) {
      document.documentElement.dataset.deepSkin = customSkin.id;
      if (customSkin.media.previewPath) {
        document.documentElement.style.setProperty(
          "--deep-skin-background",
          `url("${convertFileSrc(customSkin.media.previewPath)}")`,
        );
      } else {
        document.documentElement.style.setProperty(
          "--deep-skin-background",
          "none",
        );
      }
    } else {
      delete document.documentElement.dataset.deepSkin;
      document.documentElement.style.removeProperty("--deep-skin-background");
    }
    try { saveAppearance(appearance, theme); }
    catch (reason) { showError(`外观设置未能保存，当前修改仅在本次使用中生效：${String(reason)}`); }
  }, [
    appearance,
    companionSettings.customSkinId,
    resolvedAppearanceTheme,
    theme,
    showError,
  ]);

  useEffect(() => {
    const onEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // Agent 提及候选先处理 Esc，避免捕获阶段直接收起整个工作面板。
      if (event.target instanceof Element && event.target.closest('.multi-agent-composer-editor [role="combobox"][aria-expanded="true"]')) return;
      // 捕获阶段先放行展开的新建聊天菜单，由其关闭菜单并归还焦点。
      if (event.target instanceof Element && event.target.closest(".native-chat-create")?.querySelector(".native-chat-create-menu")) return;
      // SessionList 的会话操作菜单自行关闭 Esc，避免全局逻辑先清空当前会话选择。
      if (document.querySelector(".session-action-menu")) return;
      if (document.querySelector(".session-filter-menu")) return;
      if (event.target instanceof HTMLInputElement && event.target.closest(".session-search") && event.target.value) return;
      if (event.target instanceof HTMLInputElement && event.target.classList.contains("workspace-tab-input")) return;
      if (event.target instanceof HTMLInputElement && event.target.closest(".browser-address")) return;
      // 侧栏直接新建菜单先消费 Esc，保留当前会话选择。
      if (document.querySelector(".direct-new-menu")) return;
      if (document.querySelector(".right-sidebar-new-menu")) return;
      // Harness 编辑区自行关闭命令菜单，避免输入时退出会话视图。
      if (event.target instanceof Element && event.target.closest(".harness-chat-composer, .harness-goal-dialog")) return;
      // Modal 栈接管的弹窗（ProviderEditor / CountPrompt / MultiDirPrompt /
      // NewSessionDialog / AddProjectDialog / TranscriptViewer / SettingsDialog）
      // 自行消费 Esc，栈顶先关；栈非空时本链一律放行，保持原有优先级不回退。
      if (hasOpenModal()) return;
      // ControlSidebar 自行管理 Esc 语义，不拦截。
      if (controlOpen) return;
      if (selectedSessionId !== null) {
        event.preventDefault();
        event.stopPropagation();
        setSelectedSessionId(null);
      }
    };
    window.addEventListener("keydown", onEscape, { capture: true });
    return () =>
      window.removeEventListener("keydown", onEscape, { capture: true });
  }, [controlOpen, selectedSessionId]);

  const handleLaunch = (id: string) => {
    const p = providers.find((x) => x.id === id);
    if (!p) return;
    if (p.hidden) {
      showError("该模型已隐藏，请先在模型设置中重新显示。");
      return;
    }
    if (!(keyStates[id] ?? false)) {
      setPendingLaunchProviderId(id);
      setEditing(p);
      return;
    }
    if (!p.baseUrl) {
      setPendingLaunchProviderId(id);
      setEditing(p); // custom 未配置
      return;
    }
    setPendingLaunchProviderId(null);
    setCountPromptFor(p);
  };

  const handleCountConfirm = (
    count: number,
    options: { autoMode: boolean; yolo: boolean; initialPrompt: string },
  ) => {
    const p = countPromptFor;
    if (!p) return;
    setCountPromptFor(null);
    setDirPromptFor({ provider: p, count, ...options, project: launchProject });
  };

  const handleDirsConfirm = async (
    cwds: string[],
    options: { useWorktree: boolean; savePresetAs: string | null },
  ) => {
    const ctx = dirPromptFor;
    if (!ctx) return;
    if (!isRuntimeEnabled("claude_code")) { showError("Claude Code 未启用或配置尚未加载"); return; }
    setDirPromptFor(null);
    setLaunchProject(null);
    if (UI_PREVIEW) {
      const created = Array.from({ length: ctx.count }, (_, index): SessionRecord => ({ ...PREVIEW_SESSIONS[0], id: `preview-claude-${Date.now()}-${index}`, agentRuntimeKind: "claude_code", providerId: ctx.provider.id, providerName: ctx.provider.name, title: `${ctx.provider.name}${ctx.project ? ` · ${ctx.project.name}` : ""}`, cwd: cwds[index % cwds.length] ?? ctx.project?.path ?? PREVIEW_SESSIONS[0].cwd, projectId: ctx.project?.id ?? null, ptyId: 990 + index, status: "running", worktree: null }));
      setSessions(items => [...created, ...items]);
      setSelectedSessionId(created.length === 1 ? created[0].id : null);
      return;
    }
    try {
      const launched = await invoke<SessionRecord[]>("launch_multi", {
        providerId: ctx.provider.id,
        cwds,
        count: ctx.count,
        yolo: ctx.yolo,
        autoMode: ctx.autoMode,
        useWorktree: options.useWorktree,
        initialPrompt: ctx.initialPrompt.trim() ? ctx.initialPrompt : null,
        projectId: ctx.project?.id ?? null,
      });
      await refreshSessions();
      setSelectedSessionId(launched.length === 1 ? launched[0].id : null);
    } catch (e) {
      console.error("launch_multi 失败:", e);
      showError(`启动失败：${String(e)}`);
      await refreshSessions();
    }
    // 预设保存不阻塞启动结果：启动失败也可以存预设（参数本身有效）。
    if (options.savePresetAs) {
      try {
        await invoke<LaunchPreset>("create_launch_preset", {
          name: options.savePresetAs,
          providerId: ctx.provider.id,
          cwds,
          yolo: ctx.yolo,
          autoMode: ctx.autoMode,
          useWorktree: options.useWorktree,
          initialPrompt: ctx.initialPrompt.trim() ? ctx.initialPrompt : null,
        });
        await refreshPresets();
      } catch (e) {
        console.error("保存预设失败:", e);
        showError(`保存预设失败：${String(e)}`);
      }
    }
  };

  const handleLaunchPreset = useCallback(async (preset: LaunchPreset) => {
    if (!isRuntimeEnabled("claude_code")) { showError("Claude Code 未启用或配置尚未加载"); return; }
    const provider = providers.find((item) => item.id === preset.providerId);
    if (provider?.hidden) {
      showError(
        `预设「${preset.name}」使用的模型已隐藏，请先在模型设置中重新显示。`,
      );
      return;
    }
    try {
      const launched = await invoke<SessionRecord[]>("launch_multi", {
        providerId: preset.providerId,
        cwds: preset.cwds,
        count: preset.cwds.length,
        yolo: preset.yolo,
        autoMode: preset.autoMode,
        useWorktree: preset.useWorktree,
        initialPrompt: preset.initialPrompt ?? null,
        projectId: null,
      });
      await refreshSessions();
      setSelectedSessionId(launched.length === 1 ? launched[0].id : null);
    } catch (e) {
      console.error("按预设启动失败:", e);
      showError(`按预设「${preset.name}」启动失败：${String(e)}`);
      await refreshSessions();
    }
  }, [providers, refreshSessions, showError]);

  const handleDeletePreset = useCallback(async (preset: LaunchPreset) => {
    if (!window.confirm(`删除预设「${preset.name}」？`)) return;
    try {
      await invoke("delete_launch_preset", { id: preset.id });
      await refreshPresets();
    } catch (e) {
      showError(`删除预设失败：${String(e)}`);
    }
  }, [refreshPresets, showError]);

  const handleAddProject = async (name: string, path: string) => {
    try {
      const created = await invoke<Project>("create_project", {
        name: name || null,
        path,
      });
      await refreshProjects();
      return created;
    } catch (error) {
      showError(`添加项目失败：${String(error)}`);
      return null;
    }
  };

  const handleNewSessionInProject = useCallback((project: Project) => {
    setLaunchProject(project);
    setNewSessionOpen(true);
  }, []);

  const handleRenameProject = useCallback(async (project: Project) => {
    const name = window.prompt("项目名称", project.name)?.trim();
    if (!name || name === project.name) return;
    try {
      await invoke<Project>("update_project", {
        id: project.id,
        name,
        badgeColor: null,
      });
      await refreshProjects();
    } catch (error) {
      showError(`重命名项目失败：${String(error)}`);
    }
  }, [refreshProjects, showError]);

  const handleDeleteProject = useCallback(async (project: Project) => {
    if (
      !window.confirm(
        `删除项目「${project.name}」？\n项目下的会话会保留并回到未分组，目录不受影响。`,
      )
    ) {
      return;
    }
    try {
      await invoke("delete_project", { id: project.id });
      await Promise.all([refreshProjects(), refreshSessions()]);
    } catch (error) {
      showError(`删除项目失败：${String(error)}`);
    }
  }, [refreshProjects, refreshSessions, showError]);

  const handleNew = () => {
    setEditing({
      id: "",
      name: "",
      baseUrl: "",
      authTokenRef: "",
      env: {},
      timeoutMs: null,
      disableNonessentialTraffic: false,
      builtin: false,
    });
  };

  const handleSave = async (p: ProviderConfig, key?: string) => {
    const continueLaunch = pendingLaunchProviderId === p.id;
    await invoke("save_provider", { provider: p, key: key ?? null });
    setPendingLaunchProviderId(null);
    setEditing(null);
    await refreshProviders();
    if (continueLaunch) setCountPromptFor(p);
  };

  const handleDelete = async (id: string) => {
    if (!window.confirm(`删除 provider“${id}”？`)) return;
    try {
      await invoke("delete_provider", { id });
      await refreshProviders();
    } catch (error) {
      showError(`删除模型服务失败：${String(error)}`);
    }
  };

  const handleProviderHidden = async (id: string, hidden: boolean) => {
    try {
      await invoke("set_provider_hidden", { id, hidden });
      await refreshProviders();
    } catch (error) {
      showError(`${hidden ? "隐藏" : "显示"}模型失败：${String(error)}`);
    }
  };

  const handleRestoreBuiltinProvider = async (id: string) => {
    if (!window.confirm("恢复该内置模型的默认配置？API Key 将保留。")) return;
    try {
      await invoke("restore_builtin_provider", { id });
      await refreshProviders();
    } catch (error) {
      showError(`恢复默认失败：${String(error)}`);
    }
  };
  const providerReorderInFlight = useRef(false);
  const handleProviderReorder = async (providerIds: string[]) => {
    if (providerReorderInFlight.current) return;
    const previousIds = providers.map(provider => provider.id);
    const providerById = new Map(
      providers.map((provider) => [provider.id, provider]),
    );
    const reordered = providerIds
      .map((providerId) => providerById.get(providerId))
      .filter((provider): provider is ProviderConfig => provider !== undefined);
    if (reordered.length !== providers.length || new Set(providerIds).size !== providers.length) return;

    providerReorderInFlight.current = true;
    setProviders(reordered);
    try {
      await invoke("reorder_providers", { providerIds });
    } catch (error) {
      setProviders(current => {
        const currentById = new Map(current.map(provider => [provider.id, provider]));
        const restored = previousIds.map(id => currentById.get(id)).filter((provider): provider is ProviderConfig => !!provider);
        const previousSet = new Set(previousIds);
        return [...restored, ...current.filter(provider => !previousSet.has(provider.id))];
      });
      console.error("保存 Provider 顺序失败:", error);
      showError(`保存 Provider 顺序失败：${String(error)}`);
    } finally {
      providerReorderInFlight.current = false;
    }
  };

  const selectableProviders = providers.filter((provider) => !provider.hidden);

  const selectSession = useCallback(async (id: string | null) => {
    if (id === null) {
      setSelectedSessionId(null);
      return;
    }
    const session = sessions.find((item) => item.id === id);
    if (!session) return;
    if (session.retiredAgentRuntime || isRetiredRuntime(session.agentRuntimeKind)) { setSelectedSessionId(id); return; }
    if (sidebarTerminalIds.includes(id)) {
      try {
        if (!isSessionActive(session)) {
          const resumed = UI_PREVIEW ? { ...session, ptyId: Date.now(), status: "running" as const }
            : await invoke<SessionRecord>("resume_session", { sessionId: id });
          setSessions(items => items.map(item => item.id === id ? resumed : item));
        }
        const tab: RightSidebarTab = `terminal:${id}`;
        setOpenRightSidebarTabs(current => current.includes(tab) ? current : [...current, tab]);
        setMountedRightSidebarTabs(current => new Set([...current, tab]));
        setControlOpen(false); setGitOpen(false); setFilesOpen(false); setBrowserOpen(false);
        setExtraRightSidebarTab(tab);
        setSidebarMenuOpen(false);
        setLastRightSidebarTab(tab);
      } catch (error) { showError(`恢复终端失败：${String(error)}`); }
      return;
    }
    const nativeChat = isNativeChatSession(session);
    // 立即响应选择；恢复进程的迟到响应不能抢回用户后来选择的会话或工作区。
    setSelectedSessionId(id);
    if (nativeChat || isStructuredZCodeCli(session, switchRecords)) setMultiAgentWorkspaceProjectId(null);
    if (retainedSwitchSource(id, switchRecords)) { setMultiAgentWorkspaceProjectId(null); return; }
    if (nativeChat) {
      setSelectedSessionId(id);
      // 首次挂载由聊天面板异步连接，并在目标面板显示连接状态和错误。
      if (!visitedNativeSessions.includes(id) && session.agentRuntimeKind !== "zcode") return;
    }
    if (isSessionActive(session)) {
      if (!nativeChat) setSelectedSessionId(id);
      return;
    }

    const runtime = session.agentRuntimeKind ?? (session.kind === "claude" ? "claude_code" : null);
    if (runtime && !isRuntimeEnabled(runtime)) { setSelectedSessionId(id); return; }
    if (pendingSessionResumes.current.has(id)) return;
    pendingSessionResumes.current.add(id);
    setLoadingSessionId(id);
    try {
      const resumed = await invoke<SessionRecord>("resume_session", {
        sessionId: id,
      });
      setSessions((items) =>
        items.map((item) => (item.id === resumed.id ? resumed : item)),
      );
    } catch (error) {
      showError(`恢复会话失败：${String(error)}`);
      await refreshSessions();
    } finally {
      pendingSessionResumes.current.delete(id);
      setLoadingSessionId(current => current === id ? null : current);
    }
  }, [isRuntimeEnabled, refreshSessions, sessions, sidebarTerminalIds, showError, switchRecords, visitedNativeSessions]);

  const renameSession = useCallback(async (session: SessionRecord) => {
    const title = window.prompt("会话标题", session.title)?.trim();
    if (!title || title === session.title) return;
    try {
      const updated = await invoke<SessionRecord>("rename_session", {
        sessionId: session.id,
        title,
      });
      setSessions((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
    } catch (error) {
      showError(`重命名失败：${String(error)}`);
    }
  }, [showError]);

  const stopSession = useCallback(async (session: SessionRecord) => {
    try {
      const updated = await invoke<SessionRecord>("stop_session", {
        sessionId: session.id,
      });
      setSessions((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
    } catch (error) {
      showError(`停止会话失败：${String(error)}`);
    }
  }, [showError]);

  const deleteSession = useCallback(async (session: SessionRecord) => {
    if (!window.confirm(`从列表删除会话“${session.title}”？`)) return;
    try {
      await invoke("delete_session", { sessionId: session.id });
      setSessions((items) => items.filter((item) => item.id !== session.id));
      setSelectedSessionId(current => current === session.id ? null : current);
    } catch (error) {
      showError(`删除会话失败：${String(error)}`);
    }
  }, [selectedSessionId, showError]);

  const archiveSession = useCallback(async (session: SessionRecord) => {
    if (
      !window.confirm(
        `归档会话「${session.title}」？\n归档后可在“设置 → 归档会话”中查看和恢复。`,
      )
    )
      return;
    try {
      await invoke<SessionRecord>("archive_session", { sessionId: session.id });
      setSelectedSessionId(current => current === session.id ? null : current);
      await refreshSessions();
    } catch (error) {
      showError(`归档会话失败：${String(error)}`);
    }
  }, [refreshSessions, selectedSessionId, showError]);

  const restoreArchivedSession = async (session: SessionRecord) => {
    try {
      const restored = await invoke<SessionRecord>("restore_archived_session", {
        sessionId: session.id,
      });
      await refreshSessions();
      setSelectedSessionId(restored.id);
    } catch (error) {
      showError(`恢复会话失败：${String(error)}`);
    }
  };

  const deleteArchivedSession = async (session: SessionRecord) => {
    if (
      !window.confirm(
        `永久删除已归档会话「${session.title}」？\n此操作无法撤销，会话记录也将被删除。`,
      )
    ) {
      return;
    }
    try {
      await invoke("delete_session", { sessionId: session.id });
      if (transcriptFor?.id === session.id) setTranscriptFor(null);
      await refreshSessions();
    } catch (error) {
      showError(`永久删除归档会话失败：${String(error)}`);
    }
  };

  const archiveProjectSessions = useCallback(async (project: Project) => {
    const count = sessions.filter(
      (session) => session.projectId === project.id,
    ).length;
    if (count === 0) return;
    if (
      !window.confirm(
        `归档项目「${project.name}」下的 ${count} 个会话？\n项目和目录不会被删除。`,
      )
    )
      return;
    try {
      await invoke<number>("archive_project_sessions", {
        projectId: project.id,
      });
      setSelectedSessionId(current => sessions.some(session =>
        session.id === current && session.projectId === project.id) ? null : current);
      await refreshSessions();
    } catch (error) {
      showError(`归档项目会话失败：${String(error)}`);
    }
  }, [refreshSessions, selectedSessionId, sessions, showError]);

  const stopAll = useCallback(async () => {
    for (const session of liveSessions) {
      try {
        await invoke("stop_session", { sessionId: session.id });
      } catch (error) {
        console.error("停止会话失败:", error);
        showError(`停止「${session.title}」失败：${String(error)}`);
      }
    }
    await refreshSessions();
  }, [liveSessions, refreshSessions, showError]);

  const deleteAllSessions = useCallback(async () => {
    if (tabSessions.length === 0) return;
    const removed = new Set<string>();
    for (const session of tabSessions) {
      try {
        await invoke("delete_session", { sessionId: session.id });
        removed.add(session.id);
      } catch (error) {
        // 常见原因：会话 Worktree 有未提交改动，后端拒绝删除。
        showError(`删除「${session.title}」失败：${String(error)}`);
      }
    }
    setSelectedSessionId(current => current && removed.has(current) ? null : current);
    await refreshSessions();
  }, [refreshSessions, showError, tabSessions]);

  const launchTerminal = useCallback(async (shell: TerminalShell) => {
    if (UI_PREVIEW) return;
    const current = sessions.find(
      (session) => session.id === selectedSessionId,
    );
    const picked = await open({
      directory: true,
      multiple: false,
      title: `选择 ${shellLabel(shell)} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const launched = await invoke<SessionRecord>("launch_terminal", {
        shell,
        cwd: picked,
      });
      setSessions((items) => [
        launched,
        ...items.filter((item) => item.id !== launched.id),
      ]);
      setSessionTypeFilter("all");
      setSelectedSessionId(null);
    } catch (error) {
      showError(`启动终端失败：${String(error)}`);
      await refreshSessions();
    }
  }, [refreshSessions, selectedSessionId, sessions, showError]);

  const launchRuntimeSession = useCallback(async (
    runtimeKind: Extract<
      AgentRuntimeKind,
      | "claude_code"
      | "codex"
      | "kimi_code"
      | "pi"
      | "opencode"
      | "zcode_cli"
    >,
    project?: Project,
  ) => {
    if (!isRuntimeEnabled(runtimeKind)) { showError("该 Runtime 未启用或配置尚未加载"); return; }
    const current = sessions.find(
      (session) => session.id === selectedSessionId,
    );
    const runtimeLabel = {
      claude_code: "Claude Code",
      codex: "Codex CLI",
      kimi_code: "Kimi Code",
      pi: "Pi Agent",
      opencode: "OpenCode",
      zcode_cli: "ZCode CLI",
    }[runtimeKind];
    if (UI_PREVIEW) {
      const launched: SessionRecord = { ...PREVIEW_SESSIONS[0], id: `preview-${runtimeKind}-${Date.now()}`, kind: "terminal", shell: null, agentRuntimeKind: runtimeKind, claudeSessionId: "", providerId: `local-runtime-${runtimeKind}`, providerName: runtimeLabel, title: `${runtimeLabel}${project ? ` · ${project.name}` : ""}`, cwd: project?.path ?? current?.cwd ?? PREVIEW_SESSIONS[0].cwd, projectId: project?.id ?? null, ptyId: 999, status: "running", yolo: false, autoMode: false, worktree: null };
      setSessions(items => [launched, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(null);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${runtimeLabel} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const launched = await invoke<SessionRecord>("launch_runtime_session", {
        runtimeKind,
        cwd: picked,
        projectId: project?.id ?? null,
      });
      setSessions((items) => [
        launched,
        ...items.filter((item) => item.id !== launched.id),
      ]);
      setSessionTypeFilter("all");
      setSelectedSessionId(null);
    } catch (error) {
      showError(`启动 ${runtimeLabel} 失败：${String(error)}`);
      await refreshSessions();
    }
  }, [refreshSessions, selectedSessionId, sessions, showError]);

  const openDeepSeekHarness = useCallback(async (project?: Project) => {
    if (!isRuntimeEnabled("deepseek_harness")) { showError("DeepSeek Harness 未启用或配置尚未加载"); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-harness-${Date.now()}` : "preview-harness-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, ...(project ? { cwd: project.path, projectId: project.id, title: `DeepSeek Harness · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: "选择 DeepSeek Harness 工作目录",
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_deepseek_harness_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 DeepSeek Harness 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openZCode = useCallback(async (project?: Project) => {
    const label = "ZCode Desktop";
    if (!isRuntimeEnabled("zcode")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-zcode-${Date.now()}` : "preview-zcode-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "zcode", providerId: "local-runtime-zcode", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_zcode_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openClaudeChat = useCallback(async (project?: Project) => {
    const label = "Claude Code Chat";
    if (!isRuntimeEnabled("claude_chat")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-claude-${Date.now()}` : "preview-claude-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "claude_chat", providerId: "local-runtime-claude_chat", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_claude_chat_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openCodexChat = useCallback(async (project?: Project) => {
    // codex_chat 原生聊天持独立启停开关；适配器驱动本机 codex CLI。
    const label = "Codex Chat";
    if (!isRuntimeEnabled("codex_chat")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-codex-${Date.now()}` : "preview-codex-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "codex_chat", providerId: "local-runtime-codex_chat", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_codex_chat_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openKimiChat = useCallback(async (project?: Project) => {
    // kimi_chat 原生聊天持独立启停开关；直连本机 kimi CLI 的原生 ACP server（kimi acp）。
    const label = "Kimi Code Chat";
    if (!isRuntimeEnabled("kimi_chat")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-kimi-${Date.now()}` : "preview-kimi-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "kimi_chat", providerId: "local-runtime-kimi_chat", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_kimi_chat_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openOpenCodeChat = useCallback(async (project?: Project) => {
    // opencode_chat 原生聊天持独立启停开关；直连本机 opencode CLI 的原生 ACP server（opencode acp）。
    const label = "OpenCode Chat";
    if (!isRuntimeEnabled("opencode_chat")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-opencode-${Date.now()}` : "preview-opencode-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "opencode_chat", providerId: "local-runtime-opencode_chat", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_opencode_chat_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  const openPiChat = useCallback(async (project?: Project) => {
    // pi_chat 原生聊天持独立启停开关；直连本机 pi CLI 的 pi --mode rpc 原生会话。
    const label = "Pi Chat";
    if (!isRuntimeEnabled("pi_chat")) { showError(`${label} 未启用或配置尚未加载`); return; }
    if (UI_PREVIEW) {
      const id = project ? `preview-pi-${Date.now()}` : "preview-pi-session";
      if (project || !sessions.some(item => item.id === id)) setSessions(items => [{ ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!, id, agentRuntimeKind: "pi_chat", providerId: "local-runtime-pi_chat", providerName: label, title: label, ...(project ? { cwd: project.path, projectId: project.id, title: `${label} · ${project.name}` } : {}) }, ...items]);
      setSessionTypeFilter("all");
      setSelectedSessionId(id);
      setMultiAgentWorkspaceProjectId(null);
      return;
    }
    const current = sessions.find((session) => session.id === selectedSessionId);
    const picked = project?.path ?? await open({
      directory: true,
      multiple: false,
      title: `选择 ${label} 工作目录`,
      defaultPath: current?.cwd,
    });
    if (typeof picked !== "string") return;
    try {
      const created = await invoke<SessionRecord>("create_pi_chat_session", { cwd: picked, projectId: project?.id ?? null });
      setSessions((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setSessionTypeFilter("all");
      setSelectedSessionId(created.id);
      setMultiAgentWorkspaceProjectId(null);
    } catch (error) {
      showError(`新建 ${label} 会话失败：${String(error)}`);
    }
  }, [isRuntimeEnabled, selectedSessionId, sessions, showError]);

  // zcode CLI 是终端交互式 TUI：走通用 Runtime PTY 会话链路，与 ZCode Desktop 原生聊天分离。
  const openZCodeCli = useCallback(async (project?: Project) => launchRuntimeSession("zcode_cli", project), [launchRuntimeSession]);

  const switchNativeAgent = useCallback(async (source: SessionRecord, kind: NativeAgentKind) => {
    if (nativeAgentNavigationPending.current) return;
    if (!isRuntimeEnabled(kind)) { showError("目标 Agent 未启用或配置尚未加载"); return; }
    nativeAgentNavigationPending.current = true;
    setSwitchingNativeAgent(true);
    const workspaceId = tabOf(source.id);
    const requestedSelection = nativeViewLocation.current.sessionId;
    try {
      await createNativeAgentConversation(source, kind,
        async (command, args) => UI_PREVIEW ? {
          ...PREVIEW_SESSIONS.find(item => item.id === "preview-harness-session")!,
          id: `preview-${kind}-${crypto.randomUUID()}`, agentRuntimeKind: kind, claudeSessionId: "",
          providerId: `local-runtime-${kind}`, providerName: RUNTIME_LABELS[kind], title: RUNTIME_LABELS[kind],
          cwd: args.cwd, projectId: args.projectId, worktree: null, archivedAt: null,
        } : await invoke<SessionRecord>(command, args),
        (target) => {
          setSessionTypeFilter("all");
          setWorkspace(current => ({ ...current, assignments: { ...current.assignments, [target.id]: current.tabs.some(tab => tab.id === workspaceId) ? workspaceId : current.activeTabId } }));
          setSessions(items => [target, ...items.filter(item => item.id !== target.id)]);
          setOpenNativeSlots(current => [...current, target.id]);
          // 创建期间可能切换工作区或会话；不抢占用户的新选择。
          if (nativeViewLocation.current.tabId === workspaceId && nativeViewLocation.current.sessionId === requestedSelection) {
            setSelectedSessionId(target.id);
            setMultiAgentWorkspaceProjectId(null);
          }
        });
    } catch (error) {
      showError(`打开 Agent 聊天失败：${String(error)}`);
    } finally {
      nativeAgentNavigationPending.current = false;
      setSwitchingNativeAgent(false);
    }
  }, [isRuntimeEnabled, tabOf, showError]);

  const handleSessionSnapshot = useCallback((snapshot: SessionRecord[]) => {
    setSessions(snapshot);
    setSelectedSessionId((current) => switchSelectionAfterSnapshot(current, snapshot, switchRecords, switchFor?.id ?? null));
  }, [switchRecords, switchFor]);

  const handleHarnessSessionRecord = useCallback((record: SessionRecord) => {
    setSessions((items) => items.map((item) => item.id === record.id ? record : item));
  }, []);

  const handleSwitchRecord = useCallback((item: SessionSwitchRecord) => {
    setSwitchRecords((current) => [...current.filter((record) => record.id !== item.id), item]);
    if (item.state === "queued" && item.targetId) {
      if (item.preserveSource) setOpenNativeSlots(current => [...new Set([...current, switchSlotId(item.source.id, switchRecords), item.targetId!])]);
      setWorkspace((current) => ({ ...current, assignments: {
        ...current.assignments, [item.targetId!]: current.assignments[item.source.id] ?? current.activeTabId,
      } }));
      setSelectedSessionId((current) => current === item.source.id || (current === null && (item.targetRuntime === "zcode_cli" || item.targetRuntime === "zcode" || item.targetRuntime === "deepseek_harness" || item.targetRuntime?.endsWith("_chat"))) ? item.targetId : current);
      if (UI_PREVIEW) {
        setSessions((current) => [...current.filter((session) => session.id !== item.source.id && session.id !== item.targetId), ...(item.preserveSource ? [{ ...(current.find(session => session.id === item.source.id) ?? item.source), ptyId: null, status: "stopped" as const }] : []), {
          ...item.source, id: item.targetId!, kind: "terminal", agentRuntimeKind: item.targetRuntime ?? "codex", claudeSessionId: "",
          providerId: `local-runtime-${item.targetRuntime ?? "codex"}`, providerName: SWITCH_RUNTIME_LABELS[item.targetRuntime ?? "codex"] ?? "Agent", ptyId: 99, yolo: false,
          status: "running", worktree: null, archivedAt: null,
        }]);
      } else { void refreshSessions(); }
    } else if (item.targetId && item.state !== "cancelled") {
      if (!UI_PREVIEW) void refreshSessions();
    } else if (item.state === "cancelled") {
      setSelectedSessionId(current => current === item.targetId ? item.source.id : current);
      if (UI_PREVIEW) setSessions((current) => [...current.filter((session) => session.id !== item.targetId && session.id !== item.source.id), item.preserveSource ? { ...item.source, ptyId: null, status: "stopped" } : item.source]);
      else void refreshSessions();
    } else if (item.error && !UI_PREVIEW) { void refreshSessions(); }
  }, [refreshSessions, switchRecords]);

  const rememberRightSidebarTab = useCallback((tab: RightSidebarTab) => {
    setLastRightSidebarTab(tab);
    try {
      window.localStorage.setItem(RIGHT_SIDEBAR_TAB_STORAGE_KEY, tab.startsWith("terminal:") ? "start" : sidebarTabKind(tab));
    } catch {
      // 当前窗口仍可继续切换。
    }
  }, []);

  const mountRightSidebarTab = useCallback((tab: RightSidebarTab) => {
    if (!openRightSidebarTabsRef.current.includes(tab)) openRightSidebarTabsRef.current = [...openRightSidebarTabsRef.current, tab];
    setOpenRightSidebarTabs(current => current.includes(tab) ? current : [...current, tab]);
    setMountedRightSidebarTabs((current) => {
      if (current.has(tab)) return current;
      const next = new Set(current);
      next.add(tab);
      return next;
    });
  }, []);

  const handleControlSurfaceChange = useCallback((surface: ControlSurface) => {
    setControlSurface(surface);
    const current = activeRightSidebarTabRef.current;
    const tab: RightSidebarTab = current && sidebarTabKind(current) === "multi_agent" ? current : "multi_agent";
    setLastRightSidebarTab(tab);
    try {
      window.localStorage.setItem(RIGHT_SIDEBAR_TAB_STORAGE_KEY, sidebarTabKind(tab));
    } catch {
      // 当前窗口仍可继续切换。
    }
  }, []);

  const closeRightSidebar = useCallback(() => {
    setExtraRightSidebarTab(null);
    setSidebarMenuOpen(false);
    setSidebarMaximized(false);
    setControlOpen(false);
    setGitOpen(false);
    setFilesOpen(false);
    setBrowserOpen(false);
  }, []);

  const persistLeftSidebarWidth = useCallback((width: number) => {
    const next = Math.max(0, Math.min(window.innerWidth, width));
    setLeftSidebarWidth(next);
    try {
      window.localStorage.setItem(LEFT_SIDEBAR_WIDTH_STORAGE_KEY, String(next));
    } catch {
      // 当前窗口仍可继续使用调整后的宽度。
    }
  }, []);

  const startLeftSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    leftSidebarResizeRef.current = { startX: event.clientX, startWidth: leftSidebarWidth };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveLeftSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = leftSidebarResizeRef.current;
    if (!drag) return;
    setLeftSidebarWidth(Math.max(0, Math.min(window.innerWidth, drag.startWidth + event.clientX - drag.startX)));
  };

  const stopLeftSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = leftSidebarResizeRef.current;
    if (!drag) return;
    leftSidebarResizeRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    persistLeftSidebarWidth(drag.startWidth + event.clientX - drag.startX);
  };

  const resizeLeftSidebarWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    persistLeftSidebarWidth(leftSidebarWidth + (event.key === "ArrowLeft" ? -24 : 24));
  };

  const clampRightSidebarWidth = useCallback((width: number) => {
    const workspaceWidth = document.querySelector<HTMLElement>(".main-workspace")?.clientWidth ?? window.innerWidth;
    return clampSidebarWidth(width, workspaceWidth);
  }, []);

  const persistRightSidebarWidth = useCallback((width: number) => {
    const next = clampRightSidebarWidth(width);
    setRightSidebarDesiredWidth(next);
    saveRightSidebarWidth(next);
  }, [clampRightSidebarWidth]);

  const startRightSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!activeRightSidebarTab || sidebarMaximized) return;
    const displayedWidth = document.querySelector<HTMLElement>(".unified-right-sidebar:not(.is-closed)")?.getBoundingClientRect().width ?? rightSidebarWidth;
    rightSidebarResizeRef.current = { startX: event.clientX, startWidth: displayedWidth };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveRightSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = rightSidebarResizeRef.current;
    if (!drag || !activeRightSidebarTab) return;
    const next = clampRightSidebarWidth(drag.startWidth + drag.startX - event.clientX);
    setRightSidebarDesiredWidth(next);
  };

  const stopRightSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = rightSidebarResizeRef.current;
    if (!drag || !activeRightSidebarTab) return;
    const width = drag.startWidth + drag.startX - event.clientX;
    rightSidebarResizeRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    persistRightSidebarWidth(width);
  };

  const resizeRightSidebarWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    if (!activeRightSidebarTab) return;
    event.preventDefault();
    const displayedWidth = document.querySelector<HTMLElement>(".unified-right-sidebar:not(.is-closed)")?.getBoundingClientRect().width ?? rightSidebarWidth;
    persistRightSidebarWidth(displayedWidth + (event.key === "ArrowLeft" ? 24 : -24));
  };

  const selectRightSidebarTab = useCallback(
    async (tab: RightSidebarTab) => {
      if (tab === "files" && !workspaceRootRef.current) {
        showError("请先打开一个工作区");
        return;
      }
      setSidebarMenuOpen(false);
      activeRightSidebarTabRef.current = tab;
      setExtraRightSidebarTab(null);
      rememberRightSidebarTab(tab);
      mountRightSidebarTab(tab);
      if (tab.includes(":") && !tab.startsWith("terminal:")) {
        setControlOpen(false); setGitOpen(false); setFilesOpen(false); setBrowserOpen(false);
        setExtraRightSidebarTab(tab);
        return;
      }
      if (tab === "start" || tab.startsWith("terminal:")) {
        setControlOpen(false); setGitOpen(false); setFilesOpen(false); setBrowserOpen(false);
        setExtraRightSidebarTab(tab);
        return;
      }
    if (tab === "multi_agent") {
      const surface: ControlSurface = "multi_agent";
      saveControlSurface(surface);
      setControlSurface(surface);
      setGitOpen(false);
      setFilesOpen(false);
      setBrowserOpen(false);
      if (controlDetached && !UI_PREVIEW) {
        try {
          await invoke("dock_control_window");
        } catch (error) {
          showError(`停靠总控窗口失败：${String(error)}`);
          return;
        }
      }
      setControlDetached(false);
      setControlOpen(true);
      return;
    }
    setControlOpen(false);
    setControlDetached(false);
    if (tab === "files") {
      setGitOpen(false);
      setFilesOpen(true);
      setBrowserOpen(false);
      return;
    }
    if (tab === "browser") {
      setGitOpen(false);
      setFilesOpen(false);
      setBrowserOpen(true);
      return;
    }
      setFilesOpen(false);
      setBrowserOpen(false);
      setGitOpen(true);
    },
    [controlDetached, mountRightSidebarTab, rememberRightSidebarTab, showError],
  );

  useEffect(() => {
    if (activeRightSidebarTab) mountRightSidebarTab(activeRightSidebarTab);
  }, [activeRightSidebarTab, mountRightSidebarTab]);

  // 浏览器控制「面板优先」：只有实际浏览器工具调用才请求打开面板。
  // 后端发 browser-control-panel-needed，这里激活右侧栏浏览器标签让面板
  // webview 上线，Agent 的桥接探活即命中面板而不是弹外部浏览器。
  // 幂等激活，连续工具调用不反复关闭面板；创建失败由组件巡检重试。
  const selectRightSidebarTabRef = useRef(selectRightSidebarTab);
  selectRightSidebarTabRef.current = selectRightSidebarTab;
  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    const unlisten = listen("browser-control-panel-needed", () => {
      if (!disposed) void selectRightSidebarTabRef.current("browser");
    }).catch(error => {
      if (!disposed) showError(`浏览器面板事件监听失败：${String(error)}`);
    });
    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose?.()).catch(error => console.error("释放浏览器面板监听失败:", error));
    };
  }, [showError]);

  // 外部入口先保存地址，首次挂载的 BrowserPanel 才能收到导航恢复点。
  // 已挂载面板仍由其事件监听器完成导航。
  useEffect(() => {
    const onBrowse = (event: Event) => {
      const url = (event as CustomEvent<{ url?: string }>).detail?.url;
      if (!url) return;
      saveBrowserPanelState({ url });
      setSettingsOpen(false);
      void selectRightSidebarTab("browser");
    };
    window.addEventListener("crewvela:browse", onBrowse);
    return () => window.removeEventListener("crewvela:browse", onBrowse);
  }, [selectRightSidebarTab]);

  const detachControl = useCallback(async () => {
    try {
      await invoke("open_control_window");
      setControlDetached(true);
      setControlOpen(false);
      setExtraRightSidebarTab(null);
    } catch (error) {
      showError(`分离总控窗口失败：${String(error)}`);
    }
  }, [showError]);


  // 按 provider 汇总的累计 token（设置页 provider 卡片展示）。
  const usageByProvider = useMemo(() => {
    const totals: Record<string, number> = {};
    for (const session of sessions) {
      const usage = usages[session.id];
      if (!usage) continue;
      const total =
        usage.inputTokens +
        usage.outputTokens +
        usage.cacheCreationTokens +
        usage.cacheReadTokens;
      if (total > 0) {
        totals[session.providerId] = (totals[session.providerId] ?? 0) + total;
      }
    }
    return totals;
  }, [sessions, usages]);
  const selectedSession =
    selectedSessionId === null
      ? null
      : (sessions.find((session) => session.id === selectedSessionId) ?? null);
  const selectedRelaySource = selectedSession ? retainedSwitchSource(selectedSession.id, switchRecords) : null;
  const nativeTabs = nativeChatTabSessions(tabSessions.filter(isNativeChatSession), orderedNativeChatSlots(openNativeSlots, nativeTabOrders[workspace.activeTabId]), id => switchSlotId(id, switchRecords));
  const reorderNativeTab = (source: string, target: string, after: boolean) => {
    const workspaceId = workspace.activeTabId;
    const next = { ...nativeTabOrders, [workspaceId]: reorderNativeChatSlots(
      nativeTabOrders[workspaceId] ?? [], nativeTabs.map(item => switchSlotId(item.id, switchRecords)),
      switchSlotId(source, switchRecords), switchSlotId(target, switchRecords), after) };
    try {
      saveNativeChatTabOrders(next, window.localStorage);
      setNativeTabOrders(next);
      setNativeTabOrderError(null);
    } catch { setNativeTabOrderError("聊天标签顺序保存失败，请重试。顺序未更改。"); }
  };
  const nativeToolbarSource = selectedSession && isNativeChatSession(selectedSession) ? selectedSession : nativeTabs[0];
  const closeNativeTab = (id: string) => {
    const slot = switchSlotId(id, switchRecords);
    setOpenNativeSlots(current => current.filter(item => item !== slot));
    if (selectedSessionId === id) {
      const index = nativeTabs.findIndex(item => item.id === id);
      const remaining = nativeTabs.filter(item => item.id !== id);
      setSelectedSessionId(remaining[Math.max(0, index - 1)]?.id ?? null);
    }
  };
  const activeWorkspaceRoot =
    selectedSession && tabOf(selectedSession.id) === workspace.activeTabId
      ? selectedSession.cwd
      : tabSessions[0]?.cwd ?? projects[0]?.path ?? null;
  workspaceRootRef.current = activeWorkspaceRoot;
  usePtyPolling(handleSessionSnapshot, openRightSidebarTabs.some(tab => tab.startsWith("terminal:")) && !UI_PREVIEW);
  const openSidebarTool = async (tool: SidebarTool) => {
    if (!isTerminalShell(tool)) {
      await selectRightSidebarTab(`${tool}:${crypto.randomUUID()}`);
      return;
    }
    if (!activeWorkspaceRoot) { showError("请先打开一个工作区"); return; }
    if (sidebarTerminalLaunchRef.current) return;
    sidebarTerminalLaunchRef.current = true;
    try {
      const launched = UI_PREVIEW
        ? { ...PREVIEW_SESSIONS[0], id: `preview-sidebar-${Date.now()}`, kind: "terminal" as const,
            shell: tool, agentRuntimeKind: null, providerName: shellLabel(tool),
            title: shellLabel(tool), cwd: activeWorkspaceRoot, ptyId: Date.now(), status: "running" as const }
        : await invoke<SessionRecord>("launch_terminal", { shell: tool, cwd: activeWorkspaceRoot });
      setSidebarTerminalIds(current => [...current, launched.id]);
      setSessions(current => [launched, ...current.filter(item => item.id !== launched.id)]);
      await selectRightSidebarTab(`terminal:${launched.id}`);
    } catch (error) {
      showError(`启动终端失败：${String(error)}`);
    } finally {
      sidebarTerminalLaunchRef.current = false;
    }
  };

  const closeSidebarResource = async (tab: RightSidebarTab) => {
    if (tab.startsWith("terminal:")) {
      const id = tab.slice("terminal:".length);
      // 关闭终端标签会停止该标签拥有的进程；失败时保留标签与输出。
      try {
        if (!UI_PREVIEW) {
          const updated = await invoke<SessionRecord>("stop_session", { sessionId: id });
          setSessions(current => current.map(item => item.id === id ? updated : item));
        }
      } catch (error) { showError(`关闭终端失败：${String(error)}`); return; }
      setMountedRightSidebarTabs(current => { const next = new Set(current); next.delete(tab); return next; });
      // 保留 ownership，避免停止的右侧终端突然出现在主网格。
    }
    if (sidebarTabKind(tab) === "browser" && !UI_PREVIEW) {
      try { await invoke("browser_panel_close", { panelId: tab === "browser" ? null : tab.slice(8) }); }
      catch (error) { showError(`关闭浏览器失败：${String(error)}`); return; }
    }
    setMountedRightSidebarTabs(current => { const next = new Set(current); next.delete(tab); return next; });
    const currentTabs = openRightSidebarTabsRef.current;
    const remaining = currentTabs.filter(item => item !== tab);
    openRightSidebarTabsRef.current = remaining.length ? remaining : ["start"];
    setOpenRightSidebarTabs(current => {
      const next = current.filter(item => item !== tab);
      return next.length ? next : ["start"];
    });
    if (activeRightSidebarTabRef.current === tab) {
      const index = currentTabs.indexOf(tab);
      await selectRightSidebarTab(remaining[Math.max(0, index - 1)] ?? "start");
    }
  };

  const activeDeepSkin = findDeepSkin(companionSettings.customSkinId);
  const activeCustomSkin = loadCustomSkins().find(
    (skin) => skin.id === companionSettings.customSkinId && skin.media,
  );

  // 渲染层回调（useCallback 保持引用稳定，配合 memo 组件跳过重渲染）。
  const collapseSidebar = useCallback(() => setSidebarCollapsed(true), []);
  const openProviderSettings = useCallback(
    () => openSettingsSection("providers"),
    [openSettingsSection],
  );
  const openAttentionSession = useCallback((session: SessionRecord, kind: AttentionKind) => {
    const current = sessions.find(item => item.id === session.id);
    if (!current || current.archivedAt != null) return;
    // Viewing a failure must not restart an Agent or resubmit a prompt.
    if (attentionOpenMode(current, kind) === "history") { setTranscriptFor(current); return; }
    void selectSession(current.id);
  }, [sessions, selectSession]);
  const openNewSessionDialog = useCallback(
    () => { setLaunchProject(null); setNewSessionOpen(true); },
    [],
  );
  const openAddProjectDialog = useCallback(() => setAddProjectOpen(true), []);
  const launchRuntime = useCallback(
    (
      runtimeKind: Extract<
        AgentRuntimeKind,
        | "codex"
        | "kimi_code"
          | "pi"
        | "opencode"
      >,
    ) => void launchRuntimeSession(runtimeKind),
    [launchRuntimeSession],
  );
  const launchPreset = useCallback(
    (preset: LaunchPreset) => void handleLaunchPreset(preset),
    [handleLaunchPreset],
  );
  const deletePreset = useCallback(
    (preset: LaunchPreset) => void handleDeletePreset(preset),
    [handleDeletePreset],
  );
  const focusFirstLiveSession = useCallback(
    () =>
      setSelectedSessionId(
        (current) => current ?? liveSessions[0]?.id ?? null,
      ),
    [liveSessions],
  );
  const clearSelectedSession = useCallback(
    () => setSelectedSessionId(null),
    [],
  );
  const stopAllSessions = useCallback(() => void stopAll(), [stopAll]);
  const deleteAllTabSessions = useCallback(
    () => void deleteAllSessions(),
    [deleteAllSessions],
  );
  const toggleSidebar = useCallback(
    () => setSidebarCollapsed((current) => !current),
    [],
  );
  const toggleRightSidebar = useCallback(() => {
    if (rightSidebarOpen) closeRightSidebar();
    else void selectRightSidebarTab(openRightSidebarTabs.includes(lastRightSidebarTab) ? lastRightSidebarTab : "start");
  }, [closeRightSidebar, lastRightSidebarTab, openRightSidebarTabs, rightSidebarOpen, selectRightSidebarTab]);
  const closeSessionById = useCallback(
    (id: string) => {
      const session = sessions.find((item) => item.id === id);
      if (session) void stopSession(session);
    },
    [sessions, stopSession],
  );
  const toggleMaximizeSession = useCallback((id: string) => {
    setSelectedSessionId((current) => (current === id ? null : id));
  }, []);
  const updatePetPosition = useCallback(
    (position: { x: number; y: number }) =>
      setPetSettings((current) => ({ ...current, position })),
    [],
  );

  return (
    <div
      className={`app-shell${sidebarCollapsed ? " sidebar-collapsed" : ""}`}
      style={appShellStyle}
    >
      <DynamicSkinLayer
        skin={activeCustomSkin}
        motionEnabled={companionSettings.skinMotionEnabled}
        soundEnabled={companionSettings.skinSoundEnabled}
        soundVolume={companionSettings.skinSoundVolume}
      />
      <SkinAtmosphere
        skin={activeDeepSkin}
        motionEnabled={companionSettings.skinMotionEnabled}
        soundEnabled={companionSettings.skinSoundEnabled}
        soundVolume={companionSettings.skinSoundVolume}
      />
      <MemoAppSidebar
        sessions={tabSessions}
        projects={projects}
        usages={usages}
        selectedSessionId={selectedSessionId}
        loadingSessionId={loadingSessionId}
        collapsed={sidebarCollapsed}
        onCollapse={collapseSidebar}
        sessionTypeFilter={sessionTypeFilter}
        onSessionTypeFilter={setSessionTypeFilter}
        onSelectSession={selectSession}
        onRenameSession={renameSession}
        onStopSession={stopSession}
        onDeleteSession={deleteSession}
        onArchiveSession={archiveSession}
        onViewTranscript={setTranscriptFor}
        onSwitchAgent={openSwitch}
        sourceHistory={sourceHistory}
        logicalSessionId={logicalSessionId}
        onRelayDetails={openRelayDetails}
        onNewClaudeSession={openNewSessionDialog}
        onOpenDeepSeekHarness={() => void openDeepSeekHarness()}
        onOpenClaudeChat={() => void openClaudeChat()}
        onOpenCodexChat={() => void openCodexChat()}
        onOpenKimiChat={() => void openKimiChat()}
        onOpenOpenCodeChat={() => void openOpenCodeChat()}
        onOpenPiChat={() => void openPiChat()}
            onOpenZCode={() => void openZCode()}
            onOpenZCodeCli={() => void openZCodeCli()}
        onNewTerminal={launchTerminal}
        onNewRuntimeSession={launchRuntime}
        onAddProject={openAddProjectDialog}
        presets={presets}
        providers={providers}
        onLaunchPreset={launchPreset}
        onDeletePreset={deletePreset}
        onNewSessionInProject={handleNewSessionInProject}
        onRenameProject={handleRenameProject}
        onDeleteProject={handleDeleteProject}
        onArchiveProjectSessions={archiveProjectSessions}
        viewFocused={selectedSessionId !== null}
        viewSwitchDisabled={liveSessions.length === 0}
        onFocusView={focusFirstLiveSession}
        onOverviewView={clearSelectedSession}
        stopAllDisabled={liveSessions.length === 0}
        onStopAll={stopAllSessions}
        deleteAllDisabled={tabSessions.length === 0}
        onDeleteAll={deleteAllTabSessions}
        onOpenSection={openSettingsSection}
        onOpenSettings={openProviderSettings}
        onOpenAttentionSession={openAttentionSession}
      />
      <div className="communication-sidebar-hint"><CommunicationDiscoveryHint suppressed={Boolean(guideRequest) || settingsOpen} projectId={multiAgentWorkspaceProjectId ?? undefined} /></div>
      {!sidebarCollapsed && (
        <div
          className="left-sidebar-resizer"
          role="separator"
          aria-label="调整左侧栏宽度"
          aria-orientation="vertical"
          aria-valuemin={0}
          aria-valuemax={window.innerWidth}
          aria-valuenow={Math.round(leftSidebarWidth)}
          tabIndex={0}
          onPointerDown={startLeftSidebarResize}
          onPointerMove={moveLeftSidebarResize}
          onPointerUp={stopLeftSidebarResize}
          onPointerCancel={stopLeftSidebarResize}
          onDoubleClick={() => persistLeftSidebarWidth(DEFAULT_LEFT_SIDEBAR_WIDTH)}
          onKeyDown={resizeLeftSidebarWithKeyboard}
        />
      )}
      <div className="workspace-shell">
        {testProfile && <aside className="test-profile-banner" role="status" aria-label="新用户隔离测试">
          <strong>新用户隔离测试</strong><span>仅使用测试数据；真实 Agent、账号与全局集成操作已停用。</span><code title={testProfile.dataDir}>{testProfile.dataDir}</code>
        </aside>}
        <MemoWorkspaceTabs
          canClose={sessionsReady && switchRecordsReady}
          tabs={workspace.tabs}
          activeTabId={workspace.activeTabId}
          onSelect={selectTab}
          onAdd={addTab}
          onClose={closeTab}
          onRename={renameTab}
          sidebarCollapsed={sidebarCollapsed}
          onToggleSidebar={toggleSidebar}
          rightSidebarOpen={rightSidebarOpen}
          onToggleRightSidebar={toggleRightSidebar}
        />
        <div
          className={[
            "app",
            sidebarCollapsed ? "sidebar-collapsed" : "",
            rightSidebarOpen ? "right-sidebar-expanded" : "",
          ]
            .filter(Boolean)
            .join(" ")}
        >
          <div className="main">
            <div className={`main-workspace${guideRequest ? " communication-guide-open" : ""}`} style={rightSidebarStyle}>
              <div className="terminal-wrap">
                {!multiAgentWorkspaceProjectId && selectedSession && isNativeChatSession(selectedSession) && <NativeChatTabs
                  key={workspace.activeTabId} sessions={nativeTabs} selectedId={selectedSessionId} creating={switchingNativeAgent}
                  onReorder={reorderNativeTab} orderError={nativeTabOrderError}
                  canCreate={Boolean(nativeToolbarSource)}
                  enabledAgents={{ deepseek_harness: isRuntimeEnabled("deepseek_harness"), claude_chat: isRuntimeEnabled("claude_chat"), codex_chat: isRuntimeEnabled("codex_chat"), kimi_chat: isRuntimeEnabled("kimi_chat"), opencode_chat: isRuntimeEnabled("opencode_chat"), pi_chat: isRuntimeEnabled("pi_chat"), zcode: isRuntimeEnabled("zcode") }}
                  onSelect={id => { if (UI_PREVIEW) { setSelectedSessionId(id); setMultiAgentWorkspaceProjectId(null); } else void selectSession(id); }}
                  onClose={closeNativeTab} onCreate={kind => { if (nativeToolbarSource) void switchNativeAgent(nativeToolbarSource, kind); }}
                  onHandoff={selectedSession && isNativeChatSession(selectedSession) && !selectedRelaySource ? () => openSwitch(selectedSession) : undefined} />}
                {!multiAgentWorkspaceProjectId && selectedSession && selectedRelaySource && <div className="native-agent-view"><HandoffSourceView
                  session={selectedSession} record={selectedRelaySource} target={sessions.find(item => item.id === selectedRelaySource.targetId)} preview={UI_PREVIEW}
                  onSelectTarget={() => { if (selectedRelaySource.targetId) void selectSession(selectedRelaySource.targetId); }} onDetails={() => openRelayDetails(selectedSession)}
                /></div>}
                {!multiAgentWorkspaceProjectId && switchRecords.filter((item) => needsSwitchRecovery(item, sessions, [...sessions, ...archivedSessions])).map((item) => <button key={item.id} className="btn secondary session-switch-recovery" onClick={() => { setSwitchDraft(item); setSwitchFor(item.source); }}>接力待处理：{item.source.title}</button>)}
                {sessions.filter(item => !retainedSwitchSource(item.id, switchRecords) && (isNativeChatSession(item) || isStructuredZCodeCli(item, switchRecords)) && (visitedNativeSessions.includes(item.id) || isStructuredZCodeCli(item, switchRecords) || selectedSessionId === item.id)).map(item => {
                  const active = !multiAgentWorkspaceProjectId && selectedSessionId === item.id;
                  const props = { managedToolbar: true, session: item, preview: UI_PREVIEW, onSessionRecord: handleHarnessSessionRecord, active, switching: switchingNativeAgent, onHandoffAgent: () => openSwitch(item), onSwitchCli: isStructuredZCodeCli(item, switchRecords) ? () => openSwitch(item) : undefined, enabledAgents: { deepseek_harness: isRuntimeEnabled("deepseek_harness"), claude_chat: isRuntimeEnabled("claude_chat"), codex_chat: isRuntimeEnabled("codex_chat"), kimi_chat: isRuntimeEnabled("kimi_chat"), opencode_chat: isRuntimeEnabled("opencode_chat"), pi_chat: isRuntimeEnabled("pi_chat"), zcode: isRuntimeEnabled("zcode") }, onSwitchAgent: isStructuredZCodeCli(item, switchRecords) ? undefined : (kind: NativeAgentKind) => void switchNativeAgent(item, kind) };
                  return <div key={item.id} className="native-agent-view" hidden={!active}>{isStandaloneHarnessSession(item) ? <DeepSeekHarnessChat {...props} /> : isStandaloneClaudeChatSession(item) ? <ClaudeChat {...props} /> : isStandaloneCodexChatSession(item) ? <CodexChat {...props} /> : isStandaloneKimiChatSession(item) ? <KimiChat {...props} /> : isStandaloneOpenCodeChatSession(item) ? <OpenCodeChat {...props} /> : isStandalonePiChatSession(item) ? <PiChat {...props} /> : <ZCodeChat {...props} />}</div>;
                })}
                {multiAgentWorkspaceProjectId && (
                  <MultiAgentWorkspace
                    projectId={multiAgentWorkspaceProjectId}
                    sessions={sessions.filter(item => !sidebarTerminalIds.includes(item.id))}
                    theme={theme}
                    terminalTransparent={
                      appearance.terminalTransparent === true
                    }
                    onSessionsChange={handleSessionSnapshot}
                    onCloseSession={closeSessionById}
                    onClose={() => setMultiAgentWorkspaceProjectId(null)}
                  />
                )}
                {anyLivePtySessions && (
                  <div className="main-terminal-host" hidden={!!multiAgentWorkspaceProjectId || !!selectedRelaySource || !!(selectedSession && (isNativeChatSession(selectedSession) || isStructuredZCodeCli(selectedSession, switchRecords)))}>
                    <MemoMultiTerminal
                      sessions={displaySessions.filter(item => !sidebarTerminalIds.includes(item.id))}
                      selectedSessionId={selectedSessionId}
                      theme={theme}
                      terminalTransparent={
                        appearance.terminalTransparent === true
                      }
                      preview={UI_PREVIEW}
                      hiddenSessionIds={hiddenSessionIds}
                      onClose={closeSessionById}
                      onToggleMaximize={toggleMaximizeSession}
                      onSessionsChange={handleSessionSnapshot}
                      switchRecords={switchRecords}
                      onSwitchAgent={openSwitch}
                      onViewSourceHistory={setTranscriptFor}
                      onRelayDetails={openRelayDetails}
                    />
                  </div>
                )}
                {!multiAgentWorkspaceProjectId && selectedSession &&
                !selectedRelaySource && !isNativeChatSession(selectedSession) && !isStructuredZCodeCli(selectedSession, switchRecords) && selectedSession.ptyId === null ? (
                  <div className="session-placeholder">
                    <h2>{selectedSession.title}</h2>
                    <p>{selectedSession.retiredAgentRuntime || isRetiredRuntime(selectedSession.agentRuntimeKind)
                      ? "该运行时已移除，历史记录已保留。请使用其他 Agent 新建会话。"
                      : "该会话当前未运行。"}</p>
                    {!selectedSession.retiredAgentRuntime && !isRetiredRuntime(selectedSession.agentRuntimeKind) && <button
                      className="btn primary"
                      onClick={() => selectSession(selectedSession.id)}
                    >
                      恢复会话
                    </button>}
                    {selectedSession.lastError && (
                      <pre className="session-error">
                        {selectedSession.lastError}
                      </pre>
                    )}
                  </div>
                ) : !multiAgentWorkspaceProjectId && !selectedSession &&
                  !tabSessions.some(item => !sidebarTerminalIds.includes(item.id) && isPtySession(item)) ? (
                  <div className="empty-state">
                    <h1>
                      <img className="empty-state-mascot" src={orangeMascot} alt="" aria-hidden="true" />
                      这个工作区还没有会话
                    </h1>
                    <p>
                      <span>新建一个 Agent 会话，或从左侧列表选择已有会话；</span>
                      <span>多个 Agent 可以并排运行、互发消息。</span>
                    </p>
                    {/* 参照 Cowork 新任务页的输入框卡片：整张卡片就是入口按钮，名称取右下角的「新建会话」。 */}
                    <button
                      type="button"
                      className="empty-state-start"
                      aria-labelledby="empty-state-start-label"
                      aria-describedby="empty-state-start-prompt"
                      onClick={openNewSessionDialog}
                    >
                      <span id="empty-state-start-prompt" className="empty-state-start-prompt">
                        选择 Agent 与工作目录，开始一段新会话
                      </span>
                      <span className="empty-state-start-bar">
                        <span id="empty-state-start-label" className="empty-state-start-chip">
                          <PlusIcon size={14} weight="bold" aria-hidden="true" />
                          新建会话
                        </span>
                      </span>
                    </button>
                    <small className="empty-state-hint">
                      <FolderSimpleIcon size={14} aria-hidden="true" />
                      想固定在某个目录？点左侧「项目」旁的 + 直接在项目下新建。
                    </small>
                  </div>
                ) : null}
              </div>
              {guideRequest && <CommunicationGuide key={guideRequest.nonce} initialScope={guideRequest.scope} initialProjectId={guideRequest.projectId}
                sessions={communicationSessions(sessions)} selectedSessionId={selectedSessionId} preview={UI_PREVIEW} hidden={settingsOpen}
                onClose={() => setGuideRequest(null)} onDiagnostics={(projectId, scope) => {
                  setCollaborationInitialProjectId(projectId);
                  setCollaborationInitialTab(scope === "project" && !projectId ? "overview" : "diagnostics"); setCollaborationDiagnosticGlobal(scope === "sessions"); setSettingsTarget("collaboration"); setSettingsOpen(true);
                }} />}
              {(activeRightSidebarTab || mountedRightSidebarTabs.size > 0) && (
                <>
                {activeRightSidebarTab && !sidebarMaximized && (
                  <div
                    className="right-sidebar-resizer"
                    role="separator"
                    aria-label={
                      sidebarTabKind(activeRightSidebarTab ?? "") === "files"
                        ? "调整文件侧栏宽度"
                        : sidebarTabKind(activeRightSidebarTab ?? "") === "git"
                          ? "调整源代码控制侧栏宽度"
                          : sidebarTabKind(activeRightSidebarTab ?? "") === "browser"
                            ? "调整浏览器侧栏宽度"
                            : "调整总控侧栏宽度"
                    }
                    aria-orientation="vertical"
                    aria-valuemin={0}
                    aria-valuemax={workspaceAvailableWidth}
                    aria-valuenow={Math.round(clampRightSidebarWidth(rightSidebarWidth))}
                    tabIndex={0}
                    onPointerDown={startRightSidebarResize}
                    onPointerMove={moveRightSidebarResize}
                    onPointerUp={stopRightSidebarResize}
                    onPointerCancel={stopRightSidebarResize}
                    onDoubleClick={() => persistRightSidebarWidth(DEFAULT_RIGHT_SIDEBAR_WIDTH)}
                    onKeyDown={resizeRightSidebarWithKeyboard}
                  />
                )}
                <aside
                  className={`unified-right-sidebar${activeRightSidebarTab ? "" : " is-closed"}${sidebarMaximized ? " is-maximized" : ""}`}
                  aria-label="右侧栏"
                  aria-hidden={!activeRightSidebarTab}
                >
                  <RightSidebarActivityBar
                    activeTab={activeRightSidebarTab ?? lastRightSidebarTab}
                    onSelect={(tab) => void selectRightSidebarTab(tab)}
                    onClose={closeRightSidebar}
                    tabs={openRightSidebarTabs.map(id => ({ id, label: id === "start" ? "开始" : sidebarTabKind(id ?? "") === "files" ? "文件" : sidebarTabKind(id ?? "") === "browser" ? "浏览器" : sidebarTabKind(id ?? "") === "git" ? "Git" : sidebarTabKind(id ?? "") === "multi_agent" ? "多智能体" : shellLabel(sessions.find(item => item.id === id.slice(9))?.shell) }))}
                    onCloseTab={tab => void closeSidebarResource(tab)}
                    maximized={sidebarMaximized}
                    onToggleMaximize={() => setSidebarMaximized(value => !value)}
                    menuOpen={sidebarMenuOpen}
                    onMenuChange={setSidebarMenuOpen}
                    onOpenTool={tool => void openSidebarTool(tool)}
                    hasWorkspace={!!activeWorkspaceRoot}
                  />
                  <div className="unified-right-sidebar-content">
                    {activeRightSidebarTab === "start" && <RightSidebarStart root={activeWorkspaceRoot} onOpenTool={tool => void openSidebarTool(tool)} />}
                    {sidebarTerminalIds.map(id => {
                      const tab: RightSidebarTab = `terminal:${id}`;
                      const session = sessions.find(item => item.id === id);
                      if (!mountedRightSidebarTabs.has(tab) || !session) return null;
                      return <div key={id} className={`right-sidebar-panel right-sidebar-terminal${activeRightSidebarTab === tab ? " active" : ""}`}>
                        {session.ptyId !== null ? <TerminalPane sessionId={session.ptyId} title={session.title} providerName={session.providerName}
                          kind={session.kind} shell={session.shell} cwd={session.cwd} status={session.status} theme={theme}
                          preview={UI_PREVIEW} hidden={activeRightSidebarTab !== tab} onClose={() => void closeSidebarResource(tab)} />
                          : <div className="right-sidebar-loading">终端已停止。通过「＋」新建终端。</div>}
                      </div>;
                    })}
                    {[...mountedRightSidebarTabs].filter(tab => sidebarTabKind(tab) === "multi_agent").map(tab => (
                      <div key={tab} className={`right-sidebar-panel${activeRightSidebarTab === tab ? " active" : ""}`}>
                        <MemoControlSidebar
                        open
                        active={!guideRequest && activeRightSidebarTab === tab && (tab !== "multi_agent" || !controlDetached)}
                        variant="docked"
                        providers={selectableProviders}
                        sessions={sessions}
                        defaultCwd={activeWorkspaceRoot}
                        quickContext={{ workspaceSessionIds: tabSessions.map(s => s.id), projectId: selectedSession?.projectId ?? null, cwd: activeWorkspaceRoot }}
                        onOpenQuickSettings={() => openSettingsSection("quickCommands")}
                        onClose={closeRightSidebar}
                        onDetach={detachControl}
                        onSessionsChange={handleSessionSnapshot}
                        isolatedSurface={tab !== "multi_agent"}
                        initialSurface={tab === "multi_agent" ? controlSurface : "multi_agent"}
                        onSurfaceChange={tab === "multi_agent" ? handleControlSurfaceChange : undefined}
                        onOpenMultiAgentWorkspace={setMultiAgentWorkspaceProjectId}
                      />
                      </div>
                    ))}
                    {[...mountedRightSidebarTabs].filter(tab => sidebarTabKind(tab) === "git").map(tab => (
                      <div key={tab} className={`right-sidebar-panel${activeRightSidebarTab === tab ? " active" : ""}`}>
                        <MemoGitSourceControlPanel
                        variant="sidebar"
                        sessions={sessions}
                        projects={projects}
                        providers={selectableProviders}
                        keyStates={keyStates}
                        onClose={closeRightSidebar}
                        preview={UI_PREVIEW}
                      />
                      </div>
                    ))}
                    {[...mountedRightSidebarTabs].filter(tab => sidebarTabKind(tab) === "files").map(tab => (
                      <div key={tab} className={`right-sidebar-panel${activeRightSidebarTab === tab ? " active" : ""}`}>
                        <MemoFileExplorerPanel
                          root={activeWorkspaceRoot}
                          active={!guideRequest && activeRightSidebarTab === tab}
                        />
                      </div>
                    ))}
                    {[...mountedRightSidebarTabs].filter(tab => sidebarTabKind(tab) === "browser").map(tab => (
                      <div key={tab} className={`right-sidebar-panel${activeRightSidebarTab === tab ? " active" : ""}`}>
                        <MemoBrowserPanel panelId={tab === "browser" ? undefined : tab.slice(8)} active={!guideRequest && activeRightSidebarTab === tab && !sidebarMenuOpen} />
                      </div>
                    ))}
                    {activeRightSidebarTab && !mountedRightSidebarTabs.has(activeRightSidebarTab) && (
                      <div className="right-sidebar-loading">正在打开…</div>
                    )}
                  </div>
                </aside>
                </>
              )}
            </div>
          </div>
          <DesktopPet
            settings={petSettings}
            sessions={sessions}
            onPositionChange={updatePetPosition}
          />
          {editing && (
            <ProviderEditor
              provider={editing}
              providers={providers}
              hasKey={keyStates[editing.id] ?? false}
              onSave={handleSave}
              onClose={() => {
                setPendingLaunchProviderId(null);
                setLaunchProject(null);
                setEditing(null);
              }}
            />
          )}
          {newSessionOpen && (
            <NewSessionDialog
              project={launchProject}
              onRuntimeSelect={(runtime) => {
                const project = launchProject ?? undefined;
                if (!isRuntimeEnabled(runtime)) return;
                setNewSessionOpen(false);
                setLaunchProject(null);
                if (runtime === "deepseek_harness") void openDeepSeekHarness(project);
                else if (runtime === "claude_chat") void openClaudeChat(project);
                else if (runtime === "codex_chat") void openCodexChat(project);
                else if (runtime === "kimi_chat") void openKimiChat(project);
                else if (runtime === "opencode_chat") void openOpenCodeChat(project);
                else if (runtime === "pi_chat") void openPiChat(project);
                else if (runtime === "zcode") void openZCode(project);
                else void launchRuntimeSession(runtime, project);
              }}
              providers={selectableProviders}
              keyStates={keyStates}
              onSelect={(providerId) => {
                setNewSessionOpen(false);
                handleLaunch(providerId);
              }}
              onLocalClaudeSelect={() => {
                const project = launchProject ?? undefined;
                if (!isRuntimeEnabled("claude_code")) return;
                setNewSessionOpen(false);
                setLaunchProject(null);
                void launchRuntimeSession("claude_code", project);
              }}
              onOpenSettings={() => {
                setNewSessionOpen(false);
                setLaunchProject(null);
                openSettingsSection("providers");
              }}
              onClose={() => {
                setNewSessionOpen(false);
                setLaunchProject(null);
              }}
            />
          )}
          {addProjectOpen && (
            <AddProjectDialog
              onConfirm={handleAddProject}
              onCancel={() => setAddProjectOpen(false)}
            />
          )}
          {switchFor && <SessionSwitchDialog source={switchFor} initialTarget={switchTarget} sourcePosition={Math.max(0, displaySessions.findIndex((session) => session.id === switchFor.id))} record={switchDraft} preview={UI_PREVIEW} onRecord={handleSwitchRecord} onClose={() => setSwitchFor(null)} />}
          {transcriptFor && (
            <TranscriptViewer
              session={transcriptFor}
              relayHistory={switchRecords.some((item) => item.source.id === transcriptFor.id)}
              preview={UI_PREVIEW && switchRecords.some((item) => item.source.id === transcriptFor.id)}
              onClose={() => setTranscriptFor(null)}
            />
          )}
          {settingsOpen && (
            <SettingsDialog
              quickContext={{ workspaceSessionIds: tabSessions.map(s => s.id), projectId: selectedSession?.projectId ?? null, cwd: activeWorkspaceRoot }}
              onSessionsChange={handleSessionSnapshot}
              sessions={sessions}
              archivedSessions={archivedSessions}
              projects={projects}
              providers={providers}
              keyStates={keyStates}
              usageByProvider={usageByProvider}
              appearance={appearance}
              resolvedAppearanceTheme={resolvedAppearanceTheme}
              petSettings={petSettings}
              notificationSettings={notificationSettings}
              companionSettings={companionSettings}
              onAppearanceChange={setAppearance}
              onPetSettingsChange={setPetSettings}
              onNotificationSettingsChange={setNotificationSettings}
              onCompanionSettingsChange={setCompanionSettings}
              onEdit={(provider) => setEditing(provider)}
              onNew={handleNew}
              onDelete={handleDelete}
              onSetHidden={handleProviderHidden}
              onRestoreDefault={handleRestoreBuiltinProvider}
              onReorder={handleProviderReorder}
              onRestoreArchivedSession={restoreArchivedSession}
              onDeleteArchivedSession={deleteArchivedSession}
              onViewArchivedTranscript={(session) => {
                setSettingsOpen(false);
                setTranscriptFor(session);
              }}
              initialSection={settingsTarget}
              initialCollaborationTab={collaborationInitialTab}
              initialCollaborationDiagnosticGlobal={collaborationDiagnosticGlobal}
              initialCollaborationProjectId={collaborationInitialProjectId}
              onClose={() => setSettingsOpen(false)}
            />
          )}
          {countPromptFor && (
            <CountPrompt
              providerName={countPromptFor.name}
              dirLocked={launchProject !== null}
              onConfirm={handleCountConfirm}
              onCancel={() => {
                setCountPromptFor(null);
                setLaunchProject(null);
              }}
            />
          )}
          {dirPromptFor && (
            <MultiDirPrompt
              count={dirPromptFor.count}
              lockedCwd={dirPromptFor.project?.path ?? null}
              onConfirm={handleDirsConfirm}
              onCancel={() => {
                setDirPromptFor(null);
                setLaunchProject(null);
              }}
            />
          )}
          <ToastStack toasts={toasts} onDismiss={dismissToast} />
        </div>
      </div>
    </div>
  );
}
