import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const sidebar = readFileSync(
  new URL("../src/components/AppSidebar.tsx", import.meta.url),
  "utf8",
);
const summary = readFileSync(
  new URL("../src/components/CodexQuotaSummary.tsx", import.meta.url),
  "utf8",
);
const accountsPanel = readFileSync(
  new URL("../src/components/CodexAccountsPanel.tsx", import.meta.url),
  "utf8",
);
const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");

test("首页侧栏底部同时展示当前 Codex 账户的 5 小时和每周额度", () => {
  assert.match(sidebar, /<CodexQuotaSummary onOpen=\{props\.onOpenCodexAccounts\}/);
  assert.match(summary, /loadCodexAccountSnapshot\(\)/);
  assert.match(summary, /snapshot\?\.accounts\.find\(\(account\) => account\.current\)/);
  assert.match(summary, /5 小时 \{fiveHourRemaining/);
  assert.match(summary, /每周 \{weeklyRemaining/);
  assert.match(summary, /currentAccount\?\.nextUsedPercent/);
  assert.match(summary, /currentAccount\?\.totalUsedPercent/);
});

test("账户面板刷新账户后同步更新首页额度，点击摘要直达 Codex 账户", () => {
  assert.match(accountsPanel, /publishCodexAccountSnapshot\(nextSnapshot\)/);
  assert.match(accountsPanel, /publishCodexAccountSnapshot\(receipt\.snapshot\)/);
  assert.match(app, /setCodexAccountsOpen\(true\)/);
  assert.match(app, /<CodexAccountsPanel \/>/);
  assert.match(app, /onOpenCodexAccounts=/);
});
