// Explicit, isolated Pi extension: --no-tools --no-extensions --no-mcp plus this file.
// All authority is checked by the host on every call. This extension never opens a
// project file, executes a command, or fetches an arbitrary model-provided URL.
const ALLOWED = new Set(['cc_research_read', 'cc_research_list', 'cc_research_search',
  'cc_research_web_search', 'cc_research_web_fetch']);

export default async function crewvelaConversationResearch(pi, registerFileTools) {
  const endpoint = process.env.CREWVELA_RESEARCH_URL;
  const token = process.env.CREWVELA_RESEARCH_TOKEN;
  const url = new URL(endpoint);
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.pathname !== '/mcp' || !token) {
    throw new Error('CrewVela research gateway missing; refusing an unrestricted fallback');
  }
  let nextId = 0;
  async function rpc(method, params = {}) {
    const response = await fetch(url, { method: 'POST', redirect: 'error',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify({ jsonrpc: '2.0', id: ++nextId, method, params }),
      signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`Research gateway HTTP ${response.status}`);
    const result = await response.json();
    if (result.error) throw new Error(result.error.message ?? 'Research gateway failed');
    return result.result;
  }
  const listed = await rpc('tools/list');
  const fileOperations = new Set(registerFileTools ? ['cc_pi_edit_read', 'cc_pi_write_file'] : []);
  if (!Array.isArray(listed?.tools) || listed.tools.some(tool => !ALLOWED.has(tool.name) && !fileOperations.has(tool.name))) {
    throw new Error('Unexpected research tool contract; refusing tool registration');
  }
  const research = listed.tools.filter(tool => ALLOWED.has(tool.name));
  const names = new Set(research.map(tool => tool.name));
  if (registerFileTools) {
    if (![...fileOperations].every(name => listed.tools.some(tool => tool.name === name))) throw new Error('Pi file operations unavailable');
    for (const name of registerFileTools(rpc)) names.add(name);
  }
  for (const tool of research) {
    pi.registerTool({ name: tool.name, label: tool.name, description: tool.description,
      parameters: tool.inputSchema,
      async execute(_callId, args) {
        const result = await rpc('tools/call', { name: tool.name, arguments: args });
        if (result.isError) throw new Error(result.content?.map(item => item.text ?? '').join('\n') || 'Research tool refused');
        return { content: result.content, details: result.structuredContent ?? {} };
      } });
  }
  pi.on('tool_call', async event => names.has(event.toolName) ? undefined
    : { block: true, reason: '本次群聊只允许宿主注册的只读研究工具' });
  pi.on('session_start', async () => pi.setActiveTools([...names]));
}
