import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LocalProjectServiceStatus } from "../types";

export function projectServiceActive(status?: LocalProjectServiceStatus) {
  return status?.state === "running" || status?.state === "starting" || status?.state === "stopFailed";
}

export function useLocalProjectServices(enabled: boolean) {
  const [statuses, setStatuses] = useState<Record<string, LocalProjectServiceStatus>>({});
  const [pending, setPending] = useState<ReadonlySet<string>>(new Set());
  const [readError, setReadError] = useState("");
  const actions = useRef(new Set<string>());
  const request = useRef(0);
  const refresh = useCallback(async () => {
    const current = ++request.current;
    try {
      const next = await invoke<Record<string, LocalProjectServiceStatus>>("local_project_service_statuses");
      if (current !== request.current) return;
      setStatuses(next); setReadError("");
    } catch (reason) {
      if (current === request.current) setReadError(String(reason));
    }
  }, []);
  useEffect(() => {
    if (!enabled) { setStatuses({}); setReadError(""); return; }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => { request.current++; window.clearInterval(timer); };
  }, [enabled, refresh]);

  const change = useCallback(async (id: string, action: "start" | "stop") => {
    if (actions.current.has(id)) return;
    actions.current.add(id);
    setPending(new Set(actions.current));
    request.current++;
    try {
      const status = await invoke<LocalProjectServiceStatus>(
        action === "start" ? "local_project_service_start" : "local_project_service_stop", { id },
      );
      request.current++;
      setStatuses(current => ({ ...current, [id]: status }));
    } finally {
      actions.current.delete(id);
      setPending(new Set(actions.current));
    }
  }, []);
  return { statuses, pending, readError, change, refresh };
}

export type LocalProjectServices = ReturnType<typeof useLocalProjectServices>;
