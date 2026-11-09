export const AGENT_AVATARS = [
  { id: "orbit", name: "轨道", src: new URL("./assets/agent-avatars/orbit.svg", import.meta.url).href },
  { id: "block", name: "积木", src: new URL("./assets/agent-avatars/block.svg", import.meta.url).href },
  { id: "sprout", name: "芽芽", src: new URL("./assets/agent-avatars/sprout.svg", import.meta.url).href },
  { id: "ripple", name: "波纹", src: new URL("./assets/agent-avatars/ripple.svg", import.meta.url).href },
  { id: "prism", name: "棱镜", src: new URL("./assets/agent-avatars/prism.svg", import.meta.url).href },
  { id: "compass", name: "罗盘", src: new URL("./assets/agent-avatars/compass.svg", import.meta.url).href },
  { id: "messenger", name: "信使", src: new URL("./assets/agent-avatars/messenger.svg", import.meta.url).href },
  { id: "beans", name: "豆豆", src: new URL("./assets/agent-avatars/beans.svg", import.meta.url).href },
  { id: "crescent", name: "月牙", src: new URL("./assets/agent-avatars/crescent.svg", import.meta.url).href },
  { id: "spark", name: "火花", src: new URL("./assets/agent-avatars/spark.svg", import.meta.url).href },
  { id: "magnet", name: "磁石", src: new URL("./assets/agent-avatars/magnet.svg", import.meta.url).href },
  { id: "cloud", name: "云团", src: new URL("./assets/agent-avatars/cloud.svg", import.meta.url).href },
] as const;

export function agentAvatarSource(avatarId: string | null | undefined, runtimeSource: string): string {
  return AGENT_AVATARS.find((avatar) => avatar.id === avatarId)?.src ?? runtimeSource;
}
