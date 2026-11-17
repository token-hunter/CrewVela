import test from "node:test";
import assert from "node:assert/strict";
import { createQuickCapabilityCache } from "../src/quickCommandCapabilities.ts";
import type { RuntimeProbe } from "../src/types.ts";

test("能力检测去重并行读取，60 秒内复用，过期后刷新", async () => {
  let now = 100;
  let finish!: () => void;
  const gate = new Promise<void>(resolve => { finish = resolve; });
  const calls: string[] = [];
  const cache = createQuickCapabilityCache(async <T>(method: string) => {
    calls.push(method);
    if (method === "ma_probe_runtimes") { await gate; return [{ kind: "codex", installed: true }] as T; }
    return true as T;
  }, () => now);
  const first = cache.load(["b", "a"]);
  const second = cache.load(["a", "b", "a"]);
  assert.equal(first, second);
  assert.deepEqual(calls, ["ma_probe_runtimes", "get_key_state", "get_key_state"], "凭据状态读取不能排在慢速 CLI 检测后");
  finish();
  const data = await first;
  assert.deepEqual(data.keys, { a: true, b: true });
  assert.equal(cache.get(["b", "a"]), data);
  await cache.load(["a", "b"]);
  assert.equal(calls.length, 3);
  now += 60_000;
  assert.equal(cache.fresh(["a", "b"]), false);
  await cache.load(["a", "b"]);
  assert.equal(calls.length, 6);
});

test("强制检测失败保留展示快照但使其失效，后续读取必须重试", async () => {
  let fail = false;
  let reads = 0;
  const cache = createQuickCapabilityCache(async <T>(method: string) => {
    if (method === "ma_probe_runtimes") {
      reads++;
      if (fail) throw Error("CLI detection failed");
      return [] as RuntimeProbe[] as T;
    }
    return false as T;
  });
  const data = await cache.load([]);
  fail = true;
  await assert.rejects(cache.load([], true), /CLI detection failed/);
  assert.equal(cache.get([]), data);
  assert.equal(cache.fresh([]), false);
  fail = false;
  await cache.load([]);
  assert.equal(reads, 3);
  assert.equal(cache.fresh([]), true);
});

test("不同 Provider 集合不串用凭据状态，慢旧请求不能覆盖新集合", async () => {
  let finish!: () => void;
  const gate = new Promise<void>(resolve => { finish = resolve; });
  const cache = createQuickCapabilityCache(async <T>(method: string, args?: Record<string, unknown>) => {
    if (method === "ma_probe_runtimes") return [] as T;
    if (args?.id === "old") await gate;
    return (args?.id === "new") as T;
  });
  const old = cache.load(["old"]);
  const next = await cache.load(["new"]);
  assert.deepEqual(next.keys, { new: true });
  assert.equal(cache.get(["old"]), null);
  finish();
  await old;
  assert.deepEqual(cache.get(["new"])?.keys, { new: true });
  assert.deepEqual(cache.get(["old"])?.keys, { old: false });
});
