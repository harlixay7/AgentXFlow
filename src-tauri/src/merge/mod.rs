use chrono::Utc;
use std::path::Path;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::DbPool;
use crate::git::GitService;
use crate::models::{IntegrationAttempt, MergeQueueItem};

#[derive(Debug, Clone)]
pub struct MergeEngine {
    db: DbPool,
    git: GitService,
}

impl MergeEngine {
    pub fn new(db: DbPool, git: GitService) -> Self {
        Self { db, git }
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
        let existing = conn.query_row(
            "SELECT id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at, processed_at FROM merge_queue WHERE project_id = ?1 AND task_id = ?2 AND processed_at IS NULL",
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
        ).ok();

        if let Some(mut item) = existing {
            conn.execute(
                "UPDATE merge_queue SET branch_name = ?1, target_branch = ?2, base_sha = ?3, head_sha = ?4, status = 'READY' WHERE id = ?5",
                rusqlite::params![branch_name, target_branch, base_sha, head_sha, item.id],
            ).map_err(|e| e.to_string())?;
            item.branch_name = branch_name.to_string();
            item.target_branch = target_branch.to_string();
            item.base_sha = base_sha.to_string();
            item.head_sha = head_sha.to_string();
            item.status = "READY".to_string();
            return Ok(item);
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let max_pos: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0) FROM merge_queue WHERE project_id = ?1 AND processed_at IS NULL",
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let position = max_pos + 1;

        conn.execute(
            "INSERT OR REPLACE INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'READY', ?7, ?8, ?9)",
            rusqlite::params![id, project_id, task_id, branch_name, target_branch, position, base_sha, head_sha, now],
        ).map_err(|e| e.to_string())?;

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
                .unwrap_or(0);

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
                .unwrap_or(0);

            if running_count > 0 {
                return Err(format!(
                    "Concurrency lock: Target branch '{}' currently has an active merge integration in progress. Please wait for completion.",
                    item.target_branch
                ));
            }

            // Atomically mark RUNNING_CHECKS
            conn.execute("UPDATE merge_queue SET status = 'RUNNING_CHECKS' WHERE id = ?1", [&item.id]).ok();
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
        info!("Processing merge queue item '{}' for task '{}' (Branch: {})", item.id, item.task_id, item.branch_name);

        let target_sha_before = self.git.get_ref_sha(repo_path, &item.target_branch)?;

        // 1. Stale base detection: If target branch has moved past recorded base, STOP and mark STALE
        if target_sha_before != item.base_sha {
            let conn = self.db.lock();
            conn.execute("UPDATE merge_queue SET status = 'STALE' WHERE id = ?1", [&item.id]).ok();
            conn.execute("UPDATE tasks SET state = 'BLOCKED', substate = 'NONE' WHERE id = ?1", [&item.task_id]).ok();
            info!("Merge candidate '{}' has stale base SHA (recorded: {}, current: {}). Stopped.", item.id, item.base_sha, target_sha_before);
            return Err(format!("Target branch '{}' has moved (current SHA: {}). Candidate base is STALE. Rebase required.", item.target_branch, target_sha_before));
        }

        // 2. Ensure dedicated disposable integration worktree exists
        let integration_dir = self.git.ensure_integration_worktree(repo_path, project_id, &item.target_branch)?;

        // Reset integration workspace to exact target branch state
        self.git.run_git_cmd(&integration_dir, &["reset", "--hard", &item.target_branch]).ok();
        self.git.run_git_cmd(&integration_dir, &["clean", "-fd"]).ok();

        // 3. Execute 3-way merge simulation in integration worktree
        let merge_res = self.git.run_git_cmd(&integration_dir, &["merge", "--no-commit", "--no-ff", &item.branch_name]);

        let now = Utc::now().to_rfc3339();
        let attempt_id = Uuid::new_v4().to_string();

        match merge_res {
            Ok(_) => {
                // 4. Run real post-merge verification tests if configured
                let mut post_merge_passed = true;
                if integration_dir.join("Cargo.toml").exists() {
                    let out = std::process::Command::new("cargo")
                        .args(["test"])
                        .current_dir(&integration_dir)
                        .output();
                    if let Ok(res) = out {
                        if !res.status.success() {
                            post_merge_passed = false;
                        }
                    }
                } else if integration_dir.join("package.json").exists() {
                    let out = std::process::Command::new("npm")
                        .args(["test"])
                        .current_dir(&integration_dir)
                        .output();
                    if let Ok(res) = out {
                        if !res.status.success() {
                            post_merge_passed = false;
                        }
                    }
                }

                if !post_merge_passed {
                    self.git.run_git_cmd(&integration_dir, &["merge", "--abort"]).ok();
                    let conn = self.db.lock();
                    conn.execute("UPDATE merge_queue SET status = 'FAILED_TESTS' WHERE id = ?1", [&item.id]).ok();
                    conn.execute("UPDATE tasks SET state = 'BLOCKED', substate = 'NONE' WHERE id = ?1", [&item.task_id]).ok();
                    self.git.remove_worktree(repo_path, &integration_dir).ok();
                    return Err("Post-merge verification test suite failed in integration worktree. Integration aborted.".to_string());
                }

                // 5. Commit merge in integration worktree
                let commit_res = self.git.run_git_cmd(
                    &integration_dir,
                    &["commit", "-m", &format!("Merge task {}: {}", item.task_id, item.branch_name)],
                );

                if let Err(commit_err) = commit_res {
                    self.git.run_git_cmd(&integration_dir, &["merge", "--abort"]).ok();
                    let conn = self.db.lock();
                    conn.execute("UPDATE merge_queue SET status = 'FAILED' WHERE id = ?1", [&item.id]).ok();
                    self.git.remove_worktree(repo_path, &integration_dir).ok();
                    return Err(format!("Failed to commit merge in integration worktree: {}", commit_err));
                }

                // 6. Advance the target branch ref atomically using Compare-and-Swap (CAS)
                let integration_head = self.git.get_head_sha(&integration_dir)?;
                self.git.run_git_cmd(
                    repo_path,
                    &["update-ref", &format!("refs/heads/{}", item.target_branch), &integration_head, &target_sha_before],
                )?;

                // 7. Synchronize primary repository working directory on disk if target branch is currently checked out
                if let Ok(current_branch) = self.git.get_current_branch(repo_path) {
                    if current_branch == item.target_branch {
                        info!("Synchronizing primary repository working directory on disk to newly merged HEAD: {}", integration_head);
                        let _ = self.git.run_git_cmd(repo_path, &["reset", "--hard", "HEAD"]);
                        let _ = self.git.run_git_cmd(repo_path, &["clean", "-fd"]);
                    }
                }

                let target_sha_after = self.git.get_ref_sha(repo_path, &item.target_branch).ok();

                let conn = self.db.lock();
                conn.execute("UPDATE merge_queue SET status = 'MERGED', processed_at = ?1 WHERE id = ?2", [&now, &item.id]).ok();
                conn.execute("UPDATE tasks SET state = 'DONE', substate = 'NONE', updated_at = ?1 WHERE id = ?2", [&now, &item.task_id]).ok();
                // Complete associated masterplan steps
                conn.execute("UPDATE masterplan_steps SET status = 'COMPLETED', completed_at = ?1, updated_at = ?1 WHERE claimed_task_id = ?2", [&now, &item.task_id]).ok();

                let pending_remaining: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM masterplan_steps ms
                     JOIN masterplans mp ON ms.masterplan_id = mp.id
                     WHERE mp.project_id = ?1 AND ms.status != 'COMPLETED'",
                    [&item.project_id],
                    |r| r.get(0),
                ).unwrap_or(1);

                if pending_remaining == 0 {
                    conn.execute("UPDATE masterplans SET status = 'COMPLETED', updated_at = ?1 WHERE project_id = ?2", [&now, &item.project_id]).ok();
                }

                let attempt = IntegrationAttempt {
                    id: attempt_id,
                    merge_queue_id: item.id.clone(),
                    simulation_passed: true,
                    conflicts_json: None,
                    post_merge_verification_passed: true,
                    merge_strategy: "MERGE_COMMIT".to_string(),
                    target_sha_before,
                    target_sha_after,
                    attempted_at: now,
                };

                conn.execute(
                    "INSERT INTO integration_attempts (id, merge_queue_id, simulation_passed, conflicts_json, post_merge_verification_passed, merge_strategy, target_sha_before, target_sha_after, attempted_at)
                     VALUES (?1, ?2, 1, NULL, 1, 'MERGE_COMMIT', ?3, ?4, ?5)",
                    rusqlite::params![attempt.id, attempt.merge_queue_id, attempt.target_sha_before, attempt.target_sha_after, attempt.attempted_at],
                ).ok();

                // Clean up disposable integration worktree
                self.git.remove_worktree(repo_path, &integration_dir).ok();

                Ok(attempt)
            }
            Err(err) => {
                // Abort merge cleanly in integration worktree
                self.git.run_git_cmd(&integration_dir, &["merge", "--abort"]).ok();

                let conn = self.db.lock();
                conn.execute("UPDATE merge_queue SET status = 'BLOCKED_CONFLICT' WHERE id = ?1", [&item.id]).ok();
                conn.execute("UPDATE tasks SET state = 'BLOCKED', substate = 'NONE', updated_at = ?1 WHERE id = ?2", [&now, &item.task_id]).ok();

                error!("Merge conflict detected for task '{}': {}", item.task_id, err);

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

                // Clean up disposable integration worktree
                self.git.remove_worktree(repo_path, &integration_dir).ok();

                Ok(attempt)
            }
        }
    }
}
