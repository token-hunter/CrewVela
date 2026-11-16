import type { MultiAgentTask } from "./types";

/** UI task target; a new request ID allows repeated navigation to the same project. */
export interface CollaborationTasksRequest { projectId: string; requestId: number; }

/** Matches multi_agent/conversation.rs: discussion turns are internal executor tasks. */
export function visibleProjectTasks<T extends Pick<MultiAgentTask, "id">>(tasks: readonly T[]): T[] {
  return tasks.filter(task => !task.id.startsWith("cli-message-conversation-"));
}
