// Run after: node node_modules/typescript/bin/tsc src/lib/sharedMemoryCommands.ts
//   --target es2022 --module es2022 --outDir <temporary-directory> --skipLibCheck
// Then: node scripts/check-shared-memory-commands.mjs <temporary-directory>/sharedMemoryCommands.js
import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
const { powerShellLiteral, posixShellLiteral, sharedMemoryCommands } = await import(pathToFileURL(process.argv[2]));
assert.equal(powerShellLiteral("C:\\O'Brien\\$task` file.exe"), "'C:\\O''Brien\\$task` file.exe'");
assert.equal(sharedMemoryCommands(null), null);
assert.equal(sharedMemoryCommands({ executable: "x", dataDir: null, serverUrl: "x" }), null);
const status = { executable: "C:\\O'Brien\\ai-memory.exe", dataDir: "C:\\memory data", serverUrl: "http://127.0.0.1:47374" };
const commands = sharedMemoryCommands(status, { platform: "windows" });
for (const [group, count] of [[commands.hooks, 7], [commands.mcp, 6]]) {
  assert.equal(group.split("\n").length, count);
  assert.ok(group.split("\n").every(line => line.startsWith("& 'C:\\O''Brien\\ai-memory.exe' --data-dir 'C:\\memory data'")));
  assert.ok(group.split("\n").every(line => line.includes("--server-url 'http://127.0.0.1:47374'")));
}
assert.match(commands.hooks.split("\n")[0], /--scope project --apply$/);
assert.equal(commands.hooks.split("\n").slice(1).some(line => line.includes("--scope")), false);
assert.match(commands.mcp, /--client zcode/);
assert.equal(commands.finalize, null);
const selected = sharedMemoryCommands(status, { platform: "windows", projectDir: "C:\\project", sessionId: "12345678-1234-5678-9abc-123456789abc" });
assert.match(selected.finalize, /finalize-session --agent zcode --session-id/);
assert.match(selected.finalize, /AI_MEMORY_SERVER_URL/);
assert.equal(commands.shellLabel, "PowerShell");
assert.equal(posixShellLiteral("/Users/O'Brien/$task` file"), "'/Users/O'\"'\"'Brien/$task` file'");
const posix = sharedMemoryCommands({ executable: "/opt/ai-memory", dataDir: "/tmp/memory data", serverUrl: status.serverUrl }, { platform: "posix" });
assert.equal(posix.shellLabel, "zsh / bash");
assert.match(posix.hooks, /^'\/opt\/ai-memory' --data-dir '\/tmp\/memory data'/);
assert.equal(posix.finalize, null);
console.log("SHARED_MEMORY_COMMANDS_OK");
