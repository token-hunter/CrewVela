import { existsSync, readFileSync, realpathSync, lstatSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, relative, isAbsolute, resolve, join } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolveConfigDirectory } from './legacy-brand.mjs';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const appData = process.env.APPDATA || (process.platform === 'darwin'
  ? join(homedir(), 'Library', 'Application Support')
  : process.env.XDG_CONFIG_HOME || join(homedir(), '.config'));
const configDir = resolveConfigDirectory(appData);
const defaultPolicy = join(configDir, 'ai-memory', 'project-roots.json');

export function allowedRoots(policyFile = defaultPolicy, defaultRoot = root) {
  if (!existsSync(policyFile)) return [defaultRoot];
  try {
    const policy = JSON.parse(readFileSync(policyFile, 'utf8').replace(/^\uFEFF/, ''));
    if (policy.version !== 1 || !Array.isArray(policy.roots) ||
        !policy.roots.every(value => typeof value === 'string' && isAbsolute(value))) return [];
    return [...new Set([defaultRoot, ...policy.roots])];
  } catch { return []; }
}

export function authorizedProjectRoot(cwd, roots) {
  try {
    if (!isAbsolute(cwd)) return null;
    const candidates = roots.filter(projectRoot => {
      try { return withinProject(cwd, projectRoot); } catch { return false; }
    });
    for (let directory = realpathSync(cwd);;) {
      const marker = join(directory, '.ai-memory.toml');
      if (existsSync(marker)) {
        if (!lstatSync(marker).isFile() || lstatSync(marker).isSymbolicLink()) return null;
        return candidates.some(projectRoot => realpathSync(projectRoot) === directory) ? directory : null;
      }
      const parent = dirname(directory);
      if (parent === directory || !candidates.some(projectRoot => withinProject(parent, projectRoot))) return null;
      directory = parent;
    }
  } catch { return null; }
}

// Static ACP clients do not forward the lifecycle session ID. Use an explicit
// project coordinate instead of the HTTP server's last active project.
export function projectArguments(properties, args = {}, cwd = root, scope = { workspace: 'default', project: 'crewvela' }) {
  const result = { ...args };
  if (!properties?.workspace || !properties?.project || result.global === true ||
      result.scopes != null || result.scope === 'global' || result.scope === 'profile') return result;
  // Leave partial coordinates untouched so the server can reject them.
  if (result.workspace == null && result.project == null) {
    result.workspace = scope.workspace;
    result.project = scope.project;
    if (properties.cwd && result.cwd == null) result.cwd = cwd;
  }
  return result;
}

export function withinProject(cwd, projectRoot) {
  const suffix = relative(realpathSync(projectRoot), realpathSync(cwd));
  return suffix === '' || (!isAbsolute(suffix) && suffix !== '..' && !suffix.startsWith('..\\') && !suffix.startsWith('../'));
}

// The settings UI grants roots explicitly. Marker parsing remains owned by the
// installed CLI so routing follows the same contract as capture hooks.
export function resolveProjectRoute({ cwd, projectRoot, executable, dataDir, serverUrl, inspect }) {
  try {
    if (!isAbsolute(projectRoot) || !authorizedProjectRoot(cwd, [projectRoot])) return null;
    const run = inspect ?? ((args, input) => spawnSync(executable, args, {
      input, cwd, encoding: 'utf8', windowsHide: true, timeout: 5000, maxBuffer: 1024 * 1024,
    }));
    const result = run(['--data-dir', dataDir, 'hook', '--agent', 'codex', '--event', 'session-start',
      '--server-url', serverUrl.replace(/\/mcp\/?(?:\?.*)?$/, ''), '--check-capture'],
    JSON.stringify({ cwd, session_id: 'crewvela-read-only-scope' }));
    if (result.error || result.status !== 0) return null;
    const report = JSON.parse(result.stdout);
    if (report.marker_present !== true || report.scope_resolution !== 'explicit' || report.server_resolution !== 'install-default' ||
        report.policy_state === 'invalid' || report.admits_capture !== true ||
        typeof report.scope?.workspace !== 'string' || !report.scope.workspace.trim() ||
        typeof report.scope?.project !== 'string' || !report.scope.project.trim()) return null;
    return { workspace: report.scope.workspace, project: report.scope.project };
  } catch { return null; }
}

