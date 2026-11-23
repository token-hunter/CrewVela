import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentRuntimeKind } from "../types";
import { AGENT_AVATARS, agentAvatarSource, isImportedAgentAvatar } from "../agentAvatars";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";

const importedImages = new Map<string, Promise<string>>();
function loadImportedImage(avatarId: string): Promise<string> {
  const cached = importedImages.get(avatarId);
  if (cached) return cached;
  const request = invoke<string>("ma_agent_avatar_image", { avatarId }).catch(error => {
    if (importedImages.get(avatarId) === request) importedImages.delete(avatarId);
    throw error;
  });
  if (importedImages.size >= 64) importedImages.delete(importedImages.keys().next().value!);
  importedImages.set(avatarId, request);
  return request;
}

export function AgentAvatar({ avatarId, runtimeKind, className, decorative = false }: {
  avatarId?: string | null; runtimeKind: AgentRuntimeKind; className?: string; decorative?: boolean;
}) {
  const custom = AGENT_AVATARS.find((avatar) => avatar.id === avatarId);
  const [imported, setImported] = useState<{ id: string; src: string }>();
  useEffect(() => {
    if (!isImportedAgentAvatar(avatarId)) return;
    let active = true;
    void loadImportedImage(avatarId).then(src => {
      if (active && src.startsWith("data:image/png;base64,")) setImported({ id: avatarId, src });
    }, () => { /* A missing asset falls back to the Runtime logo. */ });
    return () => { active = false; };
  }, [avatarId]);
  const image = imported?.id === avatarId ? imported?.src : undefined;
  const label = image ? "自定义头像" : custom?.name ?? RUNTIME_LABELS[runtimeKind] ?? RUNTIME_LABELS.unsupported;
  return <img className={className} src={image ?? agentAvatarSource(avatarId, RUNTIME_LOGOS[runtimeKind] ?? RUNTIME_LOGOS.unsupported)}
    data-avatar={image ? "imported" : custom?.id} data-runtime={image || custom ? undefined : runtimeKind}
    onError={image ? () => setImported(undefined) : undefined}
    alt={decorative ? "" : label} aria-hidden={decorative || undefined} title={label} />;
}
