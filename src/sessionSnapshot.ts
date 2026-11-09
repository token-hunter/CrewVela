import type { SessionRecord } from "./types";

export function sameSessionSnapshot(
  current: SessionRecord[],
  next: SessionRecord[],
): boolean {
  return (
    current.length === next.length &&
    current.every(
      (session, index) =>
        session.id === next[index]?.id &&
        session.updatedAt === next[index]?.updatedAt,
    )
  );
}
