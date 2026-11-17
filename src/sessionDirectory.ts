import type { Project, SessionDirectory } from "./types";

function normalizePath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[A-Za-z]:/.test(path) || path.startsWith("\\\\") ? normalized.toLowerCase() : normalized;
}

/** 按最终目录重新计算项目归属，临时目录不携带之前的 projectId。 */
export function resolveSessionDirectory(path: string, projects: readonly Project[]): SessionDirectory {
  const existing = projects.find(project => normalizePath(project.path) === normalizePath(path));
  return existing ?? { path, name: path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path };
}
