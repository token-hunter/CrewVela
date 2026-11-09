export const GIT_SIDEBAR_ENTRY_LIMIT = 500;

export function limitGroupedEntries<T>(groups: T[][], limit: number): T[][] {
  let remaining = limit;
  return groups.map((entries) => {
    const visible = entries.slice(0, Math.max(remaining, 0));
    remaining -= visible.length;
    return visible;
  });
}
