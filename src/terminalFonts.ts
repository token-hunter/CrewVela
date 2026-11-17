export interface InstalledTerminalFont {
  family: string;
  aliases: string[];
  monospace: boolean;
}

const GENERIC_FAMILIES = /^(monospace|serif|sans-serif|cursive|fantasy|system-ui|ui-monospace|ui-serif|ui-sans-serif|ui-rounded|emoji|math|fangsong)$/i;

export function isGenericTerminalFont(value: string): boolean {
  return GENERIC_FAMILIES.test(value);
}

export function splitTerminalFontFamilies(value: string): string[] {
  const families = [];
  let start = 0;
  let quote = "";
  for (let index = 0; index < value.length; index++) {
    const char = value[index];
    if (quote) {
      if (char === "\\") index++;
      else if (char === quote) quote = "";
    } else if (char === ",") {
      families.push(value.slice(start, index).trim());
      start = index + 1;
    } else if ((char === '"' || char === "'") && !value.slice(start, index).trim()) {
      quote = char;
    }
  }
  families.push(value.slice(start).trim());
  return families.filter(Boolean);
}

export function terminalFontName(part: string): string {
  const quoted = /^(["'])(.*)\1$/s.exec(part);
  return quoted ? quoted[2].replace(/\\(["'\\])/g, "$1") : part;
}

export function quoteTerminalFontName(value: string): string {
  return '"' + value.replace(/[\\"]/g, "\\$&") + '"';
}

export function replacePrimaryTerminalFont(stack: string, family: string): string {
  if (!family) return "";
  const parts = splitTerminalFontFamilies(stack);
  const primary = /[,\\"]/.test(family) || family.startsWith("'") || isGenericTerminalFont(family)
    ? quoteTerminalFontName(family)
    : family;
  return [primary, ...parts.slice(1)].join(", ");
}

export function missingTerminalFonts(stack: string, fonts: readonly InstalledTerminalFont[]): string[] {
  const names = new Set(fonts.flatMap(font => [font.family, ...font.aliases]).map(name => name.toLowerCase()));
  const families = splitTerminalFontFamilies(stack).filter(part => !isGenericTerminalFont(part)).map(terminalFontName);
  return [...new Set(families.filter(family => !names.has(family.toLowerCase())))];
}
