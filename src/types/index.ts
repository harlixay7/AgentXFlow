export type TaskState =
  | 'BACKLOG'
  | 'READY'
  | 'RUNNING'
  | 'BLOCKED'
  | 'REVIEW'
  | 'MERGE_READY'
  | 'DONE'
  | 'FAILED'
  | 'CANCELLED';

export type TaskSubstate =
  | 'CLAIMING'
  | 'ANALYZING'
  | 'ACQUIRING_SCOPE'
  | 'IMPLEMENTING'
  | 'VERIFYING'
  | 'WAITING_FOR_INPUT'
  | 'NONE';

export interface Project {
  id: string;
  name: string;
  path: string;
  master_spec: string;
  target_branch: string;
  created_at: string;
  updated_at: string;
}

export interface ProjectContract {
  id: string;
  project_id: string;
  version: number;
  overview: string;
  architecture: string;
  rules_json: string;
  commands_json: string;
  testing_json: string;
  repo_map: string;
  security_constraints: string;
  contract_hash: string;
  created_at: string;
}

export interface Task {
  id: string;
  project_id: string;
  parent_id: string | null;
  epic_id: string | null;
  title: string;
  description: string;
  state: TaskState;
  substate: TaskSubstate;
  assigned_agent_id: string | null;
  priority: 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';
  risk_score: number;
  estimated_scope: string | null;
  worktree_path: string | null;
  branch_name: string | null;
  base_sha: string | null;
  head_sha: string | null;
  masterplan_id?: string | null;
  masterplan_revision_id?: string | null;
  is_stale?: boolean;
  created_at: string;
  updated_at: string;
}

export interface TaskDependency {
  id: string;
  task_id: string;
  depends_on_task_id: string;
  dependency_type: 'BLOCKS' | 'RELATED_TO' | 'PARENT_CHILD';
  created_at: string;
}

export interface TaskStep {
  id: string;
  task_id: string;
  order_index: number;
  title: string;
  description: string;
  is_mandatory: boolean;
  status: 'PENDING' | 'COMPLETED' | 'FAILED';
  completed_at: string | null;
}

export interface AcceptanceCriteria {
  id: string;
  task_id: string;
  criterion: string;
  is_satisfied: boolean;
  is_locked: boolean;
}

export interface AgentCapabilitySet {
  read_files: boolean;
  write_files: boolean;
  terminal: boolean;
  streaming: boolean;
  steering: boolean;
  interrupt: boolean;
  resume: boolean;
  fork: boolean;
  subagents: boolean;
  permissions: boolean;
  usage: boolean;
  mcp: boolean;
}

export interface Agent {
  id: string;
  name: string;
  agent_type: 'Codex' | 'OpenCode' | 'Claude' | 'Antigravity' | 'Generic';
  profile: 'Planner' | 'Implementer' | 'Reviewer' | 'Tester' | 'Security' | 'MergeResolver';
  status: 'IDLE' | 'WORKING' | 'BLOCKED' | 'DISCONNECTED';
  capabilities: AgentCapabilitySet;
  last_heartbeat: string;
  created_at: string;
}

export interface ScopeLease {
  id: string;
  task_id: string;
  agent_id: string;
  pattern: string;
  access_type: 'READ' | 'EXCLUSIVE_WRITE' | 'SHARED';
  expires_at: string;
  created_at: string;
}

export interface ScopeViolation {
  id: string;
  task_id: string;
  agent_id: string;
  file_path: string;
  violation_type: string;
  detected_at: string;
  resolved: boolean;
}

export interface CollisionRisk {
  task_a_id: string;
  task_b_id: string;
  risk_score: number;
  overlapping_patterns: string[];
  semantic_risk_factors: string[];
}

export interface VerificationRun {
  id: string;
  task_id: string;
  run_id: string | null;
  check_id: string;
  check_name: string;
  commit_sha: string;
  command: string;
  exit_code: number;
  stdout: string;
  stderr: string;
  duration_ms: number;
  is_passed: boolean;
  is_stale: boolean;
  executed_at: string;
}

export interface ProofBundle {
  task_id: string;
  project_id: string;
  agent_id: string | null;
  prompt: string;
  base_sha: string;
  head_sha: string;
  files_changed: string[];
  diff_summary: string;
  verification_runs: VerificationRun[];
  scope_violations: ScopeViolation[];
  proof_hash: string;
  generated_at: string;
}

export interface MergeQueueItem {
  id: string;
  project_id: string;
  task_id: string;
  branch_name: string;
  target_branch: string;
  position: number;
  status: 'READY' | 'STALE' | 'RUNNING_CHECKS' | 'BLOCKED_CONFLICT' | 'MERGED' | 'FAILED';
  base_sha: string;
  head_sha: string;
  queued_at: string;
  processed_at: string | null;
}

export interface IntegrationAttempt {
  id: string;
  merge_queue_id: string;
  simulation_passed: boolean;
  conflicts_json: string | null;
  post_merge_verification_passed: boolean;
  merge_strategy: string;
  target_sha_before: string;
  target_sha_after: string | null;
  attempted_at: string;
}