function options(argv) {
  const binaryName = process.platform === 'win32' ? 'ai-memory.exe' : 'ai-memory';
  const managedBinary = join(configDir, 'ai-memory', binaryName);
  const result = { url: 'http://127.0.0.1:49374/mcp?flavor=moonshot', policyFile: defaultPolicy,
    executable: process.env.CREWVELA_AI_MEMORY_EXECUTABLE || (existsSync(managedBinary) ? managedBinary : binaryName),
    dataDir: join(configDir, 'ai-memory-data') };
  if (argv[0] && !argv[0].startsWith('--')) result.url = argv.shift();
  const keys = { '--policy-file': 'policyFile', '--memory-executable': 'executable', '--data-dir': 'dataDir' };
  while (argv.length) {
    const key = keys[argv.shift()];
    const value = argv.shift();
    if (!key || !value || value.startsWith('--')) throw Error('Invalid project bridge arguments');
    result[key] = value;
  }
  return result;
}

async function main() {
  const config = options(process.argv.slice(2));
  const route = () => {
    const projectRoot = authorizedProjectRoot(process.cwd(), allowedRoots(config.policyFile));
    if (!projectRoot) return null;
    return resolveProjectRoute({ ...config, projectRoot, serverUrl: config.url, cwd: process.cwd() });
  };
  const scope = route();
  if (!scope) {
    // These user-level registrations must stay inert in other checkouts.
    const disabled = new Server({ name: 'ai-memory', version: '1.0.0' }, { capabilities: { tools: {} } });
    disabled.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: [] }));
    disabled.setRequestHandler(CallToolRequestSchema, async () => ({
      isError: true, content: [{ type: 'text', text: 'Shared memory is not enabled for this project. Select its root and configure an explicit .ai-memory.toml marker.' }],
    }));
    await disabled.connect(new StdioServerTransport());
    return;
  }
  const url = new URL(config.url);
  const upstream = new Client({ name: 'crewvela-ai-memory-bridge', version: '1.0.0' });
  await upstream.connect(new StreamableHTTPClientTransport(url));
  const info = upstream.getServerVersion();
  const instructions = upstream.getInstructions() || '';
  const schemas = new Map();
  const server = new Server(info || { name: 'ai-memory', version: 'unknown' }, {
    capabilities: { tools: {} },
    instructions: `${instructions}\nCrewVela defaults unscoped project tool calls to workspace=${scope.workspace}, project=${scope.project}. Explicit scope and global retrieval retain the upstream contract.`,
  });
  server.setRequestHandler(ListToolsRequestSchema, async (request, extra) => {
    if (!route()) return { tools: [] };
    const result = await upstream.listTools(request.params, { signal: extra.signal });
    for (const tool of result.tools) schemas.set(tool.name, tool.inputSchema.properties);
    return result;
  });
  server.setRequestHandler(CallToolRequestSchema, async (request, extra) => {
    const currentScope = route();
    const cwd = request.params.arguments?.cwd;
    const selectedRoot = authorizedProjectRoot(process.cwd(), allowedRoots(config.policyFile));
    if (!currentScope || !selectedRoot || (cwd != null &&
        (typeof cwd !== 'string' || !isAbsolute(cwd) || authorizedProjectRoot(cwd, [selectedRoot]) !== selectedRoot))) {
      return { isError: true, content: [{ type: 'text', text: 'The requested project directory is outside the configured memory boundary.' }] };
    }
    // Some clients call a known tool without first listing it.
    if (!schemas.has(request.params.name)) {
      let cursor;
      do {
        const page = await upstream.listTools(cursor ? { cursor } : undefined, { signal: extra.signal });
        for (const tool of page.tools) schemas.set(tool.name, tool.inputSchema.properties);
        cursor = page.nextCursor;
      } while (cursor);
    }
    return upstream.callTool({
      ...request.params,
      arguments: projectArguments(schemas.get(request.params.name), request.params.arguments, process.cwd(), currentScope),
    }, undefined, { signal: extra.signal });
  });
  server.onclose = () => { void upstream.close(); };
  const shutdown = async () => { await server.close(); await upstream.close(); };
  process.once('SIGTERM', () => { void shutdown(); });
  process.once('SIGINT', () => { void shutdown(); });
  await server.connect(new StdioServerTransport());
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch((error) => {
    console.error(`CrewVela ai-memory bridge: ${error.message}`);
    process.exitCode = 1;
  });
}
