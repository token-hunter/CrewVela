import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const panel = readFileSync(new URL("../src/components/GitSettingsPanel.tsx", import.meta.url), "utf8");
const settings = readFileSync(new URL("../src/components/SettingsDialog.tsx", import.meta.url), "utf8");
const backend = readFileSync(new URL("../src-tauri/src/git_source_control.rs", import.meta.url), "utf8");
const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const activityBar = readFileSync(new URL("../src/components/RightSidebarActivityBar.tsx", import.meta.url), "utf8");

test("Git 工作台位于主界面右侧，设置页只保留 Git 配置", () => {
  assert.match(settings, /id:\s*"git"[\s\S]{0,100}label:\s*"Git"/);
  assert.match(settings, /<GitSettingsPanel/);
  assert.match(settings, /title:\s*"Git 设置"/);
  assert.match(app, /const MemoGitSourceControlPanel = memo\(GitSourceControlPanel\)/);
  assert.match(app, /<MemoGitSourceControlPanel[\s\S]{0,160}variant="sidebar"/);
  assert.match(activityBar, /\["git", "源代码控制"\]/);
  assert.match(app, /<RightSidebarActivityBar[\s\S]{0,220}onSelect=\{\(tab\) => void selectRightSidebarTab\(tab\)\}/);
  assert.match(panel, /export function GitSourceControlPanel/);
  assert.match(panel, /export function GitSettingsPanel/);
  assert.match(panel, /kind:\s*"project"/);
  assert.match(panel, /kind:\s*"session"/);
  assert.match(panel, /kind:\s*"agent"/);
});

test("Git P0 暴露完整日常闭环且危险操作需要确认", () => {
  for (const command of [
    "git_status",
    "git_diff",
    "git_stage",
    "git_unstage",
    "git_discard",
    "git_commit",
    "git_remote",
    "git_generate_commit_message",
  ]) {
    assert.match(panel, new RegExp(`"${command}"`));
  }
  assert.match(panel, /window\.confirm\(`永久丢弃/);
  assert.match(panel, /--force-with-lease/);
  assert.match(backend, /"pull",\s*"--ff-only"/);
  assert.match(backend, /"push",\s*"--force-with-lease"/);
});

test("Git Merge 支持分支选择、no-ff、冲突展示和取消合并", () => {
  for (const command of ["git_branches", "git_merge", "git_merge_abort"]) {
    assert.match(panel, new RegExp(`"${command}"`));
    assert.match(lib, new RegExp(`\\b${command}\\b`));
  }
  assert.match(panel, /setMergeDialogOpen\(true\)/);
  assert.match(panel, /mergeNoFf/);
  assert.match(panel, /status\?\.mergeInProgress/);
  assert.match(panel, /取消合并/);
  assert.match(backend, /工作区存在未提交变更/);
  assert.match(backend, /vec!\["merge", "--no-edit"\]/);
  assert.match(backend, /args\.push\("--no-ff"\)/);
  assert.match(backend, /\["merge", "--abort"\]/);
});


test("Git 比较基准和 Worktree 分支前缀是真实设置而非占位", () => {
  assert.match(panel, /repository_default/);
  assert.match(panel, /branch_upstream/);
  assert.match(panel, /git_settings_save/);
  assert.match(backend, /managed_branch_name/);
});

test("等价工作区对象重建不会反复触发 Git 刷新和按钮禁用态", () => {
  assert.match(panel, /const workspaceStatusKey =/);
  assert.match(panel, /const selectedWorkspaceCwd = selectedWorkspace\?\.cwd \?\? null/);
  assert.match(panel, /\[settings\.compareStrategy, workspaceStatusKey\]/);
  assert.match(
    panel,
    /\[preview, selectedWorkspaceCwd, selectedWorkspaceId, settings\.compareStrategy\]/,
  );
  assert.doesNotMatch(panel, /\[settings\.compareStrategy, workspaces\]/);
  assert.doesNotMatch(panel, /\[selectedWorkspace, settings\.compareStrategy\]/);
});


test("Git Pull 策略是真实设置，安全底线为只读声明", () => {
  assert.match(panel, /pullStrategy/);
  assert.match(panel, /仅快进合并（--ff-only）/);
  assert.match(panel, /变基合并（--rebase）/);
  assert.match(panel, /安全保证/);
  assert.doesNotMatch(panel, /git-settings-row readonly/);
  assert.match(backend, /pull_strategy/);
  assert.match(backend, /"pull",\s*"--rebase"/);
});
