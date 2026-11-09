export type ConversationQuote = { text: string };
export function canEditUserMessage(row: { kind: string; entityId?: string; actions?: { canEdit?: boolean; editDisposition?: string } }): boolean {
  return row.kind === "userInput" && !!row.entityId && row.actions?.canEdit === true && row.actions.editDisposition === "rewind";
}
/** 与 ZCode shared/conversationSelection 的模型可见格式及额度保持一致。 */
export function addConversationQuote(quotes: ConversationQuote[], text: string): ConversationQuote[] {
  const selected = text.trim();
  if (!selected) return quotes;
  if (selected.length > 8000 || quotes.length >= 8 || quotes.reduce((sum, quote) => sum + quote.text.length, selected.length) > 16000) throw new Error("对话引用最多 8 条，单条 8000 字符，总计 16000 字符");
  return [...quotes, { text: selected }];
}
export function withConversationQuotes(text: string, quotes: ConversationQuote[]): string {
  if (!quotes.length) return text;
  const block = `# userselect:\n\`\`\`userselect\n${JSON.stringify(quotes)}\n\`\`\``;
  return text ? `${text}\n\n${block}` : block;
}
