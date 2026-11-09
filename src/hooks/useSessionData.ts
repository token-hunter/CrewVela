import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { sameSessionSnapshot } from "../sessionSnapshot";
import { loadControlSurface, type ControlSurface } from "../controlSurface";
import type { RightSidebarTab } from "../components/RightSidebarActivityBar";
import type {
  LaunchPreset,
  Project,
  ProviderConfig,
  SessionRecord,
} from "../types";

export interface UseSessionDataOptions {
  uiPreview: boolean;
  setProviders: Dispatch<SetStateAction<ProviderConfig[]>>;
  setKeyStates: Dispatch<SetStateAction<Record<string, boolean>>>;
  setSessions: Dispatch<SetStateAction<SessionRecord[]>>;
  setArchivedSessions: Dispatch<SetStateAction<SessionRecord[]>>;
  setProjects: Dispatch<SetStateAction<Project[]>>;
  setPresets: Dispatch<SetStateAction<LaunchPreset[]>>;
  setControlDetached: Dispatch<SetStateAction<boolean>>;
  setControlSurface: Dispatch<SetStateAction<ControlSurface>>;
  setLastRightSidebarTab: Dispatch<SetStateAction<RightSidebarTab>>;
  setFilesOpen: Dispatch<SetStateAction<boolean>>;
  setControlOpen: Dispatch<SetStateAction<boolean>>;
}

export interface UseSessionDataResult {
  sessionsReady: boolean;
  refreshProviders: () => Promise<void>;
  refreshSessions: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  refreshPresets: () => Promise<void>;
}

/** providers/会话/项目/预设的加载与 400ms 轮询快照。由 App.tsx 搬移而来。 */
export function useSessionData({
  uiPreview,
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
}: UseSessionDataOptions): UseSessionDataResult {
  const [sessionsReady, setSessionsReady] = useState(uiPreview);
  const requests = useRef({ providers: 0, sessions: 0, archivedSessions: 0, projects: 0, presets: 0 });
  useEffect(() => () => {
    for (const key of Object.keys(requests.current) as (keyof typeof requests.current)[]) ++requests.current[key];
  }, []);

  const refreshProviders = useCallback(async () => {
    const request = ++requests.current.providers;
    try {
      const list = await invoke<ProviderConfig[]>("list_providers");
      if (request !== requests.current.providers) return;
      setProviders(list);
      const states: Record<string, boolean> = {};
      await Promise.all(
        list.map(async (p) => {
          states[p.id] = await invoke<boolean>("get_key_state", { id: p.id });
        }),
      );
      if (request === requests.current.providers) setKeyStates(states);
    } catch (e) {
      console.error("加载 providers 失败:", e);
    }
  }, [setKeyStates, setProviders]);

  const refreshSessions = useCallback(async () => {
    const request = ++requests.current.sessions;
    const archivedRequest = ++requests.current.archivedSessions;
    try {
      const [active, archived] = await Promise.all([
        invoke<SessionRecord[]>("list_sessions"),
        invoke<SessionRecord[]>("list_archived_sessions"),
      ]);
      if (request === requests.current.sessions) { setSessions(active); setSessionsReady(true); }
      if (archivedRequest === requests.current.archivedSessions) setArchivedSessions(archived);
    } catch (e) {
      console.error("加载会话失败:", e);
    }
  }, [setArchivedSessions, setSessions]);

  const refreshProjects = useCallback(async () => {
    const request = ++requests.current.projects;
    try {
      const projects = await invoke<Project[]>("list_projects");
      if (request === requests.current.projects) setProjects(projects);
    } catch (e) {
      console.error("加载项目失败:", e);
    }
  }, [setProjects]);

  const refreshPresets = useCallback(async () => {
    const request = ++requests.current.presets;
    try {
      const presets = await invoke<LaunchPreset[]>("list_launch_presets");
      if (request === requests.current.presets) setPresets(presets);
    } catch (e) {
      console.error("加载预设失败:", e);
    }
  }, [setPresets]);

  useEffect(() => {
    if (uiPreview) return;
    void refreshProviders();
    void refreshSessions();
    void refreshProjects();
    void refreshPresets();
  }, [
    refreshPresets,
    refreshProjects,
    refreshProviders,
    refreshSessions,
    uiPreview,
  ]);

  useEffect(() => {
    if (uiPreview) return;
    let disposed = false;
    let timer: number | undefined;

    const pollControlWindow = async () => {
      const request = ++requests.current.sessions;
      try {
        const [detached, dockRequested, snapshot] = await Promise.all([
          invoke<boolean>("is_control_window_open"),
          invoke<boolean>("take_control_dock_request"),
          invoke<SessionRecord[]>("session_snapshot"),
        ]);
        if (disposed) return;

        if (request === requests.current.sessions) {
          setSessions((current) => sameSessionSnapshot(current, snapshot) ? current : snapshot);
          setSessionsReady(true);
        }
        setControlDetached(detached);
        if (dockRequested) {
          const surface = loadControlSurface();
          setControlSurface(surface);
          setLastRightSidebarTab("multi_agent");
          setControlDetached(false);
          setFilesOpen(false);
          setControlOpen(true);
        }
      } catch {
        // 应用关闭或窗口重载时终止本轮轮询。
      } finally {
        if (!disposed) {
          timer = window.setTimeout(pollControlWindow, 400);
        }
      }
    };

    void pollControlWindow();

    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [
    setControlDetached,
    setControlOpen,
    setControlSurface,
    setFilesOpen,
    setLastRightSidebarTab,
    setSessions,
    uiPreview,
  ]);

  return {
    sessionsReady,
    refreshProviders,
    refreshSessions,
    refreshProjects,
    refreshPresets,
  };
}
