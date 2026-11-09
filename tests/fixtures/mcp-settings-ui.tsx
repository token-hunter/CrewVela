import { createRoot } from "react-dom/client";
import { McpSettingsPanel } from "../../src/components/McpSettingsPanel";
import "../../src/styles.css";

let server = { id: "fixture", name: "fixture-server", transport: "stdio", command: "node", args: ["", " spaced "], env: {}, url: null, headers: {}, enabled: true, createdAt: 1, updatedAt: 1 };
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (error: Error) => void }> = [];
const calls: string[] = [];
(window as any).mcpFixture = { pending, calls, release(command: string, error?: string) {
  const index = pending.findIndex(item => item.command === command);
  if (index < 0) throw Error(`No pending ${command}`);
  const [item] = pending.splice(index, 1);
  if (error) return item.reject(new Error(error));
  if (command === "mcp_save_server") { server = { ...server, ...item.args.input }; item.resolve(server); }
  else if (command === "mcp_probe_servers") item.resolve([{ id: server.id, ok: true, tools: [{ name: "demo" }], probedAt: 1 }]);
  else item.resolve([server]);
} };
(window as any).__TAURI_INTERNALS__ = { invoke: (command: string, args: any) => {
  calls.push(command);
  return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
} };
createRoot(document.getElementById("root")!).render(<McpSettingsPanel />);
