import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const executor = readFileSync(
  new URL("../src-tauri/src/multi_agent/executor.rs", import.meta.url),
  "utf8",
);

test("OpenCode 的提示语经 stdin 传递，不会被 --file 数组吞为附件", () => {
  assert.doesNotMatch(
    executor,
    /\.arg\("请严格执行附件中的 CrewVela 任务说明。"\)/,
    "提示语不得以位置参数跟在 --file 之后（opencode 会把位置参数吞进附件数组，报 File not found）",
  );
  assert.match(
    executor,
    /Some\("请严格执行附件中的 CrewVela 任务说明。"\.to_string\(\)\)/,
  );
});
