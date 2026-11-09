use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeKind {
    ClaudeCode,
    Codex,
    KimiCode,
    /// Historical persistence sentinel only; no executable adapter remains.
    #[serde(rename = "minimax_code")]
    MiniMaxCode,
    Pi,
    OhMyPi,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "claude_chat")]
    ClaudeChat,
    #[serde(rename = "kimi_chat")]
    KimiChat,
    #[serde(rename = "pi_chat")]
    PiChat,
    #[serde(rename = "codex_chat")]
    CodexChat,
    #[serde(rename = "opencode_chat")]
    OpenCodeChat,
    #[serde(rename = "deepseek_harness", alias = "deep_seek_harness")]
    DeepSeekHarness,
    #[serde(rename = "zcode", alias = "z_code")]
    ZCode,
    #[serde(rename = "zcode_cli", alias = "z_code_cli")]
    ZCodeCli,
    /// Historical unknown runtimes remain readable but are never executable.
    #[serde(other)]
    Unsupported,
}

impl AgentRuntimeKind {
    pub fn is_retired(self) -> bool {
        matches!(self, Self::OhMyPi | Self::MiniMaxCode | Self::Unsupported)
    }

    pub fn ensure_available(self) -> anyhow::Result<()> {
        crate::runtime_preferences::ensure_available(self)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::KimiCode => "kimi_code",
            Self::MiniMaxCode => "minimax_code",
            Self::Pi => "pi",
            Self::OhMyPi => "oh_my_pi",
            Self::OpenCode => "opencode",
            Self::ClaudeChat => "claude_chat",
            Self::KimiChat => "kimi_chat",
            Self::PiChat => "pi_chat",
            Self::CodexChat => "codex_chat",
            Self::OpenCodeChat => "opencode_chat",
            Self::DeepSeekHarness => "deepseek_harness",
            Self::ZCode => "zcode",
            Self::ZCodeCli => "zcode_cli",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator,
    Worker,
    Reviewer,
    Integrator,
}

/// 成员人格预设：只约束沟通风格与工作倾向，不改变职责（职责仍由 description 表达）。
/// custom 时使用 persona_custom 自由文本注入派单提示词。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPersona {
    Efficient,
    Concise,
    Rigorous,
    Innovative,
    Custom,
}

impl Default for AgentPersona {
    fn default() -> Self {
        Self::Efficient
    }
}

impl AgentPersona {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Efficient => "efficient",
            Self::Concise => "concise",
            Self::Rigorous => "rigorous",
            Self::Innovative => "innovative",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "concise" => Self::Concise,
            "rigorous" => Self::Rigorous,
            "innovative" => Self::Innovative,
            "custom" => Self::Custom,
            _ => Self::Efficient,
        }
    }

    /// 派单提示词里的工作风格段；custom 由调用方拼接 persona_custom。
    pub fn prompt_segment(self) -> &'static str {
        match self {
            Self::Efficient => "务实高效：直接推进任务，优先解决阻塞，避免过度设计与无关扩展。",
            Self::Concise => "简洁直接：输出精炼，只保留必要信息，不做冗余铺垫。",
            Self::Rigorous => "严谨细致：逐步验证假设，交叉核对关键结论，明确列出不确定性与风险。",
            Self::Innovative => "创新探索：在满足验收标准的前提下主动提出更优方案，并说明取舍。",
            Self::Custom => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionProfile {
    Default,
    Auto,
    Yolo,
}

impl PermissionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Auto => "auto",
            Self::Yolo => "yolo",
        }
    }
}

impl AgentRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::Integrator => "integrator",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Draft,
    Ready,
    Blocked,
    Claimed,
    Running,
    WaitingApproval,
    InReview,
    ChangesRequested,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Claimed => "claimed",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::InReview => "in_review",
            Self::ChangesRequested => "changes_requested",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    WaitingInput,
    WaitingApproval,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::WaitingApproval => "waiting_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    ReadOnly,
    Worktree,
    SharedWrite,
}

