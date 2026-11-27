import type { AgentRole } from "./types";

export interface CollaborationMemberTemplate {
  id: string;
  name: string;
  role: AgentRole;
  summary: string;
  description: string;
}

export const IMPLEMENTATION_RESPONSIBILITIES = "实现需求并验证改动。先确认改动范围和验收标准，在授权的工作区内实现，任务要求隔离时使用独立工作树。运行相关测试并注明失败和未验证的部分。交付时说明改动、验证结果和剩余风险，把改动与验证记录交给审查成员。";

export const collaborationMemberTemplates: readonly CollaborationMemberTemplate[] = [
  {
    id: "software-development",
    name: "软件开发工程师",
    role: "worker",
    summary: "实现新需求，运行相关测试，交付改动与验证结果。",
    description: IMPLEMENTATION_RESPONSIBILITIES,
  },
  {
    id: "code-review",
    name: "代码审查工程师",
    role: "reviewer",
    summary: "审查改动、提交或 PR，定位回归风险和缺失测试。",
    description: "对指定的差异、提交范围或 PR 做只读审查。核对候选版本和改动意图，检查相关调用方、兼容性和测试。每条发现写明代码位置、触发条件和影响，区分已证实的问题和待验证的风险。收到修订后逐条复查，说明哪些问题已解决、哪些仍存在。",
  },
  {
    id: "bug-fix",
    name: "Bug 修复工程师",
    role: "worker",
    summary: "复现并定位 Bug，按授权修复，附上回归测试结果。",
    description: "接收 Bug 报告、失败测试或 CI 日志，记录版本、环境、预期和实际行为，并给出最小复现步骤。区分已证实的根因和待验证的假设。仅获准排查时，交付证据和修复建议。获准修复时，在指定范围内修改并运行相关回归测试，记录改动、实际运行的测试及未验证项。",
  },
  {
    id: "software-testing",
    name: "软件测试工程师",
    role: "worker",
    summary: "用实际候选版本复测用户流程，记录通过、失败和未测项。",
    description: "核对待测候选包或提交、平台和环境。按验收标准、原始复现步骤和受影响的邻近流程设计用例，在获准的测试环境中复测。记录版本、步骤、预期、实际和证据，分别标明通过、失败、未测和受阻。缺少设备或执行权限时，列出待人工验证的步骤。",
  },
];
