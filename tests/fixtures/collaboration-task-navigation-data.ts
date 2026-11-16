// Explicit local data for the real App routing test; never talks to an Agent.
const clock = 1794795600000;
export const navigationProjects = ["A", "B"].map(id => ({ id, name: `协作项目 ${id}`, rootPath: `C:/fixture/${id}`, goal: "核对导航", defaultBranch: "main", status: "active", createdAt: clock, updatedAt: clock }));
export function navigationSnapshot(projectId: string) {
  const project = navigationProjects.find(project => project.id === projectId);
  if (!project) throw Error(`Missing fixture project: ${projectId}`);
  return {
    project, agents: [{id:`agent-${projectId}`,projectId,name:`成员 ${projectId}`,runtimeKind:"codex",role:"worker",enabled:true,model:null,permissionProfile:"default",maxConcurrency:1,capabilities:[],createdAt:clock,updatedAt:clock}],
    tasks: [{id:`task-${projectId}`,projectId,parentId:null,stage:0,title:`工程任务 ${projectId}`,description:"导航验证",status:"done",assigneeAgentId:`agent-${projectId}`,reviewerAgentId:null,workspaceMode:"read_only",acceptanceCriteria:"可定位任务",verificationCommand:null,priority:0,requiredCapabilities:[],maxAttempts:2,retryBackoffSeconds:1,nextRetryAt:null,createdAt:clock,updatedAt:clock}],
    runs:[],messages:[],events:[],dependencies:[],agentSessions:[],runLogs:[],runtimeEvents:[],memories:[],instructions:[],artifacts:[],approvals:[],reviewDecisions:[],wakeups:[],
  };
}
export function navigationConversation(projectId:string) {
  return {conversation:{id:`conversation-${projectId}`,projectId,createdAt:clock},messages:[],rounds:[],executionPlans:[],supportedDiscussionRuntimes:["codex"]};
}
