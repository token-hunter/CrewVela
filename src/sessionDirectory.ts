import type { Project, SessionDirectory } from "./types";

function normalizePath(path: string): string {
  const slashes = path.replace(/\\/g, "/");
  const normalized = slashes.replace(/\/+$/, "") || (slashes.startsWith("/") ? "/" : "");
  return /^[A-Za-z]:/.test(path) || path.startsWith("\\\\") ? normalized.toLowerCase() : normalized;
}

/** 按最终目录重新计算项目归属，临时目录不携带之前的 projectId。 */
export function resolveSessionDirectory(path: string, projects: readonly Project[], currentProjectId?: string | null): SessionDirectory {
  const normalized = normalizePath(path);
  let match: { project: Project; root: string } | undefined;
  for (const project of projects) {
    if (project.archivedAt != null) continue;
    const root = normalizePath(project.path);
    const scopedSubdirectory = project.id === currentProjectId && normalized.startsWith(root === "/" ? root : `${root}/`);
    if (root && (normalized === root || scopedSubdirectory) && (!match || root.length > match.root.length)) {
      match = { project, root };
    }
  }
  if (match) return normalized === match.root ? match.project : { id: match.project.id, name: match.project.name, path };
  return { path, name: path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path };
}
