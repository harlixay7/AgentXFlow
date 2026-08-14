use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorError {
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Session expired or invalid: {0}")]
    SessionExpired(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Agent {agent_id} not registered")]
    AgentNotRegistered { agent_id: String },

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Task ownership violation: Task {task_id} is owned by agent {owner_id}, not {caller_id}")]
    TaskOwnershipViolation {
        task_id: String,
        owner_id: String,
        caller_id: String,
    },

    #[error("Invalid state transition from {from} to {to}: {reason}")]
    InvalidStateTransition {
        from: String,
        to: String,
        reason: String,
    },

    #[error("Scope collision: requested patterns {requested:?} conflict with active leases held by {conflicting_agent_id}")]
    ScopeCollision {
        requested: Vec<String>,
        conflicting_agent_id: String,
        conflicting_patterns: Vec<String>,
    },

    #[error("Scope violation: unreserved mutations detected: {violations:?}")]
    ScopeViolation { violations: Vec<String> },

    #[error("Worktree is dirty or contains uncommitted changes at {path}: {details:?}")]
    DirtyWorktree {
        path: String,
        details: Vec<String>,
    },

    #[error("Verification check '{check}' failed with exit code {exit_code}: {details}")]
    VerificationFailed {
        check: String,
        exit_code: i32,
        details: String,
    },

    #[error("Verification is stale: worktree HEAD moved from verified {verified_sha} to current {current_sha}")]
    StaleVerification {
        verified_sha: String,
        current_sha: String,
    },

    #[error("Target branch '{target_branch}' has moved from base {base_sha} to {current_sha}. Merge candidate is STALE.")]
    StaleTargetBranch {
        target_branch: String,
        base_sha: String,
        current_sha: String,
    },

    #[error("Merge conflict in target branch '{target_branch}' for task '{task_id}': {details:?}")]
    MergeConflict {
        task_id: String,
        target_branch: String,
        details: Vec<String>,
    },

    #[error("Post-merge integration test failed with exit code {exit_code}: {details}")]
    PostMergeVerificationFailed {
        exit_code: i32,
        details: String,
    },

    #[error("Acceptance criterion '{criterion}' is incomplete or unsatisfied")]
    UnsatisfiedCriterion { criterion: String },

    #[error("Database error: {0}")]
    Database(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Policy rejection: {0}")]
    Policy(String),

    #[error("Masterplan error: {0}")]
    Masterplan(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

impl From<rusqlite::Error> for CoordinatorError {
    fn from(err: rusqlite::Error) -> Self {
        CoordinatorError::Database(err.to_string())
    }
}

impl From<std::io::Error> for CoordinatorError {
    fn from(err: std::io::Error) -> Self {
        CoordinatorError::Io(err.to_string())
    }
}
