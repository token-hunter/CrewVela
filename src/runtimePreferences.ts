import { useCallback, useRef, useState } from "react";

export const RUNTIME_PREFERENCES_STORAGE_KEY = "crewvela-runtime-preferences.v1";
export const NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY = "crewvela-new-session-terminal-order.v1";
export const NEW_SESSION_CHAT_ORDER_STORAGE_KEY = "crewvela-new-session-chat-order.v1";

type RuntimePreferenceStorage = Pick<Storage, "getItem" | "setItem">;
export type RuntimeDropPosition = "before" | "after";
export type RuntimeOrderDirection = -1 | 1;

export function normalizeRuntimeOrder<T extends string>(
  candidate: unknown,
  availableOrder: readonly T[],
): T[] {
  const available = new Set(availableOrder);
  const normalized: T[] = [];
  const seen = new Set<T>();
  if (Array.isArray(candidate)) {
    for (const value of candidate) {
      if (typeof value === "string" && available.has(value as T) && !seen.has(value as T)) {
        normalized.push(value as T);
        seen.add(value as T);
      }
    }
  }
  for (const value of availableOrder) {
    if (!seen.has(value)) normalized.push(value);
  }
  return normalized;
}

export function moveRuntimeOrder<T extends string>(
  order: readonly T[],
  runtime: T,
  direction: RuntimeOrderDirection,
): T[] {
  const from = order.indexOf(runtime);
  const to = from + direction;
  if (from < 0 || to < 0 || to >= order.length) return [...order];
  const next = [...order];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

export function reorderRuntimeOrder<T extends string>(
  order: readonly T[],
  runtime: T,
  target: T,
  position: RuntimeDropPosition,
): T[] {
  const from = order.indexOf(runtime);
  let to = order.indexOf(target);
  if (from < 0 || to < 0 || from === to) return [...order];
  const next = [...order];
  const [item] = next.splice(from, 1);
  if (from < to) to -= 1;
  if (position === "after") to += 1;
  next.splice(to, 0, item);
  return next;
}

export function loadRuntimeOrder<T extends string>(
  availableOrder: readonly T[],
  storage: Pick<Storage, "getItem">,
  storageKey = RUNTIME_PREFERENCES_STORAGE_KEY,
): T[] {
  const stored = storage.getItem(storageKey);
  if (!stored) return [...availableOrder];
  let parsed: unknown;
  try {
    parsed = JSON.parse(stored);
  } catch {
    return [...availableOrder];
  }
  const candidate = Array.isArray(parsed)
    ? parsed
    : parsed && typeof parsed === "object"
      ? (parsed as { order?: unknown }).order
      : undefined;
  return normalizeRuntimeOrder(candidate, availableOrder);
}

export function persistRuntimeOrder<T extends string>(
  order: readonly T[],
  storage: Pick<Storage, "setItem">,
  storageKey = RUNTIME_PREFERENCES_STORAGE_KEY,
): void {
  storage.setItem(storageKey, JSON.stringify({ order }));
}

interface RuntimeOrderState<T extends string> {
  order: T[];
  error: string | null;
}

/** Loads and persists the Runtime order; storage errors remain visible to the caller. */
export function useRuntimeOrder<T extends string>(
  availableOrder: readonly T[],
  storageKey = RUNTIME_PREFERENCES_STORAGE_KEY,
) {
  const [state, setState] = useState<RuntimeOrderState<T>>(() => {
    try {
      return {
        order: loadRuntimeOrder(availableOrder, window.localStorage, storageKey),
        error: null,
      };
    } catch (reason) {
      return { order: [...availableOrder], error: `读取 Runtime 排序失败：${String(reason)}` };
    }
  });
  const orderRef = useRef(state.order);

  const commit = useCallback((next: T[]) => {
    if (next.every((item, index) => item === orderRef.current[index])) return;
    try {
      persistRuntimeOrder(next, window.localStorage, storageKey);
    } catch (reason) {
      setState((current) => ({
        ...current,
        error: `保存 Runtime 排序失败：${String(reason)}`,
      }));
      return;
    }
    orderRef.current = next;
    setState({ order: next, error: null });
  }, [storageKey]);

  const move = useCallback((runtime: T, direction: RuntimeOrderDirection) => {
    commit(moveRuntimeOrder(orderRef.current, runtime, direction));
  }, [commit]);

  const reorder = useCallback((runtime: T, target: T, position: RuntimeDropPosition) => {
    commit(reorderRuntimeOrder(orderRef.current, runtime, target, position));
  }, [commit]);

  return { order: state.order, error: state.error, move, reorder };
}
