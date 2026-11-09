import type { TerminalShell } from "./types";

export function isMacPlatform(platform = typeof navigator === "undefined" ? "" : navigator.platform): boolean {
  return /mac/i.test(platform);
}
export const IS_MAC = isMacPlatform();
export function terminalShells(mac = IS_MAC): readonly TerminalShell[] {
  return mac ? ["zsh", "bash"] : ["powershell", "cmd"];
}
export function isTerminalShell(value: string): value is TerminalShell {
  return ["powershell", "cmd", "zsh", "bash"].includes(value);
}
export function shellLabel(shell: TerminalShell | null | undefined): string {
  return ({ powershell: "PowerShell", cmd: "CMD", zsh: "zsh", bash: "bash" })[shell ?? terminalShells()[0]];
}
