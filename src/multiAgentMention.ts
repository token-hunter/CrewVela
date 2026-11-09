function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function mentionExpression(token: string, flags: string): RegExp {
  return new RegExp(
    `(^|[^\\p{L}\\p{N}_.-])(${escapeRegExp(token)})(?=$|[^\\p{L}\\p{N}_-])`,
    flags,
  );
}

export function hasMentionToken(value: string, token: string): boolean {
  return mentionExpression(token, "iu").test(value);
}

export function replaceMentionToken(
  value: string,
  token: string,
  replacement: string,
): string {
  return value.replace(
    mentionExpression(token, "giu"),
    (_match, prefix: string) => `${prefix}${replacement}`,
  );
}

export function toggleMentionToken(
  value: string,
  token: string,
): { value: string; selected: boolean } {
  if (!hasMentionToken(value, token)) {
    const separator = value.length > 0 && !/\s$/.test(value) ? " " : "";
    return { value: `${value}${separator}${token} `, selected: true };
  }

  const nextValue = value
    .replace(mentionExpression(token, "giu"), (_match, prefix: string) => prefix)
    .replace(/[ \t]{2,}/g, " ")
    .replace(/^[ \t]+|[ \t]+$/g, "");
  return { value: nextValue, selected: false };
}
