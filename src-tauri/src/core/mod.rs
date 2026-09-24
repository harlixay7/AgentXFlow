use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::acp::AcpRuntime;
use crate::dag::DagEngine;
use crate::db::DbPool;
use crate::git::GitService;
use crate::merge::MergeEngine;
use crate::models::{
    AcceptanceCriteria, Agent, AgentCapabilitySet, ContextPack, CurrentContext,
    DecomposedStepInput, EvaluatorResult, EventItem, EvidenceRecord, IntegrationAttempt,
    Masterplan, MasterplanStep, MasterplanSummary, MergeQueueItem, PreparedMasterplanSnapshot,
    Project, ProjectContextPack, ProofBundle, ScopeLease, ScopeViolation, Task, TaskAttempt,
    TaskDependency, TaskDetails, TaskState, TaskStep, TaskSubstate, VerificationResult,
    VerificationRun,
};
use crate::policies::PolicyEngine;
use crate::scheduler::{SchedulerConfig, SchedulerEngine};
use crate::scope::ScopeManager;
use crate::verification::VerificationEngine;

/// Initial session validity window upon registration/re-registration (days).
pub const SESSION_INITIAL_TTL_DAYS: i64 = 365;

/// Sliding session window extended upon active heartbeats (days): every heartbeat
/// pushes agent_sessions.expires_at forward so active agents never expire.
pub const SESSION_HEARTBEAT_SLIDE_DAYS: i64 = 30;

/// Stale-recovery sweep cadence (seconds): the background loops in lib.rs invoke
/// `stale_recovery_sweep` no more often than every SWEEP_INTERVAL.
pub const SWEEP_INTERVAL: u64 = 60;

/// An agent whose `last_heartbeat` is older than this many seconds is treated as
/// disconnected, so its in-flight RUNNING/VERIFYING/CLAIMING tasks are reclaimed
/// by the sweep. Agents heartbeat on every tool call (see mcp/mod.rs).
pub const STALE_AGENT_GRACE: i64 = 300;

/// Bounded protection grace period for agents actively waiting for external IDE
/// or user permission (TaskSubstate::WaitingForInput). Prevents premature reclamation
/// during human code review while ensuring genuinely abandoned tasks are still reclaimed.
pub const WAITING_PERMISSION_GRACE: i64 = 1800;

#[derive(Debug, Clone)]
pub struct CoordinatorEngine {
    pub db: DbPool,
    pub git: GitService,
    pub scope: ScopeManager,
    pub verify: VerificationEngine,
    pub merge: MergeEngine,
    pub dag: DagEngine,
    pub acp: AcpRuntime,
    pub policy: PolicyEngine,
    pub scheduler: SchedulerEngine,
    pub worktrees_root: std::path::PathBuf,
}

/// Startup reconciliation failure accounting: every failed reconcile write is
/// logged with context and aggregated so a partial reconcile is never silent.
#[derive(Default)]
struct ReconcileFailureLog {
    failures: Vec<(String, String)>,
}

impl ReconcileFailureLog {
    fn record(&mut self, category: &str, detail: String) {
        warn!(
            "Startup reconciliation write failure [{}]: {}",
            category, detail
        );
        self.failures.push((category.to_string(), detail));
    }

    fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }

    fn summary(&self) -> serde_json::Value {
        let mut by_category: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for (category, _) in &self.failures {
            *by_category.entry(category.clone()).or_insert(0) += 1;
        }
        json!({
            "total_failures": self.failures.len(),
            "by_category": by_category,
            "failures": self.failures.iter().map(|(_, detail)| detail).collect::<Vec<_>>(),
        })
    }
}

/// Snapshot of the stored proof_bundles row the merge-enqueue gate verifies (D28): every
/// field the canonical proof digest covers (and the persisted proof_hash it must match).
struct StoredProofRow {
    task_id: String,
    project_id: String,
    attempt_id: String,
    base_sha: String,
    head_sha: String,
    files_changed_json: String,
    diff_summary: String,
    verification_runs_json: String,
    criteria_json: String,
    steps_json: String,
    proof_hash: String,
}

impl CoordinatorEngine {
    pub fn new(db: DbPool) -> Self {
        let worktrees_root = dirs_next::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("AgentXFlow")
            .join("worktrees");
        Self::new_with_worktree_root(db, worktrees_root)
    }

    pub fn new_with_worktree_root(db: DbPool, worktrees_root: std::path::PathBuf) -> Self {
        let git = GitService::new();
        let scope = ScopeManager::new(db.clone());
        let verify = VerificationEngine::new(db.clone());
        let merge = MergeEngine::new(db.clone(), git.clone());
        let dag = DagEngine::new(db.clone());
        let acp = AcpRuntime::new(db.clone());
        let policy = PolicyEngine::new(db.clone());
        let scheduler = SchedulerEngine::new(
            db.clone(),
            dag.clone(),
            scope.clone(),
            SchedulerConfig::default(),
        );

        let engine = Self {
            db,
            git,
            scope,
            verify,
            merge,
            dag,
            acp,
            policy,
            scheduler,
            worktrees_root,
        };

        engine.reconcile_on_startup();
        engine
    }

    /// Sequence-numbered event emitter
    pub fn emit_event(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let _ = self.emit_event_checked(project_id, task_id, agent_id, event_type, payload);
    }

    /// Result-returning event emitter for callers that must observe telemetry
    /// write failures (e.g. startup reconciliation). Best-effort by policy:
    /// `emit_event` discards the result.
    fn emit_event_checked(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let conn = self.db.lock();
        let event_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let payload_str = payload.to_string();

        conn.execute(
            "INSERT INTO events (event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event_id, project_id, task_id, agent_id, event_type, payload_str, now],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn get_events_after(&self, last_sequence: i64) -> Result<Vec<EventItem>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT sequence, event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp FROM events WHERE sequence > ?1 ORDER BY sequence ASC LIMIT 500")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([last_sequence], |row| {
                Ok(EventItem {
                    sequence: row.get(0)?,
                    event_id: row.get(1)?,
                    project_id: row.get(2)?,
                    task_id: row.get(3)?,
                    agent_id: row.get(4)?,
                    event_type: row.get(5)?,
                    payload_json: row.get(6)?,
                    timestamp: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let events: Vec<EventItem> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read event records: {}", e))?;
        Ok(events)
    }

    /// Centralized validated task state transition: applies the transition atomically
    /// (transaction), then emits the event. The state change is authoritative: event
    /// emission is best-effort telemetry and never rolls back the committed transition.
    pub fn transition_task_state(
        &self,
        task_id: &str,
        from: &[TaskState],
        to: TaskState,
        agent_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;

        transition_task_state_on_conn(&tx, task_id, from, to)?;

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;

        let project_id: Option<String> = conn
            .query_row(
                "SELECT project_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .ok();
        drop(conn);

        self.emit_event(
            project_id.as_deref(),
            Some(task_id),
            agent_id,
            event_type,
            payload,
        );
        Ok(())
    }

    /// Self-healing startup reconciliation to repair interrupted claims and unclosed integrations.
    /// Every write failure is logged with context and aggregated; if any write failed a
    /// `RECONCILE_PARTIAL` event is emitted after the loop. Startup still proceeds
    /// (availability) but never silently.
    pub fn reconcile_on_startup(&self) {
        info!("Running AgentXFlow startup reconciliation...");
        let mut failures = ReconcileFailureLog::default();

        // 1. Reset tasks left in CLAIMING state back to READY
        let interrupted_tasks: Vec<(String, String, Option<String>)> = {
            let conn = self.db.lock();
            let scanned = match conn
                .prepare("SELECT id, project_id, worktree_path FROM tasks WHERE state = 'CLAIMING'")
            {
                Ok(mut stmt) => match stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
                {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        failures.record(
                            "claiming_task_scan",
                            format!("failed to read interrupted CLAIMING tasks: {}", e),
                        );
                        Vec::new()
                    }
                },
                Err(e) => {
                    failures.record(
                        "claiming_task_scan",
                        format!("could not list interrupted CLAIMING tasks: {}", e),
                    );
                    Vec::new()
                }
            };
            scanned
        };

        for (t_id, p_id, wt_path) in interrupted_tasks {
            let mut cleanup_failed = false;
            if let Some(ref path_str) = wt_path {
                let path = std::path::PathBuf::from(path_str.clone());
                if path.exists() {
                    let repo_path: Option<std::path::PathBuf> = {
                        let conn = self.db.lock();
                        conn.query_row("SELECT path FROM projects WHERE id = ?1", [&p_id], |r| {
                            r.get::<_, String>(0)
                        })
                        .ok()
                        .map(std::path::PathBuf::from)
                    };
                    if let Some(repo_path) = repo_path {
                        let managed_roots =
                            vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
                        if self
                            .git
                            .is_managed_worktree(&repo_path, &managed_roots, &path)
                        {
                            if let Err(e) = crate::git::GitService::safe_remove_dir_all(&path) {
                                failures.record(
                                    "worktree_cleanup",
                                    format!(
                                        "task '{}' (project '{}'): failed to remove managed worktree {:?}: {}",
                                        t_id, p_id, path, e
                                    ),
                                );
                                cleanup_failed = true;
                            }
                        } else {
                            warn!(
                                "Refusing to delete unmanaged worktree path {:?} during startup reconciliation",
                                path
                            );
                            failures.record(
                                "worktree_safety_refusal",
                                format!("task '{}': unmanaged path {:?}", t_id, path),
                            );
                            if let Err(e) = self.emit_event_checked(
                                None,
                                Some(&t_id),
                                None,
                                "WORKTREE_SAFETY_REFUSAL",
                                json!({ "path": path_str }),
                            ) {
                                failures.record(
                                    "worktree_safety_refusal_event",
                                    format!(
                                        "task '{}' (project '{}'): could not emit WORKTREE_SAFETY_REFUSAL: {}",
                                        t_id, p_id, e
                                    ),
                                );
                            }
                        }
                    } else {
                        cleanup_failed = true;
                    }
                }
            }

            let conn = self.db.lock();
            let now = Utc::now().to_rfc3339();

            // Return any claimed masterplan steps back to PENDING
            if let Err(e) = conn.execute(
                "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1 WHERE claimed_task_id = ?2 AND status != 'COMPLETED'",
                params![now, t_id],
            ) {
                failures.record(
                    "claiming_step_restore",
                    format!("task '{}' (project '{}'): failed to restore steps to PENDING: {}", t_id, p_id, e),
                );
            }

            // Clean up any scope leases
            if let Err(e) = conn.execute("DELETE FROM scope_leases WHERE task_id = ?1", [&t_id]) {
                failures.record(
                    "claiming_scope_cleanup",
                    format!(
                        "task '{}' (project '{}'): failed to delete scope leases: {}",
                        t_id, p_id, e
                    ),
                );
            }

            if cleanup_failed {
                // Recovery pointers MUST be preserved when cleanup fails:
                // Do NOT clear worktree_path or branch_name.
                // Transition task to BLOCKED with substate RECOVERABLE.
                match transition_task_state_on_conn(
                    &conn,
                    &t_id,
                    &[TaskState::Backlog],
                    TaskState::Blocked,
                ) {
                    Ok(()) => {
                        if let Err(e) = conn.execute(
                            "UPDATE tasks SET substate = 'RECOVERABLE', updated_at = ?1 WHERE id = ?2",
                            [&now, &t_id],
                        ) {
                            failures.record(
                                "claiming_task_substate",
                                format!("task '{}' (project '{}'): failed to set substate RECOVERABLE: {}", t_id, p_id, e),
                            );
                        }
                    }
                    Err(e) => {
                        failures.record(
                            "claiming_task_transition_blocked",
                            format!(
                                "task '{}' (project '{}'): failed to transition to BLOCKED: {}",
                                t_id, p_id, e
                            ),
                        );
                    }
                }
                warn!(
                    "Interrupted claiming task '{}' cleanup failed; preserved recovery pointers and transitioned to BLOCKED (substate RECOVERABLE)",
                    t_id
                );
            } else {
                // Successful cleanup: transition to READY and clear claim fields
                match transition_task_state_on_conn(
                    &conn,
                    &t_id,
                    &[TaskState::Claiming, TaskState::Backlog],
                    TaskState::Ready,
                ) {
                    Ok(()) => {
                        if let Err(e) = conn.execute(
                            "UPDATE tasks SET substate = 'NONE', assigned_agent_id = NULL, worktree_path = NULL, branch_name = NULL, updated_at = ?1 WHERE id = ?2",
                            [&now, &t_id],
                        ) {
                            failures.record(
                                "claiming_task_cleanup",
                                format!(
                                    "task '{}' (project '{}'): failed to clear claim fields: {}",
                                    t_id, p_id, e
                                ),
                            );
                        }
                        info!(
                            "Reconciled interrupted claiming task '{}' -> reset to READY",
                            t_id
                        );
                    }
                    Err(e) => {
                        failures.record(
                            "claiming_task_transition",
                            format!("task '{}' (project '{}'): {}", t_id, p_id, e),
                        );
                        warn!(
                            "Failed to reconcile interrupted claiming task '{}' to READY: {}",
                            t_id, e
                        );
                    }
                }
            }
            drop(conn);
        }

        // 2. Reconcile merge queue items interrupted during checks
        let running_merge_items: Vec<(String, String, String, String, String)> = {
            let conn = self.db.lock();
            let scanned = match conn.prepare(
                "SELECT id, project_id, task_id, target_branch, head_sha FROM merge_queue WHERE status = 'RUNNING_CHECKS'",
            ) {
                Ok(mut stmt) => match stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                    .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
                {
                    Ok(items) => items,
                    Err(e) => {
                        failures.record(
                            "merge_queue_reset",
                            format!("failed to read RUNNING_CHECKS merge items: {}", e),
                        );
                        Vec::new()
                    }
                },
                Err(e) => {
                    failures.record(
                        "merge_queue_reset",
                        format!("could not scan RUNNING_CHECKS merge items: {}", e),
                    );
                    Vec::new()
                }
            };
            scanned
        };

        for (q_id, p_id, t_id, target_branch, head_sha) in running_merge_items {
            let repo_path: Option<std::path::PathBuf> = {
                let conn = self.db.lock();
                conn.query_row("SELECT path FROM projects WHERE id = ?1", [&p_id], |r| {
                    r.get::<_, String>(0)
                })
                .ok()
                .map(std::path::PathBuf::from)
            };

            let ancestor_check = if let Some(ref repo_path) = repo_path {
                self.git.is_ancestor(repo_path, &head_sha, &target_branch)
            } else {
                Err(format!("Project '{}' repository path not found", p_id))
            };

            let conn = self.db.lock();
            let now = Utc::now().to_rfc3339();

            match ancestor_check {
                Ok(crate::git::GitAncestorResult::Ancestor) => {
                    // Git merge succeeded before interruption -> finalize state in SQLite (MERGED, DONE, COMPLETED steps)
                    if let Err(e) = conn.execute(
                        "UPDATE merge_queue SET status = 'MERGED', processed_at = ?1 WHERE id = ?2",
                        [&now, &q_id],
                    ) {
                        failures.record("merge_reconcile_finalize", e.to_string());
                    }
                    if let Err(e) = transition_task_state_on_conn(
                        &conn,
                        &t_id,
                        &[
                            TaskState::MergeReady,
                            TaskState::Verifying,
                            TaskState::Review,
                        ],
                        TaskState::Done,
                    ) {
                        failures.record("merge_task_transition_done", e);
                    } else {
                        if let Err(e) = conn
                            .execute("UPDATE tasks SET substate = 'NONE' WHERE id = ?1", [&t_id])
                        {
                            failures.record("merge_task_clear_substate", e.to_string());
                        }
                    }

                    // Complete associated masterplan steps
                    if let Err(e) = conn.execute(
                        "UPDATE masterplan_steps SET status = 'COMPLETED', completed_at = ?1, updated_at = ?1 WHERE claimed_task_id = ?2 AND status != 'COMPLETED'",
                        [&now, &t_id],
                    ) {
                        failures.record("masterplan_steps_finalize", e.to_string());
                    }
                    match conn.query_row(
                        "SELECT COUNT(*) FROM masterplan_steps ms
                         JOIN masterplans mp ON ms.masterplan_id = mp.id
                         WHERE mp.project_id = ?1 AND ms.status != 'COMPLETED'",
                        [&p_id],
                        |r| r.get::<_, i64>(0),
                    ) {
                        Ok(0) => {
                            if let Err(e) = conn.execute(
                                "UPDATE masterplans SET status = 'COMPLETED', updated_at = ?1 WHERE project_id = ?2",
                                [&now, &p_id],
                            ) {
                                failures.record("masterplan_complete", e.to_string());
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            failures.record("masterplan_count_pending", e.to_string());
                        }
                    }
                    info!(
                        "Reconciled merge queue item '{}' for task '{}': Git merge was already applied; finalized to MERGED/DONE/COMPLETED in SQLite",
                        q_id, t_id
                    );
                }
                Ok(crate::git::GitAncestorResult::NotAncestor) => {
                    // Git merge was not applied -> safely revert queue status to READY
                    if let Err(e) = conn.execute(
                        "UPDATE merge_queue SET status = 'READY' WHERE id = ?1",
                        [&q_id],
                    ) {
                        failures.record("merge_queue_reset", e.to_string());
                    }
                    info!(
                        "Reconciled interrupted merge queue item '{}' for task '{}' -> reset status to READY",
                        q_id, t_id
                    );
                }
                Err(e) => {
                    // Check if repo and target_branch are valid, but head_sha is not a valid commit in the repo
                    let target_branch_exists = repo_path
                        .as_ref()
                        .map(|rp| self.git.get_ref_sha(rp, &target_branch).is_ok())
                        .unwrap_or(false);
                    let is_invalid_head = e.contains("Not a valid commit name")
                        || e.contains("not a valid commit name");

                    if target_branch_exists && is_invalid_head {
                        // The target branch is valid and healthy, but head_sha does not exist in the repository.
                        // An object not present in the repository could not possibly be an ancestor of target_branch.
                        // Safely reset merge_queue status to READY.
                        if let Err(err) = conn.execute(
                            "UPDATE merge_queue SET status = 'READY' WHERE id = ?1",
                            [&q_id],
                        ) {
                            failures.record("merge_queue_reset", err.to_string());
                        }
                        info!(
                            "Reconciled unapplied merge queue item '{}' (non-existent head_sha '{}') -> reset status to READY",
                            q_id, head_sha
                        );
                    } else {
                        // True Git/command/ref failure: fail-closed!
                        // DO NOT revert to READY! Preserve state and record failure.
                        failures.record(
                            "merge_queue_reconcile_git_unknown",
                            format!(
                                "task '{}' (project '{}'): could not determine ancestor status: {}",
                                t_id, p_id, e
                            ),
                        );
                        warn!(
                            "Merge reconciliation could not determine Git ancestor status for queue item '{}' (task '{}'): {}. Leaving queue item intact to prevent duplicate merge.",
                            q_id, t_id, e
                        );
                    }
                }
            }
            drop(conn);
        }

        // 2b. Reconcile tasks whose merge was finalized in merge_queue but task or steps lagged
        let lagged_merged: Vec<(String, String)> = {
            let conn = self.db.lock();
            let scanned = match conn.prepare(
                "SELECT t.id, t.project_id FROM tasks t
                 JOIN merge_queue mq ON mq.task_id = t.id
                 WHERE mq.status = 'MERGED' AND (
                     t.state != 'DONE' OR EXISTS (
                         SELECT 1 FROM masterplan_steps ms WHERE ms.claimed_task_id = t.id AND ms.status != 'COMPLETED'
                     )
                 )",
            ) {
                Ok(mut stmt) => match stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
                {
                    Ok(items) => items,
                    Err(e) => {
                        failures.record("lagged_merge_scan", format!("failed to read lagged merge rows: {}", e));
                        Vec::new()
                    }
                },
                Err(e) => {
                    failures.record("lagged_merge_scan", e.to_string());
                    Vec::new()
                }
            };
            scanned
        };

        for (t_id, p_id) in lagged_merged {
            let conn = self.db.lock();
            let now = Utc::now().to_rfc3339();
            if let Err(e) = transition_task_state_on_conn(
                &conn,
                &t_id,
                &[
                    TaskState::MergeReady,
                    TaskState::Verifying,
                    TaskState::Review,
                    TaskState::Running,
                ],
                TaskState::Done,
            ) {
                failures.record(
                    "lagged_merge_task_transition",
                    format!("task '{}' (project '{}'): {}", t_id, p_id, e),
                );
            } else {
                if let Err(e) =
                    conn.execute("UPDATE tasks SET substate = 'NONE' WHERE id = ?1", [&t_id])
                {
                    failures.record(
                        "lagged_merge_task_substate",
                        format!("task '{}' (project '{}'): {}", t_id, p_id, e),
                    );
                }
            }

            if let Err(e) = conn.execute(
                "UPDATE masterplan_steps SET status = 'COMPLETED', completed_at = ?1, updated_at = ?1 WHERE claimed_task_id = ?2 AND status != 'COMPLETED'",
                [&now, &t_id],
            ) {
                failures.record(
                    "lagged_merge_steps_complete",
                    format!("task '{}' (project '{}'): {}", t_id, p_id, e),
                );
            }

            match conn.query_row(
                "SELECT COUNT(*) FROM masterplan_steps ms
                 JOIN masterplans mp ON ms.masterplan_id = mp.id
                 WHERE mp.project_id = ?1 AND ms.status != 'COMPLETED'",
                [&p_id],
                |r| r.get::<_, i64>(0),
            ) {
                Ok(0) => {
                    if let Err(e) = conn.execute(
                        "UPDATE masterplans SET status = 'COMPLETED', updated_at = ?1 WHERE project_id = ?2",
                        [&now, &p_id],
                    ) {
                        failures.record(
                            "lagged_merge_plan_complete",
                            format!("project '{}': {}", p_id, e),
                        );
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    failures.record(
                        "lagged_merge_count_pending",
                        format!("project '{}': {}", p_id, e),
                    );
                }
            }
            drop(conn);
        }

        // 3. Mark expired scope leases
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        if let Err(e) = conn.execute("DELETE FROM scope_leases WHERE expires_at < ?1", [&now]) {
            failures.record("scope_lease_cleanup", e.to_string());
        }

        // 4. Reconcile orphaned claimed masterplan steps whose tasks are missing or cancelled
        if let Err(e) = conn.execute(
            "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1
             WHERE status = 'CLAIMED' AND (
                 claimed_task_id IS NULL
                 OR claimed_task_id IN (SELECT id FROM tasks WHERE state IN ('CANCELLED', 'BLOCKED', 'FAILED') OR is_stale = 1)
                 OR NOT EXISTS (SELECT 1 FROM tasks t WHERE t.id = masterplan_steps.claimed_task_id)
             )",
            [&now],
        ) {
            failures.record("masterplan_step_reset", e.to_string());
        }
        drop(conn);

        // 5. Surface any write failures: startup proceeds, but never silently
        if !failures.is_empty() {
            error!(
                "Startup reconciliation completed with {} write failure(s): {:?}",
                failures.failures.len(),
                failures.failures
            );
            let summary = failures.summary();
            if let Err(e) = self.emit_event_checked(None, None, None, "RECONCILE_PARTIAL", summary)
            {
                error!("Failed to record RECONCILE_PARTIAL event: {}", e);
            }
        }
    }

    // --- Projects ---
    pub fn create_project(
        &self,
        name: &str,
        path: &str,
        master_spec: &str,
        target_branch: &str,
    ) -> Result<Project, String> {
        let repo_path = Path::new(path);
        if !repo_path.exists() {
            return Err(format!("Path '{}' does not exist", path));
        }

        if !self.git.check_is_git_repo(repo_path)? {
            info!(
                "Directory '{}' is not a Git repository. Auto-initializing...",
                path
            );
            self.git.init_repo(repo_path)?;
        }

        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to begin project transaction: {}", e))?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        tx.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, name, path, master_spec, target_branch, now],
        ).map_err(|e| format!("Failed to insert project: {}", e))?;

        // Initialize default project contract
        let contract_id = Uuid::new_v4().to_string();
        let mut hasher = Sha256::new();
        hasher.update(master_spec.as_bytes());
        let contract_hash = hex::encode(hasher.finalize());

        tx.execute(
            "INSERT INTO project_contracts (id, project_id, version, overview, architecture, rules_json, commands_json, testing_json, repo_map, security_constraints, contract_hash, created_at)
             VALUES (?1, ?2, 1, ?3, 'Standard Architecture', '[]', '[]', '[]', '', '[]', ?4, ?5)",
            params![contract_id, id, master_spec, contract_hash, now],
        ).map_err(|e| format!("Failed to insert default project contract: {}", e))?;

        // Initialize baseline project rules
        let rule_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO project_rules (id, project_id, category, rule_text, strictness, created_at)
             VALUES (?1, ?2, 'SYSTEM', 'All mutations must occur inside assigned Git worktrees and within granted scope leases.', 'MANDATORY', ?3)",
            params![rule_id, id, now],
        ).map_err(|e| format!("Failed to insert baseline project rule: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit project creation transaction: {}", e))?;

        let proj = Project {
            id: id.clone(),
            name: name.to_string(),
            path: path.to_string(),
            master_spec: master_spec.to_string(),
            target_branch: target_branch.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        drop(conn);
        self.emit_event(
            Some(&id),
            None,
            None,
            "PROJECT_CREATED",
            json!({ "name": name, "path": path }),
        );
        Ok(proj)
    }

    pub fn create_example_project(&self, root_dir: &str) -> Result<Project, String> {
        let example_path = Path::new(root_dir).join("example-repo");
        std::fs::create_dir_all(&example_path).map_err(|e| e.to_string())?;
        self.git.init_repo(&example_path)?;

        let proj = self.create_project(
            "AgentXFlow Example Project",
            example_path.to_str().unwrap(),
            "Example project demonstrating multi-agent coordination with worktrees and scope locks",
            "main",
        )?;

        // Create sample tasks
        self.create_task(
            &proj.id,
            "Implement User Authentication Service",
            "Build JWT-based authentication service in src/auth/",
            "HIGH",
            vec![
                (
                    "Implement Token Generation".to_string(),
                    "Generate signed JWT tokens".to_string(),
                    true,
                ),
                (
                    "Add Auth Unit Tests".to_string(),
                    "Run cargo test --test auth_test".to_string(),
                    true,
                ),
            ],
            vec![
                "Token signature matches secret".to_string(),
                "All tests pass".to_string(),
            ],
        )?;

        self.create_task(
            &proj.id,
            "Implement SQLite Database Migration Runner",
            "Build robust migration runner for 24 tables",
            "MEDIUM",
            vec![
                (
                    "Write Migration SQL".to_string(),
                    "Create initial schema".to_string(),
                    true,
                ),
                (
                    "Verify Foreign Keys".to_string(),
                    "Test cascading deletes".to_string(),
                    true,
                ),
            ],
            vec!["Schema migrations are idempotent".to_string()],
        )?;

        Ok(proj)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, path, master_spec, target_branch, created_at, updated_at FROM projects ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    master_spec: row.get(3)?,
                    target_branch: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r.map_err(|e| format!("Failed to decode project row: {}", e))?);
        }
        Ok(res)
    }

    pub fn get_project(&self, project_id: &str) -> Result<Project, String> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, name, path, master_spec, target_branch, created_at, updated_at FROM projects WHERE id = ?1",
            [project_id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    master_spec: row.get(3)?,
                    target_branch: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        ).map_err(|e| format!("Project '{}' not found: {}", project_id, e))
    }

    // --- Tasks ---
    #[allow(clippy::too_many_arguments)]
    pub fn create_task_internal(
        &self,
        project_id: &str,
        masterplan_id: Option<&str>,
        masterplan_revision_id: Option<&str>,
        title: &str,
        description: &str,
        priority: &str,
        steps: Vec<(String, String, bool)>,
        criteria: Vec<String>,
    ) -> Result<Task, String> {
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        tx.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, masterplan_revision_id, title, description, state, substate, priority, is_stale, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'BACKLOG', 'NONE', ?7, 0, ?8, ?8)",
            params![id, project_id, masterplan_id, masterplan_revision_id, title, description, priority, now],
        ).map_err(|e| e.to_string())?;

        for (idx, (step_title, step_desc, is_mand)) in steps.into_iter().enumerate() {
            let step_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO task_steps (id, task_id, order_index, title, description, is_mandatory, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING')",
                params![step_id, id, idx as i32 + 1, step_title, step_desc, is_mand],
            ).map_err(|e| format!("Failed to create task step: {}", e))?;
        }

        for crit in criteria {
            let crit_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO acceptance_criteria (id, task_id, criterion, is_satisfied, is_locked)
                 VALUES (?1, ?2, ?3, 0, 0)",
                params![crit_id, id, crit],
            )
            .map_err(|e| format!("Failed to create acceptance criterion: {}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit task transaction: {}", e))?;
        drop(conn);

        let task = Task {
            id: id.clone(),
            project_id: project_id.to_string(),
            parent_id: None,
            epic_id: None,
            title: title.to_string(),
            description: description.to_string(),
            state: TaskState::Backlog,
            substate: TaskSubstate::None,
            assigned_agent_id: None,
            priority: priority.to_string(),
            risk_score: 0.0,
            estimated_scope: None,
            worktree_path: None,
            branch_name: None,
            base_sha: None,
            head_sha: None,
            masterplan_id: masterplan_id.map(|s| s.to_string()),
            masterplan_revision_id: masterplan_revision_id.map(|s| s.to_string()),
            is_stale: false,
            created_at: now.clone(),
            updated_at: now,
        };

        self.emit_event(
            Some(project_id),
            Some(&id),
            None,
            "TASK_CREATED",
            json!({ "title": title, "priority": priority }),
        );
        Ok(task)
    }

