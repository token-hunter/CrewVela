export type PrivacyDestination = "archivedSessions" | "collaboration" | "sharedMemory" | "browserControl";
export interface PrivacyDataRow { title: string; description: string; action: string; destination?: PrivacyDestination; }
export const PRIVACY_DATA_ROWS: readonly PrivacyDataRow[] = [
  { title: "应用数据目录", description: "会话索引、协作记录与应用设置。", action: "查看位置" },
  { title: "会话与归档", description: "查看、恢复或删除 CrewVela 会话记录。", action: "管理", destination: "archivedSessions" },
  { title: "协作记录与诊断", description: "任务、团队消息与运行状态。", action: "查看", destination: "collaboration" },
  { title: "共享记忆", description: "数据位置与访问范围由当前记忆服务决定。", action: "管理", destination: "sharedMemory" },
  { title: "浏览器访问", description: "管理 Agent 的网页操作设置。", action: "设置", destination: "browserControl" },
];
export type PrivacyExplanation = "storage" | "services";
export const PRIVACY_EXPLANATIONS = {
  storage: {title:"数据保存在哪里", headings:["本机应用记录","管理与删除"]},
  services: {title:"Agent 与外部服务", headings:["Agent、模型与工具","登录与凭据","自动与手动检查应用更新"]},
} as const;
/** Read only relevant paragraphs from the same bundled policy used by Help. */
export function privacySections(document: string, headings: readonly string[]): Array<{title:string; paragraphs:string[]}> {
  return document.replace(/\r\n/g,"\n").split(/^## /m).slice(1).flatMap(block=>{
    const end=block.indexOf("\n"),title=(end<0?block:block.slice(0,end)).trim();
    return headings.includes(title) ? [{title,paragraphs:(end<0?"":block.slice(end+1)).trim().split(/\n\s*\n/).filter(Boolean)}] : [];
  });
}
