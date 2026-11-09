import { readFileSync, writeFileSync } from "node:fs";

const graph = JSON.parse(readFileSync(new URL("./graph.json", import.meta.url), "utf8"));

const summaries = {
  App: "React 主界面与工作区装配",
  components: "会话、项目协作、Git、设置等 UI",
  control: "普通会话消息队列、广播与 PTY 投递",
  messaging: "crewvela sessions/send 命令与 outbox",
  multi_agent: "计划、调度、Runtime 适配、协作桥与持久化",
  pty: "跨平台伪终端进程托管和增量输出",
  sessions: "会话身份、状态机、恢复定位与 Hook 事件",
  worktree: "普通会话的 Git Worktree 生命周期",
  git_source_control: "Git 状态、Diff、提交、同步和合并",
  providers: "模型服务配置与 Runtime 环境变量",
  secrets: "凭据安全存储",
  automation: "定时与事件触发任务",
  skill_center: "Skill 中央库、分发和版本管理",
  model_eval: "模型评测任务与结果",
  notification_history: "通知记录持久化",
  windows_notification: "Windows 原生通知",
};

const nodes = graph.nodes.map((node) => ({
  ...node,
  summary: summaries[node.id] ?? `${node.path} 中的 ${node.id} 模块`,
}));

const data = {
  project: graph.project,
  generated: new Date().toISOString().slice(0, 10),
  deps: { nodes, edges: graph.edges },
  domains: {
    groups: [
      { id: "ui", label: "React 控制面", summary: "操作入口与状态投影", members: ["App", "MultiAgentControl", "MultiAgentWorkspace", "ControlSidebar"] },
      { id: "tauri", label: "Tauri IPC 边界", summary: "前端 invoke 到 Rust command", members: ["lib", "types", "controlSurface"] },
      { id: "coordination", label: "Multi-Agent 控制平面", summary: "计划、DAG 调度、治理", members: ["coordinator", "store", "executor", "governance"] },
      { id: "bridge", label: "Agent 协作桥", summary: "本机 HTTP/MCP + Bearer Scope", members: ["bridge", "message", "artifact", "approval", "memory"] },
      { id: "runtimes", label: "Runtime 适配层", summary: "统一协作请求与 CLI 执行", members: ["Claude Code", "Codex", "Kimi Code", "Pi", "OpenCode"] },
      { id: "execution", label: "执行与隔离", summary: "PTY、进程、会话恢复、Worktree", members: ["pty", "sessions", "worktree", "git_source_control"] },
      { id: "storage", label: "持久化事实源", summary: "SQLite 状态机与 FTS5", members: ["ma_tasks", "ma_runs", "ma_messages", "ma_artifacts", "ma_events", "ma_memory_entries"] },
      { id: "session-messaging", label: "普通会话互发", summary: "CLI outbox 到 PTY 输入", members: ["messaging", "mailbox.json", "outbox/*.json", "SessionActionQueue"] },
    ],
    edges: [
      { from: "ui", to: "tauri", label: "invoke" },
      { from: "tauri", to: "coordination", label: "command" },
      { from: "coordination", to: "storage", label: "事务" },
      { from: "coordination", to: "execution", label: "调度" },
      { from: "execution", to: "runtimes", label: "CLI/PTY" },
      { from: "runtimes", to: "bridge", label: "MCP/HTTP" },
      { from: "bridge", to: "storage", label: "读写" },
      { from: "session-messaging", to: "execution", label: "PTY 投递" },
    ],
  },
  flows: [
    {
      id: "plan-schedule",
      title: "需求到并行任务",
      symptom: "任务未启动时，依次检查结构化计划、依赖状态、wakeup、Agent 并发与工作区互斥。",
      nodes: [
        { id: "user", label: "用户自然语言", code: "MultiAgentControl.tsx", detail: "选择 Runtime/Agent 并提交目标" },
        { id: "planner", label: "Claude 协调器", code: "coordinator.rs:415", detail: "JSON Schema 约束输出 Agent、任务和 DAG" },
        { id: "apply", label: "计划落库", code: "lib.rs:1295", detail: "创建项目、Agent、任务和依赖" },
        { id: "router", label: "任务路由", code: "executor.rs:897", detail: "筛选依赖已满足且能力匹配的候选任务" },
        { id: "claim", label: "事务领取", code: "store.rs:260", detail: "Immediate transaction 防止重复领取" },
        { id: "parallel", label: "并行 Run", code: "executor.rs:993", detail: "按 Agent 与写工作区互斥规则启动线程" },
      ],
      edges: [
        { from: "user", to: "planner", label: "invoke" },
        { from: "planner", to: "apply", label: "CoordinatorPlan" },
        { from: "apply", to: "router", label: "SQLite DAG" },
        { from: "router", to: "claim", label: "ready" },
        { from: "claim", to: "parallel", label: "AgentRun" },
      ],
    },
    {
      id: "runtime-execution",
      title: "统一 Runtime 执行",
      symptom: "某个 Runtime 行为异常时，先看 adapter 命令参数，再看 PTY 输出规范化与 session locator。",
      nodes: [
        { id: "request", label: "RuntimeRequest", code: "executor.rs:60", detail: "任务、Agent、依赖交付、消息、记忆和协作令牌" },
        { id: "adapter", label: "Runtime Adapter", code: "executor.rs:1604", detail: "八个适配器翻译为各 CLI 参数" },
        { id: "terminal", label: "PTY/子进程", code: "executor.rs:2290", detail: "执行、输出采集、取消与超时" },
        { id: "runtime", label: "Agent Runtime", code: "claude/codex/kimi/...", detail: "真实 CLI 进程执行工具和代码修改" },
        { id: "result", label: "结果与恢复定位", code: "executor.rs:2326", detail: "规范化输出并保存 sessionId/ptyId/cwd" },
        { id: "verify", label: "验收与结算", code: "executor.rs:1192", detail: "运行验收命令、保存提交和更新状态" },
      ],
      edges: [
        { from: "request", to: "adapter", label: "统一接口" },
        { from: "adapter", to: "terminal", label: "Command" },
        { from: "terminal", to: "runtime", label: "stdin/stdout" },
        { from: "runtime", to: "result", label: "JSON/文本" },
        { from: "result", to: "verify", label: "summary" },
      ],
    },
    {
      id: "agent-communication",
      title: "Agent 跨 Runtime 通信",
      symptom: "消息不可见时，检查 Run token 是否有效、目标 Agent scope、ma_messages 是否落库及下一次 Prompt 是否带入。",
      nodes: [
        { id: "agent-a", label: "Agent A", code: "任意 Runtime", detail: "调用 cc_message_send；无 MCP 时 POST /v1/command" },
        { id: "local-bridge", label: "本地协作桥", code: "bridge.rs:81", detail: "127.0.0.1 随机端口，Bearer token 绑定 RunScope" },
        { id: "dispatch", label: "Scope 校验与分发", code: "bridge.rs:214", detail: "只允许当前 project/task/run/agent/cwd" },
        { id: "sqlite", label: "SQLite 事实源", code: "store.rs:780", detail: "ma_messages、artifacts、approvals、events" },
        { id: "agent-b", label: "Agent B 下一次 Run", code: "executor.rs:1173", detail: "scoped_messages 写入统一任务 Prompt" },
      ],
      edges: [
        { from: "agent-a", to: "local-bridge", label: "MCP/HTTP" },
        { from: "local-bridge", to: "dispatch", label: "Bearer" },
        { from: "dispatch", to: "sqlite", label: "事务写入" },
        { from: "sqlite", to: "agent-b", label: "按范围读取" },
      ],
    },
    {
      id: "worktree-handoff",
      title: "依赖交付与集成",
      symptom: "集成失败时，检查前置 artifact 的 commit hash、目标 Worktree、主工作区脏状态和 cherry-pick/merge 冲突。",
      nodes: [
        { id: "worker", label: "Worker 修改代码", code: "独立 Worktree", detail: "每个写任务隔离工作目录" },
        { id: "checkpoint", label: "任务检查点", code: "executor.rs:1319", detail: "提交变更并记录 content hash" },
        { id: "artifact", label: "Artifact", code: "ma_artifacts", detail: "URI、摘要、提交哈希持久化" },
        { id: "dependency", label: "后继 Worktree", code: "executor.rs:3621", detail: "把依赖提交 cherry-pick 到后继任务" },
        { id: "review", label: "Reviewer/Integrator", code: "cc_review_submit", detail: "结构化批准或要求修改" },
        { id: "main", label: "主工作区", code: "executor.rs:1429", detail: "批准后 --no-ff merge，冲突则 abort" },
      ],
      edges: [
        { from: "worker", to: "checkpoint", label: "git commit" },
        { from: "checkpoint", to: "artifact", label: "publish" },
        { from: "artifact", to: "dependency", label: "cherry-pick" },
        { from: "dependency", to: "review", label: "验收" },
        { from: "review", to: "main", label: "approve" },
      ],
    },
  ],
};

writeFileSync(new URL("./arch-data.json", import.meta.url), `${JSON.stringify(data, null, 2)}\n`);
