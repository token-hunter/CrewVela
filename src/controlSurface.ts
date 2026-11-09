export type ControlSurface = "quick" | "multi_agent";

export const CONTROL_SURFACE_STORAGE_KEY = "crewvela-control-surface";

export function loadControlSurface(): ControlSurface {
  try {
    return window.localStorage.getItem(CONTROL_SURFACE_STORAGE_KEY) === "multi_agent"
      ? "multi_agent"
      : "quick";
  } catch {
    return "quick";
  }
}

export function saveControlSurface(surface: ControlSurface): void {
  try {
    window.localStorage.setItem(CONTROL_SURFACE_STORAGE_KEY, surface);
  } catch {
    // 当前窗口状态仍然有效。
  }
}
