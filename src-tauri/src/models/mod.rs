use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    Backlog,
    Ready,
    Running,
    Verifying,
    Verified,
    Blocked,
    Review,
    MergeReady,
    Done,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Backlog => "BACKLOG",
            TaskState::Ready => "READY",
            TaskState::Running => "RUNNING",
            TaskState::Verifying => "VERIFYING",
            TaskState::Verified => "VERIFIED",
            TaskState::Blocked => "BLOCKED",
            TaskState::Review => "REVIEW",
            TaskState::MergeReady => "MERGE_READY",
            TaskState::Done => "DONE",
            TaskState::Failed => "FAILED",
            TaskState::Cancelled => "CANCELLED",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "BACKLOG" => TaskState::Backlog,
            "READY" => TaskState::Ready,
            "RUNNING" | "WORKING" | "CLAIMED" | "ANALYZING" | "SCOPE_APPROVED" => TaskState::Running,
            "VERIFYING" => TaskState::Verifying,
            "VERIFIED" => TaskState::Verified,
            "BLOCKED" => TaskState::Blocked,
            "REVIEW" => TaskState::Review,
            "MERGE_READY" => TaskState::MergeReady,
            "DONE" => TaskState::Done,
            "FAILED" => TaskState::Failed,
            "CANCELLED" => TaskState::Cancelled,
            _ => TaskState::Backlog,
        }
    }

    pub fn can_transition_to(&self, next: &TaskState) -> bool {
        match (self, next) {
            (TaskState::Backlog, TaskState::Ready) => true,
            (TaskState::Ready, TaskState::Running) => true,
            (TaskState::Running, TaskState::Verifying) => true,
            (TaskState::Verifying, TaskState::Verified) => true,
            (TaskState::Verified, TaskState::MergeReady) => true,
            (TaskState::Running, TaskState::Review) => true,
            (TaskState::Running, TaskState::MergeReady) => true,
            (TaskState::Review, TaskState::MergeReady) => true,
            (TaskState::Review, TaskState::Running) => true, // Re-work
            (TaskState::MergeReady, TaskState::Done) => true,
            (TaskState::MergeReady, TaskState::Blocked) => true, // Conflict
            (TaskState::Verifying, TaskState::Failed) => true,
            (TaskState::Verifying, TaskState::Blocked) => true,
            (TaskState::Failed, TaskState::Running) => true, // Retry
            (TaskState::Failed, TaskState::Ready) => true,
            (TaskState::Blocked, TaskState::Ready) => true,
            (TaskState::Blocked, TaskState::Running) => true,
            (_, TaskState::Blocked) => true,
            (_, TaskState::Failed) => true,
            (_, TaskState::Cancelled) => true,
            (TaskState::Running, TaskState::Ready) => true, // Unclaim
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskSubstate {
    Claiming,
    Analyzing,
    AcquiringScope,
    Implementing,
    Verifying,
    WaitingForInput,
    None,
}

impl TaskSubstate {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskSubstate::Claiming => "CLAIMING",
            TaskSubstate::Analyzing => "ANALYZING",
            TaskSubstate::AcquiringScope => "ACQUIRING_SCOPE",
            TaskSubstate::Implementing => "IMPLEMENTING",
            TaskSubstate::Verifying => "VERIFYING",
            TaskSubstate::WaitingForInput => "WAITING_FOR_INPUT",
            TaskSubstate::None => "NONE",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "CLAIMING" => TaskSubstate::Claiming,
            "ANALYZING" => TaskSubstate::Analyzing,
            "ACQUIRING_SCOPE" => TaskSubstate::AcquiringScope,
            "IMPLEMENTING" => TaskSubstate::Implementing,
            "VERIFYING" => TaskSubstate::Verifying,
            "WAITING_FOR_INPUT" => TaskSubstate::WaitingForInput,
            _ => TaskSubstate::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub master_spec: String,
    pub target_branch: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContract {
    pub id: String,
    pub project_id: String,
    pub version: i32,
    pub overview: String,
    pub architecture: String,
    pub rules_json: String,
    pub commands_json: String,
    pub testing_json: String,
    pub repo_map: String,
    pub security_constraints: String,
    pub contract_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRule {
    pub id: String,
    pub project_id: String,
    pub category: String,
    pub rule_text: String,
    pub strictness: String, // MANDATORY, ADVISORY
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub id: String,
    pub project_id: String,
    pub memory_type: String, // DECISION, PITFALL, CONVENTION, REPO_FACT
    pub content: String,
    pub source_task_id: Option<String>,
    pub confidence: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub epic_id: Option<String>,
    pub title: String,
    pub description: String,
    pub state: TaskState,
    pub substate: TaskSubstate,
    pub assigned_agent_id: Option<String>,
    pub priority: String, // LOW, MEDIUM, HIGH, CRITICAL
    pub risk_score: f64,
    pub estimated_scope: Option<String>,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependency {
    pub id: String,
    pub task_id: String,
    pub depends_on_task_id: String,
    pub dependency_type: String, // BLOCKS, RELATED_TO, PARENT_CHILD
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub id: String,
    pub task_id: String,
    pub order_index: i32,
    pub title: String,
    pub description: String,
    pub is_mandatory: bool,
    pub status: String, // PENDING, COMPLETED, FAILED
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriteria {
    pub id: String,
    pub task_id: String,
    pub criterion: String,
    pub is_satisfied: bool,
    pub is_locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilitySet {
    pub read_files: bool,
    pub write_files: bool,
    pub terminal: bool,
    pub streaming: bool,
    pub steering: bool,
    pub interrupt: bool,
    pub resume: bool,
    pub fork: bool,
    pub subagents: bool,
    pub permissions: bool,
    pub usage: bool,
    pub mcp: bool,
}

impl Default for AgentCapabilitySet {
    fn default() -> Self {
        Self {
            read_files: true,
            write_files: true,
            terminal: true,
            streaming: true,
            steering: true,
            interrupt: true,
            resume: true,
            fork: false,
            subagents: true,
            permissions: true,
            usage: true,
            mcp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub agent_type: String, // Codex, OpenCode, Claude, Antigravity, Generic
    pub profile: String,    // Planner, Implementer, Reviewer, Tester, Security, MergeResolver
    pub status: String,     // IDLE, WORKING, BLOCKED, DISCONNECTED
    pub capabilities: AgentCapabilitySet,
    pub last_heartbeat: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub role_description: String,
    pub preferred_agent_type: String,
    pub required_capabilities: Vec<String>,
    pub permission_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub parent_run_id: Option<String>,
    pub role: String,
    pub prompt: String,
    pub status: String, // ACTIVE, PAUSED, COMPLETED, FAILED
    pub started_at: String,
    pub finished_at: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub agent_id: String,
    pub session_token: String,
    pub created_at: String,
    pub expires_at: String,
    pub last_activity_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPermissionRequest {
    pub id: String,
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub action_type: String, // COMMAND, FILE_WRITE, NETWORK, GIT_MUTATION
    pub target: String,
    pub reason: String,
    pub status: String, // PENDING, APPROVED, DENIED
    pub requested_at: String,
    pub responded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeLease {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub pattern: String,
    pub access_type: String, // READ, EXCLUSIVE_WRITE, SHARED
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeViolation {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub file_path: String,
    pub violation_type: String, // UNRESERVED_WRITE, OVERLAPPING_WRITE
    pub detected_at: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionRisk {
    pub task_a_id: String,
    pub task_b_id: String,
    pub risk_score: f64,
    pub overlapping_patterns: Vec<String>,
    pub semantic_risk_factors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub worktree_path: String,
    pub branch_name: String,
    pub base_sha: String,
    pub is_integration: bool,
    pub is_healthy: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub timeout_seconds: i32,
    pub is_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRun {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub check_id: String,
    pub check_name: String,
    pub commit_sha: String,
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
    pub is_passed: bool,
    pub is_stale: bool,
    pub executed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBundle {
    pub task_id: String,
    pub project_id: String,
    pub agent_id: Option<String>,
    pub prompt: String,
    pub base_sha: String,
    pub head_sha: String,
    pub files_changed: Vec<String>,
    pub diff_summary: String,
    pub verification_runs: Vec<VerificationRun>,
    pub scope_violations: Vec<ScopeViolation>,
    pub proof_hash: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueueItem {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub branch_name: String,
    pub target_branch: String,
    pub position: i32,
    pub status: String, // READY, STALE, RUNNING_CHECKS, BLOCKED_CONFLICT, MERGED, FAILED
    pub base_sha: String,
    pub head_sha: String,
    pub queued_at: String,
    pub processed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationAttempt {
    pub id: String,
    pub merge_queue_id: String,
    pub simulation_passed: bool,
    pub conflicts_json: Option<String>,
    pub post_merge_verification_passed: bool,
    pub merge_strategy: String, // SQUASH, MERGE_COMMIT, REBASE
    pub target_sha_before: String,
    pub target_sha_after: Option<String>,
    pub attempted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventItem {
    pub sequence: i64,
    pub event_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub event_type: String,
    pub payload_json: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: String,
    pub project_id: String,
    pub hook: String, // BeforeTask, BeforeTool, BeforeMutation, BeforeVerify, BeforeMerge
    pub condition_pattern: String,
    pub action: String, // ALLOW, DENY, REQUIRE_APPROVAL
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub is_valid: bool,
    pub missing_mandatory_steps: Vec<String>,
    pub missing_evidence_step_ids: Vec<String>,
    pub unresolved_scope_violations: Vec<String>,
    pub failed_coordinator_checks: Vec<String>,
    pub rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub project_id: String,
    pub project_name: String,
    pub contract_hash: String,
    pub contract_overview: String,
    pub project_rules: Vec<String>,
    pub project_memory: Vec<String>,
    pub task_id: String,
    pub task_title: String,
    pub task_prompt: String,
    pub task_state: String,
    pub task_substate: String,
    pub acceptance_criteria: Vec<AcceptanceCriteria>,
    pub required_steps: Vec<TaskStep>,
    pub dependencies: Vec<String>,
    pub reserved_scope: Vec<ScopeLease>,
    pub current_worktree: Option<String>,
    pub current_branch: Option<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Masterplan {
    pub id: String,
    pub project_id: String,
    pub raw_text: String,
    pub status: String, // UNSORTED, RESORTED, EXECUTING, COMPLETED
    pub target_step_count: i32,
    pub max_steps_per_agent: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterplanStep {
    pub id: String,
    pub masterplan_id: String,
    pub step_index: i32,
    pub title: String,
    pub description: String,
    pub suggested_scope: String,
    pub acceptance_criteria: String,
    pub status: String, // PENDING, CLAIMED, IN_PROGRESS, COMPLETED
    pub claimed_agent_id: Option<String>,
    pub claimed_task_id: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposedStepInput {
    pub step_index: i32,
    pub title: String,
    pub description: String,
    pub suggested_scope: Option<String>,
    pub acceptance_criteria: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub evidence_type: String, // COMMAND_EXECUTION, TEST_RESULT, BUILD_RESULT, GIT_DIFF, FILE_CHANGE, USER_APPROVAL, AGENT_NOTE
    pub source: String,        // COORDINATOR_OBSERVED, AGENT_REPORTED
    pub payload_json: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub attempt_number: i32,
    pub base_sha: String,
    pub head_sha: Option<String>,
    pub status: String, // ACTIVE, VERIFYING, VERIFIED, FAILED, ABORTED, SUPERSEDED
    pub rejection_reasons: Option<String>, // JSON array of reasons
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorResult {
    pub id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub criterion_id: Option<String>,
    pub evaluator_name: String,
    pub evaluator_type: String, // COMMAND, SCOPE_AUDIT, TEST, LINT, BUILD, ACCEPTANCE
    pub evaluator_version: String,
    pub commit_sha: String,
    pub exit_code: i32,
    pub stdout_output: String,
    pub stderr_output: String,
    pub output_sha256: String,
    pub duration_ms: i64,
    pub passed: bool,
    pub evaluated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProfile {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub check_type: String, // FORMAT, TYPECHECK, UNIT_TESTS, INTEGRATION_TESTS, BUILD, SCOPE_AUDIT, ACCEPTANCE
    pub command: String,
    pub args_json: String,
    pub timeout_secs: i32,
    pub required: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedMasterplanSnapshot {
    pub masterplan: Masterplan,
    pub steps: Vec<MasterplanStep>,
    pub total_steps: usize,
    pub target_step_count: i32,
    pub max_steps_per_agent: i32,
    pub handoff_prompt: String,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDetails {
    pub task: Task,
    pub steps: Vec<TaskStep>,
    pub criteria: Vec<AcceptanceCriteria>,
    pub leases: Vec<ScopeLease>,
    pub dependencies: Vec<TaskDependency>,
    pub verification_runs: Vec<VerificationRun>,
    pub violations: Vec<ScopeViolation>,
    pub evidence_records: Vec<EvidenceRecord>,
    pub proof_bundle: Option<ProofBundle>,
    pub assigned_agent: Option<Agent>,
    pub active_attempt: Option<TaskAttempt>,
    pub evaluator_results: Vec<EvaluatorResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterplanSummary {
    pub project_id: String,
    pub project_name: String,
    pub repository_path: String,
    pub masterplan_id: String,
    pub status: String,
    pub target_step_count: i32,
    pub max_steps_per_agent: i32,
    pub total_steps: usize,
    pub pending_steps: usize,
    pub claimed_steps: usize,
    pub completed_steps: usize,
    pub last_updated: String,
    pub next_action: String,
    pub handoff_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentContext {
    pub active_project_id: Option<String>,
    pub project_name: Option<String>,
    pub repository_path: Option<String>,
    pub masterplan_id: Option<String>,
    pub masterplan_status: Option<String>,
    pub masterplan_revision: Option<i32>,
    pub caller_agent_id: Option<String>,
    pub active_task_id: Option<String>,
    pub active_attempt_id: Option<String>,
    pub active_scopes: Vec<String>,
    pub current_state: Option<String>,
    pub last_updated: Option<String>,
    pub next_recommended_action: String,
    pub handoff_prompt: String,
    pub active_agents_count: usize,
    pub pending_tasks_count: usize,
    pub instructions: String,
}



