use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{ProofBundle, VerificationResult, VerificationRun};

#[derive(Debug, Clone)]
pub struct VerificationEngine {
    db: DbPool,
}

impl VerificationEngine {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }

    /// Executes a configured verification check command directly in the task worktree
    pub fn execute_check(
        &self,
        task_id: &str,
        check_id: &str,
        check_name: &str,
        worktree_path: &Path,
        commit_sha: &str,
        command_str: &str,
    ) -> Result<VerificationRun, String> {
        info!("Executing coordinator verification check '{}' [{}] in {:?}", check_name, command_str, worktree_path);

        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command string".to_string());
        }

        let start = Instant::now();

        #[cfg(target_os = "windows")]
        let output_res = Command::new("cmd")
            .args(["/c", command_str])
            .current_dir(worktree_path)
            .output();

        #[cfg(not(target_os = "windows"))]
        let output_res = Command::new("sh")
            .args(["-c", command_str])
            .current_dir(worktree_path)
            .output();

        let duration_ms = start.elapsed().as_millis() as i64;
        let (exit_code, stdout, stderr, is_passed) = match output_res {
            Ok(output) => {
                let code = output.status.code().unwrap_or(-1);
                let out = String::from_utf8_lossy(&output.stdout).to_string();
                let err = String::from_utf8_lossy(&output.stderr).to_string();
                let passed = output.status.success();
                (code, out, err, passed)
            }
            Err(e) => (-1, String::new(), format!("Failed to spawn command: {}", e), false),
        };

        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let run = VerificationRun {
            id: run_id.clone(),
            task_id: task_id.to_string(),
            run_id: None,
            check_id: check_id.to_string(),
            check_name: check_name.to_string(),
            commit_sha: commit_sha.to_string(),
            command: command_str.to_string(),
            exit_code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            duration_ms,
            is_passed,
            is_stale: false,
            executed_at: now.clone(),
        };

        // Record in SQLite
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                run.id, run.task_id, run.run_id, run.check_id, run.check_name,
                run.commit_sha, run.command, run.exit_code, run.stdout, run.stderr,
                run.duration_ms, run.is_passed, run.is_stale, run.executed_at
            ],
        ).map_err(|e| e.to_string())?;

        // Also record as first-class Coordinator-Observed evidence
        let ev_id = Uuid::new_v4().to_string();
        let payload = serde_json::json!({
            "command": command_str,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "passed": is_passed,
            "commit_sha": commit_sha,
        });

        conn.execute(
            "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
             VALUES (?1, ?2, NULL, 'TEST_RESULT', 'COORDINATOR_OBSERVED', ?3, ?4)",
            rusqlite::params![ev_id, task_id, payload.to_string(), now],
        ).ok();

        Ok(run)
    }

    /// Automatically marks previous verification runs as stale if task HEAD moved
    pub fn invalidate_stale_verifications(&self, task_id: &str, current_head_sha: &str) -> Result<(), String> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE verification_runs SET is_stale = 1 WHERE task_id = ?1 AND commit_sha != ?2",
            rusqlite::params![task_id, current_head_sha],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Verifies task submission against mandatory checklist, evidence, acceptance criteria, and coordinator checks
    pub fn verify_task_submission(&self, task_id: &str, current_head_sha: &str) -> Result<VerificationResult, String> {
        let conn = self.db.lock();

        // 1. Mandatory Steps Checklist Gate
        let mut stmt = conn
            .prepare("SELECT id, title, is_mandatory, status FROM task_steps WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;

        let steps_iter = stmt
            .query_map([task_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?, row.get::<_, String>(3)?))
            })
            .map_err(|e| e.to_string())?;

        let mut missing_steps = Vec::new();
        for step in steps_iter.flatten() {
            let (_id, title, is_mandatory, status) = step;
            if is_mandatory && status != "COMPLETED" {
                missing_steps.push(title);
            }
        }

        // 2. Mandatory Acceptance Criteria Gate
        let mut stmt_crit = conn
            .prepare("SELECT criterion, is_satisfied FROM acceptance_criteria WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;

        let crit_iter = stmt_crit
            .query_map([task_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            })
            .map_err(|e| e.to_string())?;

        let mut unsatisfied_criteria = Vec::new();
        for c in crit_iter.flatten() {
            let (crit_text, is_satisfied) = c;
            if !is_satisfied {
                unsatisfied_criteria.push(crit_text);
            }
        }

        // 3. Unresolved Scope Violations Gate
        let mut stmt_violations = conn
            .prepare("SELECT file_path FROM scope_violations WHERE task_id = ?1 AND resolved = 0")
            .map_err(|e| e.to_string())?;

        let violations_iter = stmt_violations
            .query_map([task_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let mut unresolved_violations = Vec::new();
        for v in violations_iter.flatten() {
            unresolved_violations.push(v);
        }

        // 4. Coordinator-Executed Verification Runs Gate
        let mut stmt_runs = conn
            .prepare("SELECT check_name, is_passed, is_stale FROM verification_runs WHERE task_id = ?1 AND commit_sha = ?2")
            .map_err(|e| e.to_string())?;

        let runs_iter = stmt_runs
            .query_map(rusqlite::params![task_id, current_head_sha], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))
            })
            .map_err(|e| e.to_string())?;

        let mut total_runs_count = 0;
        let mut failed_checks = Vec::new();
        for r in runs_iter.flatten() {
            total_runs_count += 1;
            let (check_name, is_passed, is_stale) = r;
            if !is_passed || is_stale {
                failed_checks.push(check_name);
            }
        }

        let mut rejection_reasons = Vec::new();
        if total_runs_count == 0 {
            rejection_reasons.push("UNVERIFIED: Zero coordinator verification checks were executed against submitted commit HEAD. At least one passing check is required.".to_string());
        }
        for step in &missing_steps {
            rejection_reasons.push(format!("Mandatory step '{}' is not marked COMPLETED", step));
        }
        for crit in &unsatisfied_criteria {
            rejection_reasons.push(format!("Acceptance criterion '{}' is not satisfied", crit));
        }
        for file in &unresolved_violations {
            rejection_reasons.push(format!("Unresolved out-of-scope modification: {}", file));
        }
        for check in &failed_checks {
            rejection_reasons.push(format!("Coordinator check '{}' failed or is stale", check));
        }

        let is_valid = rejection_reasons.is_empty();

        Ok(VerificationResult {
            is_valid,
            missing_mandatory_steps: missing_steps,
            missing_evidence_step_ids: Vec::new(),
            unresolved_scope_violations: unresolved_violations,
            failed_coordinator_checks: failed_checks,
            rejection_reasons,
        })
    }

    /// Generates a deterministic Verified Evidence Bundle with canonical SHA-256 digest
    #[allow(clippy::too_many_arguments)]
    pub fn generate_proof_bundle(
        &self,
        task_id: &str,
        project_id: &str,
        agent_id: Option<&str>,
        prompt: &str,
        base_sha: &str,
        head_sha: &str,
        files_changed: &[String],
        diff_summary: &str,
    ) -> Result<ProofBundle, String> {
        let conn = self.db.lock();

        let mut stmt = conn
            .prepare("SELECT id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at FROM verification_runs WHERE task_id = ?1 AND commit_sha = ?2")
            .map_err(|e| e.to_string())?;

        let runs_iter = stmt
            .query_map(rusqlite::params![task_id, head_sha], |row| {
                Ok(VerificationRun {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    run_id: row.get(2)?,
                    check_id: row.get(3)?,
                    check_name: row.get(4)?,
                    commit_sha: row.get(5)?,
                    command: row.get(6)?,
                    exit_code: row.get(7)?,
                    stdout: row.get(8)?,
                    stderr: row.get(9)?,
                    duration_ms: row.get(10)?,
                    is_passed: row.get(11)?,
                    is_stale: row.get(12)?,
                    executed_at: row.get(13)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut verification_runs = Vec::new();
        for r in runs_iter.flatten() {
            verification_runs.push(r);
        }

        // Canonical deterministic SHA256 digest across all verified package attributes
        let mut hasher = Sha256::new();
        hasher.update(task_id.as_bytes());
        hasher.update(project_id.as_bytes());
        hasher.update(base_sha.as_bytes());
        hasher.update(head_sha.as_bytes());
        for f in files_changed {
            hasher.update(f.as_bytes());
        }
        hasher.update(diff_summary.as_bytes());
        for run in &verification_runs {
            hasher.update(run.check_name.as_bytes());
            hasher.update(run.exit_code.to_string().as_bytes());
        }
        let proof_hash = hex::encode(hasher.finalize());

        let bundle = ProofBundle {
            task_id: task_id.to_string(),
            project_id: project_id.to_string(),
            agent_id: agent_id.map(|s| s.to_string()),
            prompt: prompt.to_string(),
            base_sha: base_sha.to_string(),
            head_sha: head_sha.to_string(),
            files_changed: files_changed.to_vec(),
            diff_summary: diff_summary.to_string(),
            verification_runs,
            scope_violations: Vec::new(),
            proof_hash: proof_hash.clone(),
            generated_at: Utc::now().to_rfc3339(),
        };

        let files_json = serde_json::to_string(&bundle.files_changed).unwrap_or("[]".to_string());
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT OR REPLACE INTO proof_bundles (id, task_id, project_id, agent_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, proof_hash, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id, bundle.task_id, bundle.project_id, bundle.agent_id, bundle.prompt,
                bundle.base_sha, bundle.head_sha, files_json, bundle.diff_summary,
                bundle.proof_hash, bundle.generated_at
            ],
        ).map_err(|e| e.to_string())?;

        Ok(bundle)
    }
}
