import type { AgentRuntimeKind } from "../types";
import { AGENT_AVATARS, agentAvatarSource } from "../agentAvatars";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";

export function AgentAvatar({ avatarId, runtimeKind, className, decorative = false }: {
  avatarId?: string | null; runtimeKind: AgentRuntimeKind; className?: string; decorative?: boolean;
}) {
  const custom = AGENT_AVATARS.find((avatar) => avatar.id === avatarId);
  const label = custom?.name ?? RUNTIME_LABELS[runtimeKind] ?? RUNTIME_LABELS.unsupported;
  return <img className={className} src={agentAvatarSource(avatarId, RUNTIME_LOGOS[runtimeKind] ?? RUNTIME_LOGOS.unsupported)}
    data-avatar={custom?.id} data-runtime={custom ? undefined : runtimeKind}
    alt={decorative ? "" : label} aria-hidden={decorative || undefined} title={label} />;
}