    pub fn create_task(
        &self,
        project_id: &str,
        title: &str,
        description: &str,
        priority: &str,
        steps: Vec<(String, String, bool)>,
        criteria: Vec<String>,
    ) -> Result<Task, String> {
        self.create_task_internal(
            project_id,
            None,
            None,
            title,
            description,
            priority,
            steps,
            criteria,
        )
    }

    pub fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>, String> {
        if project_id.trim().is_empty() {
            return Err("project_id is required for list_tasks. Query 'project_list' or 'agentxflow_current_context' to obtain valid project IDs.".to_string());
        }
        let conn = self.db.lock();
        let query = "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, masterplan_id, masterplan_revision_id, is_stale, created_at, updated_at FROM tasks WHERE project_id = ?1 ORDER BY created_at DESC";

        let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;

        let map_row = |row: &rusqlite::Row| {
            Ok(Task {
                id: row.get(0)?,
                project_id: row.get(1)?,
                parent_id: row.get(2)?,
                epic_id: row.get(3)?,
                title: row.get(4)?,
                description: row.get(5)?,
                state: TaskState::parse(&row.get::<_, String>(6)?),
                substate: TaskSubstate::parse(&row.get::<_, String>(7)?),
                assigned_agent_id: row.get(8)?,
                priority: row.get(9)?,
                risk_score: row.get(10)?,
                estimated_scope: row.get(11)?,
                worktree_path: row.get(12)?,
                branch_name: row.get(13)?,
                base_sha: row.get(14)?,
                head_sha: row.get(15)?,
                masterplan_id: row.get(16)?,
                masterplan_revision_id: row.get(17)?,
                is_stale: row.get(18)?,
                created_at: row.get(19)?,
                updated_at: row.get(20)?,
            })
        };

        let rows = stmt
            .query_map([project_id], map_row)
            .map_err(|e| e.to_string())?;
        let mut res = Vec::new();
        for r in rows {
            res.push(r.map_err(|e| format!("Failed to decode task row: {}", e))?);
        }

        Ok(res)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Task, String> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, masterplan_id, masterplan_revision_id, is_stale, created_at, updated_at FROM tasks WHERE id = ?1",
            [task_id],
            |row| {
                Ok(Task {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    epic_id: row.get(3)?,
                    title: row.get(4)?,
                    description: row.get(5)?,
                    state: TaskState::parse(&row.get::<_, String>(6)?),
                    substate: TaskSubstate::parse(&row.get::<_, String>(7)?),
                    assigned_agent_id: row.get(8)?,
                    priority: row.get(9)?,
                    risk_score: row.get(10)?,
                    estimated_scope: row.get(11)?,
                    worktree_path: row.get(12)?,
                    branch_name: row.get(13)?,
                    base_sha: row.get(14)?,
                    head_sha: row.get(15)?,
                    masterplan_id: row.get(16)?,
                    masterplan_revision_id: row.get(17)?,
                    is_stale: row.get(18)?,
                    created_at: row.get(19)?,
                    updated_at: row.get(20)?,
                })
            },
        ).map_err(|e| format!("Task '{}' not found: {}", task_id, e))
    }

    /// Explicitly cancel a task, releasing all write scope leases and returning masterplan steps to PENDING
    pub fn cancel_task(
        &self,
        task_id: &str,
        caller_agent_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<Task, String> {
        let task = self.get_task(task_id)?;

        if let Some(caller) = caller_agent_id {
            if let Some(ref assigned) = task.assigned_agent_id {
                let (canon_assigned, ..) = Self::canonicalize_ide_identity(assigned, "");
                let (canon_caller, ..) = Self::canonicalize_ide_identity(caller, "");
                if !assigned.is_empty() && assigned != caller && canon_assigned != canon_caller {
                    return Err(format!("Authorization error: Caller agent '{}' is not the owner of task '{}' (assigned to '{}')", caller, task_id, assigned));
                }
            } else {
                return Err(format!(
                    "Authorization error: Cannot cancel unassigned task '{}'",
                    task_id
                ));
            }
        }

        if task.state == TaskState::Done {
            return Err(format!(
                "Cannot cancel task '{}': Task is already DONE (merged)",
                task_id
            ));
        }

        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start cancel transaction: {}", e))?;

        // Transaction-body cancellation writes (scope release, masterplan revert,
        // merge-queue removal, CANCELLED + stale). Event emission and worktree
        // cleanup happen after commit below: both lock the DB.
        cancel_task_inner(&tx, task_id, caller_agent_id, reason)
            .map_err(|e| format!("Failed to cancel task: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit task cancellation: {}", e))?;
        drop(conn);

        // 5. Cleanup worktree if present
        if let Some(ref wt_path) = task.worktree_path {
            let p = Path::new(wt_path);
            if p.exists() {
                if let Ok(projs) = self.list_projects() {
                    if let Some(proj) = projs.into_iter().find(|pr| pr.id == task.project_id) {
                        let repo_path = Path::new(&proj.path);
                        let managed_roots =
                            vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
                        if self.git.is_managed_worktree(repo_path, &managed_roots, p) {
                            let _ = self.git.remove_worktree(repo_path, &managed_roots, p);
                            let _ = crate::git::GitService::safe_remove_dir_all(p);
                        } else {
                            warn!(
                                "Refusing to delete unmanaged worktree path {:?} during task cancellation",
                                p
                            );
                            self.emit_event(
                                Some(&task.project_id),
                                Some(task_id),
                                caller_agent_id,
                                "WORKTREE_SAFETY_REFUSAL",
                                json!({ "path": wt_path }),
                            );
                        }
                    }
                }
            }
        }

        self.emit_event(
            Some(&task.project_id),
            Some(task_id),
            caller_agent_id,
            "TASK_CANCELLED",
            json!({ "task_id": task_id, "reason": reason.unwrap_or("Task cancelled explicitly") }),
        );

        self.get_task(task_id)
    }

    /// Requeues a task chunk back to the masterplan: releases scope leases and sets step status = PENDING
    pub fn requeue_task(&self, task_id: &str, caller_agent_id: Option<&str>) -> Result<(), String> {
        self.cancel_task(
            task_id,
            caller_agent_id,
            Some("Task requeued to masterplan pending backlog"),
        )?;
        Ok(())
    }

    pub fn get_task_details(&self, task_id: &str) -> Result<TaskDetails, String> {
        let task = self.get_task(task_id)?;
        let conn = self.db.lock();

        let mut stmt_steps = conn.prepare("SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE task_id = ?1 ORDER BY order_index ASC").map_err(|e| e.to_string())?;
        let steps: Vec<TaskStep> = stmt_steps
            .query_map([task_id], |r| {
                Ok(TaskStep {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    order_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    is_mandatory: r.get(5)?,
                    status: r.get(6)?,
                    completed_at: r.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read task step row: {}", e))?;

        let mut stmt_crit = conn.prepare("SELECT id, task_id, criterion, is_satisfied, is_locked FROM acceptance_criteria WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let criteria: Vec<AcceptanceCriteria> = stmt_crit
            .query_map([task_id], |r| {
                Ok(AcceptanceCriteria {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    criterion: r.get(2)?,
                    is_satisfied: r.get(3)?,
                    is_locked: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read acceptance criteria row: {}", e))?;

        let mut stmt_leases = conn.prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let leases: Vec<ScopeLease> = stmt_leases
            .query_map([task_id], |r| {
                Ok(ScopeLease {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    pattern: r.get(3)?,
                    access_type: r.get(4)?,
                    expires_at: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read scope lease row: {}", e))?;

        let mut stmt_deps = conn.prepare("SELECT id, task_id, depends_on_task_id, dependency_type, created_at FROM task_dependencies WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let dependencies: Vec<TaskDependency> = stmt_deps
            .query_map([task_id], |r| {
                Ok(TaskDependency {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    depends_on_task_id: r.get(2)?,
                    dependency_type: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read task dependency row: {}", e))?;

        let mut stmt_runs = conn.prepare("SELECT id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at, timed_out FROM verification_runs WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let verification_runs: Vec<VerificationRun> = stmt_runs
            .query_map([task_id], |r| {
                Ok(VerificationRun {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    run_id: r.get(2)?,
                    check_id: r.get(3)?,
                    check_name: r.get(4)?,
                    commit_sha: r.get(5)?,
                    command: r.get(6)?,
                    exit_code: r.get(7)?,
                    stdout: r.get(8)?,
                    stderr: r.get(9)?,
                    duration_ms: r.get(10)?,
                    is_passed: r.get(11)?,
                    is_stale: r.get(12)?,
                    executed_at: r.get(13)?,
                    timed_out: r.get(14)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read verification run row: {}", e))?;

        let mut stmt_violations = conn.prepare("SELECT id, task_id, agent_id, file_path, violation_type, detected_at, resolved FROM scope_violations WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let violations: Vec<ScopeViolation> = stmt_violations
            .query_map([task_id], |r| {
                Ok(ScopeViolation {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    file_path: r.get(3)?,
                    violation_type: r.get(4)?,
                    detected_at: r.get(5)?,
                    resolved: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read scope violation row: {}", e))?;

        let mut stmt_evidence = conn.prepare("SELECT id, task_id, step_id, evidence_type, source, payload_json, recorded_at FROM evidence_records WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let evidence_records: Vec<EvidenceRecord> = stmt_evidence
            .query_map([task_id], |r| {
                Ok(EvidenceRecord {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    step_id: r.get(2)?,
                    evidence_type: r.get(3)?,
                    source: r.get(4)?,
                    payload_json: r.get(5)?,
                    recorded_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read evidence record row: {}", e))?;

        let proof_bundle = conn.query_row(
            "SELECT task_id, project_id, agent_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, proof_hash, generated_at, verification_runs_json FROM proof_bundles WHERE task_id = ?1 ORDER BY generated_at DESC LIMIT 1",
            [task_id],
            |r| {
                let files_json: String = r.get(6)?;
                let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
                let runs_json: String = r.get(10).unwrap_or_else(|_| "[]".to_string());
                let runs: Vec<VerificationRun> = serde_json::from_str(&runs_json).unwrap_or_default();
                Ok(ProofBundle {
                    task_id: r.get(0)?,
                    project_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    prompt: r.get(3)?,
                    base_sha: r.get(4)?,
                    head_sha: r.get(5)?,
                    files_changed: files,
                    diff_summary: r.get(7)?,
                    verification_runs: runs,
                    scope_violations: Vec::new(),
                    proof_hash: r.get(8)?,
                    generated_at: r.get(9)?,
                })
            },
        ).ok();

        let assigned_agent = if let Some(ref aid) = task.assigned_agent_id {
            conn.query_row(
                "SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents WHERE id = ?1",
                [aid],
                |r| {
                    Ok(Agent {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        agent_type: r.get(2)?,
                        profile: r.get(3)?,
                        status: r.get(4)?,
                        capabilities: AgentCapabilitySet::default(),
                        last_heartbeat: r.get(5)?,
                        created_at: r.get(6)?,
                        session_token: None,
                        active_task_id: Some(task_id.to_string()),
                        active_task_title: Some(task.title.clone()),
                        last_seen_seconds: None,
                    })
                },
            ).ok()
        } else {
            None
        };

        let active_attempt = conn.query_row(
            "SELECT id, task_id, agent_id, attempt_number, base_sha, head_sha, status, rejection_reasons, started_at, finished_at FROM task_attempts WHERE task_id = ?1 ORDER BY attempt_number DESC LIMIT 1",
            [task_id],
            |r| {
                Ok(TaskAttempt {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    attempt_number: r.get(3)?,
                    base_sha: r.get(4)?,
                    head_sha: r.get(5)?,
                    status: r.get(6)?,
                    rejection_reasons: r.get(7)?,
                    started_at: r.get(8)?,
                    finished_at: r.get(9)?,
                })
            },
        ).ok();

        let mut stmt_eval = conn.prepare("SELECT id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at FROM evaluator_results WHERE task_id = ?1 ORDER BY evaluated_at DESC").map_err(|e| e.to_string())?;
        let evaluator_results: Vec<EvaluatorResult> = stmt_eval
            .query_map([task_id], |r| {
                Ok(EvaluatorResult {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    attempt_id: r.get(2)?,
                    criterion_id: r.get(3)?,
                    evaluator_name: r.get(4)?,
                    evaluator_type: r.get(5)?,
                    evaluator_version: r.get(6)?,
                    commit_sha: r.get(7)?,
                    exit_code: r.get(8)?,
                    stdout_output: r.get(9)?,
                    stderr_output: r.get(10)?,
                    output_sha256: r.get(11)?,
                    duration_ms: r.get(12)?,
                    passed: r.get(13)?,
                    evaluated_at: r.get(14)?,
                })
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        Ok(TaskDetails {
            task,
            steps,
            criteria,
            leases,
            dependencies,
            verification_runs,
            violations,
            evidence_records,
            proof_bundle,
            assigned_agent,
            active_attempt,
            evaluator_results,
        })
    }

    /// Crash-safe Task Claiming with Compare-and-Swap & Transactional Lock
    pub fn claim_task(&self, task_id: &str, agent_id: &str) -> Result<Task, String> {
        if !self.is_agent_registered(agent_id) {
            return Err(format!(
                "Agent registration required: Agent ID '{}' is not registered.",
                agent_id
            ));
        }

        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // 1. Transactional check & reserve state as CLAIMING
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;

        // Atomic dependency gate check on the active transaction connection
        if !crate::dag::DagEngine::are_dependencies_satisfied_on_conn(&tx, task_id)? {
            return Err(format!(
                "Cannot claim task '{}': Prerequisite dependencies are not yet DONE",
                task_id
            ));
        }

        let (project_id, current_state, current_assigned): (String, String, Option<String>) = tx
            .query_row(
                "SELECT project_id, state, assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

        if current_state != "BACKLOG" && current_state != "READY" {
            return Err(format!(
                "Cannot claim task '{}': Task is already in state '{}'",
                task_id, current_state
            ));
        }

        if let Some(existing_agent) = current_assigned {
            if !existing_agent.is_empty() && existing_agent != agent_id {
                return Err(format!(
                    "Cannot claim task '{}': Already assigned to agent '{}'",
                    task_id, existing_agent
                ));
            }
        }

        let (proj_path, target_branch): (String, String) = tx
            .query_row(
                "SELECT path, target_branch FROM projects WHERE id = ?1",
                [&project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Project '{}' not found: {}", project_id, e))?;

        let repo_path = Path::new(&proj_path);
        let branch_name = format!("agentxflow/task-{}", task_id);
        let worktree_dir = self
            .worktrees_root
            .join(&project_id)
            .join(format!("task-{}", task_id));
        let worktree_path_str = worktree_dir.to_string_lossy().to_string();

        let base_sha = self
            .git
            .get_ref_sha(repo_path, &target_branch)
            .map_err(|e| {
                format!(
                    "Failed to resolve base commit '{}' for claim of task '{}': {}",
                    target_branch, task_id, e
                )
            })?;

        // Set state = 'CLAIMING'
        tx.execute(
            "UPDATE tasks SET state = 'CLAIMING', substate = 'CLAIMING', assigned_agent_id = ?1, worktree_path = ?2, branch_name = ?3, base_sha = ?4, updated_at = ?5 WHERE id = ?6",
            params![agent_id, worktree_path_str, branch_name, base_sha, now, task_id],
        ).map_err(|e| format!("Failed to record task claim: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit claim reservation: {}", e))?;
        drop(conn);

        // 2. Cut isolated Git worktree on disk
        if worktree_dir.exists() {
            let managed_roots = vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
            if self
                .git
                .is_managed_worktree(repo_path, &managed_roots, &worktree_dir)
            {
                let _ = self
                    .git
                    .remove_worktree(repo_path, &managed_roots, &worktree_dir);
                let _ = std::fs::remove_dir_all(&worktree_dir);
            } else {
                warn!(
                    "Refusing to remove unmanaged path {:?} during claim re-cut",
                    worktree_dir
                );
                self.emit_event(
                    Some(&project_id),
                    Some(task_id),
                    Some(agent_id),
                    "WORKTREE_SAFETY_REFUSAL",
                    json!({ "path": worktree_path_str }),
                );
            }
        }

        if let Err(e) =
            self.git
                .create_worktree(repo_path, &worktree_dir, &branch_name, &target_branch)
        {
            // Full compensation on failure: validated CLAIMING -> READY reset.
            let conn = self.db.lock();
            if let Err(te) = transition_task_state_on_conn(
                &conn,
                task_id,
                &[TaskState::Claiming, TaskState::Backlog],
                TaskState::Ready,
            ) {
                error!(
                    "Failed to compensate claim reservation for task '{}' after worktree creation failure: {}",
                    task_id, te
                );
            } else {
                conn.execute(
                    "UPDATE tasks SET substate = 'NONE', assigned_agent_id = NULL, worktree_path = NULL, branch_name = NULL WHERE id = ?1",
                    [task_id],
                ).ok();
            }
            return Err(format!("Failed to create isolated Git worktree: {}", e));
        }

        // 3. Mark state = 'RUNNING' and lock criteria atomically (validated transition;
        //    the current state at this point is 'CLAIMING', which parses to Backlog)
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start finalize transaction: {}", e))?;
        transition_task_state_on_conn(&tx, task_id, &[TaskState::Backlog], TaskState::Running)
            .map_err(|e| format!("Failed to mark task as RUNNING: {}", e))?;
        tx.execute(
            "UPDATE tasks SET substate = 'ANALYZING' WHERE id = ?1",
            [task_id],
        )
        .map_err(|e| format!("Failed to record task substate: {}", e))?;
        tx.execute(
            "UPDATE acceptance_criteria SET is_locked = 1 WHERE task_id = ?1",
            [task_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit()
            .map_err(|e| format!("Failed to commit claim finalization: {}", e))?;
        drop(conn);

        self.emit_event(
            Some(&project_id),
            Some(task_id),
            Some(agent_id),
            "TASK_CLAIMED",
            json!({ "agent": agent_id }),
        );
        self.get_task(task_id)
    }

    pub fn complete_step(
        &self,
        step_id: &str,
        agent_id: Option<&str>,
        evidence_json: Option<&str>,
    ) -> Result<TaskStep, String> {
        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // Verify step and caller task ownership
        let (task_id, assigned_agent): (String, Option<String>) = conn
            .query_row(
                "SELECT t.id, t.assigned_agent_id FROM task_steps s JOIN tasks t ON s.task_id = t.id WHERE s.id = ?1",
                [step_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Step '{}' not found: {}", step_id, e))?;

        if let Some(caller) = agent_id {
            if let Some(ref assigned) = assigned_agent {
                let (canon_assigned, ..) = Self::canonicalize_ide_identity(assigned, "");
                let (canon_caller, ..) = Self::canonicalize_ide_identity(caller, "");
                if assigned != caller && canon_assigned != canon_caller {
                    return Err(format!(
                        "Step ownership violation: Step belongs to task '{}' assigned to agent '{}', caller is '{}'",
                        task_id, assigned, caller
                    ));
                }
            } else {
                return Err(format!(
                    "Step ownership violation: Step belongs to unassigned task '{}', caller is '{}'",
                    task_id, caller
                ));
            }
        }

        let tx = conn.transaction().map_err(|e| e.to_string())?;

        tx.execute(
            "UPDATE task_steps SET status = 'COMPLETED', completed_at = ?1 WHERE id = ?2",
            params![now, step_id],
        )
        .map_err(|e| e.to_string())?;

        let step = tx.query_row(
            "SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE id = ?1",
            [step_id],
            |row| {
                Ok(TaskStep {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    order_index: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    is_mandatory: row.get(5)?,
                    status: row.get(6)?,
                    completed_at: row.get(7)?,
                })
            },
        ).map_err(|e| e.to_string())?;

        if let Some(ev) = evidence_json {
            let ev_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
                 VALUES (?1, ?2, ?3, 'AGENT_NOTE', 'AGENT_REPORTED', ?4, ?5)",
                params![ev_id, step.task_id, step_id, ev, now],
            ).map_err(|e| format!("Failed to record step evidence record: {}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit step completion transaction: {}", e))?;

        drop(conn);
        self.emit_event(
            None,
            Some(&step.task_id),
            agent_id,
            "STEP_COMPLETED",
            json!({ "step_id": step_id, "title": step.title }),
        );
        Ok(step)
    }

    /// Complete Authoritative Task Submission & Automated Machine Verification Gate
    pub fn submit_task(&self, task_id: &str, agent_id: &str) -> Result<VerificationResult, String> {
        let task = self.get_task(task_id)?;

        // 1. Ownership enforcement
        if let Some(ref assigned) = task.assigned_agent_id {
            let (canon_assigned, ..) = Self::canonicalize_ide_identity(assigned, "");
            let (canon_caller, ..) = Self::canonicalize_ide_identity(agent_id, "");
            if assigned != agent_id && canon_assigned != canon_caller {
                return Err(format!(
                    "Task ownership violation: Task '{}' is owned by agent '{}', not '{}'",
                    task_id, assigned, agent_id
                ));
            }
        } else {
            return Err(format!(
                "Task ownership violation: Task '{}' is not assigned to any agent",
                task_id
            ));
        }

        let _proj = self
            .list_projects()?
            .into_iter()
            .find(|p| p.id == task.project_id)
            .ok_or("Project not found")?;
        let worktree_dir = match task.worktree_path.as_deref() {
            Some(path) => Path::new(path),
            None => return Err("Task has no worktree path allocated".to_string()),
        };

        if !worktree_dir.exists() {
            return Err(format!(
                "Task worktree does not exist at {:?}",
                worktree_dir
            ));
        }

        // 2. Cleanliness check (reject dirty worktree with uncommitted changes)
        if let Err(dirty_files) = self.git.check_worktree_cleanliness(worktree_dir) {
            return Ok(VerificationResult {
                is_valid: false,
                missing_mandatory_steps: Vec::new(),
                missing_evidence_step_ids: Vec::new(),
                unresolved_scope_violations: Vec::new(),
                failed_coordinator_checks: Vec::new(),
                rejection_reasons: vec![format!("Worktree has uncommitted modifications. Please commit all changes before submission: {:?}", dirty_files)],
            });
        }

        // Transition state to VERIFYING (validated)
        let now = Utc::now().to_rfc3339();
        self.transition_task_state(
            task_id,
            &[TaskState::Running],
            TaskState::Verifying,
            Some(agent_id),
            "TASK_VERIFYING",
            json!({ "substate": "VERIFYING" }),
        )?;
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET substate = 'VERIFYING' WHERE id = ?1",
            [task_id],
        )
        .map_err(|e| format!("Failed to set task substate: {}", e))?;

        // Retrieve or create active task attempt with strict error propagation and run_number compatibility
        let attempt_opt: Option<(String, i32)> = match conn
            .query_row(
                "SELECT id, attempt_number FROM task_attempts WHERE task_id = ?1 AND status = 'ACTIVE' ORDER BY attempt_number DESC LIMIT 1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ) {
                Ok(att) => Some(att),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => {
                    return Err(format!(
                        "Failed to query active task attempt for task '{}': {}",
                        task_id, e
                    ))
                }
            };

        let (attempt_id, _attempt_num) = if let Some(att) = attempt_opt {
            att
        } else {
            let new_id = Uuid::new_v4().to_string();
            let new_num: i32 = conn
                .query_row(
                    "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM task_attempts WHERE task_id = ?1",
                    [task_id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to compute next attempt number: {}", e))?;
            conn.execute(
                "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, run_number, base_sha, worktree_path, status, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ACTIVE', ?8)",
                rusqlite::params![
                    new_id,
                    task_id,
                    agent_id,
                    new_num,
                    new_num,
                    task.base_sha.as_deref().unwrap_or(""),
                    worktree_dir.to_string_lossy().as_ref(),
                    now,
                ],
            ).map_err(|e| format!("Failed to create task attempt: {}", e))?;
            (new_id, new_num)
        };
        drop(conn);

        // 3. Exact worktree HEAD SHA is authoritative
        let head_sha = self.git.get_worktree_head_sha(worktree_dir)?;

        // 4. Invalidate stale verifications from previous commit SHAs
        self.verify
            .invalidate_stale_verifications(task_id, &head_sha)?;

        // 5. Automatically execute comprehensive verification profile and machine evaluators
        self.verify.execute_profile_for_attempt(
            task_id,
            &attempt_id,
            &task.project_id,
            worktree_dir,
            &head_sha,
        )?;

        // 6. Perform actual Git mutation audit against held scope leases for this attempt.
        //    Fail CLOSED: without a resolvable base commit or a computable git diff the
        //    submission must error instead of auditing an empty change set.
        let base_sha = task.base_sha.as_deref().ok_or_else(|| {
            format!(
                "Failed to audit task '{}': no base commit recorded for this task",
                task_id
            )
        })?;
        let changed_files = self
            .git
            .get_worktree_mutations(worktree_dir, base_sha)
            .map_err(|e| {
                format!(
                    "Failed to compute git mutations for task '{}' audit: {}",
                    task_id, e
                )
            })?;
        self.scope
            .audit_attempt_mutations(task_id, Some(&attempt_id), agent_id, &changed_files)?;

        // 7. Verification checks gate evaluation
        let verify_res = self.verify.verify_task_submission(task_id, &head_sha)?;

        let now_finished = Utc::now().to_rfc3339();

        if verify_res.is_valid {
            // 8. Generate deterministic Proof-of-Completion bundle (strictly required before MERGE_READY)
            self.verify
                .generate_proof_bundle(
                    task_id,
                    &task.project_id,
                    Some(agent_id),
                    &task.description,
                    task.base_sha.as_deref().unwrap_or(""),
                    &head_sha,
                    &changed_files,
                    "Authoritative Coordinator Automated Verification Passed",
                )
                .map_err(|e| format!("Failed to generate proof bundle: {}", e))?;

            // 9. Auto-enqueue for serialized merge queue (strictly required before MERGE_READY)
            self.enqueue_task_by_id(&task.project_id, task_id)
                .map_err(|e| format!("Failed to enqueue task in merge queue: {}", e))?;

            // 10. Atomically transition state to MERGE_READY & attempt to VERIFIED & complete masterplan steps.
            //     The step-9 enqueue already transitioned VERIFYING -> MERGE_READY, so the write here
            //     is a no-op self-leg: guard it instead of letting the primitive reject it.
            let conn = self.db.lock();
            let current_state: String = conn
                .query_row("SELECT state FROM tasks WHERE id = ?1", [task_id], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if current_state != "MERGE_READY" {
                transition_task_state_on_conn(
                    &conn,
                    task_id,
                    &[TaskState::Verifying],
                    TaskState::MergeReady,
                )
                .map_err(|e| format!("Failed to transition task to MERGE_READY: {}", e))?;
            }
            let require_approval: bool = conn
                .query_row(
                    "SELECT require_milestone_approval FROM masterplans WHERE project_id = ?1 AND is_active = 1 LIMIT 1",
                    [&task.project_id],
                    |r| r.get(0),
                )
                .unwrap_or(true);
            let substate = if require_approval {
                "WAITING_FOR_INPUT"
            } else {
                "NONE"
            };

            conn.execute(
                "UPDATE tasks SET substate = ?1, head_sha = ?2 WHERE id = ?3",
                params![substate, head_sha, task_id],
            )
            .map_err(|e| e.to_string())?;

            conn.execute(
                "UPDATE task_attempts SET status = 'VERIFIED', head_sha = ?1, finished_at = ?2 WHERE id = ?3",
                params![head_sha, now_finished, attempt_id],
            ).map_err(|e| format!("Failed to update task attempt status: {}", e))?;

            conn.execute(
                "UPDATE masterplan_steps SET status = 'COMPLETED', updated_at = ?1 WHERE claimed_task_id = ?2",
                params![now_finished, task_id],
            ).map_err(|e| format!("Failed to update masterplan steps to COMPLETED: {}", e))?;
            drop(conn);

            self.emit_event(
                Some(&task.project_id),
                Some(task_id),
                Some(agent_id),
                "TASK_VERIFIED",
                json!({ "head_sha": head_sha }),
            );
        } else {
            let reasons_json = serde_json::to_string(&verify_res.rejection_reasons)
                .unwrap_or_else(|_| "[]".to_string());
            let conn = self.db.lock();
            transition_task_state_on_conn(
                &conn,
                task_id,
                &[TaskState::Verifying],
                TaskState::Failed,
            )
            .map_err(|e| format!("Failed to set task to FAILED: {}", e))?;
            conn.execute(
                "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                [task_id],
            )
            .map_err(|e| e.to_string())?;

            conn.execute(
                "UPDATE task_attempts SET status = 'FAILED', rejection_reasons = ?1, finished_at = ?2 WHERE id = ?3",
                params![reasons_json, now_finished, attempt_id],
            ).map_err(|e| format!("Failed to update task attempt status: {}", e))?;
            drop(conn);

            self.emit_event(
                Some(&task.project_id),
                Some(task_id),
                Some(agent_id),
                "TASK_VERIFICATION_FAILED",
                json!({ "reasons": verify_res.rejection_reasons }),
            );
        }

        Ok(verify_res)
    }

    /// Backend-Authoritative Enqueue by Task ID
    pub fn enqueue_task_by_id(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<MergeQueueItem, String> {
        let task = self.get_task(task_id)?;
        if task.project_id != project_id {
            return Err(format!(
                "Task '{}' does not belong to project '{}'",
                task_id, project_id
            ));
        }

        if task.state != TaskState::Review
            && task.state != TaskState::MergeReady
            && task.state != TaskState::Verifying
        {
            return Err(format!("Task '{}' is in state '{:?}'. Only tasks in VERIFYING, REVIEW, or MERGE_READY state can be enqueued for merge.", task_id, task.state));
        }

        let proj = self
            .list_projects()?
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or("Project not found")?;
        let repo_path = Path::new(&proj.path);

        let branch_name = task
            .branch_name
            .ok_or("Task has no branch name allocated")?;
        let worktree_dir = match task.worktree_path.as_deref() {
            Some(p) => Path::new(p),
            None => return Err("Task has no worktree allocated".to_string()),
        };

        let head_sha = self.git.get_worktree_head_sha(worktree_dir)?;

        // Ensure a proof bundle exists for this HEAD and that its canonical digest is
        // intact (D28): load the LATEST proof row for (task, head), recompute the hash
        // from the row's stored inputs + evidence tables, and refuse to enqueue when the
        // stored proof_hash no longer matches - a tampered row cannot pass the gate.
        let conn = self.db.lock();
        let proof_row: Option<StoredProofRow> = match conn
            .query_row(
                "SELECT task_id, project_id, attempt_id, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash
                 FROM proof_bundles
                 WHERE task_id = ?1 AND head_sha = ?2
                 ORDER BY generated_at DESC, id DESC LIMIT 1",
                rusqlite::params![task_id, head_sha],
                |r| {
                    Ok(StoredProofRow {
                        task_id: r.get(0)?,
                        project_id: r.get(1)?,
                        attempt_id: r.get(2)?,
                        base_sha: r.get(3)?,
                        head_sha: r.get(4)?,
                        files_changed_json: r.get(5)?,
                        diff_summary: r.get(6)?,
                        verification_runs_json: r.get(7)?,
                        criteria_json: r.get(8)?,
                        steps_json: r.get(9)?,
                        proof_hash: r.get(10)?,
                    })
                },
            ) {
                Ok(row) => Some(row),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => {
                    return Err(format!(
                        "Failed to query proof bundle for task '{}' at HEAD {}: {}",
                        task_id, head_sha, e
                    ))
                }
            };

        let stored = match proof_row {
            Some(row) => row,
            None => {
                return Err(format!("No valid proof bundle found for task '{}' at commit HEAD {}. Verification is required.", task_id, head_sha));
            }
        };

        let recomputed_proof_hash = Self::recompute_proof_hash_on_conn(&conn, &stored)?;

        if recomputed_proof_hash != stored.proof_hash {
            drop(conn);
            self.emit_event(
                Some(project_id),
                Some(task_id),
                task.assigned_agent_id.as_deref(),
                "PROOF_INTEGRITY_FAILURE",
                json!({
                    "head_sha": head_sha,
                    "stored_proof_hash": stored.proof_hash,
                    "recomputed_proof_hash": recomputed_proof_hash,
                }),
            );
            return Err(format!(
                "Proof integrity check failed: stored proof for task '{}' at HEAD {} does not match its recorded evidence (tamper detected).",
                task_id, head_sha
            ));
        }
        drop(conn);

        // Check if already enqueued in READY state (idempotent)
        {
            let conn = self.db.lock();
            let existing = conn.query_row(
                "SELECT id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at, processed_at
                 FROM merge_queue WHERE task_id = ?1 AND status = 'READY'",
                [task_id],
                |row| {
                    Ok(crate::models::MergeQueueItem {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        task_id: row.get(2)?,
                        branch_name: row.get(3)?,
                        target_branch: row.get(4)?,
                        position: row.get(5)?,
                        status: row.get(6)?,
                        base_sha: row.get(7)?,
                        head_sha: row.get(8)?,
                        queued_at: row.get(9)?,
                        processed_at: row.get(10)?,
                    })
                },
            ).ok();
            if let Some(item) = existing {
                return Ok(item);
            }
        }

        let base_sha = task
            .base_sha
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                self.git
                    .get_ref_sha(repo_path, &proj.target_branch)
                    .unwrap_or_default()
            });

        let item = self.merge.enqueue_task(
            project_id,
            task_id,
            &branch_name,
            &proj.target_branch,
            &base_sha,
            &head_sha,
        )?;

        // Mark task as MERGE_READY (validated; errors propagate - D19). Skip the no-op
        // self-leg when the task is already MERGE_READY (reconcile auto-heal carry-over).
        if task.state != TaskState::MergeReady {
            self.transition_task_state(
                task_id,
                &[TaskState::Review, TaskState::Verifying],
                TaskState::MergeReady,
                None,
                "TASK_ENQUEUED_FOR_MERGE",
                json!({ "position": item.position, "branch": branch_name }),
            )?;
        }

        Ok(item)
    }

    /// Recomputes the canonical proof digest from a proof row's stored snapshot inputs, mirroring
    /// VerificationEngine::generate_proof_bundle exactly: same length-prefixed field
    /// encoding, same field order, and exact evidence snapshot serialized at bundle creation time.
    fn recompute_proof_hash_on_conn(
        conn: &rusqlite::Connection,
        row: &StoredProofRow,
    ) -> Result<String, String> {
        fn push_field(hasher: &mut Sha256, value: &[u8]) {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value);
        }
        fn push_list_count(hasher: &mut Sha256, len: usize) {
            hasher.update((len as u64).to_be_bytes());
        }

        let mut files_changed: Vec<String> =
            serde_json::from_str(&row.files_changed_json).unwrap_or_default();
        files_changed.sort();

        let verification_runs: Vec<VerificationRun> =
            serde_json::from_str(&row.verification_runs_json).unwrap_or_default();

        let mut evals_stmt = conn
            .prepare(
                "SELECT id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at FROM evaluator_results WHERE task_id = ?1 AND commit_sha = ?2 AND attempt_id = ?3 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let evaluator_results: Vec<EvaluatorResult> = evals_stmt
            .query_map(
                rusqlite::params![row.task_id, row.head_sha, row.attempt_id],
                |row| {
                    Ok(EvaluatorResult {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        attempt_id: row.get(2)?,
                        criterion_id: row.get(3)?,
                        evaluator_name: row.get(4)?,
                        evaluator_type: row.get(5)?,
                        evaluator_version: row.get(6)?,
                        commit_sha: row.get(7)?,
                        exit_code: row.get(8)?,
                        stdout_output: row.get(9)?,
                        stderr_output: row.get(10)?,
                        output_sha256: row.get(11)?,
                        duration_ms: row.get(12)?,
                        passed: row.get(13)?,
                        evaluated_at: row.get(14)?,
                    })
                },
            )
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read evaluator result row: {}", e))?;

        let mut violations_stmt = conn
            .prepare(
                "SELECT id, task_id, agent_id, file_path, violation_type, detected_at, resolved FROM scope_violations WHERE task_id = ?1 AND attempt_id = ?2 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let scope_violations: Vec<ScopeViolation> = violations_stmt
            .query_map(rusqlite::params![row.task_id, row.attempt_id], |row| {
                Ok(ScopeViolation {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    file_path: row.get(3)?,
                    violation_type: row.get(4)?,
                    detected_at: row.get(5)?,
                    resolved: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read scope violation row: {}", e))?;

        let mut hasher = Sha256::new();
        push_field(&mut hasher, row.task_id.as_bytes());
        push_field(&mut hasher, row.project_id.as_bytes());
        push_field(&mut hasher, row.attempt_id.as_bytes());
        push_field(&mut hasher, row.base_sha.as_bytes());
        push_field(&mut hasher, row.head_sha.as_bytes());
        push_list_count(&mut hasher, files_changed.len());
        for f in &files_changed {
            push_field(&mut hasher, f.as_bytes());
        }
        push_field(&mut hasher, row.diff_summary.as_bytes());
        push_field(&mut hasher, row.criteria_json.as_bytes());
        push_field(&mut hasher, row.steps_json.as_bytes());
        push_list_count(&mut hasher, verification_runs.len());
        for run in &verification_runs {
            push_field(&mut hasher, run.check_name.as_bytes());
            push_field(&mut hasher, run.exit_code.to_string().as_bytes());
            push_field(&mut hasher, run.duration_ms.to_string().as_bytes());
            push_field(&mut hasher, run.timed_out.to_string().as_bytes());
            push_field(&mut hasher, run.stdout.as_bytes());
            push_field(&mut hasher, run.stderr.as_bytes());
        }
        push_list_count(&mut hasher, evaluator_results.len());
        for ev in &evaluator_results {
            push_field(&mut hasher, ev.evaluator_name.as_bytes());
            push_field(&mut hasher, ev.evaluator_version.as_bytes());
            let result_json = serde_json::to_string(ev).unwrap_or_else(|_| "{}".to_string());
            push_field(&mut hasher, result_json.as_bytes());
        }
        let violations_json =
            serde_json::to_string(&scope_violations).unwrap_or_else(|_| "[]".to_string());
        push_field(&mut hasher, violations_json.as_bytes());
        Ok(hex::encode(hasher.finalize()))
    }

    // --- Agents ---
    pub fn canonicalize_ide_identity(
        name: &str,
        agent_type: &str,
    ) -> (String, String, String, String) {
        let combined = format!("{} {}", name, agent_type)
            .to_lowercase()
            .replace(['_', ' '], "-");
        if combined.contains("antigravity") || combined.contains("agy") {
            (
                "antigravity".into(),
                "Antigravity".into(),
                "IDE".into(),
                "Google Antigravity Advanced Agentic Coding Assistant".into(),
            )
        } else if combined.contains("claude") {
            (
                "claude-code".into(),
                "Claude Code".into(),
                "CLI".into(),
                "Anthropic Claude Code Agentic Terminal Engine".into(),
            )
        } else if combined.contains("cursor") {
            (
                "cursor".into(),
                "Cursor".into(),
                "IDE".into(),
                "Cursor AI Coding Assistant".into(),
            )
        } else if combined.contains("opencode") {
            (
                "opencode".into(),
                "OpenCode".into(),
                "IDE".into(),
                "OpenCode Multi-Agent Orchestrator".into(),
            )
        } else if combined.contains("codex") || combined.contains("openai") {
            (
                "codex".into(),
                "OpenAI Codex".into(),
                "CLI".into(),
                "OpenAI Codex Agentic Coding Engine".into(),
            )
        } else if combined.contains("gemini") {
            (
                "gemini-cli".into(),
                "Gemini CLI".into(),
                "CLI".into(),
                "Google Gemini Developer CLI".into(),
            )
        } else if combined.contains("copilot") || combined.contains("vscode") {
            (
                "copilot".into(),
                "GitHub Copilot".into(),
                "IDE".into(),
                "GitHub Copilot / VS Code Agent".into(),
            )
        } else if combined.contains("windsurf") || combined.contains("codeium") {
            (
                "windsurf".into(),
                "Windsurf".into(),
                "IDE".into(),
                "Codeium Windsurf AI Cascade IDE".into(),
            )
        } else if combined.contains("junie") || combined.contains("jetbrains") {
            (
                "junie".into(),
                "Junie".into(),
                "IDE".into(),
                "JetBrains Junie AI Assistant".into(),
            )
        } else if combined.contains("aider") {
            (
                "aider".into(),
                "Aider".into(),
                "CLI".into(),
                "Aider AI Pair Programmer".into(),
            )
        } else {
            let clean_id = name.trim().to_lowercase().replace(['_', ' '], "-");
            let id = if clean_id.is_empty() {
                "custom-agent".to_string()
            } else {
                clean_id
            };
            let profile = format!("Custom AI Agent ({})", name);
            let default_type = if agent_type.trim().is_empty() {
                "Implementer".to_string()
            } else {
                agent_type.to_string()
            };
            (id, name.to_string(), default_type, profile)
        }
    }

    pub fn register_agent(&self, name: &str, agent_type: &str) -> Result<Agent, String> {
        let (canonical_id, canonical_name, default_type, profile) =
            Self::canonicalize_ide_identity(name, agent_type);
        let actual_type = if agent_type.trim().is_empty() || agent_type == "Generic" {
            default_type
        } else {
            agent_type.to_string()
        };
        let conn = self.db.lock();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + chrono::Duration::days(SESSION_INITIAL_TTL_DAYS)).to_rfc3339();
        // D11: session tokens must be unpredictable and non-forgeable. Same
        // construction as the master MCP token (security/mod.rs): two random
        // UUIDs plus nanos-of-now, SHA-256 hex, kept in the axf_sess_ namespace.
        let raw = format!(
            "{}-{}-{}",
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let session_token = format!("axf_sess_{}", hex::encode(hasher.finalize()));

        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at, session_token)
             VALUES (?1, ?2, ?3, ?4, 'IDLE', ?5, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 agent_type = excluded.agent_type,
                 profile = excluded.profile,
                 last_heartbeat = excluded.last_heartbeat,
                 session_token = excluded.session_token",
            rusqlite::params![canonical_id, canonical_name, actual_type, profile, now_str, session_token],
        ).map_err(|e| format!("Failed to register canonical agent: {}", e))?;

        let sess_id = format!("sess_{}", canonical_id.replace('-', "_"));
        conn.execute(
            "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 expires_at = excluded.expires_at,
                 last_activity_at = excluded.last_activity_at,
                 session_token = excluded.session_token",
            rusqlite::params![sess_id, canonical_id, session_token, now_str, expires_str],
        ).map_err(|e| format!("Failed to register agent session: {}", e))?;

        // Authoritative reload of persisted canonical agent row
        let (id, name_db, agent_type_db, profile_db, status_db, last_heartbeat_db, created_at_db): (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents WHERE id = ?1",
                [&canonical_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .map_err(|e| format!("Failed to reload canonical agent: {}", e))?;

        let active_task: Option<(String, String)> = conn
            .query_row(
                "SELECT id, title FROM tasks WHERE (assigned_agent_id = ?1 OR assigned_agent_id = ?2) AND state IN ('RUNNING', 'VERIFYING') AND is_stale = 0 ORDER BY updated_at DESC LIMIT 1",
                [&id, &canonical_name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        let (active_task_id, active_task_title) = match active_task {
            Some((t_id, t_title)) => (Some(t_id), Some(t_title)),
            None => (None, None),
        };

        let last_dt = chrono::DateTime::parse_from_rfc3339(&last_heartbeat_db)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(now);
        let elapsed_secs = (now - last_dt).num_seconds().max(0);

        let final_status = if active_task_id.is_some() {
            if elapsed_secs > 120 {
                "DISCONNECTED".to_string()
            } else {
                "WORKING".to_string()
            }
        } else if elapsed_secs > 120 {
            "DISCONNECTED".to_string()
        } else if status_db == "WORKING" || status_db == "RUNNING" {
            status_db
        } else {
            "IDLE".to_string()
        };

        let agent = Agent {
            id: canonical_id.clone(),
            name: name_db,
            agent_type: agent_type_db,
            profile: profile_db,
            status: final_status,
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: last_heartbeat_db,
            created_at: created_at_db,
            session_token: Some(session_token),
            active_task_id,
            active_task_title,
            last_seen_seconds: Some(elapsed_secs),
        };

        drop(conn);
        self.emit_event(
            None,
            None,
            Some(&canonical_id),
            "AGENT_REGISTERED",
            json!({ "name": agent.name, "type": actual_type, "id": canonical_id }),
        );
        Ok(agent)
    }

    pub fn get_agent_by_session(&self, token: &str) -> Option<Agent> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        let agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM agent_sessions WHERE session_token = ?1 AND expires_at > ?2",
                rusqlite::params![token, now],
                |r| r.get(0),
            )
            .ok()?;

        let mut stmt = conn
            .prepare("SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents WHERE id = ?1")
            .ok()?;

        let agent = stmt
            .query_row([&agent_id], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    agent_type: row.get(2)?,
                    profile: row.get(3)?,
                    status: row.get(4)?,
                    capabilities: AgentCapabilitySet::default(),
                    last_heartbeat: row.get(5)?,
                    created_at: row.get(6)?,
                    session_token: Some(token.to_string()),
                    active_task_id: None,
                    active_task_title: None,
                    last_seen_seconds: None,
                })
            })
            .ok();

        if agent.is_some() {
            conn.execute(
                "UPDATE agent_sessions SET last_activity_at = ?1 WHERE session_token = ?2",
                rusqlite::params![now, token],
            )
            .ok();
        }

        agent
    }

    pub fn satisfy_acceptance_criterion(
        &self,
        task_id: &str,
        criterion_id: &str,
        evidence: Option<&str>,
    ) -> Result<(), String> {
        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let rows_affected = tx
            .execute(
                "UPDATE acceptance_criteria SET is_satisfied = 1 WHERE id = ?1 AND task_id = ?2",
                rusqlite::params![criterion_id, task_id],
            )
            .map_err(|e| e.to_string())?;

        if rows_affected == 0 {
            return Err(format!(
                "Criterion '{}' for task '{}' not found",
                criterion_id, task_id
            ));
        }

        let ev_id = Uuid::new_v4().to_string();
        let note = evidence.unwrap_or("Manual User / Verification Sign-off");
        tx.execute(
            "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
             VALUES (?1, ?2, NULL, 'USER_APPROVAL', 'COORDINATOR_OBSERVED', ?3, ?4)",
            rusqlite::params![ev_id, task_id, note, now],
        ).map_err(|e| format!("Failed to record acceptance criterion evidence record: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit acceptance criterion satisfaction: {}", e))?;

        drop(conn);
        self.emit_event(
            None,
            Some(task_id),
            None,
            "CRITERIA_SATISFIED",
            json!({ "criterion_id": criterion_id }),
        );
        Ok(())
    }

    pub fn touch_agent_activity(&self, agent_id: &str) {
        if agent_id.trim().is_empty() {
            return;
        }
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2 OR id = ?3",
            rusqlite::params![now, agent_id, canonical_id],
        )
        .ok();
        conn.execute(
            "UPDATE agent_sessions SET last_activity_at = ?1 WHERE agent_id = ?2 OR agent_id = ?3",
            rusqlite::params![now, agent_id, canonical_id],
        )
        .ok();
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents ORDER BY name")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    agent_type: row.get(2)?,
                    profile: row.get(3)?,
                    status: row.get(4)?,
                    capabilities: AgentCapabilitySet::default(),
                    last_heartbeat: row.get(5)?,
                    created_at: row.get(6)?,
                    session_token: None,
                    active_task_id: None,
                    active_task_title: None,
                    last_seen_seconds: None,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut raw_agents: Vec<Agent> = rows.flatten().collect();
        let now = Utc::now();

        // Dynamically evaluate live status and in-flight tasks for each agent
        for agent in &mut raw_agents {
            let last_dt = chrono::DateTime::parse_from_rfc3339(&agent.last_heartbeat)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);
            let elapsed_secs = (now - last_dt).num_seconds().max(0);
            agent.last_seen_seconds = Some(elapsed_secs);

            // Query active in-flight task assigned to this agent (canonical ID or raw ID)
            let active_task: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, title FROM tasks WHERE (assigned_agent_id = ?1 OR assigned_agent_id = ?2) AND state IN ('RUNNING', 'VERIFYING') AND is_stale = 0 ORDER BY updated_at DESC LIMIT 1",
                    [&agent.id, &agent.name],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();

            if let Some((t_id, t_title)) = active_task {
                agent.active_task_id = Some(t_id);
                agent.active_task_title = Some(t_title);
                if elapsed_secs > 120 {
                    // Silent > 2 minutes with in-flight task
                    agent.status = "DISCONNECTED".to_string();
                } else {
                    agent.status = "WORKING".to_string();
                }
            } else if elapsed_secs > 120 {
                agent.status = "DISCONNECTED".to_string();
            } else {
                agent.status = "IDLE".to_string();
            }
        }

        Ok(raw_agents)
    }

    /// Unregisters an agent with complete transactional cleanup: every non-DONE
    /// task it owns is cancelled (scope leases released, masterplan steps
    /// reverted to PENDING, merge-queue entries removed, CANCELLED + stale),
    /// any masterplan steps still CLAIMED by the agent are reverted, remaining
    /// scope leases are released, ACTIVE agent_runs are terminated (agent_runs
    /// has no FK to agents; the CANCELLED tasks persist), and the agent row is
    /// deleted (cascades sessions). All of this runs inside ONE transaction —
    /// any failure aborts and returns Err, leaving no partial cleanup. Worktree
    /// removal (containment-checked) and event emission happen after commit,
    /// since both lock the DB.
    pub fn unregister_agent(&self, agent_id: &str) -> Result<(), String> {
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");

        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start unregister transaction: {}", e))?;
        let now = Utc::now().to_rfc3339();

        // 1. Collect every task owned by the agent. DONE is excluded: it is
        //    final (the merge path owns its worktree lifecycle) and cancelling
        //    it is illegal by design.
        let mut stmt = tx
            .prepare(
                "SELECT id, project_id, worktree_path FROM tasks
                 WHERE (assigned_agent_id = ?1 OR assigned_agent_id = ?2) AND state != 'DONE'",
            )
            .map_err(|e| e.to_string())?;
        let owned: Vec<(String, String, Option<String>)> = stmt
            .query_map([agent_id, &canonical_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        drop(stmt);

        // 2. Unclaim every owned task via the shared cancellation semantics
        for (tid, _, _) in &owned {
            cancel_task_inner(
                &tx,
                tid,
                None,
                Some("Agent unregistered: task reclaimed to masterplan pending backlog"),
            )?;
        }

        // 3. Revert any masterplan steps still CLAIMED by the agent
        tx.execute(
            "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1 WHERE (claimed_agent_id = ?2 OR claimed_agent_id = ?3) AND status = 'CLAIMED'",
            params![now, agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        // 4. Release any remaining scope leases held by the agent
        tx.execute(
            "DELETE FROM scope_leases WHERE agent_id = ?1 OR agent_id = ?2",
            params![agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        // 5. Terminate ACTIVE agent runs: safe state transition, not deletion
        //    (the runs reference task_id rows that persist as CANCELLED)
        tx.execute(
            "UPDATE agent_runs SET status = 'FAILED', finished_at = ?1 WHERE (agent_id = ?2 OR agent_id = ?3) AND status = 'ACTIVE'",
            params![now, agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        // 6. Delete the agent row (cascades agent_sessions)
        tx.execute(
            "DELETE FROM agents WHERE id = ?1 OR id = ?2",
            params![agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        tx.commit()
            .map_err(|e| format!("Failed to commit agent unregistration: {}", e))?;
        drop(conn);

        // 7. Post-commit: containment-checked worktree cleanup for every
        //    unclaimed task (git ops only, no DB access while worktrees move)
        for (tid, proj_id, wt_path) in &owned {
            if let Some(ref wt_path) = wt_path {
                let p = Path::new(wt_path);
                if p.exists() {
                    if let Ok(projs) = self.list_projects() {
                        if let Some(proj) = projs.into_iter().find(|pr| pr.id == *proj_id) {
                            let repo_path = Path::new(&proj.path);
                            let managed_roots =
                                vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
                            if self.git.is_managed_worktree(repo_path, &managed_roots, p) {
                                let _ = self.git.remove_worktree(repo_path, &managed_roots, p);
                                let _ = crate::git::GitService::safe_remove_dir_all(p);
                            } else {
                                warn!(
                                    "Refusing to delete unmanaged worktree path {:?} during agent unregistration",
                                    p
                                );
                                self.emit_event(
                                    Some(proj_id.as_str()),
                                    Some(tid.as_str()),
                                    Some(agent_id),
                                    "WORKTREE_SAFETY_REFUSAL",
                                    json!({ "path": wt_path }),
                                );
                            }
                        }
                    }
                }
            }
        }

        // 8. Post-commit: event emission (each cancelled task + the unregister)
        for (tid, proj_id, _) in &owned {
            self.emit_event(
                Some(proj_id.as_str()),
                Some(tid.as_str()),
                Some(agent_id),
                "TASK_CANCELLED",
                json!({ "task_id": tid, "reason": "Agent unregistered" }),
            );
        }
        self.emit_event(
            None,
            None,
            Some(agent_id),
            "AGENT_UNREGISTERED",
            json!({ "agent_id": agent_id }),
        );
        Ok(())
    }

    /// Safely unclaims all active tasks for an agent: reverts masterplan steps to PENDING, releases scopes, and cleans worktrees
    pub fn unclaim_agent_tasks(&self, agent_id: &str) -> Result<Vec<String>, String> {
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id FROM tasks WHERE (assigned_agent_id = ?1 OR assigned_agent_id = ?2) AND state IN ('RUNNING', 'VERIFYING') AND is_stale = 0")
            .map_err(|e| e.to_string())?;

        let task_ids: Vec<String> = stmt
            .query_map([agent_id, &canonical_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        drop(stmt);
        drop(conn);

        let mut unassigned = Vec::new();
        for tid in &task_ids {
            if let Ok(cancelled_task) = self.cancel_task(
                tid,
                Some(agent_id),
                Some("Reclaimed to masterplan pending backlog by user/coordinator"),
            ) {
                unassigned.push(cancelled_task.id);
            }
        }

        self.emit_event(
            None,
            None,
            Some(agent_id),
            "AGENT_TASKS_UNCLAIMED",
            json!({ "agent_id": agent_id, "reclaimed_tasks": unassigned }),
        );
        Ok(unassigned)
    }

    /// Sets task substate to WAITING_FOR_INPUT and refreshes agent heartbeat.
    /// Used when an agent is waiting for external IDE or user permission.
    pub fn set_task_waiting_for_permission(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> Result<(), String> {
        let (canonical_id, ..) = Self::canonicalize_ide_identity(agent_id, "");
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;
        let now = Utc::now().to_rfc3339();

        let assigned: Option<String> = tx
            .query_row(
                "SELECT assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

        if let Some(ref aid) = assigned {
            let (canon_assigned, ..) = Self::canonicalize_ide_identity(aid, "");
            if aid != agent_id && canon_assigned != canonical_id {
                return Err(format!(
                    "Task ownership violation: Task '{}' is owned by '{}', caller is '{}'",
                    task_id, aid, agent_id
                ));
            }
        } else {
            return Err(format!("Task '{}' is not assigned", task_id));
        }

        tx.execute(
            "UPDATE tasks SET substate = 'WAITING_FOR_INPUT', updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2 OR id = ?3",
            params![now, agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);

        self.emit_event(
            None,
            Some(task_id),
            Some(agent_id),
            "TASK_WAITING_FOR_PERMISSION",
            json!({ "task_id": task_id, "substate": "WAITING_FOR_INPUT" }),
        );
        Ok(())
    }

    /// Periodic stale-recovery sweep (defects D21/D37). Recovers state orphaned by
    /// disconnected agents. Idempotent, safe transitions only:
    ///   1. Revoke sessions past `expires_at` (delete `agent_sessions` rows; the
    ///      session lookup in get_agent_by_session already enforces expiry, so this
    ///      is cleanup — deleting the row also kills the dead session's token).
    ///   2. Cancel RUNNING/VERIFYING/CLAIMING tasks whose assigned agent's
    ///      `last_heartbeat` is older than STALE_AGENT_GRACE (or whose agent row no
    ///      longer exists) through the same safe unclaim path as Task 10.1
    ///      (CANCELLED + stale, steps -> PENDING, worktree removed, merge-queue
    ///      entry removed). Never touches MERGE_READY/DONE/BLOCKED/CANCELLED tasks.
    ///   3. Revert masterplan steps still CLAIMED whose `claimed_agent_id` no longer
    ///      exists (agent deleted) back to PENDING with claimed_agent_id NULL.
    ///
    /// Emits STALE_RECOVERY events per recovered item. Post-commit work (worktree
    /// removal + event emission) runs after the transaction commits, since both
    /// lock the DB.
    pub fn stale_recovery_sweep(&self) -> Result<(), String> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let stale_before = (now - chrono::Duration::seconds(STALE_AGENT_GRACE)).to_rfc3339();
        let waiting_stale_before =
            (now - chrono::Duration::seconds(WAITING_PERMISSION_GRACE)).to_rfc3339();

        // ---- Phase 1: collect everything on the live connection ----
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start stale-recovery transaction: {}", e))?;

        // 1. Revoke expired sessions
        let revoked_sessions: Vec<String> = {
            let mut stmt = tx
                .prepare("SELECT id FROM agent_sessions WHERE expires_at <= ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([&now_str], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            let ids = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read expired session rows: {}", e))?;
            drop(stmt);
            tx.execute(
                "DELETE FROM agent_sessions WHERE expires_at <= ?1",
                [&now_str],
            )
            .map_err(|e| e.to_string())?;
            ids
        };

        // 2. Collect stale in-flight tasks: assigned agent heartbeat older than
        //    STALE_AGENT_GRACE (or WAITING_PERMISSION_GRACE if waiting for permission),
        //    or the assigned agent row is gone entirely.
        let stale_tasks: Vec<(String, String, Option<String>, Option<String>)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT t.id, t.project_id, t.worktree_path, t.assigned_agent_id
                     FROM tasks t
                     WHERE t.state IN ('RUNNING', 'VERIFYING', 'CLAIMING')
                       AND t.is_stale = 0
                       AND t.assigned_agent_id IS NOT NULL AND t.assigned_agent_id != ''
                       AND (
                         NOT EXISTS (
                           SELECT 1 FROM agents a
                           WHERE a.id = t.assigned_agent_id OR a.name = t.assigned_agent_id
                         )
                         OR EXISTS (
                           SELECT 1 FROM agents a
                           WHERE (a.id = t.assigned_agent_id OR a.name = t.assigned_agent_id)
                             AND (
                               (t.substate != 'WAITING_FOR_INPUT' AND a.last_heartbeat < ?1)
                               OR (t.substate = 'WAITING_FOR_INPUT' AND a.last_heartbeat < ?2)
                             )
                         )
                       )",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([&stale_before, &waiting_stale_before], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read stale task rows: {}", e))?
        };

        // 3. Collect CLAIMED masterplan steps whose claiming agent no longer exists or whose task is dead/missing
        let orphaned_steps: Vec<String> = {
            let mut stmt = tx
                .prepare(
                    "SELECT s.id FROM masterplan_steps s
                     WHERE s.status = 'CLAIMED'
                       AND (
                         s.claimed_agent_id IS NULL
                         OR s.claimed_task_id IS NULL
                         OR NOT EXISTS (
                           SELECT 1 FROM agents a WHERE a.id = s.claimed_agent_id OR a.name = s.claimed_agent_id
                         )
                         OR NOT EXISTS (
                           SELECT 1 FROM tasks t WHERE t.id = s.claimed_task_id AND t.state IN ('RUNNING', 'VERIFYING', 'CLAIMING', 'VERIFIED', 'REVIEW', 'MERGE_READY')
                         )
                       )",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read orphaned step rows: {}", e))?;
            rows
        };

        // ---- Phase 2: act on the transaction ----
        for (tid, _, _, _) in &stale_tasks {
            cancel_task_inner(
                &tx,
                tid,
                None,
                Some("Stale agent recovery: task reclaimed to masterplan pending backlog"),
            )?;
        }
        for sid in &orphaned_steps {
            tx.execute(
                "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1 WHERE id = ?2 AND status = 'CLAIMED'",
                params![now_str, sid],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit stale-recovery sweep: {}", e))?;
        drop(conn);

        // ---- Phase 3: post-commit work (worktree removal + events; both lock DB) ----
        for (tid, proj_id, wt_path, agent_id) in &stale_tasks {
            if let Some(ref wt_path) = wt_path {
                let p = Path::new(wt_path);
                if p.exists() {
                    if let Ok(projs) = self.list_projects() {
                        if let Some(proj) = projs.into_iter().find(|pr| pr.id == *proj_id) {
                            let repo_path = Path::new(&proj.path);
                            let managed_roots =
                                vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
                            if self.git.is_managed_worktree(repo_path, &managed_roots, p) {
                                let _ = self.git.remove_worktree(repo_path, &managed_roots, p);
                                let _ = crate::git::GitService::safe_remove_dir_all(p);
                            } else {
                                warn!(
                                    "Refusing to delete unmanaged worktree path {:?} during stale recovery",
                                    p
                                );
                                self.emit_event(
                                    Some(proj_id.as_str()),
                                    Some(tid.as_str()),
                                    agent_id.as_deref(),
                                    "WORKTREE_SAFETY_REFUSAL",
                                    json!({ "path": wt_path }),
                                );
                            }
                        }
                    }
                }
            }
            self.emit_event(
                Some(proj_id.as_str()),
                Some(tid.as_str()),
                agent_id.as_deref(),
                "STALE_RECOVERY",
                json!({ "kind": "task", "task_id": tid, "reason": "assigned agent heartbeat stale" }),
            );
        }
        for sid in &orphaned_steps {
            self.emit_event(
                None,
                None,
                None,
                "STALE_RECOVERY",
                json!({ "kind": "masterplan_step", "step_id": sid, "reason": "claiming agent no longer exists" }),
            );
        }
        for sid in &revoked_sessions {
            self.emit_event(
                None,
                None,
                None,
                "STALE_RECOVERY",
                json!({ "kind": "session", "session_id": sid, "reason": "session expired" }),
            );
        }

        Ok(())
    }

    /// Forces an agent to IDLE state, unclaiming any active tasks and resetting heartbeat
    pub fn force_agent_idle(&self, agent_id: &str) -> Result<(), String> {
        let _ = self.unclaim_agent_tasks(agent_id);
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");

        conn.execute(
            "UPDATE agents SET status = 'IDLE', last_heartbeat = ?1 WHERE id = ?2 OR id = ?3",
            rusqlite::params![now, agent_id, canonical_id],
        )
        .map_err(|e| e.to_string())?;

        drop(conn);
        self.emit_event(
            None,
            None,
            Some(agent_id),
            "AGENT_STATUS_CHANGED",
            json!({ "agent_id": agent_id, "status": "IDLE" }),
        );
        Ok(())
    }

    pub fn is_agent_registered(&self, agent_id: &str) -> bool {
        if agent_id.trim().is_empty() {
            return false;
        }
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");
        let conn = self.db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agents WHERE id = ?1 OR id = ?2",
                [agent_id, &canonical_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    pub fn agent_heartbeat(&self, agent_id: &str) -> Result<(), String> {
        let (canonical_id, _, _, _) = Self::canonicalize_ide_identity(agent_id, "");
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + chrono::Duration::hours(4)).to_rfc3339();

        let updated = tx
            .execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2 OR id = ?3",
                rusqlite::params![now_str, agent_id, canonical_id],
            )
            .map_err(|e| e.to_string())?;

        if updated == 0 {
            return Err(format!("Agent '{}' not found", agent_id));
        }

        // Transactionally renew active scope leases for this agent
        tx.execute(
            "UPDATE scope_leases SET expires_at = ?1 WHERE agent_id = ?2 OR agent_id = ?3",
            params![expires_str, agent_id, canonical_id],
        )
        .map_err(|e| format!("Failed to renew scope leases on heartbeat: {}", e))?;

        // Slide the session expiry forward so active agents never expire.
        let session_slide_expires_str =
            (now + chrono::Duration::days(SESSION_HEARTBEAT_SLIDE_DAYS)).to_rfc3339();
        tx.execute(
            "UPDATE agent_sessions SET
                expires_at = CASE WHEN expires_at < ?1 THEN ?1 ELSE expires_at END,
                last_activity_at = ?2
             WHERE agent_id = ?3 OR agent_id = ?4",
            params![session_slide_expires_str, now_str, agent_id, canonical_id],
        )
        .map_err(|e| format!("Failed to renew agent session on heartbeat: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit heartbeat transaction: {}", e))?;
        drop(conn);
        Ok(())
    }

    pub fn get_project_context(&self, project_id: &str) -> Result<ProjectContextPack, String> {
        let proj = self
            .list_projects()?
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project '{}' not found", project_id))?;

        let conn = self.db.lock();

        // 1. Contract & Hash
        let (contract_hash, contract_overview): (String, String) = conn
            .query_row(
                "SELECT contract_hash, overview FROM project_contracts WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| {
                let mut hasher = Sha256::new();
                hasher.update(proj.master_spec.as_bytes());
                (hex::encode(hasher.finalize()), proj.master_spec.clone())
            });

        // 2. Real Project Rules
        let mut stmt_rules = conn
            .prepare(
                "SELECT rule_text FROM project_rules WHERE project_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let project_rules: Vec<String> = stmt_rules
            .query_map([project_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read project rule row: {}", e))?;

        // 3. Real Project Memory
        let mut stmt_mem = conn
            .prepare(
                "SELECT content FROM project_memory WHERE project_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let project_memory: Vec<String> = stmt_mem
            .query_map([project_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read project memory row: {}", e))?;

        Ok(ProjectContextPack {
            project_id: proj.id,
            project_name: proj.name,
            contract_hash,
            contract_overview,
            project_rules,
            project_memory,
        })
    }

    pub fn get_context_pack(&self, project_id: &str, task_id: &str) -> Result<ContextPack, String> {
        let task = self.get_task(task_id)?;
        if task.project_id != project_id {
            return Err(format!(
                "Task '{}' belongs to project '{}', not '{}'",
                task_id, task.project_id, project_id
            ));
        }

        let proj_ctx = self.get_project_context(project_id)?;

        let conn = self.db.lock();

        // Steps & Criteria
        let mut stmt_steps = conn.prepare("SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE task_id = ?1 ORDER BY order_index ASC").map_err(|e| e.to_string())?;
        let steps: Vec<TaskStep> = stmt_steps
            .query_map([task_id], |r| {
                Ok(TaskStep {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    order_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    is_mandatory: r.get(5)?,
                    status: r.get(6)?,
                    completed_at: r.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read task step row: {}", e))?;

        let mut stmt_crit = conn.prepare("SELECT id, task_id, criterion, is_satisfied, is_locked FROM acceptance_criteria WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let criteria: Vec<AcceptanceCriteria> = stmt_crit
            .query_map([task_id], |r| {
                Ok(AcceptanceCriteria {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    criterion: r.get(2)?,
                    is_satisfied: r.get(3)?,
                    is_locked: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read acceptance criteria row: {}", e))?;

        // Blocking Dependencies
        let mut stmt_deps = conn.prepare("SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1 AND dependency_type = 'BLOCKS'").map_err(|e| e.to_string())?;
        let dependencies: Vec<String> = stmt_deps
            .query_map([task_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read task dependency row: {}", e))?;

        // Scope Leases
        let mut stmt_leases = conn.prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let leases: Vec<ScopeLease> = stmt_leases
            .query_map([task_id], |r| {
                Ok(ScopeLease {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    pattern: r.get(3)?,
                    access_type: r.get(4)?,
                    expires_at: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read scope lease row: {}", e))?;

        Ok(ContextPack {
            project_id: proj_ctx.project_id,
            project_name: proj_ctx.project_name,
            contract_hash: proj_ctx.contract_hash,
            contract_overview: proj_ctx.contract_overview,
            project_rules: proj_ctx.project_rules,
            project_memory: proj_ctx.project_memory,
            task_id: task.id,
            task_title: task.title,
            task_prompt: task.description,
            task_state: task.state.as_str().to_string(),
            task_substate: task.substate.as_str().to_string(),
            acceptance_criteria: criteria,
            required_steps: steps,
            dependencies,
            reserved_scope: leases,
            current_worktree: task.worktree_path,
            current_branch: task.branch_name,
            base_sha: task.base_sha,
            head_sha: task.head_sha,
        })
    }

    pub fn create_or_update_masterplan(
        &self,
        project_id: &str,
        raw_text: &str,
        target_step_count: i32,
        max_steps_per_agent: i32,
    ) -> Result<Masterplan, String> {
        let now = Utc::now().to_rfc3339();

        let existing: Option<(String, String)> = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT id, raw_text FROM masterplans WHERE project_id = ?1 AND is_active = 1 LIMIT 1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
            .or_else(|| {
                conn.query_row(
                    "SELECT id, raw_text FROM masterplans WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                    [project_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).ok()
            })
        };

        // Cancel unmerged in-flight tasks bound to this specific masterplan
        let uncompleted_tasks: Vec<String> = {
            let conn = self.db.lock();
            if let Some((ref plan_id, _)) = existing {
                let mut stmt = conn
                    .prepare("SELECT id FROM tasks WHERE masterplan_id = ?1 AND state != 'DONE' AND state != 'CANCELLED'")
                    .map_err(|e| e.to_string())?;
                let ids = stmt
                    .query_map([plan_id], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .collect();
                ids
            } else {
                let mut stmt = conn
                    .prepare("SELECT id FROM tasks WHERE project_id = ?1 AND masterplan_id IS NULL AND state != 'DONE' AND state != 'CANCELLED'")
                    .map_err(|e| e.to_string())?;
                let ids = stmt
                    .query_map([project_id], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .collect();
                ids
            }
        };

        for tid in &uncompleted_tasks {
            self.cancel_task(
                tid,
                None,
                Some("Masterplan created/updated with new specification text"),
            )
            .map_err(|e| {
                format!(
                    "Failed to cancel in-flight task '{}' during masterplan update: {}",
                    tid, e
                )
            })?;
        }

        let (plan_id, plan_title, require_approval, is_active) = if let Some((id, old_text)) =
            existing
        {
            let mut conn = self.db.lock();
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            let rev_id = Uuid::new_v4().to_string();
            let rev_num: i32 = tx
                .query_row(
                    "SELECT COALESCE(MAX(revision_number), 0) + 1 FROM masterplan_revisions WHERE masterplan_id = ?1",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to compute masterplan revision number: {}", e))?;

            let (existing_title, existing_approval, existing_active): (String, bool, bool) = tx
                .query_row(
                    "SELECT COALESCE(title, 'Masterplan'), COALESCE(require_milestone_approval, 1), COALESCE(is_active, 1) FROM masterplans WHERE id = ?1",
                    [&id],
                    |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get::<_, i64>(2)? != 0)),
                )
                .map_err(|e| format!("Failed to query existing masterplan metadata: {}", e))?;

            tx.execute(
                "INSERT INTO masterplan_revisions (id, masterplan_id, project_id, revision_number, raw_text, reason, steps_snapshot_json, archived_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'User edited masterplan specification', '[]', ?6)",
                rusqlite::params![rev_id, id, project_id, rev_num, old_text, now],
            )
            .map_err(|e| format!("Failed to insert masterplan revision: {}", e))?;

            tx.execute(
                "DELETE FROM masterplan_steps WHERE masterplan_id = ?1",
                [&id],
            )
            .map_err(|e| format!("Failed to delete existing masterplan steps: {}", e))?;

            tx.execute(
                "UPDATE masterplans SET raw_text = ?1, status = 'UNSORTED', target_step_count = ?2, max_steps_per_agent = ?3, is_active = ?4, updated_at = ?5 WHERE id = ?6",
                params![raw_text, target_step_count, max_steps_per_agent, if existing_active { 1 } else { 0 }, now, id],
            )
            .map_err(|e| format!("Failed to update masterplan: {}", e))?;

            tx.commit()
                .map_err(|e| format!("Failed to commit masterplan update transaction: {}", e))?;

            (id, existing_title, existing_approval, existing_active)
        } else {
            let id = Uuid::new_v4().to_string();
            let title = "Primary Masterplan".to_string();
            let conn = self.db.lock();
            conn.execute(
                "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, require_milestone_approval, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'UNSORTED', ?5, ?6, 1, 1, ?7, ?7)",
                params![id, project_id, title, raw_text, target_step_count, max_steps_per_agent, now],
            ).map_err(|e| e.to_string())?;
            (id, title, true, true)
        };

        self.emit_event(
            Some(project_id),
            None,
            None,
            "MASTERPLAN_UPDATED",
            json!({ "status": "UNSORTED", "plan_id": plan_id }),
        );

        Ok(Masterplan {
            id: plan_id,
            project_id: project_id.to_string(),
            title: plan_title,
            raw_text: raw_text.to_string(),
            status: "UNSORTED".to_string(),
            target_step_count,
            max_steps_per_agent,
            require_milestone_approval: require_approval,
            is_active,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn create_masterplan(
        &self,
        project_id: &str,
        title: Option<&str>,
        raw_text: &str,
        target_step_count: i32,
        max_steps_per_agent: i32,
        activate: bool,
    ) -> Result<Masterplan, String> {
        let now = Utc::now().to_rfc3339();
        let plan_id = Uuid::new_v4().to_string();
        let plan_title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or("Masterplan");

        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        if activate {
            tx.execute(
                "UPDATE masterplans SET is_active = 0, updated_at = ?1 WHERE project_id = ?2",
                params![now, project_id],
            )
            .map_err(|e| e.to_string())?;
        }

        let is_act = if activate { 1 } else { 0 };
        tx.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, require_milestone_approval, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'UNSORTED', ?5, ?6, 1, ?7, ?8, ?8)",
            params![plan_id, project_id, plan_title, raw_text, target_step_count, max_steps_per_agent, is_act, now],
        ).map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);

        self.emit_event(
            Some(project_id),
            None,
            None,
            "MASTERPLAN_CREATED",
            json!({ "plan_id": plan_id, "title": plan_title, "is_active": activate }),
        );

        Ok(Masterplan {
            id: plan_id,
            project_id: project_id.to_string(),
            title: plan_title.to_string(),
            raw_text: raw_text.to_string(),
            status: "UNSORTED".to_string(),
            target_step_count,
            max_steps_per_agent,
            require_milestone_approval: true,
            is_active: activate,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_masterplans_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<Masterplan>, String> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, COALESCE(title, 'Masterplan'), raw_text, status, target_step_count, max_steps_per_agent, COALESCE(require_milestone_approval, 1), COALESCE(is_active, 0), created_at, updated_at
             FROM masterplans WHERE project_id = ?1
             ORDER BY is_active DESC, updated_at DESC",
        ).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([project_id], |r| {
                Ok(Masterplan {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    title: r.get(2)?,
                    raw_text: r.get(3)?,
                    status: r.get(4)?,
                    target_step_count: r.get(5)?,
                    max_steps_per_agent: r.get(6)?,
                    require_milestone_approval: r.get::<_, i64>(7)? != 0,
                    is_active: r.get::<_, i64>(8)? != 0,
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.flatten().collect())
    }

    pub fn get_masterplan_by_id(&self, masterplan_id: &str) -> Result<Option<Masterplan>, String> {
        let conn = self.db.lock();
        let plan = conn.query_row(
            "SELECT id, project_id, COALESCE(title, 'Masterplan'), raw_text, status, target_step_count, max_steps_per_agent, COALESCE(require_milestone_approval, 1), COALESCE(is_active, 0), created_at, updated_at
             FROM masterplans WHERE id = ?1",
            [masterplan_id],
            |r| {
                Ok(Masterplan {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    title: r.get(2)?,
                    raw_text: r.get(3)?,
                    status: r.get(4)?,
                    target_step_count: r.get(5)?,
                    max_steps_per_agent: r.get(6)?,
                    require_milestone_approval: r.get::<_, i64>(7)? != 0,
                    is_active: r.get::<_, i64>(8)? != 0,
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                })
            },
        ).ok();
        Ok(plan)
    }

    pub fn set_masterplan_active_toggle(
        &self,
        masterplan_id: &str,
        is_active: bool,
        force: bool,
    ) -> Result<Masterplan, String> {
        let plan = self
            .get_masterplan_by_id(masterplan_id)?
            .ok_or_else(|| format!("Masterplan '{}' not found", masterplan_id))?;

        let now = Utc::now().to_rfc3339();
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        if is_active {
            let active_other: Option<(String, String)> = tx.query_row(
                "SELECT id, title FROM masterplans WHERE project_id = ?1 AND is_active = 1 AND id != ?2 LIMIT 1",
                [&plan.project_id, masterplan_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).ok();

            if let Some((other_id, other_title)) = active_other {
                if !force {
                    return Err(format!(
                        "CONFLICT: Masterplan '{}' (ID: {}) is currently active for this project. Only one masterplan can be active at a time.",
                        other_title, other_id
                    ));
                }
                tx.execute(
                    "UPDATE masterplans SET is_active = 0, updated_at = ?1 WHERE project_id = ?2",
                    rusqlite::params![now, plan.project_id],
                )
                .map_err(|e| e.to_string())?;
            }

            tx.execute(
                "UPDATE masterplans SET is_active = 1, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, masterplan_id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            tx.execute(
                "UPDATE masterplans SET is_active = 0, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, masterplan_id],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);

        self.emit_event(
            Some(&plan.project_id),
            None,
            None,
            "MASTERPLAN_ACTIVATION_CHANGED",
            json!({ "plan_id": masterplan_id, "is_active": is_active }),
        );

        self.get_masterplan_by_id(masterplan_id)?
            .ok_or_else(|| "Masterplan not found after update".to_string())
    }

    pub fn delete_masterplan(&self, masterplan_id: &str) -> Result<(), String> {
        let plan = self
            .get_masterplan_by_id(masterplan_id)?
            .ok_or_else(|| format!("Masterplan '{}' not found", masterplan_id))?;

        let conn = self.db.lock();
        conn.execute(
            "DELETE FROM masterplan_steps WHERE masterplan_id = ?1",
            [masterplan_id],
        )
        .ok();
        conn.execute(
            "DELETE FROM masterplan_revisions WHERE masterplan_id = ?1",
            [masterplan_id],
        )
        .ok();
        conn.execute("DELETE FROM masterplans WHERE id = ?1", [masterplan_id])
            .map_err(|e| format!("Failed to delete masterplan: {}", e))?;
        drop(conn);

        self.emit_event(
            Some(&plan.project_id),
            None,
            None,
            "MASTERPLAN_DELETED",
            json!({ "plan_id": masterplan_id }),
        );

        Ok(())
    }

    pub fn set_masterplan_milestone_approval(
        &self,
        project_id: &str,
        require_approval: bool,
    ) -> Result<bool, String> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE masterplans SET require_milestone_approval = ?1, updated_at = ?2 WHERE project_id = ?3 AND is_active = 1",
            rusqlite::params![require_approval, now, project_id],
        ).or_else(|_| {
            conn.execute(
                "UPDATE masterplans SET require_milestone_approval = ?1, updated_at = ?2 WHERE project_id = ?3",
                rusqlite::params![require_approval, now, project_id],
            )
        }).map_err(|e| format!("Failed to update milestone approval mode: {}", e))?;
        drop(conn);
        self.emit_event(
            Some(project_id),
            None,
            None,
            "MASTERPLAN_UPDATED",
            json!({ "require_milestone_approval": require_approval }),
        );
        Ok(require_approval)
    }

    pub fn get_masterplan(&self, project_id: &str) -> Result<Option<Masterplan>, String> {
        let conn = self.db.lock();
        let plan = conn
            .query_row(
                "SELECT id, project_id, COALESCE(title, 'Masterplan'), raw_text, status, target_step_count, max_steps_per_agent, COALESCE(require_milestone_approval, 1), COALESCE(is_active, 0), created_at, updated_at
                 FROM masterplans WHERE project_id = ?1 AND is_active = 1 LIMIT 1",
                [project_id],
                |r| {
                    Ok(Masterplan {
                        id: r.get(0)?,
                        project_id: r.get(1)?,
                        title: r.get(2)?,
                        raw_text: r.get(3)?,
                        status: r.get(4)?,
                        target_step_count: r.get(5)?,
                        max_steps_per_agent: r.get(6)?,
                        require_milestone_approval: r.get::<_, i64>(7)? != 0,
                        is_active: r.get::<_, i64>(8)? != 0,
                        created_at: r.get(9)?,
                        updated_at: r.get(10)?,
                    })
                },
            )
            .ok();
        Ok(plan)
    }

    pub fn list_masterplan_steps(&self, project_id: &str) -> Result<Vec<MasterplanStep>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT ms.id, ms.masterplan_id, ms.step_index, ms.title, ms.description, ms.suggested_scope, ms.acceptance_criteria, ms.status, ms.claimed_agent_id, ms.claimed_task_id, ms.completed_at, ms.created_at, ms.updated_at
                 FROM masterplan_steps ms
                 JOIN masterplans m ON ms.masterplan_id = m.id
                 WHERE m.project_id = ?1 AND m.is_active = 1
                 ORDER BY ms.step_index ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([project_id], |r| {
                Ok(MasterplanStep {
                    id: r.get(0)?,
                    masterplan_id: r.get(1)?,
                    step_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    suggested_scope: r.get(5)?,
                    acceptance_criteria: r.get(6)?,
                    status: r.get(7)?,
                    claimed_agent_id: r.get(8)?,
                    claimed_task_id: r.get(9)?,
                    completed_at: r.get(10)?,
                    created_at: r.get(11)?,
                    updated_at: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let res: Vec<MasterplanStep> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read masterplan step row: {}", e))?;
        drop(stmt);
        drop(conn);

        if !res.is_empty() {
            return Ok(res);
        }

        // Fallback to querying by direct masterplan_id
        self.list_masterplan_steps_by_plan_id(project_id)
    }

    pub fn list_masterplan_steps_by_plan_id(
        &self,
        masterplan_id: &str,
    ) -> Result<Vec<MasterplanStep>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT ms.id, ms.masterplan_id, ms.step_index, ms.title, ms.description, ms.suggested_scope, ms.acceptance_criteria, ms.status, ms.claimed_agent_id, ms.claimed_task_id, ms.completed_at, ms.created_at, ms.updated_at
                 FROM masterplan_steps ms
                 WHERE ms.masterplan_id = ?1
                 ORDER BY ms.step_index ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([masterplan_id], |r| {
                Ok(MasterplanStep {
                    id: r.get(0)?,
                    masterplan_id: r.get(1)?,
                    step_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    suggested_scope: r.get(5)?,
                    acceptance_criteria: r.get(6)?,
                    status: r.get(7)?,
                    claimed_agent_id: r.get(8)?,
                    claimed_task_id: r.get(9)?,
                    completed_at: r.get(10)?,
                    created_at: r.get(11)?,
                    updated_at: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read masterplan step row: {}", e))
    }

    pub fn decompose_masterplan(
        &self,
        project_id: &str,
        steps: Vec<DecomposedStepInput>,
        append: Option<bool>,
        idempotency_key: Option<String>,
    ) -> Result<Vec<MasterplanStep>, String> {
        if steps.is_empty() {
            return Err("Cannot decompose masterplan with empty step list".to_string());
        }

        let mut seen_indexes = std::collections::HashSet::new();
        for s in &steps {
            if s.step_index <= 0 {
                return Err(format!(
                    "Invalid step_index {}: step index must be positive (> 0)",
                    s.step_index
                ));
            }
            if !seen_indexes.insert(s.step_index) {
                return Err(format!(
                    "Duplicate step_index {} detected in decomposition payload",
                    s.step_index
                ));
            }
        }

        let plan = self
            .get_masterplan(project_id)?
            .ok_or_else(|| format!("No masterplan found for project '{}'", project_id))?;

        let is_append = append.unwrap_or(false);
        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let steps_json = serde_json::to_string(&steps).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(steps_json.as_bytes());
        let request_hash = hex::encode(hasher.finalize());

        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;

        // Invariant 4 & K: If an idempotency key is provided, check for a stored result inside the transaction.
        // Identical retry returns stored steps; conflicting retry with different payload is rejected.
        if let Some(ref key) = idempotency_key {
            let existing: Option<(String, String, Option<String>)> = tx
                .query_row(
                    "SELECT masterplan_id, result_json, request_hash FROM masterplan_operations WHERE idempotency_key = ?1",
                    [key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(|e| format!("Failed to check idempotency key: {}", e))?;

            if let Some((stored_plan_id, result_json, stored_hash)) = existing {
                if stored_plan_id != plan.id {
                    return Err(format!(
                        "Idempotency key '{}' is already bound to masterplan '{}', cannot reuse with masterplan '{}'",
                        key, stored_plan_id, plan.id
                    ));
                }
                if let Some(ref sh) = stored_hash {
                    if sh != &request_hash {
                        return Err(format!(
                            "Conflicting retry: idempotency key '{}' reused with different step contents",
                            key
                        ));
                    }
                }
                let stored_steps: Vec<MasterplanStep> = serde_json::from_str(&result_json)
                    .map_err(|e| {
                        format!("Failed to deserialize stored idempotency result: {}", e)
                    })?;
                return Ok(stored_steps);
            }
        }

        if is_append {
            // Invariant check L: In append mode, never overwrite a CLAIMED or COMPLETED step unless it is an identical retry
            for s in &steps {
                let existing: Option<(String, String, String)> = tx
                    .query_row(
                        "SELECT status, title, description FROM masterplan_steps WHERE masterplan_id = ?1 AND step_index = ?2",
                        rusqlite::params![plan.id, s.step_index],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()
                    .map_err(|e| format!("Failed to query step #{} status in masterplan: {}", s.step_index, e))?;

                if let Some((status, existing_title, existing_desc)) = existing {
                    if status != "PENDING" {
                        // Allow identical retry of already claimed/completed step without error, but reject any modification
                        let is_identical = existing_title.trim() == s.title.trim()
                            && existing_desc.trim() == s.description.trim();
                        if !is_identical {
                            return Err(format!(
                                "Cannot overwrite step #{}: Step is already {} in masterplan",
                                s.step_index, status
                            ));
                        }
                    }
                }
            }
        } else {
            // Invariant check 1: Reject hostile re-decomposition if active claims exist, but allow idempotent retries
            let active_claims: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1 AND status != 'PENDING'",
                    [&plan.id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to query active claims: {}", e))?;

            if active_claims > 0 {
                let mut stmt_chk = tx
                    .prepare(
                        "SELECT step_index, title FROM masterplan_steps WHERE masterplan_id = ?1 ORDER BY step_index ASC",
                    )
                    .map_err(|e| e.to_string())?;
                let existing_chk: Vec<(i32, String)> = stmt_chk
                    .query_map([&plan.id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("Failed to decode existing step check rows: {}", e))?;
                drop(stmt_chk);

                if existing_chk.len() == steps.len() {
                    let is_identical = existing_chk.iter().zip(&steps).all(|(ext, inc)| {
                        ext.0 == inc.step_index && ext.1.trim() == inc.title.trim()
                    });
                    if is_identical {
                        drop(tx);
                        drop(conn);
                        return self.list_masterplan_steps_by_plan_id(&plan.id);
                    }
                }

                return Err(format!(
                    "Cannot re-decompose masterplan: {} step(s) are actively claimed, in-progress, or completed. Reset the plan first via 'reset_masterplan' or submit active chunks.",
                    active_claims
                ));
            }

            // Invariant check 2: Reject re-decomposition if active tasks are in flight for this masterplan
            let active_project_tasks: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE masterplan_id = ?1 AND state IN ('RUNNING', 'VERIFYING', 'VERIFIED', 'REVIEW', 'MERGE_READY') AND is_stale = 0",
                    [&plan.id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to query active project tasks: {}", e))?;

            if active_project_tasks > 0 {
                return Err(format!(
                    "Cannot re-decompose masterplan: {} active task(s) are currently in flight for masterplan '{}'. Complete, submit, or cancel active tasks first (or call reset_masterplan).",
                    active_project_tasks, plan.id
                ));
            }

            tx.execute(
                "DELETE FROM masterplan_steps WHERE masterplan_id = ?1",
                [&plan.id],
            )
            .map_err(|e| e.to_string())?;
        }

        for s in steps {
            let suggested_scope = s.suggested_scope.unwrap_or_else(|| "src/**".to_string());
            let criteria = s
                .acceptance_criteria
                .unwrap_or_else(|| "All automated tests pass".to_string());

            if is_append {
                let existing_id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM masterplan_steps WHERE masterplan_id = ?1 AND step_index = ?2",
                        rusqlite::params![plan.id, s.step_index],
                        |r| r.get(0),
                    )
                    .ok();

                if let Some(id) = existing_id {
                    tx.execute(
                        "UPDATE masterplan_steps SET title = ?1, description = ?2, suggested_scope = ?3, acceptance_criteria = ?4, updated_at = ?5 WHERE id = ?6 AND status = 'PENDING'",
                        params![s.title, s.description, suggested_scope, criteria, now, id],
                    ).map_err(|e| e.to_string())?;
                    continue;
                }
            }

            let step_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PENDING', ?8, ?8)",
                params![step_id, plan.id, s.step_index, s.title, s.description, suggested_scope, criteria, now],
            ).map_err(|e| e.to_string())?;
        }

        tx.execute(
            "UPDATE masterplans SET status = 'RESORTED', updated_at = ?1 WHERE id = ?2",
            params![now, plan.id],
        )
        .map_err(|e| e.to_string())?;

        // Query the complete resulting step list within the transaction for exactness
        let mut stmt_res = tx.prepare(
            "SELECT id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, claimed_agent_id, claimed_task_id, completed_at, created_at, updated_at
             FROM masterplan_steps WHERE masterplan_id = ?1 ORDER BY step_index ASC"
        ).map_err(|e| e.to_string())?;

        let resulting_steps: Vec<MasterplanStep> = stmt_res
            .query_map([&plan.id], |r| {
                Ok(MasterplanStep {
                    id: r.get(0)?,
                    masterplan_id: r.get(1)?,
                    step_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    suggested_scope: r.get(5)?,
                    acceptance_criteria: r.get(6)?,
                    status: r.get(7)?,
                    claimed_agent_id: r.get(8)?,
                    claimed_task_id: r.get(9)?,
                    completed_at: r.get(10)?,
                    created_at: r.get(11)?,
                    updated_at: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to decode resulting step rows: {}", e))?;
        drop(stmt_res);

        // Invariant 4: Persist the idempotency record inside the exact same transaction before commit
        if let Some(ref key) = idempotency_key {
            let result_json = serde_json::to_string(&resulting_steps)
                .map_err(|e| format!("Failed to serialize decomposition steps: {}", e))?;
            tx.execute(
                "INSERT INTO masterplan_operations (idempotency_key, masterplan_id, result_json, request_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(idempotency_key) DO UPDATE SET result_json = excluded.result_json, request_hash = excluded.request_hash",
                params![key, plan.id, result_json, request_hash, now],
            )
            .map_err(|e| format!("Failed to record idempotency operation: {}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit masterplan decomposition: {}", e))?;
        drop(conn);

        self.emit_event(Some(project_id), None, None, "MASTERPLAN_DECOMPOSED", json!({ "total_steps": resulting_steps.len(), "status": "RESORTED", "is_append": is_append }));

        Ok(resulting_steps)
    }

    /// Transactional, race-safe chunk reservation with complete compensation and atomic scope acquisition
    pub fn claim_masterplan_chunk(
        &self,
        project_id: &str,
        agent_id: &str,
        requested_count: Option<i32>,
    ) -> Result<Task, String> {
        if !self.is_agent_registered(agent_id) {
            return Err(format!(
                "Agent registration required: Agent ID '{}' is not registered. Call 'agent.register' first.",
                agent_id
            ));
        }

        let (canonical_agent_id, ..) = Self::canonicalize_ide_identity(agent_id, "");

        let plan = self
            .get_masterplan(project_id)?
            .ok_or_else(|| format!("No masterplan found for project '{}'", project_id))?;

        if plan.status == "UNSORTED" {
            return Err("Cannot claim steps from an UNSORTED masterplan. Decompose the plan first via 'masterplan.decompose'.".to_string());
        }

        if let Some(rc) = requested_count {
            if rc <= 0 {
                return Err(format!(
                    "Invalid chunk count {}: requested count must be greater than 0.",
                    rc
                ));
            }
        }

        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start transaction: {}", e))?;

        // 1. Check active anti-hoarding limit inside transaction (Fail-Closed)
        let currently_active: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE (claimed_agent_id = ?1 OR claimed_agent_id = ?2) AND status = 'CLAIMED'",
                [agent_id, &canonical_agent_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Failed to query active claims for agent '{}': {}", agent_id, e))?;

        if currently_active >= plan.max_steps_per_agent as i64 {
            return Err(format!(
                "Anti-hoarding cap reached: Agent '{}' already has {} active claimed steps. Submit or complete your current chunk before claiming more.",
                agent_id, currently_active
            ));
        }

        let allowed = (plan.max_steps_per_agent as i64 - currently_active).max(1) as i32;
        let count = requested_count.unwrap_or(allowed).min(allowed).max(1);

        // 2. Select pending steps (Fail-Closed)
        let mut stmt = tx
            .prepare("SELECT id, step_index, title, description, suggested_scope, acceptance_criteria FROM masterplan_steps WHERE masterplan_id = ?1 AND status = 'PENDING' ORDER BY step_index ASC LIMIT ?2")
            .map_err(|e| e.to_string())?;

        let reserved_steps_iter = stmt
            .query_map(rusqlite::params![plan.id, count], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut reserved_steps = Vec::new();
        for r in reserved_steps_iter {
            let step =
                r.map_err(|e| format!("Failed to read pending masterplan step row: {}", e))?;
            reserved_steps.push(step);
        }
        drop(stmt);

        if reserved_steps.is_empty() {
            return Err("No pending steps available in masterplan. All steps are either claimed or completed.".to_string());
        }

        let first_idx = reserved_steps.first().unwrap().1;
        let last_idx = reserved_steps.last().unwrap().1;

        let total_pending: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1 AND status = 'PENDING'",
                [&plan.id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Failed to query pending step count: {}", e))?;
        let is_final_chunk = total_pending <= reserved_steps.len() as i64;

        let task_title = format!(
            "Masterplan Chunk: Steps {}-{} ({})",
            first_idx,
            last_idx,
            reserved_steps.first().unwrap().2
        );
        let task_desc = reserved_steps
            .iter()
            .map(|(_, idx, title, desc, ..)| format!("Step #{}: {}\n{}", idx, title, desc))
            .collect::<Vec<String>>()
            .join("\n\n---\n\n");

        let task_steps: Vec<(String, String, bool)> = reserved_steps
            .iter()
            .map(|(_, idx, title, desc, ..)| {
                (format!("Step #{}: {}", idx, title), desc.clone(), true)
            })
            .collect();

        let criteria: Vec<String> = reserved_steps
            .iter()
            .map(|(.., crit)| crit.clone())
            .collect();

        let mut final_desc = task_desc;
        let mut final_criteria = criteria;
        if is_final_chunk {
            final_desc.push_str("\n\n=================================================================\nFINAL RELEASE DELIVERY REQUIREMENTS (Final Masterplan Chunk):\nAs the agent completing the final step(s) of this masterplan, you must perform the Final Release Delivery:\n1. Build the production bundle / executable.\n2. Create or verify the automated launcher script (`run.bat` for Windows / `start.sh` for Unix or tech-stack launcher).\n   - Windows launcher MUST resolve paths from script location (`cd /d \"%~dp0\"`), check exit status on dependency install/build (`if %errorlevel% neq 0 pause`), start server, and auto-open the browser (`start http://localhost:<port>`).\n3. Test and verify that the application launches successfully.\n4. Create or update a comprehensive user manual (`USER_GUIDE.md` / `HOW_TO_USE.md`) explaining the complete application architecture, configuration, and exact step-by-step instructions on how to use the entire application.\n5. Commit all launcher scripts, build artifacts, and guide documentation before calling `task_submit`.\n=================================================================");
            final_criteria.push("Automated launcher script (e.g. run.bat with cd /d %~dp0 and errorlevel checks) created and tested, and comprehensive USER_GUIDE.md created".to_string());
        }

        let task_id = Uuid::new_v4().to_string();

        let (proj_path, target_branch): (String, String) = tx
            .query_row(
                "SELECT path, target_branch FROM projects WHERE id = ?1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Project '{}' not found: {}", project_id, e))?;

        let repo_path = Path::new(&proj_path);
        let branch_name = format!("agentxflow/task-{}", task_id);
        let worktree_dir = self
            .worktrees_root
            .join(project_id)
            .join(format!("task-{}", task_id));
        let worktree_path_str = worktree_dir.to_string_lossy().to_string();

        let base_sha = self
            .git
            .get_ref_sha(repo_path, &target_branch)
            .map_err(|e| {
                format!(
                    "Failed to resolve base commit '{}' for claim of task '{}': {}",
                    target_branch, task_id, e
                )
            })?;

        // 3. Insert Task in CLAIMING state into tasks table on tx
        tx.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, masterplan_revision_id, title, description, state, substate, priority, is_stale, assigned_agent_id, worktree_path, branch_name, base_sha, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'CLAIMING', 'CLAIMING', 'HIGH', 0, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![task_id, project_id, plan.id, task_title, final_desc, canonical_agent_id, worktree_path_str, branch_name, base_sha, now],
        ).map_err(|e| format!("Failed to insert chunk task: {}", e))?;

        // Insert task_steps
        for (idx, (step_title, step_desc, is_mand)) in task_steps.into_iter().enumerate() {
            let step_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO task_steps (id, task_id, order_index, title, description, is_mandatory, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING')",
                params![step_id, task_id, idx as i32 + 1, step_title, step_desc, is_mand],
            ).map_err(|e| format!("Failed to insert task step: {}", e))?;
        }

        // Insert acceptance criteria
        for crit in final_criteria {
            let crit_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO acceptance_criteria (id, task_id, criterion, is_satisfied, is_locked)
                 VALUES (?1, ?2, ?3, 0, 0)",
                params![crit_id, task_id, crit],
            )
            .map_err(|e| format!("Failed to insert acceptance criterion: {}", e))?;
        }

        // 4. Atomic Scope Lease Acquisition on tx
        let scope_patterns: Vec<String> = reserved_steps
            .iter()
            .map(|(.., scope, _)| scope.clone())
            .filter(|s| !s.trim().is_empty())
            .collect();

        if !scope_patterns.is_empty() {
            self.scope.acquire_scope_tx(
                &tx,
                &task_id,
                &canonical_agent_id,
                scope_patterns,
                "EXCLUSIVE_WRITE",
            )?;
        }

        // 5. Authoritatively bind masterplan steps to task_id on tx
        for (id, _idx, title, ..) in &reserved_steps {
            let rows_affected = tx
                .execute(
                    "UPDATE masterplan_steps SET status = 'CLAIMED', claimed_agent_id = ?1, claimed_task_id = ?2, updated_at = ?3 WHERE id = ?4 AND status = 'PENDING'",
                    params![canonical_agent_id, task_id, now, id],
                )
                .map_err(|e| e.to_string())?;
            if rows_affected != 1 {
                return Err(format!(
                    "Step {} ('{}') is no longer pending (concurrent claim)",
                    id, title
                ));
            }
        }

        // 6. Update masterplan status to EXECUTING
        tx.execute(
            "UPDATE masterplans SET status = 'EXECUTING', updated_at = ?1 WHERE id = ?2",
            params![now, plan.id],
        )
        .map_err(|e| format!("Failed to update masterplan status: {}", e))?;

        // 7. Commit the entire atomic reservation transaction
        tx.commit()
            .map_err(|e| format!("Failed to commit chunk claim transaction: {}", e))?;
        drop(conn);

        // 8. Cut isolated Git worktree on disk
        if worktree_dir.exists() {
            let managed_roots = vec![self.worktrees_root.clone(), repo_path.join(".agentxflow")];
            if self
                .git
                .is_managed_worktree(repo_path, &managed_roots, &worktree_dir)
            {
                let _ = self
                    .git
                    .remove_worktree(repo_path, &managed_roots, &worktree_dir);
                let _ = crate::git::GitService::safe_remove_dir_all(&worktree_dir);
            } else {
                warn!(
                    "Refusing to remove unmanaged path {:?} during chunk claim re-cut",
                    worktree_dir
                );
            }
        }

        if let Err(e) =
            self.git
                .create_worktree(repo_path, &worktree_dir, &branch_name, &target_branch)
        {
            // Rollback on worktree failure: cancel task and restore steps to PENDING
            let conn = self.db.lock();
            let _ = cancel_task_inner(
                &conn,
                &task_id,
                None,
                Some("Worktree creation failed during masterplan chunk claim"),
            );
            return Err(format!(
                "Failed to create worktree for chunk task '{}': {}",
                task_id, e
            ));
        }

        // 9. Worktree ready: transition task state to RUNNING
        {
            let conn = self.db.lock();
            let transition_res = transition_task_state_on_conn(
                &conn,
                &task_id,
                &[TaskState::Claiming, TaskState::Backlog],
                TaskState::Running,
            );
            match transition_res {
                Ok(()) => {
                    if let Err(e) = conn.execute(
                        "UPDATE tasks SET substate = 'NONE', updated_at = ?1 WHERE id = ?2",
                        [&now, &task_id],
                    ) {
                        let _ = conn.execute(
                            "UPDATE tasks SET state = 'BLOCKED', substate = 'RECOVERABLE', updated_at = ?1 WHERE id = ?2",
                            [&now, &task_id],
                        );
                        drop(conn);
                        self.emit_event(
                            Some(project_id),
                            Some(&task_id),
                            Some(&canonical_agent_id),
                            "TASK_CLAIM_FINALIZATION_FAILED",
                            json!({ "task_id": task_id, "error": e.to_string() }),
                        );
                        return Err(format!("Failed to finalize task claim in database: {}", e));
                    }
                }
                Err(e) => {
                    let _ = conn.execute(
                        "UPDATE tasks SET state = 'BLOCKED', substate = 'RECOVERABLE', updated_at = ?1 WHERE id = ?2",
                        [&now, &task_id],
                    );
                    drop(conn);
                    self.emit_event(
                        Some(project_id),
                        Some(&task_id),
                        Some(&canonical_agent_id),
                        "TASK_CLAIM_FINALIZATION_FAILED",
                        json!({ "task_id": task_id, "error": e.to_string() }),
                    );
                    return Err(format!("Failed to transition task claim to RUNNING: {}", e));
                }
            }
        }

        self.emit_event(
            Some(project_id),
            Some(&task_id),
            Some(&canonical_agent_id),
            "TASK_CLAIMED",
            json!({ "agent_id": canonical_agent_id, "chunk_steps": reserved_steps.len() }),
        );
        self.emit_event(
            Some(project_id),
            Some(&task_id),
            Some(&canonical_agent_id),
            "MASTERPLAN_CHUNK_CLAIMED",
            json!({
                "task_id": task_id,
                "agent_id": canonical_agent_id,
                "step_range": [first_idx, last_idx],
                "step_count": reserved_steps.len(),
                "is_final_chunk": is_final_chunk
            }),
        );

        self.get_task(&task_id)
    }

    pub fn reset_masterplan(
        &self,
        project_id: &str,
        masterplan_id: Option<&str>,
    ) -> Result<(), String> {
        let _now = Utc::now().to_rfc3339();
        let is_explicit = masterplan_id.map(|s| !s.trim().is_empty()).unwrap_or(false);

        // 1. Validate masterplan target and project boundary
        let plan_ids: Vec<String> = {
            let conn = self.db.lock();
            if let Some(mp_id) = masterplan_id {
                let trimmed = mp_id.trim();
                if !trimmed.is_empty() {
                    let plan_proj: Result<String, _> = conn.query_row(
                        "SELECT project_id FROM masterplans WHERE id = ?1",
                        [trimmed],
                        |r| r.get(0),
                    );
                    match plan_proj {
                        Ok(p_id) => {
                            if p_id != project_id {
                                return Err(format!(
                                    "Masterplan '{}' belongs to project '{}', not '{}'",
                                    trimmed, p_id, project_id
                                ));
                            }
                            vec![trimmed.to_string()]
                        }
                        Err(rusqlite::Error::QueryReturnedNoRows) => {
                            return Err(format!("Masterplan '{}' not found", trimmed));
                        }
                        Err(e) => {
                            return Err(format!("Failed to query masterplan '{}': {}", trimmed, e));
                        }
                    }
                } else {
                    let mut stmt = conn
                        .prepare("SELECT id FROM masterplans WHERE project_id = ?1")
                        .map_err(|e| e.to_string())?;
                    let ids: Result<Vec<String>, _> = stmt
                        .query_map([project_id], |r| r.get(0))
                        .map_err(|e| e.to_string())?
                        .collect();
                    ids.map_err(|e| e.to_string())?
                }
            } else {
                let mut stmt = conn
                    .prepare("SELECT id FROM masterplans WHERE project_id = ?1")
                    .map_err(|e| e.to_string())?;
                let ids: Result<Vec<String>, _> = stmt
                    .query_map([project_id], |r| r.get(0))
                    .map_err(|e| e.to_string())?
                    .collect();
                ids.map_err(|e| e.to_string())?
            }
        };

        // 2. Invalidate and cancel only tasks belonging to the target masterplan(s)
        let active_tasks: Vec<String> = {
            let conn = self.db.lock();
            if is_explicit {
                let trimmed = masterplan_id.unwrap().trim();
                let mut stmt = conn
                    .prepare("SELECT id FROM tasks WHERE project_id = ?1 AND masterplan_id = ?2 AND state NOT IN ('CANCELLED', 'DONE')")
                    .map_err(|e| e.to_string())?;
                let ids: Result<Vec<String>, _> = stmt
                    .query_map(params![project_id, trimmed], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .collect();
                ids.map_err(|e| e.to_string())?
            } else {
                let mut stmt = conn
                    .prepare("SELECT id FROM tasks WHERE project_id = ?1 AND state NOT IN ('CANCELLED', 'DONE')")
                    .map_err(|e| e.to_string())?;
                let ids: Result<Vec<String>, _> = stmt
                    .query_map([project_id], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .collect();
                ids.map_err(|e| e.to_string())?
            }
        };

        // 2a. Preflight validation: verify all selected tasks are cancellable before mutating any task
        for tid in &active_tasks {
            let task = self
                .get_task(tid)
                .map_err(|e| format!("Preflight check failed for task '{}': {}", tid, e))?;
            if task.state == TaskState::Done {
                return Err(format!(
                    "Preflight rejection: Cannot cancel task '{}': Task is already DONE (merged)",
                    tid
                ));
            }
        }

        let mut cancellation_errors = Vec::new();
        for tid in &active_tasks {
            if let Err(e) =
                self.cancel_task(tid, None, Some("Masterplan was reset; task invalidated"))
            {
                cancellation_errors.push(format!("Task '{}': {}", tid, e));
            }
        }

        if !cancellation_errors.is_empty() {
            return Err(format!(
                "Cannot reset masterplan: failed to cancel active task(s): {}",
                cancellation_errors.join("; ")
            ));
        }

        // 3. Fetch and delete the target masterplan(s) and all their associated steps & revisions transactionally
        {
            let mut conn = self.db.lock();
            let tx = conn
                .transaction()
                .map_err(|e| format!("Failed to begin reset transaction: {}", e))?;
            for pid in &plan_ids {
                tx.execute(
                    "DELETE FROM masterplan_steps WHERE masterplan_id = ?1",
                    [pid],
                )
                .map_err(|e| format!("Failed to delete masterplan steps: {}", e))?;
                tx.execute(
                    "DELETE FROM masterplan_revisions WHERE masterplan_id = ?1",
                    [pid],
                )
                .map_err(|e| format!("Failed to delete masterplan revisions: {}", e))?;
                tx.execute("DELETE FROM masterplans WHERE id = ?1", [pid])
                    .map_err(|e| format!("Failed to delete masterplan: {}", e))?;
            }
            tx.commit()
                .map_err(|e| format!("Failed to commit masterplan reset: {}", e))?;
        }

        // 4. On-disk cleanup: managed worktrees only using hardened safe deletion.
        // The user's primary checkout is a protected resource and is NEVER reset or cleaned.
        if let Ok(proj) = self.get_project(project_id) {
            let repo_path = Path::new(&proj.path);
            if !is_explicit {
                let wt_dir = repo_path.join(".agentxflow").join("worktrees");
                if wt_dir.exists() {
                    GitService::safe_remove_dir_all(&wt_dir).map_err(|e| {
                        format!("Failed to clean worktrees directory {:?}: {}", wt_dir, e)
                    })?;
                }
            }
            // Prune git's worktree metadata for worktrees removed by cancel_task above.
            self.git
                .run_git_cmd(repo_path, &["worktree", "prune"])
                .map_err(|e| format!("Failed to prune git worktrees: {}", e))?;
        }

        self.emit_event(
            Some(project_id),
            None,
            None,
            "MASTERPLAN_RESET",
            json!({ "project_id": project_id, "masterplan_id": masterplan_id }),
        );
        Ok(())
    }

    pub fn list_all_masterplans(&self) -> Result<Vec<MasterplanSummary>, String> {
        let projects = self.list_projects()?;
        let conn = self.db.lock();
        let mut summaries = Vec::new();

        for proj in projects {
            let mut stmt = conn
                .prepare(
                    "SELECT id, COALESCE(title, 'Masterplan'), status, target_step_count, max_steps_per_agent, COALESCE(is_active, 0), updated_at
                     FROM masterplans WHERE project_id = ?1
                     ORDER BY is_active DESC, updated_at DESC",
                )
                .map_err(|e| format!("Failed to prepare masterplans query for project '{}': {}", proj.id, e))?;

            let plans: Vec<(String, String, String, i32, i32, bool, String)> = stmt
                .query_map([&proj.id], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get::<_, i64>(5)? != 0,
                        r.get(6)?,
                    ))
                })
                .map_err(|e| {
                    format!(
                        "Failed to query masterplans for project '{}': {}",
                        proj.id, e
                    )
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    format!(
                        "Failed to read masterplan row for project '{}': {}",
                        proj.id, e
                    )
                })?;

            for (
                plan_id,
                title,
                status,
                target_step_count,
                max_steps_per_agent,
                is_active,
                updated_at,
            ) in plans
            {
                let mut step_stmt = conn
                    .prepare("SELECT status FROM masterplan_steps WHERE masterplan_id = ?1")
                    .map_err(|e| {
                        format!(
                            "Failed to prepare masterplan steps query for plan '{}': {}",
                            plan_id, e
                        )
                    })?;
                let step_statuses: Vec<String> = step_stmt
                    .query_map([&plan_id], |r| r.get(0))
                    .map_err(|e| {
                        format!(
                            "Failed to query step statuses for plan '{}': {}",
                            plan_id, e
                        )
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        format!(
                            "Failed to read step status row for plan '{}': {}",
                            plan_id, e
                        )
                    })?;

                let total = step_statuses.len();
                let pending = step_statuses
                    .iter()
                    .filter(|s| s.as_str() == "PENDING")
                    .count();
                let claimed = step_statuses
                    .iter()
                    .filter(|s| s.as_str() == "CLAIMED" || s.as_str() == "IN_PROGRESS")
                    .count();
                let completed = step_statuses
                    .iter()
                    .filter(|s| s.as_str() == "COMPLETED")
                    .count();

                let next_action = if status == "UNSORTED" {
                    "masterplan_decompose".to_string()
                } else if pending > 0 {
                    "masterplan_claim_chunk".to_string()
                } else {
                    "all_steps_claimed_or_completed".to_string()
                };

                let handoff_prompt = if status == "UNSORTED" {
                    format!("Decompose masterplan '{}' for project '{}' (ID: {}) located at '{}' using tool 'masterplan_decompose'.", title, proj.name, proj.id, proj.path)
                } else {
                    format!("Claim next available chunk for masterplan '{}' for project '{}' (ID: {}) located at '{}' using tool 'masterplan_claim_chunk'.", title, proj.name, proj.id, proj.path)
                };

                summaries.push(MasterplanSummary {
                    project_id: proj.id.clone(),
                    project_name: proj.name.clone(),
                    repository_path: proj.path.clone(),
                    masterplan_id: plan_id,
                    title,
                    is_active,
                    status,
                    target_step_count,
                    max_steps_per_agent,
                    total_steps: total,
                    pending_steps: pending,
                    claimed_steps: claimed,
                    completed_steps: completed,
                    last_updated: updated_at,
                    next_action,
                    handoff_prompt,
                });
            }
        }

        Ok(summaries)
    }

    /// Automatically parses raw masterplan specification into structured execution steps
    pub fn parse_masterplan_text_to_steps(
        &self,
        raw_text: &str,
        _target_step_count: i32,
    ) -> Result<Vec<DecomposedStepInput>, String> {
        let infer_scope = |title: &str, desc: &str| -> String {
            let lower = format!("{} {}", title, desc).to_lowercase();
            if lower.contains("backend")
                || lower.contains("rust")
                || lower.contains("src-tauri")
                || lower.contains("tauri")
                || lower.contains("mcp")
                || lower.contains("coordinator")
                || lower.contains("sqlite")
                || lower.contains("migration")
                || lower.contains("database")
            {
                "src-tauri/**".to_string()
            } else if lower.contains("frontend")
                || lower.contains("ui")
                || lower.contains("component")
                || lower.contains("react")
                || lower.contains("css")
                || lower.contains("view")
                || lower.contains("workbench")
                || lower.contains("modal")
            {
                "src/**".to_string()
            } else if lower.contains("crates/") {
                "crates/**".to_string()
            } else if lower.contains("packages/") {
                "packages/**".to_string()
            } else if lower.contains("apps/") {
                "apps/**".to_string()
            } else if lower.contains("test") || lower.contains("tests/") {
                "tests/**".to_string()
            } else if lower.contains("doc")
                || lower.contains("readme")
                || lower.contains("specification")
            {
                "*.md".to_string()
            } else {
                "**".to_string()
            }
        };

        let mut steps = Vec::new();
        let lines: Vec<&str> = raw_text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        let mut current_title: Option<String> = None;
        let mut current_desc: Vec<String> = Vec::new();
        let mut step_index = 1;

        for line in lines {
            let is_header = line.starts_with("# ")
                || line.starts_with("## ")
                || line.starts_with("### ")
                || line.starts_with("Step ")
                || line.starts_with("Phase ")
                || (line
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && (line.contains(". ") || line.contains(": ")))
                || line.starts_with("- [ ] ")
                || line.starts_with("- [x] ");

            if is_header {
                if let Some(title) = current_title.take() {
                    let desc = if current_desc.is_empty() {
                        title.clone()
                    } else {
                        current_desc.join("\n")
                    };
                    let scope = infer_scope(&title, &desc);
                    steps.push(DecomposedStepInput {
                        step_index,
                        title,
                        description: desc,
                        suggested_scope: Some(scope),
                        acceptance_criteria: Some(
                            "Code builds cleanly and all verification tests pass.".to_string(),
                        ),
                    });
                    step_index += 1;
                    current_desc.clear();
                }

                let clean_title = line
                    .trim_start_matches('#')
                    .trim_start_matches('-')
                    .trim_start_matches('[')
                    .trim_start_matches(']')
                    .trim_start_matches('x')
                    .trim_start_matches(' ')
                    .trim();

                current_title = Some(clean_title.to_string());
            } else if current_title.is_some() {
                current_desc.push(line.to_string());
            } else {
                current_title = Some(line.to_string());
            }
        }

        if let Some(title) = current_title {
            let desc = if current_desc.is_empty() {
                title.clone()
            } else {
                current_desc.join("\n")
            };
            let scope = infer_scope(&title, &desc);
            steps.push(DecomposedStepInput {
                step_index,
                title,
                description: desc,
                suggested_scope: Some(scope),
                acceptance_criteria: Some(
                    "Code builds cleanly and all verification tests pass.".to_string(),
                ),
            });
        }

        if steps.is_empty() {
            steps.push(DecomposedStepInput {
                step_index: 1,
                title: "Execute Masterplan Specification".to_string(),
                description: raw_text.to_string(),
                suggested_scope: Some("**".to_string()),
                acceptance_criteria: Some(
                    "Code builds cleanly and all verification tests pass.".to_string(),
                ),
            });
        }

        Ok(steps)
    }

    /// Automatically processes the next serialized merge in queue for a project
    pub fn process_next_merge(
        &self,
        project_id: &str,
    ) -> Result<Option<IntegrationAttempt>, String> {
        let proj = self
            .list_projects()?
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or("Project not found")?;
        let queue = self.merge.list_queue(project_id)?;
        let next_ready = queue.into_iter().find(|item| item.status == "READY");
        if let Some(item) = next_ready {
            let attempt = self
                .merge
                .process_merge_by_id(&item.id, Path::new(&proj.path))?;
            if let Err(error) = self.scope.release_scope(&item.task_id) {
                warn!(task_id = %item.task_id, %error, "Merged task scopes could not be released automatically");
            }
            Ok(Some(attempt))
        } else {
            Ok(None)
        }
    }

    /// Reconciles task status, attempt, proof bundle, and merge queue health
    pub fn reconcile_task(&self, task_id: &str) -> Result<serde_json::Value, String> {
        let mut task = self.get_task(task_id)?;
        let mut reconciliation_performed = false;

        let conn = self.db.lock();
        let attempt: Option<(String, String)> = match conn.query_row(
            "SELECT id, status FROM task_attempts WHERE task_id = ?1 ORDER BY attempt_number DESC LIMIT 1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ) {
            Ok(val) => Some(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                return Err(format!(
                    "Database error reading task attempts for task '{}': {}",
                    task_id, e
                ));
            }
        };

        let has_proof: bool = match conn.query_row(
            "SELECT COUNT(*) > 0 FROM proof_bundles WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        ) {
            Ok(val) => val,
            Err(e) => {
                return Err(format!(
                    "Database error reading proof bundles for task '{}': {}",
                    task_id, e
                ));
            }
        };

        let queue_item: Option<(String, String, String)> = match conn.query_row(
            "SELECT id, status, target_branch FROM merge_queue WHERE task_id = ?1 AND processed_at IS NULL",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ) {
            Ok(val) => Some(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                return Err(format!(
                    "Database error reading merge queue for task '{}': {}",
                    task_id, e
                ));
            }
        };
        drop(conn);

        let mut queue_status = queue_item.as_ref().map(|(_, status, _)| status.clone());

        // Authoritative Git ancestor check: if the task commit was already merged into
        // the target branch (e.g. Git CAS update-ref succeeded before SQLite finalization),
        // converge SQLite state immediately to MERGED, DONE, and COMPLETED steps.
        if task.state != TaskState::Done {
            let projs = self.list_projects()?;
            if let Some(project) = projs.into_iter().find(|p| p.id == task.project_id) {
                if let Some(ref head_sha) = task.head_sha {
                    if !head_sha.trim().is_empty() {
                        let repo_path = Path::new(&project.path);
                        let target_branch = queue_item
                            .as_ref()
                            .map(|(_, _, tb)| tb.as_str())
                            .unwrap_or(&project.target_branch);

                        let anc_check = self.git.is_ancestor(repo_path, head_sha, target_branch);
                        match anc_check {
                            Ok(crate::git::GitAncestorResult::Ancestor) => {
                                let mut conn = self.db.lock();
                                let tx = conn.transaction().map_err(|e| {
                                    format!("Failed to start reconciliation transaction: {}", e)
                                })?;
                                let now = Utc::now().to_rfc3339();

                                tx.execute(
                                    "UPDATE merge_queue SET status = 'MERGED', processed_at = ?1 WHERE task_id = ?2",
                                    params![now, task_id],
                                )
                                .map_err(|e| format!("Failed to update merge queue in reconciliation: {}", e))?;

                                transition_task_state_on_conn(
                                    &tx,
                                    task_id,
                                    &[
                                        TaskState::Backlog,
                                        TaskState::Ready,
                                        TaskState::Running,
                                        TaskState::Verifying,
                                        TaskState::Verified,
                                        TaskState::Review,
                                        TaskState::MergeReady,
                                        TaskState::Blocked,
                                    ],
                                    TaskState::Done,
                                )
                                .map_err(|e| {
                                    format!(
                                        "Failed to transition task to DONE in reconciliation: {}",
                                        e
                                    )
                                })?;

                                tx.execute(
                                    "UPDATE tasks SET substate = 'NONE', updated_at = ?1 WHERE id = ?2",
                                    params![now, task_id],
                                )
                                .map_err(|e| format!("Failed to clear task substate: {}", e))?;

                                tx.execute(
                                    "UPDATE masterplan_steps SET status = 'COMPLETED', completed_at = ?1, updated_at = ?1 WHERE claimed_task_id = ?2 AND status != 'COMPLETED'",
                                    params![now, task_id],
                                )
                                .map_err(|e| format!("Failed to complete masterplan steps: {}", e))?;

                                let pending_remaining: i64 = tx
                                    .query_row(
                                        "SELECT COUNT(*) FROM masterplan_steps ms
                                     JOIN masterplans mp ON ms.masterplan_id = mp.id
                                     WHERE mp.project_id = ?1 AND ms.status != 'COMPLETED'",
                                        [&task.project_id],
                                        |r| r.get(0),
                                    )
                                    .map_err(|e| {
                                        format!("Failed to count remaining masterplan steps: {}", e)
                                    })?;

                                if pending_remaining == 0 {
                                    tx.execute(
                                        "UPDATE masterplans SET status = 'COMPLETED', updated_at = ?1 WHERE project_id = ?2",
                                        params![now, &task.project_id],
                                    )
                                    .map_err(|e| format!("Failed to complete masterplan: {}", e))?;
                                }

                                tx.execute(
                                    "DELETE FROM scope_leases WHERE task_id = ?1",
                                    [task_id],
                                )
                                .map_err(|e| format!("Failed to release scope leases: {}", e))?;

                                tx.commit().map_err(|e| {
                                    format!("Failed to commit reconciliation transaction: {}", e)
                                })?;
                                drop(conn);

                                task.state = TaskState::Done;
                                queue_status = Some("MERGED".to_string());
                                reconciliation_performed = true;
                            }
                            Ok(crate::git::GitAncestorResult::NotAncestor) => {
                                // Task commit is not in target branch; no Git merge convergence needed
                            }
                            Err(e) => {
                                let is_invalid_head = e.contains("Not a valid commit name")
                                    || e.contains("not a valid commit name");
                                if !is_invalid_head {
                                    return Err(format!(
                                        "Unable to determine authoritative Git merge status for task '{}': {}",
                                        task_id, e
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // A verified task can become stale while an earlier FIFO candidate is
        // merged. Rebase the queue expectation to the current target and let
        // the normal merge simulation detect real conflicts.
        if task.state == TaskState::Blocked
            && queue_status.as_deref() == Some("STALE")
            && attempt.as_ref().map(|(_, status)| status.as_str()) == Some("VERIFIED")
            && has_proof
        {
            if let Some((queue_id, _, target_branch)) = queue_item.as_ref() {
                let project = self
                    .list_projects()?
                    .into_iter()
                    .find(|project| project.id == task.project_id)
                    .ok_or("Project not found")?;
                let target_sha = self
                    .git
                    .get_ref_sha(Path::new(&project.path), target_branch)?;
                let mut conn = self.db.lock();
                let tx = conn
                    .transaction()
                    .map_err(|e| format!("Failed to begin queue rebase transaction: {}", e))?;
                tx.execute(
                    "UPDATE merge_queue SET base_sha = ?1, status = 'READY' WHERE id = ?2 AND status = 'STALE'",
                    params![target_sha, queue_id],
                ).map_err(|e| format!("Failed to update queue base_sha: {}", e))?;
                transition_task_state_on_conn(
                    &tx,
                    task_id,
                    &[TaskState::Blocked],
                    TaskState::MergeReady,
                )
                .map_err(|e| format!("Failed to transition task to MERGE_READY: {}", e))?;
                tx.execute(
                    "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                    params![task_id],
                )
                .map_err(|e| format!("Failed to update task substate: {}", e))?;
                tx.commit()
                    .map_err(|e| format!("Failed to commit queue rebase: {}", e))?;
                drop(conn);
                task.state = TaskState::MergeReady;
                queue_status = Some("READY".to_string());
                reconciliation_performed = true;
            }
        }

        // Auto-heal if MERGE_READY but not enqueued
        if task.state == TaskState::MergeReady
            && queue_status.is_none()
            && self.enqueue_task_by_id(&task.project_id, task_id).is_ok()
        {
            reconciliation_performed = true;
        }

        let state_repr =
            if task.state == TaskState::Backlog && task.substate == TaskSubstate::Claiming {
                "CLAIMING"
            } else {
                task.state.as_str()
            };

        Ok(serde_json::json!({
            "task_id": task.id,
            "state": state_repr,
            "reconciliation_status": if reconciliation_performed { "SUCCESSFULLY_RECONCILED" } else { "NO_REPAIR_NEEDED" },
            "attempt": attempt.map(|(id, st)| serde_json::json!({ "attempt_id": id, "status": st })),
            "has_proof_bundle": has_proof,
            "merge_queue_status": queue_status.unwrap_or_else(|| "NOT_ENQUEUED".to_string()),
        }))
    }

    /// Single atomic backend operation: Saves revision, parses steps, normalizes scopes, decomposes, and emits event
    pub fn prepare_masterplan(
        &self,
        project_id: &str,
        raw_text: &str,
        target_step_count: i32,
        max_steps_per_agent: i32,
    ) -> Result<PreparedMasterplanSnapshot, String> {
        let plan = self.create_or_update_masterplan(
            project_id,
            raw_text,
            target_step_count,
            max_steps_per_agent,
        )?;
        let parsed_steps = self.parse_masterplan_text_to_steps(raw_text, target_step_count)?;
        let steps = self.decompose_masterplan(project_id, parsed_steps, None, None)?;

        let proj = self
            .list_projects()?
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or("Project not found")?;
        let handoff_prompt = format!(
            "Claim next available chunk for project '{}' (ID: {}) located at '{}' using tool 'masterplan_claim_chunk'.",
            proj.name, proj.id, proj.path
        );

        self.emit_event(
            Some(project_id),
            None,
            None,
            "MASTERPLAN_PREPARED",
            json!({ "plan_id": plan.id, "step_count": steps.len() }),
        );

        Ok(PreparedMasterplanSnapshot {
            masterplan: plan,
            total_steps: steps.len(),
            steps,
            target_step_count,
            max_steps_per_agent,
            handoff_prompt,
            next_action: "masterplan_claim_chunk".to_string(),
        })
    }

    #[allow(clippy::type_complexity)]
    pub fn get_current_context(
        &self,
        caller_agent_id: Option<&str>,
        project_id_filter: Option<&str>,
    ) -> Result<CurrentContext, String> {
        let summaries = self.list_all_masterplans()?;
        let agents = self.list_agents()?;
        let conn = self.db.lock();

        // 1. If caller has an active running task, prioritize caller task context
        if let Some(agent_id) = caller_agent_id {
            let (canon_caller, ..) = Self::canonicalize_ide_identity(agent_id, "");
            let active_task_opt: Option<(String, String, String, String, Option<String>, Option<String>)> = conn
                .query_row(
                    "SELECT t.id, t.project_id, t.title, t.state, t.worktree_path, p.name FROM tasks t
                     JOIN projects p ON t.project_id = p.id
                     WHERE (t.assigned_agent_id = ?1 OR t.assigned_agent_id = ?2) AND t.state IN ('RUNNING', 'CLAIMING', 'VERIFYING') AND t.is_stale = 0
                     ORDER BY t.updated_at DESC LIMIT 1",
                    [agent_id, &canon_caller],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )
                .ok();

            if let Some((
                task_id,
                project_id,
                task_title,
                task_state,
                worktree_path,
                project_name,
            )) = active_task_opt
            {
                let mut stmt_scopes = conn
                    .prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1")
                    .map_err(|e| e.to_string())?;
                let rows = stmt_scopes
                    .query_map([&task_id], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                let active_scopes: Vec<String> = rows
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("Failed to read active scope lease row: {}", e))?;
                drop(stmt_scopes);

                let active_attempt_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM task_attempts WHERE task_id = ?1 AND status = 'ACTIVE' ORDER BY attempt_number DESC LIMIT 1",
                        [&task_id],
                        |r| r.get(0),
                    )
                    .ok();

                let now_str = Utc::now().to_rfc3339();
                drop(conn);

                let wt = worktree_path.unwrap_or_else(|| "N/A".to_string());
                return Ok(CurrentContext {
                    active_project_id: Some(project_id.clone()),
                    project_name,
                    repository_path: Some(wt.clone()),
                    masterplan_id: None,
                    masterplan_status: Some("EXECUTING".to_string()),
                    masterplan_revision: None,
                    caller_agent_id: Some(agent_id.to_string()),
                    active_task_id: Some(task_id.clone()),
                    active_attempt_id,
                    active_scopes,
                    current_state: Some(task_state),
                    last_updated: Some(now_str),
                    next_recommended_action: "task_submit".to_string(),
                    handoff_prompt: format!(
                        "Continue active task '{}' (ID: {}) in worktree '{}'. Implement required steps and submit with 'task_submit(task_id=\"{}\", agent_id=\"{}\")'.",
                        task_title, task_id, wt, task_id, agent_id
                    ),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: format!("Work strictly inside worktree '{}'. Verify code and submit via task_submit.", wt),
                });
            }
        }
        drop(conn);

        // 2. Otherwise return targeted or primary project masterplan context
        let target = if let Some(pid) = project_id_filter {
            summaries.iter().find(|s| s.project_id == pid).cloned()
        } else {
            summaries.first().cloned()
        };

        if let Some(target) = target {
            let instructions = if target.status == "UNSORTED" {
                format!(
                    "1. Call 'project_context(project_id=\"{}\")' to fetch rules.\n2. Call 'masterplan_get(project_id=\"{}\")' to read specification.\n3. Call 'masterplan_decompose(project_id=\"{}\", steps=[...])' to structure the plan.",
                    target.project_id, target.project_id, target.project_id
                )
            } else {
                format!(
                    "1. Call 'agent_register(name=\"...\", agent_type=\"...\")' to get session token.\n2. Call 'masterplan_claim_chunk(project_id=\"{}\", agent_id=your_id, count={})' to allocate worktree.\n3. Acquire scope and implement steps.",
                    target.project_id, target.max_steps_per_agent
                )
            };

            let pending_tasks = target.pending_steps;

            Ok(CurrentContext {
                active_project_id: Some(target.project_id.clone()),
                project_name: Some(target.project_name.clone()),
                repository_path: Some(target.repository_path.clone()),
                masterplan_id: Some(target.masterplan_id.clone()),
                masterplan_status: Some(target.status.clone()),
                masterplan_revision: None,
                caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                active_task_id: None,
                active_attempt_id: None,
                active_scopes: Vec::new(),
                current_state: None,
                last_updated: Some(target.last_updated.clone()),
                next_recommended_action: target.next_action.clone(),
                handoff_prompt: target.handoff_prompt.clone(),
                active_agents_count: agents.len(),
                pending_tasks_count: pending_tasks,
                instructions,
            })
        } else {
            let projects = self.list_projects()?;
            if let Some(proj) = projects.first() {
                Ok(CurrentContext {
                    active_project_id: Some(proj.id.clone()),
                    project_name: Some(proj.name.clone()),
                    repository_path: Some(proj.path.clone()),
                    masterplan_id: None,
                    masterplan_status: None,
                    masterplan_revision: None,
                    caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                    active_task_id: None,
                    active_attempt_id: None,
                    active_scopes: Vec::new(),
                    current_state: None,
                    last_updated: Some(proj.created_at.clone()),
                    next_recommended_action: "create_masterplan".to_string(),
                    handoff_prompt: format!(
                        "Create masterplan for project '{}' ({}) in the AgentXFlow UI.",
                        proj.name, proj.id
                    ),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: "Create a masterplan in the AgentXFlow Workbench or add tasks."
                        .to_string(),
                })
            } else {
                Ok(CurrentContext {
                    active_project_id: None,
                    project_name: None,
                    repository_path: None,
                    masterplan_id: None,
                    masterplan_status: None,
                    masterplan_revision: None,
                    caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                    active_task_id: None,
                    active_attempt_id: None,
                    active_scopes: Vec::new(),
                    current_state: None,
                    last_updated: None,
                    next_recommended_action: "create_project".to_string(),
                    handoff_prompt: "No projects created yet. Create a project in AgentXFlow."
                        .to_string(),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: "Open AgentXFlow and create or import a Git repository."
                        .to_string(),
                })
            }
        }
    }

    /// Problem 7: Authoritatively resolves the active worktree path for a task,
    /// verifying caller ownership and ensuring the directory exists on disk.
    pub fn get_task_workspace_path(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> Result<std::path::PathBuf, String> {
        let conn = self.db.lock();
        let (worktree_path, assigned_agent): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT worktree_path, assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;
        drop(conn);

        // Caller authorization check
        if !agent_id.is_empty()
            && agent_id != "master"
            && agent_id != "system"
            && agent_id != "coordinator"
        {
            if let Some(ref assigned) = assigned_agent {
                let (canon_assigned, ..) = Self::canonicalize_ide_identity(assigned, "");
                let (canon_caller, ..) = Self::canonicalize_ide_identity(agent_id, "");
                if assigned != agent_id && canon_assigned != canon_caller {
                    return Err(format!(
                        "Task ownership violation: Task '{}' belongs to agent '{}', caller is '{}'",
                        task_id, assigned, agent_id
                    ));
                }
            } else {
                return Err(format!("Task '{}' is not assigned to any agent", task_id));
            }
        }

        let wt_str = worktree_path
            .ok_or_else(|| format!("Task '{}' has no worktree allocated yet", task_id))?;
        let wt_path = std::path::PathBuf::from(wt_str);
        if !wt_path.exists() {
            return Err(format!(
                "Task '{}' worktree path '{:?}' does not exist on disk",
                task_id, wt_path
            ));
        }

        Ok(wt_path)
    }

    /// Safely resolves and prevents directory traversal outside the task worktree
    fn resolve_worktree_relative_path(
        worktree_root: &Path,
        relative_path: &str,
    ) -> Result<std::path::PathBuf, String> {
        let clean = relative_path.replace('\\', "/");
        let clean = clean.trim_start_matches("./").trim_start_matches('/');

        // Reject paths containing parent navigation components
        for seg in clean.split('/') {
            if seg == ".." {
                return Err(format!(
                    "Path traversal prohibited: relative path '{}' contains '..' segments",
                    relative_path
                ));
            }
        }

        let target = worktree_root.join(clean);
        Ok(target)
    }

    /// Problem 7: Reads a file from the task's authoritative isolated worktree
    pub fn task_workspace_read(
        &self,
        task_id: &str,
        agent_id: &str,
        relative_path: &str,
    ) -> Result<String, String> {
        let worktree_dir = self.get_task_workspace_path(task_id, agent_id)?;
        let target_file = Self::resolve_worktree_relative_path(&worktree_dir, relative_path)?;

        if !crate::git::path_is_under_root(&target_file, &worktree_dir) {
            return Err(format!(
                "Path traversal prohibited: path '{}' escapes worktree boundary",
                relative_path
            ));
        }

        if !target_file.exists() {
            return Err(format!(
                "File '{}' not found in task worktree",
                relative_path
            ));
        }

        std::fs::read_to_string(&target_file)
            .map_err(|e| format!("Failed to read file '{}': {}", relative_path, e))
    }

    /// Problem 7: Writes a file in the task's authoritative isolated worktree,
    /// enforcing directory containment and write scope lease coverage.
    pub fn task_workspace_write(
        &self,
        task_id: &str,
        agent_id: &str,
        relative_path: &str,
        content: &str,
    ) -> Result<(), String> {
        let worktree_dir = self.get_task_workspace_path(task_id, agent_id)?;
        let target_file = Self::resolve_worktree_relative_path(&worktree_dir, relative_path)?;

        if !crate::git::path_is_under_root(&target_file, &worktree_dir) {
            return Err(format!(
                "Path traversal prohibited: path '{}' escapes worktree boundary",
                relative_path
            ));
        }

        let norm_path = relative_path.replace('\\', "/");
        let clean_path = norm_path
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_string();

        // Check write scope restrictions (fail-closed)
        let violations = self.scope.audit_attempt_mutations(
            task_id,
            None,
            agent_id,
            std::slice::from_ref(&clean_path),
        )?;

        if !violations.is_empty() {
            return Err(format!(
                "Scope violation: File '{}' is not covered by any active exclusive write scope lease for task '{}'",
                clean_path, task_id
            ));
        }

        let project_id: String = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT project_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?
        };

        // Policy engine guardrail evaluation
        let (action, reason) = self.policy.evaluate_hook(&project_id, "pre-mutation", &clean_path)?;
        if action == "DENY" {
            return Err(format!(
                "Policy violation: file write to '{}' denied: {}",
                clean_path,
                reason.unwrap_or_else(|| "Blocked by security policy".to_string())
            ));
        }

        if let Some(parent) = target_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create parent directory for '{}': {}",
                    relative_path, e
                )
            })?;
        }

        std::fs::write(&target_file, content)
            .map_err(|e| format!("Failed to write file '{}': {}", relative_path, e))
    }

    /// Problem 7: Executes a command strictly inside the task's worktree with bounded timeout
    pub fn task_workspace_exec(
        &self,
        task_id: &str,
        agent_id: &str,
        command: &str,
        args: Option<Vec<String>>,
        timeout_secs: Option<u64>,
    ) -> Result<crate::models::VerificationRun, String> {
        let worktree_dir = self.get_task_workspace_path(task_id, agent_id)?;

        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(60).clamp(1, 300));
        let full_cmd = if let Some(args_vec) = args {
            if args_vec.is_empty() {
                command.to_string()
            } else {
                let args_json = serde_json::to_string(&args_vec).unwrap_or_default();
                crate::verification::build_command_with_args(command, &args_json)
            }
        } else {
            command.to_string()
        };

        let project_id: String = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT project_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?
        };

        // Policy engine guardrail evaluation
        let (action, reason) = self.policy.evaluate_hook(&project_id, "pre-command", &full_cmd)?;
        if action == "DENY" {
            return Err(format!(
                "Policy violation: command execution '{}' denied: {}",
                full_cmd,
                reason.unwrap_or_else(|| "Blocked by security policy".to_string())
            ));
        }

        let head_sha = self
            .git
            .get_head_sha(&worktree_dir)
            .unwrap_or_else(|_| "HEAD".to_string());

        self.verify.execute_check(
            task_id,
            "task_workspace_exec",
            "WORKSPACE_EXEC",
            &worktree_dir,
            &head_sha,
            &full_cmd,
            timeout,
        )
    }
}

/// Validated task state transition on a caller-owned connection/transaction.
/// Reads the current state, validates it against `from` AND `TaskState::can_transition_to`,
/// then executes the UPDATE. Errors with "Illegal transition" when the current state is not
/// an allowed source or the transition is absent from the legal transition table, and errors
/// when the task does not exist (0 rows affected). The caller owns the transaction.
pub(crate) fn transition_task_state_on_conn(
    conn: &rusqlite::Connection,
    task_id: &str,
    from: &[TaskState],
    to: TaskState,
) -> Result<(), String> {
    let current_state: String = conn
        .query_row("SELECT state FROM tasks WHERE id = ?1", [task_id], |r| {
            r.get(0)
        })
        .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

    let cur = TaskState::parse(&current_state);
    if !from.contains(&cur) {
        return Err(format!(
            "Illegal transition: task '{}' is in state '{}' which is not an allowed source state ({:?})",
            task_id, current_state, from
        ));
    }
    if !cur.can_transition_to(&to) {
        return Err(format!(
            "Illegal transition: task '{}' is in state '{}' which cannot transition to '{}'",
            task_id,
            current_state,
            to.as_str()
        ));
    }

    let now = Utc::now().to_rfc3339();
    let rows = conn
        .execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![to.as_str(), now, task_id],
        )
        .map_err(|e| format!("Failed to update task '{}' state: {}", task_id, e))?;

    if rows == 0 {
        return Err(format!("Task '{}' not found", task_id));
    }
    Ok(())
}

/// Transaction-body cancellation writes on a caller-owned connection/transaction:
/// releases scope leases, reverts claimed masterplan steps to PENDING, removes
/// merge-queue entries, and transitions the task to CANCELLED + stale.
/// DONE is guarded by the caller (`cancel_task` rejects it before entering).
/// Event emission and worktree cleanup are the caller's responsibility and must
/// happen AFTER commit, since both lock the DB.
pub(crate) fn cancel_task_inner(
    conn: &rusqlite::Connection,
    task_id: &str,
    caller_agent_id: Option<&str>,
    _reason: Option<&str>,
) -> Result<(), String> {
    if let Some(caller) = caller_agent_id {
        let (state, assigned_agent): (String, Option<String>) = conn
            .query_row(
                "SELECT state, assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

        if state == "DONE" {
            return Err(format!(
                "Cannot cancel task '{}': Task is already DONE (merged)",
                task_id
            ));
        }

        if let Some(ref assigned) = assigned_agent {
            let (canon_assigned, ..) = CoordinatorEngine::canonicalize_ide_identity(assigned, "");
            let (canon_caller, ..) = CoordinatorEngine::canonicalize_ide_identity(caller, "");
            if !assigned.is_empty() && assigned != caller && canon_assigned != canon_caller {
                return Err(format!(
                    "Authorization error: Caller agent '{}' is not the owner of task '{}' (assigned to '{}')",
                    caller, task_id, assigned
                ));
            }
        } else {
            return Err(format!(
                "Authorization error: Cannot cancel unassigned task '{}'",
                task_id
            ));
        }
    }

    let now = Utc::now().to_rfc3339();

    // 1. Release all scope leases held by this task
    conn.execute("DELETE FROM scope_leases WHERE task_id = ?1", [task_id])
        .map_err(|e| e.to_string())?;

    // 2. Return any claimed masterplan steps back to PENDING (completed steps are strictly preserved)
    conn.execute(
        "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1 WHERE claimed_task_id = ?2 AND status != 'COMPLETED'",
        params![now, task_id],
    )
    .map_err(|e| e.to_string())?;

    // 3. Remove from merge queue if present
    conn.execute("DELETE FROM merge_queue WHERE task_id = ?1", [task_id])
        .map_err(|e| e.to_string())?;

    // 4. Update task state to CANCELLED and mark is_stale = 1
    // (DONE is guarded by the caller, so every other state is a legal source)
    transition_task_state_on_conn(
        conn,
        task_id,
        &[
            TaskState::Backlog,
            TaskState::Ready,
            TaskState::Running,
            TaskState::Verifying,
            TaskState::Verified,
            TaskState::Blocked,
            TaskState::Review,
            TaskState::MergeReady,
            TaskState::Failed,
            TaskState::Cancelled,
        ],
        TaskState::Cancelled,
    )?;
    conn.execute(
        "UPDATE tasks SET substate = 'NONE', is_stale = 1 WHERE id = ?1",
        [task_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests;
