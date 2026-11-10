export interface DistributionAsset { id: string; name: string; description: string; trustState: string }
export interface DistributionDeployment { skillId: string; status: string }
export type DistributionFilter = "all" | "undeployed" | "synced" | "attention" | "untrusted";

export function scopedDistribution<T extends { scope: string; projectPath: string | null; taskId: string | null; agentId: string | null }>(items: readonly T[], scope: string, projectPath: string | null, taskId: string, agentId: string): T[] {
  return items.filter(item => item.scope === scope && (scope === "global" || item.projectPath === projectPath)
    && (scope !== "task" || (item.taskId === (taskId.trim() || null) && item.agentId === (agentId.trim() || null))));
}

/** A Runtime view only exposes its current filtered assets for bulk operations. */
export function distributionAssets<T extends DistributionAsset>(assets: readonly T[], deployments: readonly DistributionDeployment[], query: string, filter: DistributionFilter): T[] {
  const search = query.trim().toLocaleLowerCase();
  const statuses = new Map(deployments.map(item => [item.skillId, item.status]));
  return assets.filter(asset => {
    if (search && !`${asset.name} ${asset.description}`.toLocaleLowerCase().includes(search)) return false;
    const status = statuses.get(asset.id);
    return filter === "all" || (filter === "undeployed" && !status)
      || (filter === "synced" && status === "synced")
      || (filter === "attention" && !!status && status !== "synced")
      || (filter === "untrusted" && asset.trustState !== "trusted");
  });
}

export function distributionSelection(cells: ReadonlySet<string>, runtime: string, assets: readonly Pick<DistributionAsset, "id">[]): string[] {
  return assets.map(asset => `${asset.id}::${runtime}`).filter(key => cells.has(key));
}
