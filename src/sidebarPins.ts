export const PINNED_SESSIONS_STORAGE_KEY = "crewvela-pinned-sessions.v1";
export const PINNED_PROJECTS_STORAGE_KEY = "crewvela-pinned-projects.v1";

export function loadSidebarPins(key: string, storage: Pick<Storage, "getItem">): Set<string> {
  const value: unknown = JSON.parse(storage.getItem(key) ?? "[]");
  return new Set(Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : []);
}

export function toggleSidebarPin(pins: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(pins);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

// 稳定分区：置顶和普通项各自保留原顺序，取消置顶后恢复原有排序。
export function pinnedFirst<T>(items: readonly T[], pins: ReadonlySet<string>, id: (item: T) => string): T[] {
  return [
    ...items.filter((item) => pins.has(id(item))),
    ...items.filter((item) => !pins.has(id(item))),
  ];
}
