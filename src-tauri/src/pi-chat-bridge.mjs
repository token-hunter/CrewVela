// crewvela pi_chat 审批桥扩展：随 `pi --mode rpc -e` 注入。
// pi 没有内建的工具审批机制；本扩展拦截写类工具调用，经 ctx.ui.confirm
// 变成 RPC 的 extension_ui_request(confirm) 通知，由 CrewVela 渲染审批卡片，
// 用户应答后 CrewVela 回写 extension_ui_response(confirmed)。
// confirm 的 title 携带机器可读 JSON 信封（toolCallId/toolName/input），
// message 携带人类可读摘要；CrewVela 侧优先解析信封，失败时回退原文展示。
const GATED_TOOLS = new Set(['bash', 'powershell', 'write', 'edit']);

export default function crewvelaPiChatBridge(pi) {
  pi.on('tool_call', async (event, ctx) => {
    if (!GATED_TOOLS.has(event.toolName)) return undefined;
    const input = event.input ?? {};
    const envelope = {
      kind: 'crewvela-permission',
      toolCallId: event.toolCallId,
      toolName: event.toolName,
      input,
    };
    let preview = '';
    try {
      preview = JSON.stringify(input);
      if (preview.length > 600) preview = `${preview.slice(0, 600)}…`;
    } catch {
      preview = String(input);
    }
    let allowed = false;
    try {
      allowed = await ctx.ui.confirm(JSON.stringify(envelope), `${event.toolName} 调用：${preview}`);
    } catch {
      allowed = false;
    }
    if (!allowed) {
      return { block: true, reason: '用户在 CrewVela 审批卡片中拒绝了该工具调用' };
    }
    return undefined;
  });
}
