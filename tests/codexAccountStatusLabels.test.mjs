import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/components/CodexAccountsPanel.tsx", import.meta.url),
  "utf8",
);

test("额度未获取的账户显示额度未知而不是可用", () => {
  const unknownQuota = source.indexOf('remaining === null) return { label: "额度未知", className: "warning" }');
  const available = source.indexOf('return { label: "可用", className: "" };');
  assert.ok(unknownQuota > 0, "accountState 应包含额度未知分支");
  assert.ok(available > unknownQuota, "额度未知分支应先于可用兜底返回");
});

test("当前账户仍优先显示当前账户标签", () => {
  const current = source.indexOf('if (account.current) return { label: "当前账户", className: "current" };');
  const unknownQuota = source.indexOf('remaining === null) return { label: "额度未知"');
  assert.ok(current > 0 && unknownQuota > current);
});

test("Codex 账户表分别展示五小时和每周额度", () => {
  assert.match(source, />5 小时额度</);
  assert.match(source, />每周额度</);
  assert.match(source, /account\.nextUsedPercent/);
  assert.match(source, /account\.nextReset/);
  assert.match(source, /account\.totalUsedPercent/);
  assert.match(source, /account\.totalReset/);
});
