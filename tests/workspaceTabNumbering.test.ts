import test from "node:test";
import assert from "node:assert/strict";
import { createWorkspaceTab } from "../src/workspaceTabs.ts";

const tabs = (...titles: string[]) => titles.map((title, index) => ({ id: `existing-${index}`, title }));

test("新工作区优先补齐缺失的最小编号", () => {
  assert.equal(createWorkspaceTab(tabs("工作区 6", "工作区 2", "工作区 3", "工作区 4", "工作区 5")).title, "工作区 1");
  assert.equal(createWorkspaceTab(tabs("工作区 1", "工作区 3", "工作区 5")).title, "工作区 2");
  assert.equal(createWorkspaceTab([]).title, "工作区 1");
});

test("编号连续时使用下一个编号，自定义名称和现有页签不变", () => {
  const existing = tabs("工作区 1", "自定义项目", "工作区 2");
  const baseline = structuredClone(existing);
  const created = createWorkspaceTab(existing);
  assert.equal(created.title, "工作区 3");
  assert.ok(!existing.some(tab => tab.id === created.id));
  assert.deepEqual(existing, baseline);
});

test("重复新建不会重名，关闭页签后优先复用缺失编号", () => {
  const existing = tabs("工作区 2", "工作区 4", "自定义");
  const first = createWorkspaceTab(existing);
  assert.equal(first.title, "工作区 1");
  const second = createWorkspaceTab([...existing, first]);
  assert.equal(second.title, "工作区 3");
  assert.notEqual(first.id, second.id);
  assert.equal(createWorkspaceTab([...existing, first, second].filter(tab => tab.title !== "工作区 2")).title, "工作区 2");
});
