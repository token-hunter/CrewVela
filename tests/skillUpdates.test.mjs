import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const read = (file) => readFileSync(new URL(file, import.meta.url), "utf8");
const panel = read("../src/components/SkillUpdatesPanel.tsx");
const center = read("../src/components/SkillCenterPanel.tsx");
const commands = read("../src-tauri/src/lib.rs");
const backend = read("../src-tauri/src/skill_center/updates.rs");
test("版本页签提供检查、差异、信任确认、目标选择和回滚", () => {
  for (const label of [
    "检查全部更新",
    "候选版本",
    "我已审阅",
    "同步所选部署",
    "预览回滚差异",
  ])
    assert.ok(panel.includes(label));
  assert.ok(center.includes('activeTab === "updates"'));
  assert.ok(panel.includes("useState<string[]>([])"));
});
test("更新命令注册且慢操作不占用 UI 线程", () => {
  for (const command of [
    "skill_center_check_updates",
    "skill_center_apply_update",
    "skill_center_prepare_rollback",
    "skill_center_sync_update",
  ]) {
    assert.ok(commands.includes(`async fn ${command}`));
    assert.ok(commands.includes(`            ${command},`));
    assert.ok(panel.includes(command));
  }
  assert.ok(commands.includes("spawn_blocking"));
});
test("中央版本使用独立目录并保存历史，不原地改写共享链接", () => {
  assert.ok(backend.includes('join(format!("version-{}", check.id))'));
  assert.ok(backend.includes("INSERT INTO skill_versions"));
  assert.ok(backend.includes("expected_hash"));
  assert.ok(
    backend.includes("is_owned_copy") || backend.includes("remove_owned_copy"),
  );
  assert.ok(backend.includes("多个 Runtime 共用目录"));
});
