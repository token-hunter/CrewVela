// Isolated IPC for Runtime-first UI acceptance. No filesystem or network access.
export function installSkillRuntimeFixture(params: URLSearchParams) {
  const names = ["browser-control", "computer-control", "agent-image", "agent-reach", "agnes-ai"];
  const assets = (params.get("skills") === "empty" ? [] : params.get("skills") === "long" ? [...names, ...Array.from({ length: 60 }, (_, i) => `example-skill-${i}`)] : names).map((name, i) => ({
    id: name, name, description: `示例 Skill：${name}`, sourceType: "git", sourceRef: "https://example.invalid/skills", sourceRevision: "fixture-revision",
    centralPath: `/fixture/skills/${name}`, contentHash: `fixture-hash-${i}`, trustState: i === 2 || i === 4 ? "unknown" : "trusted", fileCount: 2, scriptCount: 0, riskSummary: [], updatedAt: 0,
  }));
  let deployments: any[] = names.slice(0, 2).map((skillId, i) => ({ id: `deployment-${i}`, skillId, runtime: "claude_code", scope: "global", projectPath: null, taskId: null, agentId: null, targetPath: `/fixture/claude/${skillId}`, mode: "copy", status: "synced", sourceHash: assets[i]?.contentHash, targetHash: assets[i]?.contentHash, lastError: null, updatedAt: 0 }));
  deployments.push({ ...deployments[0], id: "conflict", runtime: "codex", status: "conflict", targetPath: "/fixture/codex/browser-control", lastError: "示例冲突：Runtime 副本已被修改" });
  let sets: any[] = [];
  const runtimes = [["claude_code", "Claude Code", ".claude/skills"], ["codex", "Codex", ".codex/skills"], ["kimi_code", "Kimi Code", ".kimi/skills"], ["pi", "Pi", ".pi/skills"], ["opencode", "OpenCode", ".opencode/skills"]].map(([runtime, label, projectRoot]) => ({ runtime, label, projectRoot, globalRoot: `/fixture/${runtime}/skills`, discoveryRoots: [], installed: runtime !== "opencode", recursiveScan: false }));
  let failNext = params.get("fail");
  let failRead = params.get("failRead") === "1";
  const log: any[] = [];
  const record = (command: string, args: any) => {
    log.push({ command, args });
    const output = document.getElementById("fixture-skills-log");
    if (output) output.textContent = JSON.stringify(log, null, 2);
  };
  // Deterministic fixture confirmation, including cancellation, without a native dialog.
  window.confirm = message => {
    const accepted = params.get("confirm") !== "cancel";
    record("fixture_confirmation", { message, accepted });
    return accepted;
  };
  const snap = () => structuredClone({ assets, deployments, skillSets: sets, runtimes });
  (window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any = {}) {
    record(command, args);
    if (command === "skill_center_snapshot") {
      if (failRead) { failRead = false; throw Error("示例：读取中央库失败"); }
      return snap();
    }
    if (command === "skill_center_update_checks" || command === "skill_center_market_repositories" || command === "skill_center_versions") return [];
    if (command === "skill_center_market_browse") return { skills: [], errors: [] };
    if (command === "plugin:event|listen") return 1;
    if (command === "plugin:event|unlisten") return;
    await new Promise(resolve => setTimeout(resolve, 350));
    if (failNext && command === `skill_center_${failNext}`) { failNext = null; throw Error("示例：操作失败，未改动 Runtime"); }
    if (command === "skill_center_preview") return { skillId: args.skillId, skillMd: `# ${args.skillId}\n\n示例说明。`, files: [{ path: "SKILL.md", size: 60, isScript: false }] };
    if (command === "skill_center_read_file") return "# 示例文件";
    if (command === "skill_center_set_trust") { const asset = assets.find(a => a.id === args.skillId); if (asset) asset.trustState = args.trustState; return; }
    if (command === "skill_center_discover") return [{ runtime: "claude_code", name: "example-local", path: "/fixture/discovered/example-local", description: "示例本地 Skill", contentHash: "fixture", alreadyManaged: false }];
    if (command === "plugin:dialog|open") return args.options?.directory ? ["/fixture/imported"] : ["/fixture/example.zip"];
    if (command === "skill_center_import") return {};
    if (command === "skill_center_save_set") { sets = [...sets, { ...args.input, id: `set-${sets.length}`, description: "示例配置集", updatedAt: 0 }]; return sets.at(-1); }
    if (command === "skill_center_delete_set") { sets = sets.filter(set => set.id !== args.id); return; }
    if (command === "skill_center_apply_set") return;
    if (command === "skill_center_deploy_batch") {
      const conflict = args.inputs.some((input: any) => deployments.some(d => d.skillId === input.skillId && d.runtime === input.runtime && d.scope === input.scope && d.projectPath === input.projectPath && d.status === "conflict"));
      if (conflict) throw Error("示例冲突：整批未写入");
      for (const input of args.inputs) {
        deployments = deployments.filter(d => !(d.skillId === input.skillId && d.runtime === input.runtime && d.scope === input.scope && d.projectPath === input.projectPath && d.taskId === input.taskId && d.agentId === input.agentId));
        deployments.push({ ...input, id: `new-${deployments.length}`, targetPath: `/fixture/${input.runtime}/${input.skillId}`, mode: "copy", status: "synced", sourceHash: "fixture", targetHash: "fixture", lastError: null, updatedAt: 0 });
      }
      return [];
    }
    if (command === "skill_center_undeploy_batch" || command === "skill_center_undeploy") {
      const ids = args.deploymentIds ?? [args.deploymentId];
      if (deployments.some(d => ids.includes(d.id) && d.status === "conflict")) throw Error("示例：存在本地修改，拒绝删除");
      deployments = deployments.filter(d => !ids.includes(d.id)); return;
    }
    throw Error(`隔离页面未模拟 ${command}`);
  } };
}
