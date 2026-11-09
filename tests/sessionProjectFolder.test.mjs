import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/components/SessionList.tsx", import.meta.url),
  "utf8",
);

test("项目折叠状态使用关闭和打开文件夹图标", () => {
  assert.match(source, /FolderIcon/);
  assert.match(source, /FolderOpenIcon/);
  assert.match(
    source,
    /collapsed\s*\?\s*\([\s\S]*?<FolderIcon[\s\S]*?\)\s*:\s*\([\s\S]*?<FolderOpenIcon/,
  );
  assert.doesNotMatch(source, /project-badge-dot/);
  assert.doesNotMatch(source, /color=\{project\.badgeColor\}/);
});
