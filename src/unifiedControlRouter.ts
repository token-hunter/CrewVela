export type UnifiedControlRoute = "quick" | "multi_agent";

const COLLABORATION_PREFIX =
  /^(?:用|在|通过)?(?:项目协作|协作模式|协作项目)[，,、：:\s]+/u;
const QUICK_PREFIX = /^(?:用|在|通过)?(?:快速指挥|总控)[，,、：:\s]+/u;

export function stripUnifiedControlPrefix(message: string): string {
  return message
    .trim()
    .replace(COLLABORATION_PREFIX, "")
    .replace(QUICK_PREFIX, "")
    .trim();
}

export function resolveUnifiedControlRoute(
  message: string,
  explicitCollaborationTargets = 0,
): UnifiedControlRoute {
  const normalized = message.trim();
  if (explicitCollaborationTargets > 0) return "multi_agent";
  if (COLLABORATION_PREFIX.test(normalized)) return "multi_agent";
  if (QUICK_PREFIX.test(normalized)) return "quick";

  // @目标只在项目协作与终端定向投递中存在；普通自然语言默认保持轻量快速指挥。
  if (/(^|\s)@[\p{L}\p{N}_-]+/u.test(normalized)) return "multi_agent";

  // Broadcast content may mention a project; the outer action still selects sessions.
  if (/(?:发送|广播|发给).*(?:所有|全部|运行).*会话/u.test(normalized)) return "quick";

  // 没有 @ 时，仅明确的项目生命周期或任务编排指令进入项目协作。
  if (
    /(?:启动|暂停|恢复|继续|取消|检查|打开).*(?:当前|这个|该)?(?:协作)?项目/u.test(
      normalized,
    ) ||
    /(?:当前|这个|该)(?:协作)?项目.*(?:启动|暂停|恢复|继续|取消|状态|工作区|任务)/u.test(
      normalized,
    ) ||
    /(?:创建|新增|拆分|重试|重派|审核|批准).*(?:协作任务|项目任务|Agent|智能体)/iu.test(
      normalized,
    )
  ) {
    return "multi_agent";
  }

  return "quick";
}

