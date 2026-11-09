import { createRoot } from "react-dom/client";
import { GitSettingsPanel, GitSourceControlPanel } from "../../src/components/GitSettingsPanel";
import type { Project, ProviderConfig } from "../../src/types";
import "../../src/styles.css";

const settings = { branchPrefix: "fixture/", compareStrategy: "repository_default", pullStrategy: "ff_only", commitInstruction: "fixture" };
const mode = new URLSearchParams(location.search).get("mode");
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (reason: Error) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
const snapshot = (cwd: string, label = cwd) => ({ cwd, repoRoot: cwd, branch: label, upstream: null, ahead: 0, behind: 0, mergeInProgress: false, compareBase: null, committedEntries: [], entries: [{ path: `${label}.txt`, status: "M.", staged: true, unstaged: false, untracked: false, conflicted: false, additions: 1, deletions: 0 }] });
const fixture = {
  calls, pending,
  invoke(command: string, args: any) {
    calls.push({ command, args });
    if (command === "get_runtime_preferences") return Promise.resolve({ disabledRuntimes: [] });
    if (command === "git_settings_get" && mode !== "settings") return Promise.resolve(settings);
    if (command === "git_status") return Promise.resolve(snapshot(args.cwd));
    if (command === "git_diff") return Promise.resolve(`diff ${args.cwd} ${args.path}`);
    if (command.startsWith("git_")) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    return Promise.resolve([]);
  },
  release(command: string, error?: string, cwd?: string) {
    const index = pending.findIndex(item => item.command === command && (!cwd || item.args?.cwd === cwd));
    if (index < 0) throw Error(`No pending ${command} ${cwd}`);
    const [item] = pending.splice(index, 1);
    if (error) { item.reject(Error(error)); return; }
    if (command === "git_branches") item.resolve([{ name: `${item.args.cwd}-branch`, remote: false }]);
    else if (command === "git_generate_commit_message") item.resolve("generated message");
    else if (command.startsWith("git_settings_")) item.resolve(item.args?.settings ?? settings);
    else if (["git_stage", "git_unstage", "git_discard"].includes(command)) item.resolve(snapshot(item.args.cwd, "old-receipt"));
    else item.resolve({ status: snapshot(item.args.cwd, "committed"), message: "fixture completed", warning: false });
  },
};
(window as any).gitMock = fixture;
const projects = [{ id: "a", name: "Repo A", path: "/Repo" }, { id: "b", name: "Repo B", path: "/repo" }] as Project[];
createRoot(document.getElementById("root")!).render(mode === "settings" ? <GitSettingsPanel /> : <GitSourceControlPanel variant="sidebar" sessions={[]} projects={projects} providers={[{ id: "provider", name: "fixture" } as ProviderConfig]} keyStates={{ provider: true }} />);
