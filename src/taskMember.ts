import { RUNTIME_LABELS } from "./runtimeLogos";
import type { AgentProfile, ConversationExecutionPlan } from "./types";

export function taskMember(agents: AgentProfile[], id: string | null | undefined, plan?: ConversationExecutionPlan) {
  const profile = agents.find(agent => agent.id === id && (!plan || agent.projectId === plan.projectId));
  const saved = id ? plan?.managed?.target.members?.[id] : undefined;
  const runtimeKind = saved?.runtime ?? profile?.runtimeKind;
  let name = profile?.name ?? "未分配";
  if (!profile && id && plan?.managed) {
    const runtimeName = runtimeKind && RUNTIME_LABELS[runtimeKind];
    name = runtimeName ? `${runtimeName}（历史成员）` : "历史成员";
  }
  return { name, avatarId: profile?.avatarId, runtimeKind };
}