export interface EventItem {
  sequence: number;
  event_id: string;
  project_id: string | null;
  task_id: string | null;
  agent_id: string | null;
  event_type: string;
  payload_json: string;
  timestamp: string;
}

export interface VerificationResult {
  is_valid: boolean;
  missing_mandatory_steps: string[];
  missing_evidence_step_ids: string[];
  unresolved_scope_violations: string[];
  failed_coordinator_checks: string[];
  rejection_reasons: string[];
}

export interface RepoInspectionResult {
  is_git_repo: boolean;
  active_branch: string | null;
  remote_url: string | null;
  languages: string[];
  package_managers: string[];
  build_scripts: string[];
  test_scripts: string[];
  lint_scripts: string[];
  has_ci: boolean;
  has_instruction_file: boolean;
}

export interface ContextPack {
  project_id: string;
  project_name: string;
  contract_hash: string;
  contract_overview: string;
  project_rules: string[];
  project_memory: string[];
  task_id: string;
  task_title: string;
  task_prompt: string;
  task_state: string;
  task_substate: string;
  acceptance_criteria: AcceptanceCriteria[];
  required_steps: TaskStep[];
  dependencies: string[];
  reserved_scope: ScopeLease[];
  current_worktree: string | null;
  current_branch: string | null;
  base_sha: string | null;
  head_sha: string | null;
}

export interface EventLog {
  id: string;
  task_id: string | null;
  agent_id: string | null;
  event_type: string;
  payload: string;
  created_at: string;
}

export interface McpInfo {
  url: string;
  sse_url: string;
  token: string;
  protocol_version: string;
}

export interface Masterplan {
  id: string;
  project_id: string;
  title: string;
  raw_text: string;
  status: 'UNSORTED' | 'RESORTED' | 'EXECUTING' | 'COMPLETED';
  target_step_count: number;
  max_steps_per_agent: number;
  require_milestone_approval?: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface MasterplanStep {
  id: string;
  masterplan_id: string;
  step_index: number;
  title: string;
  description: string;
  suggested_scope: string;
  acceptance_criteria: string;
  status: 'PENDING' | 'CLAIMED' | 'IN_PROGRESS' | 'COMPLETED';
  claimed_agent_id: string | null;
  claimed_task_id: string | null;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface DecomposedStepInput {
  step_index: number;
  title: string;
  description: string;
  suggested_scope?: string;
  acceptance_criteria?: string;
}

export interface EvidenceRecord {
  id: string;
  task_id: string;
  step_id: string | null;
  evidence_type: string;
  source: string;
  payload_json: string;
  recorded_at: string;
}

export interface TaskAttempt {
  id: string;
  task_id: string;
  agent_id: string;
  attempt_number: number;
  base_sha: string;
  head_sha: string | null;
  status: 'ACTIVE' | 'VERIFYING' | 'VERIFIED' | 'FAILED' | 'ABORTED' | 'SUPERSEDED';
  rejection_reasons: string | null;
  started_at: string;
  finished_at: string | null;
}

export interface EvaluatorResult {
  id: string;
  task_id: string;
  attempt_id: string;
  criterion_id: string | null;
  evaluator_name: string;
  evaluator_type: string;
  evaluator_version: string;
  commit_sha: string;
  exit_code: number;
  stdout_output: string;
  stderr_output: string;
  output_sha256: string;
  duration_ms: number;
  passed: boolean;
  evaluated_at: string;
}

export interface VerificationProfile {
  id: string;
  project_id: string;
  task_id: string | null;
  check_type: string;
  command: string;
  args_json: string;
  timeout_secs: number;
  required: boolean;
  created_at: string;
}

export interface PreparedMasterplanSnapshot {
  masterplan: Masterplan;
  steps: MasterplanStep[];
  total_steps: number;
  target_step_count: number;
  max_steps_per_agent: number;
  handoff_prompt: string;
  next_action: string;
}

export interface TaskDetails {
  task: Task;
  steps: TaskStep[];
  criteria: AcceptanceCriteria[];
  leases: ScopeLease[];
  verification_runs: VerificationRun[];
  violations: ScopeViolation[];
  proof_bundle: ProofBundle | null;
  evidence_records: EvidenceRecord[];
  active_attempt?: TaskAttempt | null;
  evaluator_results?: EvaluatorResult[];
}

export interface MasterplanSummary {
  project_id: string;
  project_name: string;
  repository_path: string;
  masterplan_id: string;
  title: string;
  is_active: boolean;
  status: string;
  target_step_count: number;
  max_steps_per_agent: number;
  total_steps: number;
  pending_steps: number;
  claimed_steps: number;
  completed_steps: number;
  last_updated: string;
  next_action: string;
  handoff_prompt: string;
}

export interface CurrentContext {
  active_project_id: string | null;
  project_name: string | null;
  repository_path: string | null;
  masterplan_id: string | null;
  masterplan_status: string | null;
  masterplan_revision?: number | null;
  caller_agent_id?: string | null;
  active_task_id?: string | null;
  active_attempt_id?: string | null;
  active_scopes?: string[];
  current_state?: string | null;
  last_updated: string | null;
  next_recommended_action: string;
  handoff_prompt: string;
  active_agents_count: number;
  pending_tasks_count: number;
  instructions: string;
}


