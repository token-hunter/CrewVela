import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import {
  APPEARANCE_STORAGE_KEY,
  LEGACY_THEME_STORAGE_KEY,
  applyAppearanceToRoot,
  loadAppearance,
  resolveAppearanceTheme,
} from "../appearance";
import type { ProviderConfig, SessionRecord } from "../types";
import { ControlSidebar } from "./ControlSidebar";

export function ControlWindow() {
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  useLayoutEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const syncAppearance = () => {
      const appearance = loadAppearance();
      applyAppearanceToRoot(appearance, resolveAppearanceTheme(appearance, media?.matches ?? true));
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key === APPEARANCE_STORAGE_KEY || event.key === LEGACY_THEME_STORAGE_KEY || event.key === null) {
        syncAppearance();
      }
    };
    syncAppearance();
    window.addEventListener("storage", onStorage);
    media?.addEventListener("change", syncAppearance);
    document.body.classList.add("detached-control-window");
    return () => {
      document.body.classList.remove("detached-control-window");
      window.removeEventListener("storage", onStorage);
      media?.removeEventListener("change", syncAppearance);
    };
  }, []);

  useEffect(() => {
    const openPreview = (event: Event) => {
      const url = (event as CustomEvent<{ url?: string }>).detail?.url;
      if (url) void emit("crewvela-project-preview-browse", { url })
        .catch(reason => setError(`打开项目网页失败：${String(reason)}`));
    };
    window.addEventListener("crewvela:browse", openPreview);
    return () => window.removeEventListener("crewvela:browse", openPreview);
  }, []);

  useEffect(() => {
    let disposed = false;
    let sessionTimer: number | undefined;
    let providerTimer: number | undefined;

    const refreshSessions = async () => {
      try {
        const snapshot = await invoke<SessionRecord[]>("session_snapshot");
        if (!disposed) setSessions(snapshot);
      } catch {
        // 主窗口关闭时应用会退出，无需在独立窗口重复显示错误。
      } finally {
        if (!disposed) {
          sessionTimer = window.setTimeout(refreshSessions, 400);
        }
      }
    };

    const refreshProviders = async () => { if (!disposed) setProviders([]); };

    refreshSessions();
    refreshProviders();
    return () => {
      disposed = true;
      if (sessionTimer !== undefined) window.clearTimeout(sessionTimer);
      if (providerTimer !== undefined) window.clearTimeout(providerTimer);
    };
  }, []);

  const handleSessionsChange = useCallback((snapshot: SessionRecord[]) => {
    setSessions(snapshot);
  }, []);

  const closeWindow = useCallback(async () => {
    setError(null);
    try { await invoke("close_control_window"); }
    catch (reason) { setError(`关闭总控窗口失败：${String(reason)}`); }
  }, []);

  const dockWindow = useCallback(async () => {
    setError(null);
    try { await invoke("dock_control_window"); }
    catch (reason) { setError(`停靠总控窗口失败：${String(reason)}`); }
  }, []);

  const defaultCwd =
    sessions.find(
      (session) => session.kind === "claude" && session.ptyId !== null,
    )?.cwd ??
    sessions[0]?.cwd ??
    null;

  return (
    <main className="control-window-root">
      {error && <p role="alert">{error}</p>}
      <ControlSidebar
        open
        variant="detached"
        providers={providers.filter((provider) => !provider.hidden)}
        sessions={sessions}
        defaultCwd={defaultCwd}
        onClose={closeWindow}
        onDock={dockWindow}
        onSessionsChange={handleSessionsChange}
        onOpenSession={async (sessionId, original) => { await emit("crewvela-conversation-open-session", { sessionId, original }); }}
        onOpenProjectSettings={(projectId, target) => { void emit("crewvela-conversation-open-settings", { projectId, target }).catch(reason => setError(`打开协作设置失败：${String(reason)}`)); }}
      />
    </main>
  );
}
