export interface DistributionAsset { id: string; name: string; description: string; trustState: string }
export interface DistributionDeployment { skillId: string; runtime: string; status: string }
export type DistributionFilter = "all" | "undeployed" | "synced" | "attention" | "untrusted";

export function scopedDistribution<T extends { scope: string; projectPath: string | null; taskId: string | null; agentId: string | null }>(items: readonly T[], scope: string, projectPath: string | null, taskId: string, agentId: string): T[] {
  return items.filter(item => item.scope === scope && (scope === "global" || item.projectPath === projectPath)
    && (scope !== "task" || (item.taskId === (taskId.trim() || null) && item.agentId === (agentId.trim() || null))));
}

/** Filter against every selected target; one synced Runtime must not hide a missing or failed deployment. */
export function distributionAssets<T extends DistributionAsset>(assets: readonly T[], deployments: readonly DistributionDeployment[], query: string, filter: DistributionFilter, runtimes: readonly string[]): T[] {
  const search = query.trim().toLocaleLowerCase();
  const statuses = new Map(deployments.map(item => [`${item.skillId}::${item.runtime}`, item.status]));
  return assets.filter(asset => {
    if (search && !`${asset.name} ${asset.description}`.toLocaleLowerCase().includes(search)) return false;
    const targets = runtimes.map(runtime => statuses.get(`${asset.id}::${runtime}`));
    return filter === "all" || (filter === "undeployed" && targets.some(status => !status))
      || (filter === "synced" && targets.length > 0 && targets.every(status => status === "synced"))
      || (filter === "attention" && targets.some(status => !!status && status !== "synced"))
      || (filter === "untrusted" && asset.trustState !== "trusted");
  });
}

export function distributionSelection(skillIds: ReadonlySet<string>, runtimes: readonly string[], assets: readonly Pick<DistributionAsset, "id">[]): string[] {
  return assets.filter(asset => skillIds.has(asset.id)).flatMap(asset => [...new Set(runtimes)].map(runtime => `${asset.id}::${runtime}`));
}
