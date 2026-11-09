import { useState } from "react";
import { createRoot } from "react-dom/client";
import { FileExplorerPanel } from "../../src/components/FileExplorerPanel";
import "../../src/styles.css";

const entry = (name: string) => ({ name, relativePath: name, directory: false, size: 12, modifiedAt: 0 });
const files = new Map<string, string>([["A/a.txt", "A original"], ["A/b.txt", "B original"], ["B/a.txt", "Other workspace"]]);
const held = new Set<string>();
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (reason: string) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
const result = (command: string, args: any) => {
  if (command === "workspace_list_directory") return args.relativePath ? [entry("nested.txt")] : args.root === "A" ? [entry("a.txt"), entry("b.txt"), { ...entry("folder"), directory: true }] : [entry("a.txt"), entry("other.txt")];
  if (command === "workspace_search_files") return [entry(`${args.query}.txt`)];
  if (command === "workspace_read_text") return files.get(`${args.root}/${args.relativePath}`) ?? args.relativePath;
  if (command === "workspace_write_text") {
    const key = `${args.root}/${args.relativePath}`;
    if ((files.get(key) ?? args.relativePath) !== args.expectedContents) {
      throw new Error("文件已被其他程序修改，未覆盖磁盘内容；请保留草稿并重新打开文件后合并修改。");
    }
    files.set(key, args.contents);
    return args.contents.length;
  }
  return null;
};
(window as any).__TAURI_INTERNALS__ = {
  invoke: (command: string, args: any) => {
    calls.push({ command, args });
    if (held.has(`${args.root}:${command}`)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    return Promise.resolve(result(command, args));
  },
  convertFileSrc: (path: string) => path,
};
(window as any).fileExplorerMock = {
  held, calls, files, pending,
  release(command: string, workspace: string, failure?: string) {
    const index = pending.findIndex(item => item.command === command && item.args.root === workspace);
    if (index < 0) throw new Error("No pending mock request");
    const [item] = pending.splice(index, 1);
    if (failure) item.reject(failure);
    else {
      try { item.resolve(result(command, item.args)); }
      catch (error) { item.reject(String(error)); }
    }
  },
};
function Fixture() {
  const [root, setRoot] = useState("A");
  return <><nav className="fixture-controls"><button onClick={() => setRoot("A")}>工作区 A</button><button onClick={() => setRoot("B")}>工作区 B</button><strong>当前 {root}</strong></nav><FileExplorerPanel root={root} active /></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
