import type { RuntimeProbe } from "./types.ts";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export interface QuickCapabilities { probes: RuntimeProbe[]; keys: Record<string, boolean> }
const MAX_AGE_MS = 60_000;

// Memory only: reuse detection across palette/settings mounts, never persist credentials
// or session snapshots. Execution still validates the selected targets in the backend.
export function createQuickCapabilityCache(invoke: Invoke, now = Date.now) {
  const values = new Map<string, { data: QuickCapabilities; checkedAt: number }>();
  const pending = new Map<string, Promise<QuickCapabilities>>();
  const keyFor = (ids: string[]) => JSON.stringify([...new Set(ids)].sort());
  const get = (ids: string[]) => values.get(keyFor(ids));
  const fresh = (ids: string[]) => {
    const value = get(ids);
    return !!value && now() - value.checkedAt < MAX_AGE_MS;
  };
  const load = (ids: string[], force = false): Promise<QuickCapabilities> => {
    const key = keyFor(ids);
    const active = pending.get(key);
    if (active) return active;
    if (!force && fresh(ids)) return Promise.resolve(get(ids)!.data);
    // A failed explicit refresh must not leave the old result marked fresh.
    const previous = values.get(key);
    if (previous) previous.checkedAt = -Infinity;
    const request = Promise.all([
      invoke<RuntimeProbe[]>("ma_probe_runtimes"),
      Promise.all((JSON.parse(key) as string[]).map(async id => [id, await invoke<boolean>("get_key_state", { id })] as const)),
    ]).then(([probes, states]) => {
      const data = { probes, keys: Object.fromEntries(states) };
      values.set(key, { data, checkedAt: now() });
      return data;
    }).finally(() => { pending.delete(key); });
    pending.set(key, request);
    return request;
  };
  return { get: (ids: string[]) => get(ids)?.data ?? null, fresh, load };
}