impl WorkspaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Worktree => "worktree",
            Self::SharedWrite => "shared_write",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiAgentProject {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub root_path: String,
    pub default_branch: String,
    pub status: ProjectStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectInput {
    pub name: String,
    pub goal: String,
    pub root_path: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    #[serde(default)]
    pub avatar_id: Option<String>,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub runtime_kind: AgentRuntimeKind,
    pub role: AgentRole,
    pub description: String,
    #[serde(default)]
    pub persona: AgentPersona,
    #[serde(default)]
    pub persona_custom: String,
    pub model: Option<String>,
    pub permission_profile: PermissionProfile,
    pub max_concurrency: u32,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentInput {
    #[serde(default)]
    pub avatar_id: Option<String>,
    pub project_id: String,
    pub name: String,
    pub runtime_kind: AgentRuntimeKind,
    pub role: AgentRole,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub persona: AgentPersona,
    #[serde(default)]
    pub persona_custom: String,
    pub model: Option<String>,
    #[serde(default = "default_permission_profile")]
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentInput {
    #[serde(default)]
    pub avatar_id: Option<String>,
    pub project_id: String,
    pub agent_id: String,
    pub name: String,
    pub runtime_kind: AgentRuntimeKind,
    pub role: AgentRole,
    pub description: String,
    #[serde(default)]
    pub persona: AgentPersona,
    #[serde(default)]
    pub persona_custom: String,
    pub model: Option<String>,
    pub enabled: bool,
}

fn default_permission_profile() -> PermissionProfile {
    PermissionProfile::Default
}

fn default_max_concurrency() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiAgentTask {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub stage: u32,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub verification_command: Option<String>,
    pub status: TaskStatus,
    pub assignee_agent_id: Option<String>,
    pub reviewer_agent_id: Option<String>,
    pub workspace_mode: WorkspaceMode,
    pub priority: i32,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    pub max_attempts: u32,
    pub retry_backoff_seconds: u64,
    pub next_retry_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 任务契约是否要求会话间 CLI 通信（CREWVELA_CLI sessions/send）。
/// 此类任务必须走 PTY 路径：PTY 注入 CREWVELA_CLI 环境变量，ACP 路径没有。
pub fn contract_requires_session_cli(description: &str, acceptance_criteria: &str) -> bool {
    let contract = format!("{description}\n{acceptance_criteria}");
    contract.contains("CREWVELA_CLI")
        && (contract.contains("sessions") || contract.contains("send --to"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub project_id: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub stage: u32,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: String,
    pub verification_command: Option<String>,
    pub assignee_agent_id: Option<String>,
    pub reviewer_agent_id: Option<String>,
    #[serde(default = "default_workspace_mode")]
    pub workspace_mode: WorkspaceMode,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_backoff_seconds")]
    pub retry_backoff_seconds: u64,
}

fn default_workspace_mode() -> WorkspaceMode {
    WorkspaceMode::Worktree
}

fn default_max_attempts() -> u32 {
    3
}

fn default_retry_backoff_seconds() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub attempt: u32,
    pub status: RunStatus,
    pub session_locator: Option<serde_json::Value>,
    pub lease_expires_at: i64,
    pub last_heartbeat_at: Option<i64>,
    pub output_summary: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
}

impl ReviewVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecision {
    pub id: String,
    pub project_id: String,
    pub review_task_id: String,
    pub review_run_id: String,
    pub reviewer_agent_id: String,
    pub target_task_id: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub created_at: i64,
    pub applied_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReviewDecisionInput {
    pub review_run_id: String,
    pub target_task_id: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeupRequest {
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub reason: String,
    pub status: String,
    pub idempotency_key: String,
    pub coalesced_count: u32,
    pub requested_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub seq: i64,
    pub project_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDependencyRecord {
    pub task_id: String,
    pub depends_on_task_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionStatus {
    Idle,
    Starting,
    Running,
    Waiting,
    Completed,
    Failed,
    Stopped,
}

impl AgentSessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentControlMode {
    Auto,
    Manual,
}

impl AgentControlMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub current_task_id: Option<String>,
    pub current_run_id: Option<String>,
    pub app_session_id: Option<String>,
    pub status: AgentSessionStatus,
    pub control_mode: AgentControlMode,
    pub last_output_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogEntry {
    pub seq: i64,
    pub project_id: String,
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub stream: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstruction {
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub body: String,
    pub status: String,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiAgentSnapshot {
    pub project: MultiAgentProject,
    pub agents: Vec<AgentProfile>,
    pub tasks: Vec<MultiAgentTask>,
    pub runs: Vec<AgentRun>,
    pub wakeups: Vec<WakeupRequest>,
    pub events: Vec<ActivityEvent>,
    pub messages: Vec<CollaborationMessage>,
    pub artifacts: Vec<ArtifactRecord>,
    pub approvals: Vec<ApprovalRecord>,
    pub dependencies: Vec<TaskDependencyRecord>,
    pub agent_sessions: Vec<AgentSessionRecord>,
    pub run_logs: Vec<RunLogEntry>,
    pub instructions: Vec<AgentInstruction>,
    pub runtime_events: Vec<RuntimeEventRecord>,
    pub memories: Vec<MemoryEntry>,
    pub review_decisions: Vec<ReviewDecision>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeoverAgentInput {
    pub project_id: String,
    pub agent_id: String,
    pub instruction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnAgentControlInput {
    pub project_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlReceipt {
    pub project_id: String,
    pub agent_id: String,
    pub control_mode: AgentControlMode,
    pub queued_instruction: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDependencyInput {
    pub task_id: String,
    pub depends_on_task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimTaskInput {
    pub task_id: String,
    pub agent_id: String,
    #[serde(default = "default_lease_seconds")]
    pub lease_seconds: u64,
}

fn default_lease_seconds() -> u64 {
    30 * 60
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueWakeupInput {
    pub project_id: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRunInput {
    pub run_id: String,
    pub status: RunStatus,
    pub output_summary: Option<String>,
    pub error: Option<String>,
    pub session_locator: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReceipt {
    pub interrupted_runs: usize,
    pub requeued_read_only_tasks: usize,
    pub blocked_write_tasks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoordinatorMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorHistoryMessage {
    pub role: CoordinatorMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorIntent {
    PlanProject,
    AddTasks,
    StartProject,
    PauseProject,
    ResumeProject,
    CancelProject,
    InspectProject,
    RetryTask,
    ReassignTask,
    ApproveTask,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAgent {
    pub key: String,
    pub name: String,
    pub runtime_kind: AgentRuntimeKind,
    pub role: AgentRole,
    pub description: String,
    pub model: Option<String>,
    pub permission_profile: PermissionProfile,
    #[serde(default)]
    pub existing_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTask {
    pub key: String,
    pub stage: u32,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub verification_command: Option<String>,
    pub assignee_key: String,
    pub reviewer_key: Option<String>,
    pub depends_on: Vec<String>,
    pub workspace_mode: WorkspaceMode,
    pub priority: i32,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_backoff_seconds")]
    pub retry_backoff_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorPlan {
    pub intent: CoordinatorIntent,
    pub reply: String,
    pub project_name: Option<String>,
    pub goal: Option<String>,
    pub root_path: Option<String>,
    pub target_task_id: Option<String>,
    pub target_agent_id: Option<String>,
    #[serde(default)]
    pub agents: Vec<PlannedAgent>,
    #[serde(default)]
    pub tasks: Vec<PlannedTask>,
    #[serde(default)]
    pub requested_runtimes: Vec<AgentRuntimeKind>,
    #[serde(default)]
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCoordinatorPlanInput {
    pub project_id: Option<String>,
    pub root_path: String,
    pub plan: CoordinatorPlan,
    #[serde(default)]
    pub conversation: Vec<CoordinatorHistoryMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCoordinatorPlanReceipt {
    pub project_id: String,
    pub created_agents: usize,
    pub created_tasks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationMessage {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub sender_type: String,
    pub sender_id: Option<String>,
    pub target_agent_id: Option<String>,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub artifact_type: String,
    pub uri: String,
    pub summary: String,
    pub content_hash: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub approval_type: String,
    pub summary: String,
    pub status: String,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionKind {
    ApprovalRequired,
    AgentInputRequired,
    RetryExhausted,
    DependencyBlocked,
    SemanticReview,
}

impl InterventionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval_required",
            Self::AgentInputRequired => "agent_input_required",
            Self::RetryExhausted => "retry_exhausted",
            Self::DependencyBlocked => "dependency_blocked",
            Self::SemanticReview => "semantic_review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionSeverity {
    Critical,
    High,
    Medium,
    Low,
}

impl InterventionSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionStatus {
    Open,
    Snoozed,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionRecommendedAction {
    Approve,
    Respond,
    Retry,
    OpenProject,
    Review,
}

impl InterventionRecommendedAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Respond => "respond",
            Self::Retry => "retry",
            Self::OpenProject => "open_project",
            Self::Review => "review",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub kind: InterventionKind,
    pub severity: InterventionSeverity,
    pub title: String,
    pub summary: String,
    pub status: InterventionStatus,
    pub recommended_action: InterventionRecommendedAction,
    pub fingerprint: String,
    pub snoozed_until: Option<i64>,
    pub metadata: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionAction {
    Approve,
    Respond,
    Retry,
    Reassign,
    Snooze,
    Dismiss,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInterventionInput {
    pub intervention_id: String,
    pub action: InterventionAction,
    pub response: Option<String>,
    pub agent_id: Option<String>,
    pub snooze_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionResolutionReceipt {
    pub intervention_id: String,
    pub project_id: String,
    pub resume_project: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogDisposition {
    Scheduled,
    WaitingForHuman,
    ClearToComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEventRecord {
    pub seq: i64,
    pub project_id: String,
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendRuntimeEventInput {
    pub run_id: String,
    pub event_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub project_id: String,
    pub namespace: String,
    pub key: String,
    pub value: String,
    pub source_task_id: Option<String>,
    pub source_run_id: Option<String>,
    pub confidence: f64,
    pub verified: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutMemoryInput {
    pub project_id: String,
    pub namespace: String,
    pub key: String,
    pub value: String,
    pub source_task_id: Option<String>,
    pub source_run_id: Option<String>,
    #[serde(default = "default_memory_confidence")]
    pub confidence: f64,
}

fn default_memory_confidence() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMemoryInput {
    pub project_id: String,
    pub query: String,
    pub namespace: Option<String>,
    pub current_run_id: Option<String>,
    #[serde(default = "default_memory_search_limit")]
    pub limit: u32,
}

fn default_memory_search_limit() -> u32 {
    20
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchKnowledgeInput {
    #[serde(default)]
    pub offset: u32,
    pub project_id: String,
    pub query: String,
    #[serde(default)]
    pub source_types: Vec<String>,
    pub task_id: Option<String>,
    #[serde(default = "default_knowledge_search_limit")]
    pub limit: u32,
}

fn default_knowledge_search_limit() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchResult {
    pub id: String,
    pub project_id: String,
    pub source_type: String,
    pub source_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub title: String,
    pub snippet: String,
    pub content: String,
    pub source_uri: String,
    pub relevance: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeIndexReceipt {
    pub project_id: String,
    pub document_count: u32,
    pub source_counts: std::collections::BTreeMap<String, u32>,
    pub indexed_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendCollaborationMessageInput {
    pub project_id: String,
    pub task_id: Option<String>,
    pub sender_type: String,
    pub sender_id: Option<String>,
    pub target_agent_id: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishArtifactInput {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub artifact_type: String,
    pub uri: String,
    pub summary: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestApprovalInput {
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub approval_type: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRunInput {
    pub run_id: String,
    #[serde(default = "default_heartbeat_lease_seconds")]
    pub lease_seconds: u64,
}

fn default_heartbeat_lease_seconds() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatReceipt {
    pub run_id: String,
    pub lease_expires_at: i64,
    pub heartbeat_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExecutionReceipt {
    pub project_id: String,
    pub accepted: bool,
    pub message: String,
}

#[cfg(test)]
mod serialization_tests {
    use super::AgentRuntimeKind;

    #[test]
    fn opencode_uses_the_public_runtime_protocol_name() {
        assert_eq!(
            serde_json::to_string(&AgentRuntimeKind::OpenCode).unwrap(),
            "\"opencode\""
        );
        assert_eq!(
            serde_json::from_str::<AgentRuntimeKind>("\"opencode\"").unwrap(),
            AgentRuntimeKind::OpenCode
        );
    }

    #[test]
    fn oh_my_pi_uses_the_public_runtime_protocol_name() {
        assert_eq!(
            serde_json::to_string(&AgentRuntimeKind::OhMyPi).unwrap(),
            "\"oh_my_pi\""
        );
        assert_eq!(
            serde_json::from_str::<AgentRuntimeKind>("\"oh_my_pi\"").unwrap(),
            AgentRuntimeKind::OhMyPi
        );
    }

    #[test]
    fn removed_cli_keeps_only_its_legacy_serialization_identity() {
        assert_eq!(
            serde_json::to_string(&AgentRuntimeKind::MiniMaxCode).unwrap(),
            "\"minimax_code\""
        );
        assert_eq!(
            serde_json::from_str::<AgentRuntimeKind>("\"minimax_code\"").unwrap(),
            AgentRuntimeKind::MiniMaxCode
        );
        assert!(AgentRuntimeKind::MiniMaxCode.is_retired());
        assert!(AgentRuntimeKind::MiniMaxCode.ensure_available().is_err());
    }

    #[test]
    fn zcode_uses_the_public_runtime_protocol_name() {
        assert_eq!(
            serde_json::to_value(AgentRuntimeKind::ZCode).unwrap(),
            serde_json::json!("zcode")
        );
        assert_eq!(
            serde_json::from_value::<AgentRuntimeKind>(serde_json::json!("zcode")).unwrap(),
            AgentRuntimeKind::ZCode
        );
        // 兼容修复前后端已经写入的历史值。
        assert_eq!(
            serde_json::from_value::<AgentRuntimeKind>(serde_json::json!("z_code")).unwrap(),
            AgentRuntimeKind::ZCode
        );
        assert_eq!(AgentRuntimeKind::ZCode.as_str(), "zcode");
    }

    #[test]
    fn zcode_cli_uses_the_public_runtime_protocol_name() {
        assert_eq!(
            serde_json::to_value(AgentRuntimeKind::ZCodeCli).unwrap(),
            serde_json::json!("zcode_cli")
        );
        assert_eq!(
            serde_json::from_value::<AgentRuntimeKind>(serde_json::json!("zcode_cli")).unwrap(),
            AgentRuntimeKind::ZCodeCli
        );
        assert_eq!(
            serde_json::from_value::<AgentRuntimeKind>(serde_json::json!("z_code_cli")).unwrap(),
            AgentRuntimeKind::ZCodeCli
        );
        assert_eq!(AgentRuntimeKind::ZCodeCli.as_str(), "zcode_cli");
    }

    #[test]
    fn unknown_runtime_is_a_non_executable_historical_sentinel() {
        let kind: AgentRuntimeKind = serde_json::from_value(serde_json::json!("removed_runtime")).unwrap();
        assert_eq!(kind, AgentRuntimeKind::Unsupported);
        assert_eq!(kind.as_str(), "unsupported");
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!("unsupported"));
        assert!(kind.is_retired());
        assert!(kind.ensure_available().is_err());
    }

    #[test]
    fn all_runtime_names_match_the_public_protocol() {
        for kind in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::MiniMaxCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OhMyPi,
            AgentRuntimeKind::OpenCode,
            AgentRuntimeKind::DeepSeekHarness,
            AgentRuntimeKind::ZCode,
            AgentRuntimeKind::ZCodeCli,
        ] {
            let name = serde_json::json!(kind.as_str());
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<AgentRuntimeKind>(name).unwrap(),
                kind
            );
        }
        assert_eq!(
            serde_json::from_value::<AgentRuntimeKind>(serde_json::json!("deep_seek_harness"))
                .unwrap(),
            AgentRuntimeKind::DeepSeekHarness
        );
    }
}

/// Built-in original member avatars; no external URLs or file paths.
pub const AGENT_AVATAR_IDS: &[&str] = &[
    "orbit",
    "block",
    "sprout",
    "ripple",
    "prism",
    "compass",
    "messenger",
    "beans",
    "crescent",
    "spark",
    "magnet",
    "cloud",
];
pub fn validate_agent_avatar(id: Option<&str>) -> anyhow::Result<()> {
    if id.is_some_and(|value| !AGENT_AVATAR_IDS.contains(&value)) {
        return Err(anyhow::anyhow!(
            "未知成员头像，请选择内置头像或 Runtime 默认图标"
        ));
    }
    Ok(())
}
