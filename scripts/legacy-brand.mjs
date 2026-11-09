// Read compatibility only. All newly created roots use the CrewVela identity.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
export const LEGACY_NAME = 'cc-launcher';
export function resolveConfigDirectory(base) {
  const current = join(base, 'CrewVela');
  if (existsSync(current)) return current;
  const previous = join(base, LEGACY_NAME);
  return existsSync(previous) ? previous : current;
}
