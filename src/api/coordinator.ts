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
} from '../types';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const coordinatorApi = {
  async pickFolder(): Promise<string | null> {
    if (!isTauri) return 'b:/AgentXFlow';
    return await invoke('pick_folder');
  },

  async inspectRepository(path: string): Promise<RepoInspectionResult> {
    if (!isTauri) {
      return {
        is_git_repo: true,
        active_branch: 'main',
        remote_url: 'https://github.com/agentxflow/engine.git',
        languages: ['Rust', 'TypeScript'],
        package_managers: ['Cargo', 'npm'],
        build_scripts: ['cargo build', 'npm run build'],
        test_scripts: ['cargo test', 'npm test'],
        lint_scripts: ['cargo clippy', 'npm run lint'],
        has_ci: true,
        has_instruction_file: true,
      };
    }
    return await invoke('inspect_repository', { path });
  },

  async getMcpInfo(): Promise<McpInfo> {
    if (!isTauri) {
      return {
        url: 'http://127.0.0.1:7890/mcp',
        sse_url: 'http://127.0.0.1:7890/mcp/sse',
        token: 'axf_sec_v2_live_token_7890',
        protocol_version: '2026-07-28',
      };
    }
    return await invoke('get_mcp_info');
  },

  async listProjects(): Promise<Project[]> {
    if (!isTauri) {
      return [
        {
          id: 'proj-agentxflow-v2',
          name: 'AgentXFlow V2 Engine',
          path: 'b:/AgentXFlow',
          master_spec: 'Authoritative Multi-Agent Software Engineering Control Plane',
          target_branch: 'main',
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ];
    }
    return await invoke('list_projects');
  },

  async createProject(
    name: string,
    path: string,
    master_spec: string,
    target_branch: string = 'main'
  ): Promise<Project> {
    if (!isTauri) {
      return {
        id: `proj-${Date.now()}`,
        name,
        path,
        master_spec,
        target_branch,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
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

  async enqueueTaskForMerge(
    projectId: string,
    taskId: string,
    branchName: string,
    targetBranch: string,
    baseSha: string,
    headSha: string
  ): Promise<MergeQueueItem> {
    return await invoke('enqueue_task_for_merge', { projectId, taskId, branchName, targetBranch, baseSha, headSha });
  },

  async processMergeCandidate(projectId: string, item: MergeQueueItem): Promise<IntegrationAttempt> {
    return await invoke('process_merge_candidate', { projectId, item });
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

  async getTaskDetails(taskId: string): Promise<import('../types').TaskDetails> {
    return await invoke('get_task_details', { taskId });
  },

  async rotateMcpToken(): Promise<string> {
    return await invoke('rotate_mcp_token');
  },

  async createExampleProject(rootDir: string): Promise<Project> {
    return await invoke('create_example_project', { rootDir });
  },

  async processMergeById(projectId: string, queueItemId: string): Promise<IntegrationAttempt> {
    return await invoke('process_merge_by_id', { projectId, queueItemId });
  },

  async listAgents(): Promise<Agent[]> {
    if (!isTauri) return [];
    return await invoke('list_agents');
  },
};
