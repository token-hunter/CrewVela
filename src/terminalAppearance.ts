/** 终端外观（通用设置里的字体/字号）：进程内缓存一次读取，
 * 后端保存时发 general-settings-changed 事件，所有终端即时热更新。 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import {
  DEFAULT_GENERAL_SETTINGS,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  type GeneralSettings,
} from "./generalSettings";
import { isGenericTerminalFont, quoteTerminalFontName, splitTerminalFontFamilies, terminalFontName } from "./terminalFonts";

export const FALLBACK_FONT_FAMILY = '"Cascadia Code", Consolas, monospace';

/** 用户输入 → xterm fontFamily：
 * - 空 → 默认栈；
 * - 单个字体名（含空格未加引号）→ 自动加引号；
 * - 字体栈 → 保留字体顺序，统一处理引号；
 * - 结尾没有通用族时补 ", monospace" 兜底。 */
export function resolveFontFamily(raw: string): string {
  const parts = splitTerminalFontFamilies(raw);
  if (!parts.length) return FALLBACK_FONT_FAMILY;
  const families = parts.map(part => isGenericTerminalFont(part) ? part : quoteTerminalFontName(terminalFontName(part)));
  if (!parts.some(isGenericTerminalFont)) families.push("monospace");
  return families.join(", ");
}

export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return 12;
  const rounded = Math.round(size);
  return Math.min(MAX_TERMINAL_FONT_SIZE, Math.max(MIN_TERMINAL_FONT_SIZE, rounded));
}

export interface TerminalAppearance {
  fontFamily: string;
  fontSize: number;
}

let cached: Promise<GeneralSettings> | null = null;

function loadGeneralSettingsOnce(): Promise<GeneralSettings> {
  cached ??= invoke<{ settings: GeneralSettings }>("general_settings_get")
    .then((status) => status.settings)
    .catch(() => DEFAULT_GENERAL_SETTINGS);
  return cached;
}

export function invalidateGeneralSettingsCache(next: GeneralSettings) {
  cached = Promise.resolve(next);
}

export async function getTerminalAppearance(): Promise<TerminalAppearance> {
  const settings = await loadGeneralSettingsOnce();
  return {
    fontFamily: resolveFontFamily(settings.terminalFontFamily),
    fontSize: clampFontSize(settings.terminalFontSize),
  };
}

/** 订阅保存事件；返回退订函数。处理器直接收到解析好的外观。 */
export function subscribeTerminalAppearance(
  handler: (appearance: TerminalAppearance) => void,
): () => void {
  const unlisten = listen<GeneralSettings>("general-settings-changed", (event) => {
    const settings = event.payload;
    invalidateGeneralSettingsCache(settings);
    handler({
      fontFamily: resolveFontFamily(settings.terminalFontFamily),
      fontSize: clampFontSize(settings.terminalFontSize),
    });
  }).catch(error => {
    console.error("订阅终端外观失败:", error);
    return () => {};
  });
  return () => {
    void unlisten.then((dispose) => dispose());
  };
}
