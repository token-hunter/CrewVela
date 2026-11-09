import { migrateStorageNamespace } from "./storageNamespaceMigration.ts";

// Complete before importing App and the detached views that read settings.
export async function initializeStorage(
  storage: Storage = window.localStorage,
  allowedNamespaces?: ReadonlySet<string>,
): Promise<void> {
  const result = await migrateStorageNamespace(storage, allowedNamespaces);
  if (result.failed > 0) throw new Error("本地设置迁移失败，原数据已保留。请恢复存储后重试。");
  if (result.retained > 0) console.warn("CrewVela 本地设置存在冲突，保留当前配置和原数据。", result.retained);
}
