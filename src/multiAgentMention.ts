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

export interface MentionTextSegment { text: string; mention: boolean }
/** Use the same complete-token boundaries as routing. Longer names win overlaps. */
export function mentionTextSegments(value: string, tokens: readonly string[]): MentionTextSegment[] {
  const ranges: Array<{start:number;end:number}> = [];
  for (const token of new Set(tokens.filter(Boolean))) {
    for (const match of value.matchAll(mentionExpression(token,"giu"))) {
      const start = match.index! + match[1].length;
      ranges.push({start,end:start+match[2].length});
    }
  }
  ranges.sort((a,b)=>a.start-b.start || b.end-a.end);
  const segments: MentionTextSegment[] = [];
  let cursor = 0;
  for (const range of ranges) {
    if (range.start < cursor) continue;
    if (range.start > cursor) segments.push({text:value.slice(cursor,range.start),mention:false});
    segments.push({text:value.slice(range.start,range.end),mention:true}); cursor=range.end;
  }
  if (cursor < value.length) segments.push({text:value.slice(cursor),mention:false});
  return segments;
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

export function selectMentionTokens(value: string, tokens: readonly string[], selected: boolean): string {
  return [...new Set(tokens)].reduce((draft, token) =>
    hasMentionToken(draft, token) === selected ? draft : toggleMentionToken(draft, token).value,
  value);
}
