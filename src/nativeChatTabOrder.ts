export const NATIVE_CHAT_TAB_ORDER_KEY = "crewvela-native-chat-tab-order.v1";
export type NativeChatTabOrders = Record<string, string[]>;

export function loadNativeChatTabOrders(storage: Pick<Storage, "getItem">): NativeChatTabOrders {
  const raw = storage.getItem(NATIVE_CHAT_TAB_ORDER_KEY);
  if (!raw) return {};
  const parsed: unknown = JSON.parse(raw);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("聊天标签顺序格式无效");
  const entries = Object.entries(parsed);
  if (entries.some(([, order]) => !Array.isArray(order) || order.some(id => typeof id !== "string"))) throw new Error("聊天标签顺序格式无效");
  return Object.fromEntries(entries.map(([workspace, order]) => [workspace, [...new Set(order as string[])]]));
}

/** 保存逻辑 slot；隐藏或关闭的标签保留位置，新 slot 追加到末尾。 */
export function orderedNativeChatSlots(open: readonly string[], saved: readonly string[] = []): string[] {
  return [...new Set([...saved.filter(slot => open.includes(slot)), ...open])];
}

export function reorderNativeChatSlots(saved: readonly string[], visible: readonly string[], source: string, target: string, after: boolean): string[] {
  const all = [...new Set([...saved, ...visible])];
  if (!visible.includes(source) || !visible.includes(target) || source === target) return all;
  const next = visible.filter(slot => slot !== source);
  next.splice(next.indexOf(target) + (after ? 1 : 0), 0, source);
  let index = 0;
  return all.map(slot => visible.includes(slot) ? next[index++] : slot);
}

export function saveNativeChatTabOrders(orders: NativeChatTabOrders, storage: Pick<Storage, "setItem">): void {
  storage.setItem(NATIVE_CHAT_TAB_ORDER_KEY, JSON.stringify(orders));
}
