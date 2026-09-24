use chrono::Utc;
use rusqlite::OptionalExtension;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::core::transition_task_state_on_conn;
use crate::db::DbPool;
use crate::git::GitService;
use crate::models::{IntegrationAttempt, MergeQueueItem, TaskState};
use crate::verification::VerificationEngine;

/// Maximum time allowed for post-merge verification (cargo test / npm test) to run in the
/// integration worktree. Without this bound, a hung merge test would stall the background
/// merge worker (and the 3s merge loop) forever. The verification executor tree-kills the
/// offending process when the timeout fires.
pub(crate) const POST_MERGE_VERIFY_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct MergeEngine {
    db: DbPool,
    git: GitService,
}

impl MergeEngine {
    pub fn new(db: DbPool, git: GitService) -> Self {
        Self { db, git }
    }

    /// Sequence-numbered event emitter. Mirrors `CoordinatorEngine::emit_event` column-for-column
    /// (`events.event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp`;
    /// `sequence` is AUTOINCREMENT) so merge lifecycle events are indistinguishable from core events.
    /// Best-effort telemetry: a failed INSERT never fails the merge (same policy as core).
    fn emit_event(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let conn = self.db.lock();
        let event_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let payload_str = payload.to_string();

        conn.execute(
            "INSERT INTO events (event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![event_id, project_id, task_id, agent_id, event_type, payload_str, now],
        )
        .ok();
    }

    /// Adds a verified task to the serialized merge queue
    pub fn enqueue_task(
        &self,
        project_id: &str,
        task_id: &str,
        branch_name: &str,
        target_branch: &str,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<MergeQueueItem, String> {
        let conn = self.db.lock();
        let existing = match conn.query_row(
            "SELECT id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at, processed_at
             FROM merge_queue WHERE project_id = ?1 AND task_id = ?2",
            [project_id, task_id],
            |r| {
                Ok(MergeQueueItem {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    task_id: r.get(2)?,
                    branch_name: r.get(3)?,
                    target_branch: r.get(4)?,
                    position: r.get(5)?,
                    status: r.get(6)?,
                    base_sha: r.get(7)?,
                    head_sha: r.get(8)?,
                    queued_at: r.get(9)?,
                    processed_at: r.get(10)?,
                })
            },
        ) {
            Ok(item) => Some(item),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(format!("Failed to query merge queue for task '{}': {}", task_id, e)),
        };

        let now = Utc::now().to_rfc3339();

        if let Some(mut item) = existing {
            if item.status == "MERGED" {
                return Err(format!(
                    "Task '{}' has already been merged into target branch '{}'. Cannot re-enqueue merged task.",
                    task_id, item.target_branch
                ));
            }
            if item.status == "RUNNING_CHECKS" {
                return Err(format!(
                    "Task '{}' is currently undergoing merge checks in queue item '{}'",
                    task_id, item.id
                ));
            }

            // For existing items: update in-place preserving existing primary key id & history (prevents CASCADE deletion)
            conn.execute(
                "UPDATE merge_queue SET branch_name = ?1, target_branch = ?2, base_sha = ?3, head_sha = ?4, status = 'READY', queued_at = ?5, processed_at = NULL WHERE id = ?6",
                rusqlite::params![branch_name, target_branch, base_sha, head_sha, now, item.id],
            ).map_err(|e| format!("Failed to update existing merge queue item: {}", e))?;

            item.branch_name = branch_name.to_string();
            item.target_branch = target_branch.to_string();
            item.base_sha = base_sha.to_string();
            item.head_sha = head_sha.to_string();
            item.status = "READY".to_string();
            item.queued_at = now;
            item.processed_at = None;
            return Ok(item);
        }

        let id = Uuid::new_v4().to_string();

        let max_pos: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0) FROM merge_queue WHERE project_id = ?1 AND processed_at IS NULL",
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let position = max_pos + 1;

        conn.execute(
            "INSERT INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'READY', ?7, ?8, ?9)",
            rusqlite::params![id, project_id, task_id, branch_name, target_branch, position, base_sha, head_sha, now],
        ).map_err(|e| format!("Failed to insert merge queue item: {}", e))?;

        Ok(MergeQueueItem {
            id,
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
            branch_name: branch_name.to_string(),
            target_branch: target_branch.to_string(),
            position,
            status: "READY".to_string(),
            base_sha: base_sha.to_string(),
            head_sha: head_sha.to_string(),
            queued_at: now,
            processed_at: None,
        })
    }

