import test from "node:test";
import assert from "node:assert/strict";

import {
  decodeMultiAgentDraft,
  encodeMultiAgentDraft,
  getRuntimeHealth,
  isUsablePlanText,
  planIsLowRiskCommunication,
  sanitizeGeneratedPlan,
} from "../src/multiAgentDraftState.ts";

const validPlan = {
  intent: "plan_project",
  reply: "已生成计划",
  projectName: "贪吃蛇",
  goal: "完成可运行的游戏",
  targetTaskId: null,
  targetAgentId: null,
  agents: [],
  tasks: [],
  requiresConfirmation: true,
};

test("ZCode CLI plans retain runtime identity and pending context across restart", () => {
  const state = {
    projectPath: "C:/fixture", coordinatorProjectId: "project-A", coordinatorDraft: "", mentionBindings: [], coordinatorMessages: [],
    pendingPlan: { ...validPlan, rootPath: "C:/fixture", requestedRuntimes: ["zcode_cli"], agents: [{ key: "z", name: "ZCode CLI", runtimeKind: "zcode_cli", role: "worker", description: "实现", model: null, permissionProfile: "default", existingAgentId: null }] },
    pendingPlanContext: { projectId: null, projectPath: "C:/fixture", requestedRuntimes: ["zcode_cli"], autoStart: true },
  };
  assert.deepEqual(decodeMultiAgentDraft(encodeMultiAgentDraft(state)), state);
});

test("unconfirmed coordinator state survives serialization", () => {
  const state = {
    projectPath: "F:\\project\\tanchishe",
    coordinatorProjectId: "project-1",
    coordinatorDraft: "继续细化测试任务",
    mentionBindings: [],
    coordinatorMessages: [{ role: "user", content: "创建贪吃蛇" }],
    pendingPlan: {
      ...validPlan,
      rootPath: null,
      requestedRuntimes: [],
    },
    pendingPlanContext: {
      projectId: null,
      projectPath: "F:\\project\\tanchishe",
    },
  };

  assert.deepEqual(decodeMultiAgentDraft(encodeMultiAgentDraft(state)), state);
});

test("invalid persisted coordinator state falls back safely", () => {
  assert.deepEqual(decodeMultiAgentDraft("{broken"), {
    projectPath: "",
    coordinatorProjectId: null,
    coordinatorDraft: "",
    mentionBindings: [],
    coordinatorMessages: [],
    pendingPlan: null,
    pendingPlanContext: null,
  });
  assert.equal(
    decodeMultiAgentDraft(
      `{"pendingPlan":${JSON.stringify({ ...validPlan, agents: [null] })}}`,
    ).pendingPlan,
    null,
  );
});

test("legacy pending plans recover a stable target context", () => {
  const restoredNewProject = decodeMultiAgentDraft(
    JSON.stringify({
      projectPath: "F:\\project\\new-game",
      coordinatorProjectId: null,
      pendingPlan: validPlan,
    }),
  );
  assert.deepEqual(restoredNewProject.pendingPlanContext, {
    projectId: null,
    projectPath: "F:\\project\\new-game",
  });

  const restoredExistingProject = decodeMultiAgentDraft(
    JSON.stringify({
      projectPath: "F:\\project\\workspace",
      coordinatorProjectId: "project-a",
      pendingPlan: { ...validPlan, intent: "add_tasks" },
    }),
  );
  assert.deepEqual(restoredExistingProject.pendingPlanContext, {
    projectId: "project-a",
    projectPath: "F:\\project\\workspace",
  });
});

test("generated null-like project metadata is normalized", () => {
  const sanitized = sanitizeGeneratedPlan({
    ...validPlan,
    projectName: " null ",
    goal: "undefined",
  });

  assert.equal(sanitized.projectName, null);
  assert.equal(sanitized.goal, null);
  assert.equal(isUsablePlanText("贪吃蛇"), true);
  assert.equal(isUsablePlanText("NULL"), false);
});

test("runtime probe errors are warnings rather than healthy", () => {
  assert.equal(
    getRuntimeHealth({ installed: true, error: "读取版本超时" }),
    "warning",
  );
  assert.equal(getRuntimeHealth({ installed: true, error: null }), "installed");
  assert.equal(getRuntimeHealth({ installed: false, error: "未找到" }), "missing");
});

test("纯会话通信计划判定为低风险免确认", () => {
  const greetingPlan = {
    ...validPlan,
    intent: "add_tasks",
    agents: [
      {
        key: "designer",
        name: "OpenCode 设计师",
        runtimeKind: "opencode",
        role: "orchestrator",
        description: "",
        model: null,
        permissionProfile: "auto",
        existingAgentId: null,
      },
    ],
    tasks: [
      {
        key: "greet",
        title: "互相打招呼",
        description: "运行 CREWVELA_CLI sessions 发现目标，再 send --to <目标> 发送问候",
        acceptanceCriteria: "通过 CREWVELA_CLI send --to 送达问候",
        verificationCommand: null,
        workspaceMode: "read_only",
        assigneeKey: "designer",
        reviewerKey: null,
        dependsOn: [],
        priority: 0,
        requiredCapabilities: [],
        maxAttempts: 3,
        retryBackoffSeconds: 5,
        stage: 0,
      },
    ],
  };
  assert.equal(planIsLowRiskCommunication(greetingPlan), true);

  assert.equal(
    planIsLowRiskCommunication({
      ...greetingPlan,
      tasks: [{ ...greetingPlan.tasks[0], verificationCommand: "npm test" }],
    }),
    false,
    "带验收命令的计划仍需确认",
  );
  assert.equal(
    planIsLowRiskCommunication({
      ...greetingPlan,
      tasks: [
        {
          ...greetingPlan.tasks[0],
          description: "实现登录模块",
          acceptanceCriteria: "功能可用并通过验收",
        },
      ],
    }),
    false,
    "非通信任务仍需确认",
  );
  assert.equal(
    planIsLowRiskCommunication({ ...greetingPlan, tasks: [] }),
    false,
    "空任务计划不构成免确认",
  );
  assert.equal(
    planIsLowRiskCommunication({ ...greetingPlan, intent: "inspect_project" }),
    false,
    "非落地类意图不走该判定",
  );
});
