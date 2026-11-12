/** 结构相等；对象键顺序无关——Rust 的 HashMap 每次序列化的键顺序都可能不同。 */
export function sameValue(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    return Array.isArray(a) && Array.isArray(b) && a.length === b.length
      && a.every((item, index) => sameValue(item, b[index]));
  }
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length
    && keys.every((key) => key in right && sameValue(left[key], right[key]));
}

/** 轮询结果与当前状态内容相同就沿用旧引用，React 会跳过这次整棵 App 的重渲染。 */
export function keepIfUnchanged<T>(current: T, next: T): T {
  return sameValue(current, next) ? current : next;
}
