import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppTheme, ProviderConfig, SessionRecord } from "../types";
import { ControlSidebar } from "./ControlSidebar";
const THEME_STORAGE_KEY = "crewvela-theme";

function loadTheme(): AppTheme {
  try {
    return window.localStorage.getItem(THEME_STORAGE_KEY) === "light"
      ? "light"
      : "dark";
  } catch {
    return "dark";
  }
}

export function ControlWindow() {
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [theme] = useState<AppTheme>(loadTheme);
  const [error, setError] = useState<string | null>(null);
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.body.classList.add("detached-control-window");
    return () => document.body.classList.remove("detached-control-window");
  }, [theme]);

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

    const refreshProviders = async () => {
      try {
        const items = await invoke<ProviderConfig[]>("list_providers");
        if (!disposed) setProviders(items);
      } catch {
        // 主窗口关闭时应用会退出，无需在独立窗口重复显示错误。
      } finally {
        if (!disposed) {
          providerTimer = window.setTimeout(refreshProviders, 1000);
        }
      }
    };

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
      />
    </main>
  );
}
