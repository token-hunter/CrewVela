import catalog from "./runtimeLaunchModes.json" with { type: "json" };
import type { AgentRuntimeKind, WorktreeOptions } from "./types";

export type CliRuntime = keyof typeof catalog;
export interface RuntimeLaunchSelection {
  mode: string;
  saveAsDefault: boolean;
  worktreeOptions?: WorktreeOptions;
  models?: { model: string | null; reasoning: string | null };
}
export function isCliRuntime(kind: string): kind is CliRuntime { return Object.prototype.hasOwnProperty.call(catalog, kind); }
export function launchChoices(kind: string) { return isCliRuntime(kind) ? catalog[kind] : []; }
export function launchModeLabel(kind: string | null | undefined, mode: string | null | undefined): string | null {
  if (!kind || !mode) return null;
  return launchChoices(kind).find(choice => choice.id === mode)?.label ?? `未知模式：${mode}`;
}
export const CLI_RUNTIMES = Object.keys(catalog) as CliRuntime[];
export const cliRuntime = (kind: AgentRuntimeKind) => isCliRuntime(kind);
