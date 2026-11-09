import type { ProviderConfig } from "./types";

export type ProviderTone = "kimi" | "glm" | "deepseek" | "minimax" | "custom";

const kimiModelLogo = new URL("./assets/agent-kimi-code.svg", import.meta.url).href;
const glmModelLogo = new URL("./assets/model-zhipu.svg", import.meta.url).href;
const deepSeekModelLogo = new URL("./assets/model-deepseek.svg", import.meta.url).href;
const miniMaxModelLogo = new URL("./assets/model-minimax.svg", import.meta.url).href;

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
