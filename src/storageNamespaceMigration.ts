// Migrate known application settings by their stable semantic suffix. Values
// (including session/workspace IDs and user text) are copied without rewriting.
export const SETTINGS_SUFFIXES = [
  "appearance", "theme",
  "right-sidebar-tab", "right-sidebar-width", "file-sidebar-width", "control-sidebar-width",
  "git-sidebar-width", "browser-sidebar-width", "left-sidebar-width", "workspace-tabs",
  "native-chat-tab-order.v1", "file-preview-settings-v1", "browser-panel-state-v1",
  "notification-settings-v1", "notification-history-v1", "pet-settings-v1",
  "runtime-preferences", "runtime-preferences.v1",
  "new-session-terminal-order.v1", "new-session-chat-order.v1", "sidebar-shortcuts",
  "multi-agent-project", "control-messages", "control-draft", "file-tree-width",
  "multi-agent-draft-v1", "multi-agent-auto-report-v1", "favorite-sessions", "collapsed-projects",
  "control-surface",
] as const;

export interface StorageMigrationResult {
  migrated: number;
  retained: number;
  failed: number;
}

// Exact identity of the previously released storage namespace. A fingerprint
// keeps retired product identifiers out of current interfaces and source text.
const RELEASED_NAMESPACE_FINGERPRINTS = new Set([
  "7393b7a43671e4a418832e8eada06031df18058b865cc8fcef757e1b29d988ff",
  "e3946634d41c620eba19d52341225a0f0a2c3c58ee487584f654bc4ca4a1c341",
]);

export async function namespaceFingerprint(namespace: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(namespace));
  return [...new Uint8Array(bytes)].map((value) => value.toString(16).padStart(2, "0")).join("");
}

function targetKey(key: string): { namespace: string; target: string } | null {
  if (key.startsWith("crewvela-") || key.startsWith("crewvela:")) return null;
  const restartSuffix = ":codex-restart-required";
  if (key.endsWith(restartSuffix)) return {
    namespace: key.slice(0, -restartSuffix.length), target: `crewvela${restartSuffix}`,
  };
  const dynamic = "-collaboration-auto:";
  const dynamicIndex = key.indexOf(dynamic);
  if (dynamicIndex > 0 && key.length > dynamicIndex + dynamic.length) {
    return { namespace: key.slice(0, dynamicIndex), target: `crewvela${key.slice(dynamicIndex)}` };
  }
  const suffix = SETTINGS_SUFFIXES.find((value) => key.endsWith(`-${value}`));
  return suffix && key.length > suffix.length + 1 ? {
    namespace: key.slice(0, -(suffix.length + 1)), target: `crewvela-${suffix}`,
  } : null;
}

function mergeHistory(current: string, previous: string): string | null {
  try {
    const newer: unknown = JSON.parse(current);
    const older: unknown = JSON.parse(previous);
    if (!Array.isArray(newer) || !Array.isArray(older)) return null;
    const records = new Map<string, unknown>();
    for (const record of [...newer, ...older]) {
      if (!record || typeof record !== "object" || typeof record.id !== "string") return null;
      if (!records.has(record.id)) records.set(record.id, record);
    }
    return JSON.stringify([...records.values()]);
  } catch {
    return null;
  }
}

export async function migrateStorageNamespace(
  storage: Storage,
  allowedNamespaces: ReadonlySet<string> = RELEASED_NAMESPACE_FINGERPRINTS,
): Promise<StorageMigrationResult> {
  const result = { migrated: 0, retained: 0, failed: 0 };
  const candidates = new Map<string, string[]>();
  const namespaces = new Map<string, Promise<boolean>>();
  const keys: string[] = [];
  // Snapshot before writing/removing: Storage indexes change after removal.
  for (let index = 0; index < storage.length; index += 1) {
    const source = storage.key(index);
    if (source === null) continue;
    keys.push(source);
  }
  for (const source of keys) {
    const match = targetKey(source);
    if (!match) continue;
    if (!namespaces.has(match.namespace)) namespaces.set(match.namespace,
      namespaceFingerprint(match.namespace).then((fingerprint) => allowedNamespaces.has(fingerprint)));
    if (await namespaces.get(match.namespace)) {
      candidates.set(match.target, [...(candidates.get(match.target) ?? []), source]);
    }
  }
  for (const [target, sources] of candidates) {
    // Do not guess if more than one previous namespace owns the same setting.
    if (sources.length !== 1) { result.retained += sources.length; continue; }
    const source = sources[0];
    try {
      const previous = storage.getItem(source);
      if (previous === null) continue;
      const current = storage.getItem(target);
      let value = previous;
      if (current !== null && current !== previous) {
        const merged = target === "crewvela-notification-history-v1" ? mergeHistory(current, previous) : null;
        if (merged === null) { result.retained += 1; continue; }
        value = merged;
      }
      storage.setItem(target, current === previous ? current : value);
      // Failed writes must never delete the only durable copy.
      if (storage.getItem(target) !== (current === previous ? current : value)) {
        result.retained += 1;
        result.failed += 1;
        continue;
      }
      storage.removeItem(source);
      result.migrated += 1;
    } catch {
      result.retained += 1;
      result.failed += 1;
    }
  }
  return result;
}
