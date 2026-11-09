import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/components/CodexAccountsPanel.tsx", import.meta.url),
  "utf8",
);

test("自动选择显式向 Tauri 传递空 selector", () => {
  assert.match(
    source,
    /run\("auto",\s*"cid_use_account",\s*\{\s*selector:\s*null\s*\},\s*true\)/,
  );
});

test("自动选择比较前后账户并在按钮附近显示反馈", () => {
  assert.match(source, /beforeCurrentAccount/);
  assert.match(source, /afterCurrentAccount/);
  assert.match(source, /autoSelectionFeedback/);
  assert.match(source, /cid-auto-selection-feedback/);
  assert.match(source, /receipt\.message \|\| "当前账户仍有剩余额度，无需切换/);
  assert.match(source, /仅当当前账户额度用尽或不可用时切换/);
});
