export type SubagentRow = { rowId: number; kind: string; turnId?: string; toolCallId?: string; parentToolCallId?: string; childSessionId?: string; subagentType?: string; summaryText?: string; status?: string };
/** Only a unique exact turn/tool identity can replace a tool card. */
export function pairedSubagent(rows: SubagentRow[], tool: SubagentRow): SubagentRow | undefined {
  if (!tool.turnId || !tool.toolCallId) return undefined;
  const matches = rows.filter(row => row.kind === "subagent" && row.turnId === tool.turnId && row.parentToolCallId === tool.toolCallId);
  return matches.length === 1 ? matches[0] : undefined;
}
