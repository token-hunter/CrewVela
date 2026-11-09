import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/codexAccounts.ts", import.meta.url), "utf8");
const module = { exports: {} };
new Function("require", "module", "exports", ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS },
}).outputText)(() => ({ invoke() { throw Error("No IPC expected"); } }), module, module.exports);
const remaining = (weekly, fiveHour) => module.exports.remainingPercent({ totalUsedPercent: weekly, nextUsedPercent: fiveHour });

test("either exhausted quota window makes an account unavailable", () => {
  assert.equal(remaining(90, 0), 0);
  assert.equal(remaining(0, 90), 0);
  assert.equal(remaining(80, 7), 7);
  assert.equal(remaining(8, 75), 8);
});

test("unknown windows retain the known quota without inventing a value", () => {
  assert.equal(remaining(null, 40), 40);
  assert.equal(remaining(60, null), 60);
  assert.equal(remaining(null, null), null);
  assert.equal(remaining(NaN, 70), null);
  assert.equal(remaining(120, 70), null);
});
