import type { ProviderConfig } from "./types";
import { NEUTRAL_AGENT_ICONS } from "./neutralAgentAssets";

export type ProviderTone = "kimi" | "glm" | "deepseek" | "minimax" | "custom";

const kimiModelLogo = NEUTRAL_AGENT_ICONS.kimi;
const glmModelLogo = NEUTRAL_AGENT_ICONS.zcode;
const deepSeekModelLogo = NEUTRAL_AGENT_ICONS.deepseek;
const miniMaxModelLogo = NEUTRAL_AGENT_ICONS.minimax;

export const PROVIDER_LOGOS: Partial<Record<ProviderTone, string>> = {
  kimi: kimiModelLogo,
  glm: glmModelLogo,
  deepseek: deepSeekModelLogo,
  minimax: miniMaxModelLogo,
};

export function providerTone(provider: ProviderConfig): ProviderTone {
  const identity = `${provider.id} ${provider.name}`.toLowerCase();
  if (identity.includes("kimi")) return "kimi";
  if (identity.includes("glm")) return "glm";
  if (identity.includes("deepseek")) return "deepseek";
  if (identity.includes("minimax")) return "minimax";
  return "custom";
}
