// Public client configuration only. No credential/config-store reads or auth headers.
const { join, dirname } = require('node:path');
async function workflowPolicy(input, request = fetch) {
  try {
    const version = input.version || require(join(dirname(input.builtin), '..', '..', 'app.asar', 'package.json')).version;
    if (typeof version !== 'string' || !version.trim()) throw Error('version');
    const endpoint = new URL(process.env.ZCODE_BASE_URL?.trim() || process.env.ZCODE_ENDPOINT_ORIGIN?.trim() || 'https://zcode.z.ai');
    if (!['http:', 'https:'].includes(endpoint.protocol)) throw Error('endpoint');
    const url = new URL('/api/v1/client/configs', endpoint.origin);
    url.searchParams.set('app_version', version);
    url.searchParams.set('platform', `${process.platform}-${process.arch}`);
    const response = await request(url, { method: 'GET', redirect: 'error', signal: AbortSignal.timeout(15000) });
    if (!response.ok) throw Error('status');
    const reader = response.body.getReader(); let total = 0; const chunks = [];
    while (true) { const { done, value } = await reader.read(); if (done) break; total += value.length; if (total > 1024 * 1024) { await reader.cancel(); throw Error('size'); } chunks.push(Buffer.from(value)); }
    const payload = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (payload.success === false || (payload.code !== undefined && ![0, 200].includes(payload.code))) throw Error('envelope');
    const raw = payload.data?.configs?.dynamicWorkflow?.mode;
    const mode = typeof raw === 'string' ? raw.trim() : '';
    if (!['disabled', 'onDemand', 'alwaysOn'].includes(mode)) return { enabled: false, mode: 'disabled', notice: '原生工作流配置未开放，创建入口暂不可用' };
    return { enabled: mode !== 'disabled', mode, ...(mode === 'disabled' ? { notice: '原生工作流当前关闭，创建入口暂不可用' } : {}) };
  } catch { return { enabled: false, mode: 'disabled', notice: '无法读取原生工作流公开配置，创建入口暂不可用' }; }
}
module.exports = { workflowPolicy };
if (require.main === module || process.env.CREWVELA_ZCODE_WORKFLOW_HELPER === '1') {
  let bytes = ''; process.stdin.setEncoding('utf8'); process.stdin.on('data', value => bytes += value);
  process.stdin.on('end', async () => { try { process.stdout.write(JSON.stringify(await workflowPolicy(JSON.parse(bytes)))); } catch { process.stdout.write(JSON.stringify({ enabled: false, mode: 'disabled', notice: '原生工作流配置不可用' })); } });
}
