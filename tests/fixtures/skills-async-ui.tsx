import { createRoot } from "react-dom/client";
import { SkillCenterPanel } from "../../src/components/SkillCenterPanel";
import { SkillMarketPanel } from "../../src/components/SkillMarketPanel";
import { SkillUpdatesPanel } from "../../src/components/SkillUpdatesPanel";
import "../../src/styles.css";

const asset = (id: string) => ({ id, name: `Skill-${id}`, description: `Description-${id}`, sourceType: "archive", sourceRef: "/fixture.zip", sourceRevision: null, contentHash: "hash-old", centralPath: "/fixture", trustState: "trusted", fileCount: 1, scriptCount: 0, riskSummary: [], updatedAt: 1 });
let assets = [asset("A"), asset("B")];
const snapshot = () => ({ assets, deployments: [], skillSets: [], runtimes: [{ runtime: "codex", label: "Codex", globalRoot: "/fixture", projectRoot: "/fixture", installed: true, discoveryRoots: [], recursiveScan: false }] });
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (error: Error) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
let checked = false;
const checks = () => checked ? [{ id: "check-A", skillId: "A", status: "available", message: "fixture candidate", checkedAt: 1, baseHash: "hash-old", candidate: { ...assets[0], contentHash: "hash-new" }, omittedFiles: 0, changes: [] }] : [];
const mock = { calls, pending, deferSnapshots: true, snapshot,
  invoke(command: string, args: any) {
    calls.push({ command, args });
    if ((command === "skill_center_snapshot" && mock.deferSnapshots) || ["plugin:dialog|open", "skill_center_import", "skill_center_preview", "skill_center_skills_sh_search", "skill_center_market_install", "skill_center_check_updates", "skill_center_apply_update"].includes(command)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    if (command === "skill_center_snapshot") return Promise.resolve(snapshot());
    if (command === "skill_center_update_checks") return Promise.resolve(checks());
    if (command === "skill_center_market_browse") return Promise.resolve({ skills: [], errors: [] });
    return Promise.resolve([]);
  },
  release(command: string, value: any, error?: string, query?: string) {
    const index = pending.findIndex(item => item.command === command && (!query || item.args.query === query));
    if (index < 0) throw Error(`No pending ${command}/${query}`);
    const item = pending.splice(index, 1)[0];
    if (command === "skill_center_snapshot") mock.deferSnapshots = false;
    if (error) { item.reject(Error(error)); return; }
    if (command === "skill_center_import" && item.args.input.source === "one") assets = [...assets, asset("new")];
    if (command === "skill_center_check_updates") checked = true;
    item.resolve(value);
  },
};
(window as any).skillsMock = mock;
const mode = new URLSearchParams(location.search).get("mode");
createRoot(document.getElementById("root")!).render(mode === "market" ? <SkillMarketPanel onInstalled={async () => { await mock.invoke("fixture_reload", {}); }} /> : mode === "updates" ? <SkillUpdatesPanel assets={assets} deployments={[]} initialSkillId="A" onChanged={async () => { await mock.invoke("fixture_reload", {}); }} /> : <SkillCenterPanel projects={[]} />);
