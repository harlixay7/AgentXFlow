import { invoke } from '@tauri-apps/api/core';
import {
  Project,
  Task,
  TaskStep,
  Agent,
  ScopeLease,
  CollisionRisk,
  TaskDependency,
  MergeQueueItem,
  IntegrationAttempt,
  EventItem,
  VerificationResult,
  RepoInspectionResult,
  McpInfo,
  ContextPack,
  TaskDetails,
} from '../types';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const coordinatorApi = {
  async pickFolder(): Promise<string | null> {
    if (!isTauri) return null;
    return await invoke('pick_folder');
  },

  async inspectRepository(path: string): Promise<RepoInspectionResult> {
    if (!isTauri) {
      throw new Error('Coordinator backend is available only inside the AgentXFlow desktop app.');
    }
    return await invoke('inspect_repository', { path });
  },

  async getMcpInfo(): Promise<McpInfo> {
    if (!isTauri) {
      throw new Error('Coordinator backend is available only inside the AgentXFlow desktop app.');
    }
    return await invoke('get_mcp_info');
  },

  async listProjects(): Promise<Project[]> {
    if (!isTauri) return [];
    return await invoke('list_projects');
  },

  async createProject(
    name: string,
    path: string,
    master_spec: string,
    target_branch: string = 'main'
  ): Promise<Project> {
    if (!isTauri) {
      throw new Error('Coordinator backend is available only inside the AgentXFlow desktop app.');
    }
    return await invoke('create_project', { name, path, masterSpec: master_spec, targetBranch: target_branch });
  },

  async listTasks(projectId: string): Promise<Task[]> {
    if (!isTauri) return [];
    return await invoke('list_tasks', { projectId });
  },

  async getTask(taskId: string): Promise<Task> {
    return await invoke('get_task', { taskId });
  },

  async createTask(
    projectId: string,
    title: string,
    description: string,
    priority: string = 'MEDIUM',
    steps: [string, string, boolean][] = [],
    criteria: string[] = []
  ): Promise<Task> {
    return await invoke('create_task', { projectId, title, description, priority, steps, criteria });
  },

  async claimTask(taskId: string, agentId: string): Promise<Task> {
    return await invoke('claim_task', { taskId, agentId });
  },

  async completeStep(stepId: string, evidenceJson?: string): Promise<TaskStep> {
    return await invoke('complete_step', { stepId, evidenceJson });
  },

  async submitTask(taskId: string, agentId: string): Promise<VerificationResult> {
    return await invoke('submit_task', { taskId, agentId });
  },

  async requestScope(taskId: string, agentId: string, patterns: string[]): Promise<ScopeLease[]> {
    return await invoke('request_scope', { taskId, agentId, patterns });
  },

  async calculateCollisionRisk(taskAId: string, taskBId: string): Promise<CollisionRisk> {
    return await invoke('calculate_collision_risk', { taskAId, taskBId });
  },

  async addTaskDependency(taskId: string, dependsOnTaskId: string, dependencyType: string = 'BLOCKS'): Promise<TaskDependency> {
    return await invoke('add_task_dependency', { taskId, dependsOnTaskId, dependencyType });
  },

  async getTaskDependencies(taskId: string): Promise<TaskDependency[]> {
    return await invoke('get_task_dependencies', { taskId });
  },

  async listMergeQueue(projectId: string): Promise<MergeQueueItem[]> {
    if (!isTauri) return [];
    return await invoke('list_merge_queue', { projectId });
  },

  async enqueueTaskById(projectId: string, taskId: string): Promise<MergeQueueItem> {
    return await invoke('enqueue_task_by_id', { projectId, taskId });
  },

  async processMergeById(projectId: string, queueItemId: string): Promise<IntegrationAttempt> {
    return await invoke('process_merge_by_id', { projectId, queueItemId });
  },

  async getEventsAfter(lastSequence: number): Promise<EventItem[]> {
    if (!isTauri) return [];
    return await invoke('get_events_after', { lastSequence });
  },

  async getContextPack(projectId: string, taskId: string): Promise<ContextPack> {
    return await invoke('get_context_pack', { projectId, taskId });
  },

  async registerAgent(name: string, agentType: string): Promise<Agent> {
    return await invoke('register_agent', { name, agentType });
  },

  async getTaskDetails(taskId: string): Promise<TaskDetails> {
    return await invoke('get_task_details', { taskId });
  },

  async rotateMcpToken(): Promise<string> {
    return await invoke('rotate_mcp_token');
  },

  async createExampleProject(rootDir: string): Promise<Project> {
    return await invoke('create_example_project', { rootDir });
  },

  async satisfyAcceptanceCriterion(taskId: string, criterionId: string, evidence?: string): Promise<void> {
    return await invoke('satisfy_acceptance_criterion', { taskId, criterionId, evidence });
  },

  async listAgents(): Promise<Agent[]> {
    if (!isTauri) return [];
    return await invoke('list_agents');
  },
};
