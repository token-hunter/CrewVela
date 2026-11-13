export type MacControlTool = "openComputerUse" | "kimiCu";
export interface MacControlInstallProgress {
  tool: MacControlTool;
  requestId: string;
  message: string;
}
export interface MacControlStatus {
  supported: boolean;
  installed: boolean;
  executable: string | null;
  mcpRegistered: boolean | null;
  accessibility: boolean | null;
  screenRecording: boolean | null;
  serviceRunning: boolean | null;
  detail: string;
}
export function permissionLabel(value: boolean | null | undefined): string {
  return value == null ? "待检查" : value ? "已授予" : "待授权";
}
export function macControlLabel(status: MacControlStatus | undefined): string {
  if (!status) return "尚未检测";
  if (!status.supported) return "不支持当前系统或架构";
  if (!status.installed) return "未安装或安装无效";
  if (status.accessibility === false || status.screenRecording === false) return "待授权";
  if (status.serviceRunning === false) return "后台服务未运行";
  if (status.accessibility == null || status.screenRecording == null) return "已安装 · 权限待检查";
  return "权限已授予 · 工具连接待验证";
}
