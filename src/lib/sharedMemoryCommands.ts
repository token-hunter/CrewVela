export interface SharedMemoryCommandStatus {
  executable: string | null;
  dataDir: string | null;
  serverUrl: string;
}

export const sharedMemoryAgents = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "antigravity-cli", label: "Antigravity" },
  { id: "zcode", label: "ZCode" },
  { id: "kimi-code", label: "Kimi" },
  { id: "dsh", label: "DeepSeek Harness" },
  { id: "opencode", label: "OpenCode" },
  { id: "pi", label: "Pi" },
] as const;

type AutomaticSetupAgent = Exclude<typeof sharedMemoryAgents[number]["id"], "dsh">;

/** PowerShell literal strings keep $, backticks and spaces inert. */
export function powerShellLiteral(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

export function posixShellLiteral(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

export interface SharedMemoryCommandOptions {
  platform?: "windows" | "posix";
  projectDir?: string | null;
  /** Exact ai-memory UUID returned by the project-scoped session list. */
  sessionId?: string | null;
}

function defaultPlatform(): "windows" | "posix" {
  return typeof navigator === "undefined" || /win/i.test(navigator.platform) ? "windows" : "posix";
}

function exactSessionCommand(status: SharedMemoryCommandStatus, options: SharedMemoryCommandOptions, windows: boolean): string | null {
  const id = options.sessionId?.trim();
  const directory = options.projectDir;
  if (!id || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(id) || !directory || /[\0\r\n]/.test(directory)) return null;
  const absolute = windows ? /^(?:[a-z]:[\\/]|\\\\)/i.test(directory) : directory.startsWith("/");
  if (!absolute) return null;
  const literal = windows ? powerShellLiteral : posixShellLiteral;
  const command = `${windows ? "& " : ""}${literal(status.executable!)} --data-dir ${literal(status.dataDir!)} finalize-session --agent zcode --session-id ${literal(id)}`;
  // A child shell/environment keeps the selected server local to this command.
  // finalize-session has no --server-url flag, unlike the installer commands.
  if (windows) {
    const script = `Set-Location -LiteralPath ${literal(directory)} -ErrorAction Stop; $env:AI_MEMORY_SERVER_URL = ${literal(status.serverUrl)}; ${command}; exit $LASTEXITCODE`;
    return `& powershell.exe -NoProfile -NonInteractive -Command ${powerShellLiteral(script)}`;
  }
  return `(cd ${literal(directory)} && env AI_MEMORY_SERVER_URL=${literal(status.serverUrl)} ${command})`;
}

export function sharedMemoryCommands(status: SharedMemoryCommandStatus | null, options: SharedMemoryCommandOptions = {}) {
  if (!status?.executable || !status.dataDir) return null;
  const windows = (options.platform ?? defaultPlatform()) === "windows";
  const literal = windows ? powerShellLiteral : posixShellLiteral;
  const prefix = `${windows ? "& " : ""}${literal(status.executable)} --data-dir ${literal(status.dataDir)}`;
  const endpoint = `--server-url ${literal(status.serverUrl)}`;
  const forAgent = (agent: AutomaticSetupAgent) => ({
    hooks: `${prefix} install-hooks --agent ${agent} ${endpoint}${agent === "claude-code" ? " --scope project" : ""} --apply`,
    mcp: agent === "pi" ? null : `${prefix} install-mcp --client ${agent} ${endpoint} --apply`,
  });
  const agents = sharedMemoryAgents.flatMap(({ id }) => id === "dsh" ? [] : [forAgent(id)]);
  return {
    shellLabel: windows ? "PowerShell" : "zsh / bash",
    hooks: agents.map(agent => agent.hooks).join("\n"),
    mcp: agents.flatMap(agent => agent.mcp ?? []).join("\n"),
    forAgent,
    deepSeek: {
      hooks: `${prefix} install-hooks --agent dsh ${endpoint} --apply`,
      mcp: `${prefix} install-mcp --client dsh ${endpoint}`,
    },
    finalize: exactSessionCommand(status, options, windows),
  };
}
