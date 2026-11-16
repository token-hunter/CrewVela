import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentRuntimeKind } from "./types";
import { isRetiredRuntime } from "./runtimeTaxonomy.ts";

export interface RuntimePreferences { disabledRuntimes: AgentRuntimeKind[]; defaultLaunchModes?: Record<string, string> }
const PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
const STORAGE_KEY = "crewvela-runtime-preferences";
let state: { prefs: RuntimePreferences; ready: boolean; error: string | null } = {
  prefs: { disabledRuntimes: [] }, ready: false, error: null,
};
const subscribers = new Set<() => void>();
let request: Promise<void> | null = null;
let initialization: Promise<void> | null = null;
let generation = 0;
let pendingMutations = 0;
function publish(prefs: RuntimePreferences, error: string | null = null) {
  generation += 1;
  state = { prefs, ready: !error, error };
  subscribers.forEach((notify) => notify());
}
export function isRuntimeEnabled(kind: AgentRuntimeKind): boolean {
  return state.ready && !isRetiredRuntime(kind) && !state.prefs.disabledRuntimes.includes(kind);
}
function validatePreferences(prefs: RuntimePreferences): RuntimePreferences {
  if (!prefs || !Array.isArray(prefs.disabledRuntimes)) throw new Error("Runtime 启用配置格式无效");
  return prefs;
}
function acceptPreferences(prefs: RuntimePreferences) {
  try { publish(validatePreferences(prefs)); }
  catch (reason) { publish(state.prefs, String(reason)); }
}
function initialize(): Promise<void> {
  if (initialization) return initialization;
  initialization = (async () => {
    if (PREVIEW) {
      window.addEventListener("storage", (event) => {
        if (event.key !== STORAGE_KEY) return;
        try { acceptPreferences(JSON.parse(event.newValue ?? '{"disabledRuntimes":[]}')); }
        catch (reason) { publish(state.prefs, String(reason)); }
      });
    } else {
      await listen<RuntimePreferences>("runtime-preferences-changed", (event) => {
        acceptPreferences(event.payload);
      });
    }
  })().catch((reason) => {
    initialization = null;
    throw reason;
  });
  return initialization;
}
export function refreshRuntimePreferences(): Promise<void> {
  if (request) return request;
  request = (async () => {
    const requestGeneration = generation;
    try {
      // Register before reading so a concurrent change cannot be missed.
      await initialize();
      const prefs = PREVIEW
        ? JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{"disabledRuntimes":[]}') as RuntimePreferences
        : await invoke<RuntimePreferences>("get_runtime_preferences");
      if (requestGeneration === generation && pendingMutations === 0) publish(validatePreferences(prefs));
    } catch (reason) {
      if (requestGeneration === generation && pendingMutations === 0) publish(state.prefs, String(reason));
    } finally { request = null; }
  })();
  return request;
}
export async function setRuntimeEnabled(kind: AgentRuntimeKind, enabled: boolean): Promise<void> {
  if (!state.ready) throw new Error(state.error ?? "Runtime 启用配置尚未加载");
  if (isRetiredRuntime(kind)) throw new Error("该 CLI 已停用，不能重新启用");
  // Invalidate older reads before changing authoritative preferences.
  const mutationGeneration = ++generation;
  pendingMutations += 1;
  try {
    const prefs = PREVIEW ? {
      ...state.prefs,
      disabledRuntimes: enabled
        ? state.prefs.disabledRuntimes.filter((item) => item !== kind)
        : [...new Set([...state.prefs.disabledRuntimes, kind])],
    } : await invoke<RuntimePreferences>("set_runtime_enabled", { kind, enabled });
    if (mutationGeneration !== generation) return;
    try {
      validatePreferences(prefs);
      if (PREVIEW) window.localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
      publish(prefs);
    } catch (reason) {
      publish(state.prefs, String(reason));
      throw reason;
    }
  } finally { pendingMutations -= 1; }
}
export function useRuntimeAvailability() {
  const snapshot = useSyncExternalStore((notify) => {
    subscribers.add(notify);
    return () => { subscribers.delete(notify); };
  }, () => state);
  useEffect(() => { if (!initialization) void refreshRuntimePreferences(); }, []);
  return { ...snapshot, isRuntimeEnabled, refresh: refreshRuntimePreferences, setRuntimeEnabled, setRuntimeLaunchDefault };
}

export async function setRuntimeLaunchDefault(kind: string, mode: string): Promise<void> {
  if (!state.ready) throw new Error(state.error ?? "Agent 配置尚未加载");
  const mutationGeneration = ++generation;
  pendingMutations += 1;
  try {
    const prefs = PREVIEW
      ? { ...state.prefs, defaultLaunchModes: { ...state.prefs.defaultLaunchModes, [kind]: mode } }
      : await invoke<RuntimePreferences>("set_runtime_launch_default", { kind, mode });
    validatePreferences(prefs);
    if (PREVIEW) window.localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
    if (mutationGeneration === generation) publish(prefs);
  } finally { pendingMutations -= 1; }
}
