import test from "node:test";
import assert from "node:assert/strict";
import { resolveSessionDirectory } from "../src/sessionDirectory.ts";
import type { Project } from "../src/types.ts";

const project = (id: string, path: string): Project => ({ id, path, name: id, badgeColor: "", createdAt: 0, updatedAt: 0 });
test("Windows 目录大小写、分隔符和末尾斜线匹配已有项目，并使用项目规范路径", () => {
  const saved = project("saved", "F:\\项目\\CrewVela");
  assert.equal(resolveSessionDirectory("f:/项目/crewvela/", [saved]), saved);
  assert.equal(resolveSessionDirectory("f:\\项目\\crewvela\\", [saved]), saved);
});
test("临时目录不继承旧项目，子目录也不误关联父项目", () => {
  const saved = project("saved", "F:\\project");
  assert.deepEqual(resolveSessionDirectory("D:\\临时目录", [saved]), { name: "临时目录", path: "D:\\临时目录" });
  assert.equal(resolveSessionDirectory("F:\\project\\child", [saved]).id, undefined);
});
test("从项目内选择子目录时保留项目身份和实际目录，归档后不再作为启动目标", () => {
  const parent = project("saved", "F:\\project");
  const nested = project("nested", "F:\\project\\nested");
  assert.deepEqual(resolveSessionDirectory("F:\\project\\app", [parent, nested], parent.id),
    { id: parent.id, name: parent.name, path: "F:\\project\\app" });
  assert.equal(resolveSessionDirectory(nested.path, [parent, nested], parent.id), nested);
  assert.equal(resolveSessionDirectory(parent.path, [{ ...parent, archivedAt: 1 }], parent.id).id, undefined);
  assert.equal(resolveSessionDirectory("F:\\project-other", [parent], parent.id).id, undefined);
});
test("UNC 共享路径忽略大小写；POSIX 路径保留大小写区别", () => {
  const unc = project("share", "\\\\server\\share\\repo");
  assert.equal(resolveSessionDirectory("\\\\SERVER\\SHARE\\REPO\\", [unc]), unc);
  const posix = project("posix", "/home/Repo");
  assert.equal(resolveSessionDirectory("/home/Repo/", [posix]), posix);
  assert.equal(resolveSessionDirectory("/home/repo", [posix]).id, undefined);
});
test("已删除项目的目录退回未分组，盘符和根目录可用", () => {
  assert.deepEqual(resolveSessionDirectory("C:\\", []), { path: "C:\\", name: "C:" });
  assert.deepEqual(resolveSessionDirectory("/", []), { path: "/", name: "/" });
});