    pub fn list_queue(&self, project_id: &str) -> Result<Vec<MergeQueueItem>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at, processed_at FROM merge_queue WHERE project_id = ?1 ORDER BY position ASC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([project_id], |row| {
                Ok(MergeQueueItem {
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
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows.flatten() {
            items.push(r);
        }
        Ok(items)
    }

    /// Authoritative serialized FIFO merge processor: Reloads candidate by queue_item_id from SQLite
    pub fn process_merge_by_id(
        &self,
        queue_item_id: &str,
        repo_path: &Path,
    ) -> Result<IntegrationAttempt, String> {
        let item: MergeQueueItem = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at, processed_at FROM merge_queue WHERE id = ?1",
                [queue_item_id],
                |row| {
                    Ok(MergeQueueItem {
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
            ).map_err(|e| format!("Queue item '{}' not found: {}", queue_item_id, e))?
        };

        // 1. Strict FIFO Serialization Check: No earlier item may be skipped
        {
            let conn = self.db.lock();
            let older_ready_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM merge_queue WHERE project_id = ?1 AND target_branch = ?2 AND status = 'READY' AND position < ?3",
                    rusqlite::params![item.project_id, item.target_branch, item.position],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to query FIFO merge queue state: {}", e))?;

            if older_ready_count > 0 {
                return Err(format!(
                    "FIFO queue ordering violation: {} earlier candidate(s) are queued ahead of item '{}'. Merges must proceed sequentially.",
                    older_ready_count, item.id
                ));
            }

            // 2. Active integration check: Max 1 active integration per target branch
            let running_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM merge_queue WHERE project_id = ?1 AND target_branch = ?2 AND status = 'RUNNING_CHECKS' AND id != ?3",
                    rusqlite::params![item.project_id, item.target_branch, item.id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("Failed to query active merge integrations: {}", e))?;

            if running_count > 0 {
                return Err(format!(
                    "Concurrency lock: Target branch '{}' currently has an active merge integration in progress. Please wait for completion.",
                    item.target_branch
                ));
            }

            // Atomically mark RUNNING_CHECKS via CAS: require status == 'READY' and affected_rows == 1
            let affected_rows = conn
                .execute(
                    "UPDATE merge_queue SET status = 'RUNNING_CHECKS' WHERE id = ?1 AND status = 'READY'",
                    [&item.id],
                )
                .map_err(|e| format!("Failed to claim merge queue item '{}': {}", item.id, e))?;

            if affected_rows != 1 {
                return Err(format!(
                    "Concurrency conflict: Merge queue item '{}' was already claimed or is no longer READY (affected rows: {}).",
                    item.id, affected_rows
                ));
            }

            let project_id = item.project_id.clone();
            let task_id = item.task_id.clone();
            let item_id = item.id.clone();
            drop(conn);
            self.emit_event(
                Some(&project_id),
                Some(&task_id),
                None,
                "MERGE_STARTED",
                json!({ "queue_item_id": item_id }),
            );
        }

        self.process_merge(&item.project_id, repo_path, &item)
    }

    /// Merges candidate using isolated disposable integration worktree without dirtying user root checkout
    pub fn process_merge(
        &self,
        project_id: &str,
        repo_path: &Path,
        item: &MergeQueueItem,
    ) -> Result<IntegrationAttempt, String> {
        info!(
            "Processing merge queue item '{}' for task '{}' (Branch: {})",
            item.id, item.task_id, item.branch_name
        );

        let target_sha_before = self.git.get_ref_sha(repo_path, &item.target_branch)?;

        // 1. Stale base detection: If target branch has moved past recorded base, STOP and mark STALE
        if target_sha_before != item.base_sha {
            let conn = self.db.lock();
            conn.execute(
                "UPDATE merge_queue SET status = 'STALE' WHERE id = ?1",
                [&item.id],
            )
            .ok();
            let blocked = transition_task_state_on_conn(
                &conn,
                &item.task_id,
                &[TaskState::MergeReady],
                TaskState::Blocked,
            );
            match &blocked {
                Ok(()) => {
                    conn.execute(
                        "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                        [&item.task_id],
                    )
                    .ok();
                }
                Err(e) => error!(
                    "Failed to mark task '{}' BLOCKED (stale base): {}",
                    item.task_id, e
                ),
            }
            drop(conn);
            if blocked.is_ok() {
                self.emit_event(
                    Some(&item.project_id),
                    Some(&item.task_id),
                    None,
                    "MERGE_BLOCKED_STALE",
                    json!({
                        "queue_item_id": item.id.clone(),
                        "base_sha": item.base_sha.clone(),
                        "current_sha": target_sha_before,
                    }),
                );
            }
            info!(
                "Merge candidate '{}' has stale base SHA (recorded: {}, current: {}). Stopped.",
                item.id, item.base_sha, target_sha_before
            );
            return Err(format!("Target branch '{}' has moved (current SHA: {}). Candidate base is STALE. Rebase required.", item.target_branch, target_sha_before));
        }

        // 2. Ensure dedicated disposable integration worktree exists
        let integration_dir =
            self.git
                .ensure_integration_worktree(repo_path, project_id, &item.target_branch)?;

        let now = Utc::now().to_rfc3339();
        let attempt_id = Uuid::new_v4().to_string();

        // Reset integration workspace to exact target branch state
        if let Err(reset_err) = self
            .git
            .run_git_cmd(&integration_dir, &["reset", "--hard", &item.target_branch])
            .and_then(|_| self.git.run_git_cmd(&integration_dir, &["clean", "-fd"]))
        {
            let conn = self.db.lock();
            conn.execute(
                "UPDATE merge_queue SET status = 'BLOCKED' WHERE id = ?1",
                [&item.id],
            )
            .ok();
            let blocked = transition_task_state_on_conn(
                &conn,
                &item.task_id,
                &[TaskState::MergeReady],
                TaskState::Blocked,
            );
            match &blocked {
                Ok(()) => {
                    conn.execute(
                        "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                        [&item.task_id],
                    )
                    .ok();
                }
                Err(e) => error!(
                    "Failed to mark task '{}' BLOCKED (integration worktree reset): {}",
                    item.task_id, e
                ),
            }
            let attempt = IntegrationAttempt {
                id: attempt_id,
                merge_queue_id: item.id.clone(),
                simulation_passed: false,
                conflicts_json: Some(format!(
                    "Integration worktree reset to '{}' failed: {}",
                    item.target_branch, reset_err
                )),
                post_merge_verification_passed: false,
                merge_strategy: "MERGE_COMMIT".to_string(),
                target_sha_before,
                target_sha_after: None,
                attempted_at: now,
            };
            conn.execute(
                "INSERT INTO integration_attempts (id, merge_queue_id, simulation_passed, conflicts_json, post_merge_verification_passed, merge_strategy, target_sha_before, target_sha_after, attempted_at)
                 VALUES (?1, ?2, 0, ?3, 0, 'MERGE_COMMIT', ?4, NULL, ?5)",
                rusqlite::params![attempt.id, attempt.merge_queue_id, attempt.conflicts_json, attempt.target_sha_before, attempt.attempted_at],
            )
            .ok();
            drop(conn);
            if blocked.is_ok() {
                self.emit_event(
                    Some(&item.project_id),
                    Some(&item.task_id),
                    None,
                    "MERGE_BLOCKED_RESET",
                    json!({ "queue_item_id": item.id.clone() }),
                );
            }
            error!(
                "Integration worktree reset failed for task '{}': {}",
                item.task_id, reset_err
            );
            self.git
                .remove_worktree(
                    repo_path,
                    &[repo_path.join(".agentxflow")],
                    &integration_dir,
                )
                .ok();
            return Err(format!(
                "Failed to reset integration worktree to target branch '{}': {}",
                item.target_branch, reset_err
            ));
        }

        // 3. Execute 3-way merge in integration worktree
        let merge_commit_msg = format!("Merge task {}: {}", item.task_id, item.branch_name);
        let merge_res = self.git.run_git_cmd(
            &integration_dir,
            &[
                "merge",
                "--no-ff",
                "-m",
                &merge_commit_msg,
                &item.branch_name,
            ],
        );

        match merge_res {
            Ok(_) => {
                // Invariant 2: Determine the exact integration-tree commit/HEAD that is actually being tested
                let integration_head = self.git.get_head_sha(&integration_dir)?;

                // 4. Invariant 3: Run real post-merge verification tests honoring the configured verification contract.
                // Reuses the shared verification executor, bounds by POST_MERGE_VERIFY_TIMEOUT, and tree-kills hung processes.
                let verify = VerificationEngine::new(self.db.clone());
                let outcome = match verify.execute_post_merge_verification(
                    &item.project_id,
                    &item.task_id,
                    &integration_dir,
                    &integration_head,
                    POST_MERGE_VERIFY_TIMEOUT,
                ) {
                    Ok(o) => o,
                    Err(e) => {
                        error!(
                            "Post-merge verification could not be executed for task '{}': {}",
                            item.task_id, e
                        );
                        crate::verification::PostMergeVerificationOutcome {
                            passed: false,
                            timed_out: false,
                            runs: Vec::new(),
                        }
                    }
                };

                let post_merge_passed = outcome.passed;
                let post_merge_timed_out = outcome.timed_out;

                if !post_merge_passed {
                    let conn = self.db.lock();
                    conn.execute(
                        "UPDATE merge_queue SET status = 'FAILED_TESTS' WHERE id = ?1",
                        [&item.id],
                    )
                    .ok();
                    let blocked = transition_task_state_on_conn(
                        &conn,
                        &item.task_id,
                        &[TaskState::MergeReady],
                        TaskState::Blocked,
                    );
                    match &blocked {
                        Ok(()) => {
                            conn.execute(
                                "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                                [&item.task_id],
                            )
                            .ok();
                        }
                        Err(e) => error!(
                            "Failed to mark task '{}' BLOCKED (post-merge tests): {}",
                            item.task_id, e
                        ),
                    }
                    drop(conn);
                    if blocked.is_ok() {
                        self.emit_event(
                            Some(&item.project_id),
                            Some(&item.task_id),
                            None,
                            "MERGE_BLOCKED_TESTS",
                            json!({
                                "queue_item_id": item.id.clone(),
                                "timed_out": post_merge_timed_out,
                            }),
                        );
                    }
                    self.git
                        .remove_worktree(
                            repo_path,
                            &[repo_path.join(".agentxflow")],
                            &integration_dir,
                        )
                        .ok();
                    if post_merge_timed_out {
                        warn!(
                            "Post-merge verification TIMED OUT after {} seconds for task '{}' in integration worktree {:?}; process tree terminated. Integration aborted.",
                            POST_MERGE_VERIFY_TIMEOUT.as_secs(),
                            item.task_id,
                            integration_dir
                        );
                        return Err(format!(
                            "Post-merge verification test suite timed out after {} seconds and was terminated in the integration worktree. Integration aborted.",
                            POST_MERGE_VERIFY_TIMEOUT.as_secs()
                        ));
                    }
                    return Err("Post-merge verification test suite failed in integration worktree. Integration aborted.".to_string());
                }

                // 5. Target branch authority: Re-read authoritative target state right before finalizing.
                // If the target branch moved while post-merge verification was running, do not finalize
                // based on stale assumptions. Abort cleanly, mark STALE, and block task for rebase.
                let target_sha_current = self.git.get_ref_sha(repo_path, &item.target_branch)?;
                if target_sha_current != target_sha_before {
                    let conn = self.db.lock();
                    conn.execute(
                        "UPDATE merge_queue SET status = 'STALE' WHERE id = ?1",
                        [&item.id],
                    )
                    .ok();
                    let blocked = transition_task_state_on_conn(
                        &conn,
                        &item.task_id,
                        &[TaskState::MergeReady],
                        TaskState::Blocked,
                    );
                    if blocked.is_ok() {
                        conn.execute(
                            "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                            [&item.task_id],
                        )
                        .ok();
                    }
                    drop(conn);
                    self.git
                        .remove_worktree(
                            repo_path,
                            &[repo_path.join(".agentxflow")],
                            &integration_dir,
                        )
                        .ok();
                    self.emit_event(
                        Some(&item.project_id),
                        Some(&item.task_id),
                        None,
                        "MERGE_BLOCKED_STALE",
                        json!({
                            "queue_item_id": item.id.clone(),
                            "base_sha": target_sha_before,
                            "current_sha": target_sha_current,
                        }),
                    );
                    return Err(format!(
                        "Target branch '{}' moved during post-merge verification (recorded: {}, current: {}). Candidate base is STALE. Rebase required.",
                        item.target_branch, target_sha_before, target_sha_current
                    ));
                }

                // Check if candidate is already an ancestor of target branch (e.g. prior Git advance succeeded before interruption)
                let already_merged = match self.git.is_ancestor(
                    repo_path,
                    &item.head_sha,
                    &item.target_branch,
                ) {
                    Ok(crate::git::GitAncestorResult::Ancestor) => true,
                    Ok(crate::git::GitAncestorResult::NotAncestor) => false,
                    Err(e) => {
                        let is_invalid_head = e.contains("Not a valid commit name")
                            || e.contains("not a valid commit name");
                        if is_invalid_head {
                            false
                        } else {
                            return Err(format!(
                                "Cannot authoritatively determine ancestor status of commit '{}' on target branch '{}': {}",
                                item.head_sha, item.target_branch, e
                            ));
                        }
                    }
                };

                if !already_merged {
                    // 6. Advance the target branch ref atomically using Compare-and-Swap (CAS)
                    self.git.run_git_cmd(
                        repo_path,
                        &[
                            "update-ref",
                            &format!("refs/heads/{}", item.target_branch),
                            &integration_head,
                            &target_sha_before,
                        ],
                    )?;
                } else {
                    info!(
                        "Task '{}' commit '{}' is already in target branch '{}'; skipping ref update and proceeding to finalization",
                        item.task_id, item.head_sha, item.target_branch
                    );
                }

                // 7. Synchronize primary repository working directory on disk if target branch is currently checked out.
                // SAFETY (master plan Invariant A): never destroy user work. Only fast-forward a CLEAN primary checkout.
                if let Ok(current_branch) = self.git.get_current_branch(repo_path) {
                    if current_branch == item.target_branch {
                        match self.git.check_worktree_cleanliness(repo_path) {
                            Ok(()) => {
                                info!("Synchronizing primary repository working directory on disk to newly merged HEAD: {}", integration_head);
                                // The CAS update-ref at step 6 already advanced the branch; on a clean tree
                                // merge --ff-only verifies the tree matches HEAD before touching anything.
                                if let Err(e) = self.git.run_git_cmd(
                                    repo_path,
                                    &["merge", "--ff-only", &integration_head],
                                ) {
                                    warn!("Primary checkout fast-forward sync failed (ref already advanced; working copy left untouched): {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Primary checkout has uncommitted/staged/untracked changes on {}; working copy left untouched. Ref was advanced to {}. {}", item.target_branch, integration_head, e.join(", "));
                            }
                        }
                    }
                }

                let target_sha_after = self
                    .git
                    .get_ref_sha(repo_path, &item.target_branch)
                    .map_err(|e| {
                        format!(
                            "Failed to read target branch '{}' ref SHA after merge: {}",
                            item.target_branch, e
                        )
                    })?;

                // Authoritative Git verification: verify the target branch actually contains the merged candidate
                match self
                    .git
                    .is_ancestor(repo_path, &item.head_sha, &target_sha_after)
                {
                    Ok(crate::git::GitAncestorResult::Ancestor) => {}
                    Ok(crate::git::GitAncestorResult::NotAncestor) => {
                        return Err(format!(
                            "Target branch '{}' ref divergence detected: candidate commit '{}' is not an ancestor of current target ref '{}'. Finalization aborted.",
                            item.target_branch, item.head_sha, target_sha_after
                        ));
                    }
                    Err(e) => {
                        return Err(format!(
                            "Cannot authoritatively verify candidate commit '{}' in target branch '{}' ref '{}': {}. Finalization aborted.",
                            item.head_sha, item.target_branch, target_sha_after, e
                        ));
                    }
                }

                let mut conn = self.db.lock();
                let tx = conn.transaction().map_err(|e| {
                    format!("Failed to start merge finalization transaction: {}", e)
                })?;

                tx.execute(
                    "UPDATE merge_queue SET status = 'MERGED', processed_at = ?1 WHERE id = ?2",
                    [&now, &item.id],
                )
                .map_err(|e| format!("Failed to update merge_queue to MERGED: {}", e))?;

                transition_task_state_on_conn(
                    &tx,
                    &item.task_id,
                    &[
                        TaskState::MergeReady,
                        TaskState::Verifying,
                        TaskState::Review,
                    ],
                    TaskState::Done,
                )
                .map_err(|e| {
                    format!(
                        "Failed to mark task '{}' DONE after merge: {}",
                        item.task_id, e
                    )
                })?;

                tx.execute(
                    "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                    [&item.task_id],
                )
                .map_err(|e| format!("Failed to clear task substate after merge: {}", e))?;

                // Complete associated masterplan steps
                tx.execute(
                    "UPDATE masterplan_steps SET status = 'COMPLETED', completed_at = ?1, updated_at = ?1 WHERE claimed_task_id = ?2",
                    [&now, &item.task_id],
                )
                .map_err(|e| format!("Failed to update masterplan steps to COMPLETED after merge of task '{}': {}", item.task_id, e))?;

                let task_masterplan_id: Option<String> = tx
                    .query_row(
                        "SELECT masterplan_id FROM tasks WHERE id = ?1",
                        [&item.task_id],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .unwrap_or(None)
                    .flatten();

                if let Some(mp_id) = task_masterplan_id {
                    let pending_remaining: i64 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1 AND status != 'COMPLETED'",
                            [&mp_id],
                            |r| r.get(0),
                        )
                        .map_err(|e| format!("Failed to query remaining masterplan steps for plan '{}': {}", mp_id, e))?;

                    if pending_remaining == 0 {
                        tx.execute(
                            "UPDATE masterplans SET status = 'COMPLETED', updated_at = ?1 WHERE id = ?2",
                            [&now, &mp_id],
                        )
                        .map_err(|e| format!("Failed to update masterplan status to COMPLETED for plan '{}': {}", mp_id, e))?;
                    }
                }

                let attempt = IntegrationAttempt {
                    id: attempt_id,
                    merge_queue_id: item.id.clone(),
                    simulation_passed: true,
                    conflicts_json: None,
                    post_merge_verification_passed: true,
                    merge_strategy: "MERGE_COMMIT".to_string(),
                    target_sha_before,
                    target_sha_after: Some(target_sha_after),
                    attempted_at: now,
                };

                tx.execute(
                    "INSERT INTO integration_attempts (id, merge_queue_id, simulation_passed, conflicts_json, post_merge_verification_passed, merge_strategy, target_sha_before, target_sha_after, attempted_at)
                     VALUES (?1, ?2, 1, NULL, 1, 'MERGE_COMMIT', ?3, ?4, ?5)",
                    rusqlite::params![attempt.id, attempt.merge_queue_id, attempt.target_sha_before, attempt.target_sha_after, attempt.attempted_at],
                )
                .map_err(|e| format!("Failed to persist integration attempt: {}", e))?;

                tx.commit()
                    .map_err(|e| format!("Failed to commit merge finalization: {}", e))?;
                drop(conn);

                self.emit_event(
                    Some(&item.project_id),
                    Some(&item.task_id),
                    None,
                    "MERGE_DONE",
                    json!({
                        "queue_item_id": item.id.clone(),
                        "target_sha_after": attempt.target_sha_after.clone(),
                    }),
                );

                // Clean up disposable integration worktree
                self.git
                    .remove_worktree(
                        repo_path,
                        &[repo_path.join(".agentxflow")],
                        &integration_dir,
                    )
                    .ok();

                Ok(attempt)
            }
            Err(err) => {
                // Abort merge cleanly in integration worktree
                self.git
                    .run_git_cmd(&integration_dir, &["merge", "--abort"])
                    .ok();

                let conn = self.db.lock();
                conn.execute(
                    "UPDATE merge_queue SET status = 'BLOCKED_CONFLICT' WHERE id = ?1",
                    [&item.id],
                )
                .ok();
                let blocked = transition_task_state_on_conn(
                    &conn,
                    &item.task_id,
                    &[TaskState::MergeReady],
                    TaskState::Blocked,
                );
                match &blocked {
                    Ok(()) => {
                        conn.execute(
                            "UPDATE tasks SET substate = 'NONE' WHERE id = ?1",
                            [&item.task_id],
                        )
                        .ok();
                    }
                    Err(e) => error!(
                        "Failed to mark task '{}' BLOCKED (merge conflict): {}",
                        item.task_id, e
                    ),
                }

                error!(
                    "Merge conflict detected for task '{}': {}",
                    item.task_id, err
                );

                let attempt = IntegrationAttempt {
                    id: attempt_id,
                    merge_queue_id: item.id.clone(),
                    simulation_passed: false,
                    conflicts_json: Some(err),
                    post_merge_verification_passed: false,
                    merge_strategy: "MERGE_COMMIT".to_string(),
                    target_sha_before,
                    target_sha_after: None,
                    attempted_at: now,
                };

                conn.execute(
                    "INSERT INTO integration_attempts (id, merge_queue_id, simulation_passed, conflicts_json, post_merge_verification_passed, merge_strategy, target_sha_before, target_sha_after, attempted_at)
                     VALUES (?1, ?2, 0, ?3, 0, 'MERGE_COMMIT', ?4, NULL, ?5)",
                    rusqlite::params![attempt.id, attempt.merge_queue_id, attempt.conflicts_json, attempt.target_sha_before, attempt.attempted_at],
                ).ok();

                drop(conn);
                if blocked.is_ok() {
                    self.emit_event(
                        Some(&item.project_id),
                        Some(&item.task_id),
                        None,
                        "MERGE_BLOCKED_CONFLICT",
                        json!({ "queue_item_id": item.id.clone() }),
                    );
                }

                // Clean up disposable integration worktree
                self.git
                    .remove_worktree(
                        repo_path,
                        &[repo_path.join(".agentxflow")],
                        &integration_dir,
                    )
                    .ok();

                Ok(attempt)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbPool;
    use crate::git::GitService;
    use std::sync::Arc;

    fn setup_test_merge_engine() -> (MergeEngine, DbPool) {
        let dir = std::env::temp_dir().join(format!("test_merge_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test_merge.db");
        let pool = DbPool::new(&db_path).unwrap();
        (MergeEngine::new(pool.clone(), GitService::new()), pool)
    }

    #[test]
    fn test_concurrent_merge_queue_claim_is_atomic() {
        let (merge, pool) = setup_test_merge_engine();
        let merge = Arc::new(merge);

        // Insert a dummy project and task, then enqueue a READY merge item
        let conn = pool.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t1', 'p1', 'T1', 'Desc', 'MERGE_READY', 'NONE', 'MEDIUM', 0, ?1, ?1)",
            [&now],
        ).unwrap();
        drop(conn);

        let item = merge
            .enqueue_task(
                "p1",
                "t1",
                "agentxflow/task-t1",
                "main",
                "base_sha",
                "head_sha",
            )
            .unwrap();
        let item_id = item.id.clone();

        // Spawn 2 competing threads trying to process the same item simultaneously
        let merge_clone1 = merge.clone();
        let item_id1 = item_id.clone();
        let handle1 = std::thread::spawn(move || {
            merge_clone1.process_merge_by_id(&item_id1, Path::new("dummy/path"))
        });

        let merge_clone2 = merge.clone();
        let item_id2 = item_id.clone();
        let handle2 = std::thread::spawn(move || {
            merge_clone2.process_merge_by_id(&item_id2, Path::new("dummy/path"))
        });

        let res1 = handle1.join().unwrap();
        let res2 = handle2.join().unwrap();

        // Exactly ONE claimant can successfully claim RUNNING_CHECKS; the other must receive concurrency error
        let (success_count, conflict_count) = match (res1, res2) {
            (Ok(_), Err(e))
                if e.contains("Concurrency conflict") || e.contains("Concurrency lock") =>
            {
                (1, 1)
            }
            (Err(e), Ok(_))
                if e.contains("Concurrency conflict") || e.contains("Concurrency lock") =>
            {
                (1, 1)
            }
            (Err(e1), Err(e2)) => {
                // If git path is dummy, the winner will fail later at git execution, but only ONE failed with Concurrency conflict
                let c1 = e1.contains("Concurrency conflict") || e1.contains("Concurrency lock");
                let c2 = e2.contains("Concurrency conflict") || e2.contains("Concurrency lock");
                assert!(
                    c1 ^ c2,
                    "Exactly one thread must fail with concurrency conflict (e1: {}, e2: {})",
                    e1,
                    e2
                );
                (1, 1)
            }
            _ => (0, 0),
        };

        assert_eq!(success_count, 1, "Exactly one claim must proceed");
        assert_eq!(
            conflict_count, 1,
            "Competing claim must be rejected cleanly"
        );
    }

    #[test]
    fn test_merge_queue_fifo_ordering_enforcement() {
        let pool = DbPool::new_in_memory().unwrap();
        let merge = MergeEngine::new(pool.clone(), GitService::new());
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Create project, task, and 2 merge queue items: item 1 (position 1), item 2 (position 2)
        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p_mq', 'P MQ', 'dummy/path', 'Spec', 'main', ?1, ?1)",
                [&now],
            ).unwrap();
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t1', 'p_mq', 'T1', 'D', 'VERIFYING', 'READY_FOR_MERGE', 'HIGH', 0, ?1, ?1)",
                [&now],
            ).unwrap();
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t2', 'p_mq', 'T2', 'D', 'VERIFYING', 'READY_FOR_MERGE', 'HIGH', 0, ?1, ?1)",
                [&now],
            ).unwrap();
        }

        let mq1 = merge
            .enqueue_task("p_mq", "t1", "feat/1", "main", "base", "head")
            .unwrap();
        let mq2 = merge
            .enqueue_task("p_mq", "t2", "feat/2", "main", "base", "head")
            .unwrap();

        // Attempting to process mq2 while mq1 is earlier READY candidate must fail with FIFO error
        let res_skip = merge.process_merge_by_id(&mq2.id, Path::new("dummy/path"));
        assert!(
            res_skip.is_err(),
            "Skipping earlier item must fail with FIFO violation"
        );
        assert!(res_skip
            .unwrap_err()
            .contains("FIFO queue ordering violation"));

        // Processing mq1 transitions past FIFO check (may fail later due to dummy git path, but passed FIFO check)
        let res_first = merge.process_merge_by_id(&mq1.id, Path::new("dummy/path"));
        if let Err(ref e) = res_first {
            assert!(
                !e.contains("FIFO queue ordering violation"),
                "Earliest item must not fail FIFO check: {}",
                e
            );
        }
    }
}
