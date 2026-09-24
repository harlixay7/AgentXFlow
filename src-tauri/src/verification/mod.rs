use chrono::Utc;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{EvaluatorResult, ProofBundle, VerificationResult, VerificationRun};

const MAX_OUTPUT_BYTES: usize = 65_536; // 64 KB per stream

fn kill_process_tree(child: &std::process::Child) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &format!("-{}", child.id())])
            .output();
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredCheck {
    pub id: String,
    pub check_type: String,
    pub command: String,
    pub args_json: String,
    pub timeout_secs: i32,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct PostMergeVerificationOutcome {
    pub passed: bool,
    pub timed_out: bool,
    pub runs: Vec<VerificationRun>,
}

pub fn build_command_with_args(cmd: &str, args_json: &str) -> String {
    if args_json.trim().is_empty() || args_json == "[]" {
        return cmd.trim().to_string();
    }
    match serde_json::from_str::<Vec<String>>(args_json) {
        Ok(args) if !args.is_empty() => {
            let mut full = cmd.trim().to_string();
            for arg in args {
                full.push(' ');
                if arg.contains(' ') || arg.contains('"') {
                    full.push('"');
                    full.push_str(&arg.replace('"', "\\\""));
                    full.push('"');
                } else {
                    full.push_str(&arg);
                }
            }
            full
        }
        _ => cmd.trim().to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct VerificationEngine {
    db: DbPool,
}

impl VerificationEngine {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }

    /// Executes a configured verification check command directly in the task worktree
    /// with strict timeout, process termination, output capping, and duration tracking.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_check(
        &self,
        task_id: &str,
        check_id: &str,
        check_name: &str,
        worktree_path: &Path,
        commit_sha: &str,
        command_str: &str,
        timeout: Duration,
    ) -> Result<VerificationRun, String> {
        if timeout.as_secs() < 1 {
            return Err("timeout must be >= 1 second".to_string());
        }

        info!(
            "Executing coordinator verification check '{}' [{}] in {:?}",
            check_name, command_str, worktree_path
        );

        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command string".to_string());
        }

        let start = Instant::now();

        let mut cmd = Command::new(if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        });
        if cfg!(target_os = "windows") {
            cmd.args(["/c", command_str]);
        } else {
            cmd.args(["-c", command_str]);
        }
        cmd.current_dir(worktree_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn command '{}': {}", command_str, e))?;

        let mut out_reader = child.stdout.take().map(|mut handle| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = handle.read_to_end(&mut buf);
                buf
            })
        });
        let mut err_reader = child.stderr.take().map(|mut handle| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = handle.read_to_end(&mut buf);
                buf
            })
        });

        // Poll with timeout
        let mut timed_out = false;
        let mut exit_status: Option<std::process::ExitStatus> = None;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    break;
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        timed_out = true;
                        kill_process_tree(&child);
                        let _ = child.kill();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    warn!("Error waiting on child process: {}", e);
                    kill_process_tree(&child);
                    let _ = child.kill();
                    break;
                }
            }
        }
        let _ = child.wait(); // reap the direct child

        let stdout = out_reader
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        let stderr = err_reader
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        let mut out = String::from_utf8_lossy(&stdout).into_owned();
        let mut err = String::from_utf8_lossy(&stderr).into_owned();

        if out.len() > MAX_OUTPUT_BYTES {
            out.truncate(MAX_OUTPUT_BYTES);
            out.push_str("\n\n[...STDOUT TRUNCATED BY COORDINATOR (EXCEEDED 64KB)...]");
        }
        if err.len() > MAX_OUTPUT_BYTES {
            err.truncate(MAX_OUTPUT_BYTES);
            err.push_str("\n\n[...STDERR TRUNCATED BY COORDINATOR (EXCEEDED 64KB)...]");
        }

        if timed_out {
            let timeout_msg = format!(
                "Command timed out after {} seconds and was terminated.",
                timeout.as_secs()
            );
            if !err.is_empty() {
                err.push_str("\n\n");
            }
            err.push_str(&timeout_msg);
        }

        let (exit_code, is_passed) = match (timed_out, exit_status) {
            (true, _) => (-1, false),
            (false, Some(s)) => (s.code().unwrap_or(-1), s.code() == Some(0)),
            (false, None) => (-1, false),
        };

        let duration_ms = start.elapsed().as_millis() as i64;

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
            stdout: out.clone(),
            stderr: err.clone(),
            duration_ms,
            is_passed,
            is_stale: false,
            executed_at: now.clone(),
            timed_out,
        };

        // Record in SQLite inside an atomic transaction
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to begin verification run transaction: {}", e))?;

        tx.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at, timed_out)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                run.id, run.task_id, run.run_id, run.check_id, run.check_name,
                run.commit_sha, run.command, run.exit_code, run.stdout, run.stderr,
                run.duration_ms, run.is_passed, run.is_stale, run.executed_at, timed_out
            ],
        ).map_err(|e| format!("Failed to record verification run: {}", e))?;

        // Also record as first-class Coordinator-Observed evidence
        let ev_id = Uuid::new_v4().to_string();
        let payload = serde_json::json!({
            "command": command_str,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "passed": is_passed,
            "timed_out": timed_out,
            "commit_sha": commit_sha,
        });

        tx.execute(
            "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
             VALUES (?1, ?2, NULL, 'TEST_RESULT', 'COORDINATOR_OBSERVED', ?3, ?4)",
            rusqlite::params![ev_id, task_id, payload.to_string(), now],
        ).map_err(|e| format!("Failed to record verification evidence record: {}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit verification run transaction: {}", e))?;
        drop(conn);

        Ok(run)
    }

    /// Automatically marks previous verification runs as stale if task HEAD moved
    pub fn invalidate_stale_verifications(
        &self,
        task_id: &str,
        current_head_sha: &str,
    ) -> Result<(), String> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE verification_runs SET is_stale = 1 WHERE task_id = ?1 AND commit_sha != ?2",
            rusqlite::params![task_id, current_head_sha],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Returns all configured verification profile checks for a project/task, or builds sensible repository defaults.
    pub fn get_verification_profiles(
        &self,
        project_id: &str,
        task_id: Option<&str>,
        worktree_path: &Path,
    ) -> Result<Vec<ConfiguredCheck>, String> {
        let mut configured_checks = {
            let conn = self.db.lock();
            let target_task = task_id.unwrap_or("");
            let mut stmt_prof = conn
                .prepare(
                    "SELECT id, check_type, command, args_json, timeout_secs, required 
                     FROM verification_profiles 
                     WHERE project_id = ?1 AND (task_id IS NULL OR task_id = ?2)",
                )
                .map_err(|e| format!("Failed to prepare verification profiles query: {}", e))?;

            let rows = stmt_prof
                .query_map(rusqlite::params![project_id, target_task], |row| {
                    Ok(ConfiguredCheck {
                        id: row.get(0)?,
                        check_type: row.get(1)?,
                        command: row.get(2)?,
                        args_json: row.get(3)?,
                        timeout_secs: row.get(4)?,
                        required: row.get(5)?,
                    })
                })
                .map_err(|e| format!("Failed to query verification profiles: {}", e))?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read verification profile row: {}", e))?
        };

        // If no custom profile configured, build standard suite from repository markers
        if configured_checks.is_empty() {
            if worktree_path.join("Cargo.toml").exists() {
                configured_checks.push(ConfiguredCheck {
                    id: "chk_cargo_check".to_string(),
                    check_type: "TYPECHECK".to_string(),
                    command: "cargo check".to_string(),
                    args_json: "[]".to_string(),
                    timeout_secs: 60,
                    required: true,
                });
                configured_checks.push(ConfiguredCheck {
                    id: "chk_cargo_test".to_string(),
                    check_type: "UNIT_TESTS".to_string(),
                    command: "cargo test".to_string(),
                    args_json: "[]".to_string(),
                    timeout_secs: 120,
                    required: true,
                });
            } else if worktree_path.join("package.json").exists() {
                let pkg_path = worktree_path.join("package.json");
                let pkg_content = std::fs::read_to_string(&pkg_path)
                    .map_err(|e| format!("Failed to read package.json at {:?}: {}", pkg_path, e))?;
                let pkg_json: serde_json::Value =
                    serde_json::from_str(&pkg_content).map_err(|e| {
                        format!("Failed to parse package.json at {:?}: {}", pkg_path, e)
                    })?;
                let scripts = pkg_json.get("scripts");

                let mut added_npm_check = false;

                if let Some(scripts_obj) = scripts.and_then(|s| s.as_object()) {
                    // 1. Build check
                    if scripts_obj.contains_key("build") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_build".to_string(),
                            check_type: "BUILD".to_string(),
                            command: "npm run build".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 120,
                            required: true,
                        });
                        added_npm_check = true;
                    }

                    // 2. Typecheck check
                    if scripts_obj.contains_key("typecheck") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_typecheck".to_string(),
                            check_type: "TYPECHECK".to_string(),
                            command: "npm run typecheck".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    } else if scripts_obj.contains_key("type-check") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_typecheck".to_string(),
                            check_type: "TYPECHECK".to_string(),
                            command: "npm run type-check".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    } else if worktree_path.join("tsconfig.json").exists()
                        && !scripts_obj.contains_key("build")
                    {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_tsc_check".to_string(),
                            check_type: "TYPECHECK".to_string(),
                            command: "npx tsc --noEmit".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    }

                    // 3. Unit test check
                    if scripts_obj.contains_key("test") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_test".to_string(),
                            check_type: "UNIT_TESTS".to_string(),
                            command: "npm test".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    }

                    // 4. Lint check (optional by default)
                    if scripts_obj.contains_key("lint") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_lint".to_string(),
                            check_type: "LINT".to_string(),
                            command: "npm run lint".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: false,
                        });
                    }

                    // 5. Smoke / Runtime check
                    if scripts_obj.contains_key("smoke") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_smoke".to_string(),
                            check_type: "SMOKE_TEST".to_string(),
                            command: "npm run smoke".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    } else if scripts_obj.contains_key("test:smoke") {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_smoke".to_string(),
                            check_type: "SMOKE_TEST".to_string(),
                            command: "npm run test:smoke".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                        added_npm_check = true;
                    }
                }

                if !added_npm_check {
                    if worktree_path.join("tsconfig.json").exists() {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_tsc_check".to_string(),
                            check_type: "TYPECHECK".to_string(),
                            command: "npx tsc --noEmit".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                    } else {
                        configured_checks.push(ConfiguredCheck {
                            id: "chk_npm_test".to_string(),
                            check_type: "UNIT_TESTS".to_string(),
                            command: "npm test".to_string(),
                            args_json: "[]".to_string(),
                            timeout_secs: 60,
                            required: true,
                        });
                    }
                }
            } else if worktree_path.join("pyproject.toml").exists()
                || worktree_path.join("requirements.txt").exists()
                || worktree_path.join("setup.py").exists()
            {
                configured_checks.push(ConfiguredCheck {
                    id: "chk_python_test".to_string(),
                    check_type: "UNIT_TESTS".to_string(),
                    command: "pytest".to_string(),
                    args_json: "[]".to_string(),
                    timeout_secs: 60,
                    required: true,
                });
            } else {
                // Generic smoke check
                configured_checks.push(ConfiguredCheck {
                    id: "chk_git_status".to_string(),
                    check_type: "BUILD".to_string(),
                    command: "git status".to_string(),
                    args_json: "[]".to_string(),
                    timeout_secs: 30,
                    required: true,
                });
            }
        }

        Ok(configured_checks)
    }

    /// Executes all configured verification profile checks and machine evaluators for an attempt
    pub fn execute_profile_for_attempt(
        &self,
        task_id: &str,
        attempt_id: &str,
        project_id: &str,
        worktree_path: &Path,
        commit_sha: &str,
    ) -> Result<Vec<EvaluatorResult>, String> {
        info!(
            "Running comprehensive verification profile for task '{}' (attempt '{}', commit '{}')",
            task_id, attempt_id, commit_sha
        );

        let configured_checks =
            self.get_verification_profiles(project_id, Some(task_id), worktree_path)?;
        let mut results = Vec::new();
        let now_str = Utc::now().to_rfc3339();

        for check in &configured_checks {
            let timeout = Duration::from_secs(check.timeout_secs.max(1) as u64);
            let full_cmd = build_command_with_args(&check.command, &check.args_json);
            let run_res = self.execute_check(
                task_id,
                &check.id,
                &check.check_type,
                worktree_path,
                commit_sha,
                &full_cmd,
                timeout,
            )?;

            // Compute SHA256 of stdout + stderr
            let mut hasher = Sha256::new();
            hasher.update(run_res.stdout.as_bytes());
            hasher.update(run_res.stderr.as_bytes());
            let out_hash = hex::encode(hasher.finalize());

            let eval_id = Uuid::new_v4().to_string();
            let eval_res = EvaluatorResult {
                id: eval_id.clone(),
                task_id: task_id.to_string(),
                attempt_id: attempt_id.to_string(),
                criterion_id: None,
                evaluator_name: check.check_type.clone(),
                evaluator_type: check.check_type.clone(),
                evaluator_version: "1.0.0".to_string(),
                commit_sha: commit_sha.to_string(),
                exit_code: run_res.exit_code,
                stdout_output: run_res.stdout.clone(),
                stderr_output: run_res.stderr.clone(),
                output_sha256: out_hash.clone(),
                duration_ms: run_res.duration_ms,
                passed: run_res.is_passed,
                evaluated_at: now_str.clone(),
            };

            let conn = self.db.lock();
            conn.execute(
                "INSERT INTO evaluator_results (id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    eval_res.id, eval_res.task_id, eval_res.attempt_id, eval_res.criterion_id,
                    eval_res.evaluator_name, eval_res.evaluator_type, eval_res.evaluator_version,
                    eval_res.commit_sha, eval_res.exit_code, eval_res.stdout_output,
                    eval_res.stderr_output, eval_res.output_sha256, eval_res.duration_ms,
                    eval_res.passed, eval_res.evaluated_at
                ],
            ).map_err(|e| format!("Failed to record evaluator result for '{}': {}", eval_res.evaluator_name, e))?;

            results.push(eval_res);
        }

        // Automatic machine criteria satisfaction derived strictly from passing required evaluators
        let all_required_passed = !results.is_empty()
            && configured_checks
                .iter()
                .zip(&results)
                .filter(|(c, _)| c.required)
                .all(|(_, r)| r.passed);

        if all_required_passed {
            let conn = self.db.lock();
            conn.execute(
                "UPDATE acceptance_criteria SET is_satisfied = 1 WHERE task_id = ?1",
                [task_id],
            )
            .map_err(|e| format!("Failed to update acceptance criteria: {}", e))?;
        }

        Ok(results)
    }

    /// Executes post-merge verification against the exact integration HEAD commit.
    /// Honors configured verification profiles, args_json, and required/optional semantics.
    pub fn execute_post_merge_verification(
        &self,
        project_id: &str,
        task_id: &str,
        worktree_path: &Path,
        commit_sha: &str,
        max_timeout: Duration,
    ) -> Result<PostMergeVerificationOutcome, String> {
        let configured_checks =
            self.get_verification_profiles(project_id, Some(task_id), worktree_path)?;
        let mut runs = Vec::new();
        let mut required_passed = true;
        let mut timed_out = false;

        for check in &configured_checks {
            let configured_timeout = Duration::from_secs(check.timeout_secs.max(1) as u64);
            let timeout = configured_timeout.min(max_timeout);
            let full_cmd = build_command_with_args(&check.command, &check.args_json);

            let run = self.execute_check(
                task_id,
                &check.id,
                &check.check_type,
                worktree_path,
                commit_sha,
                &full_cmd,
                timeout,
            )?;

            if run.timed_out {
                timed_out = true;
            }

            if check.required && !run.is_passed {
                required_passed = false;
            }

            runs.push(run);
        }

        Ok(PostMergeVerificationOutcome {
            passed: required_passed,
            timed_out,
            runs,
        })
    }

    /// Verifies task submission against mandatory checklist, evidence, machine evaluators, and coordinator checks
    pub fn verify_task_submission(
        &self,
        task_id: &str,
        current_head_sha: &str,
    ) -> Result<VerificationResult, String> {
        let conn = self.db.lock();

        // 1. Mandatory Steps Checklist Gate
        let mut stmt = conn
            .prepare("SELECT id, title, is_mandatory, status FROM task_steps WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;

        let steps_iter = stmt
            .query_map([task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut missing_steps = Vec::new();
        for step in steps_iter {
            let (_id, title, is_mandatory, status) =
                step.map_err(|e| format!("Failed to read task step row: {}", e))?;
            if is_mandatory && status != "COMPLETED" {
                missing_steps.push(title);
            }
        }

        // 2. Unresolved Scope Violations Gate
        let mut stmt_violations = conn
            .prepare("SELECT file_path FROM scope_violations WHERE task_id = ?1 AND resolved = 0")
            .map_err(|e| e.to_string())?;

        let violations_iter = stmt_violations
            .query_map([task_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let mut unresolved_violations = Vec::new();
        for v in violations_iter {
            unresolved_violations
                .push(v.map_err(|e| format!("Failed to read scope violation row: {}", e))?);
        }

        // 3. Coordinator-Executed Verification Runs Gate
        let mut stmt_runs = conn
            .prepare(
                "SELECT vr.check_name, vr.is_passed, vr.is_stale, COALESCE(vp.required, 1)
                 FROM verification_runs vr
                 LEFT JOIN verification_profiles vp ON vr.check_id = vp.id
                 WHERE vr.task_id = ?1 AND vr.commit_sha = ?2",
            )
            .map_err(|e| e.to_string())?;

        let runs_iter = stmt_runs
            .query_map(rusqlite::params![task_id, current_head_sha], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut total_runs_count = 0;
        let mut failed_required_checks = Vec::new();
        for r in runs_iter {
            let (check_name, is_passed, is_stale, is_required) =
                r.map_err(|e| format!("Failed to read verification run row: {}", e))?;
            total_runs_count += 1;
            if is_required && (!is_passed || is_stale) {
                failed_required_checks.push(check_name);
            }
        }

        // Query configured required verification profiles for this project / task
        let mut stmt_req_profiles = conn
            .prepare(
                "SELECT id, check_type FROM verification_profiles
                 WHERE (project_id = (SELECT project_id FROM tasks WHERE id = ?1) OR task_id = ?1)
                   AND (task_id IS NULL OR task_id = ?1)
                   AND required = 1",
            )
            .map_err(|e| e.to_string())?;

        let req_profiles: Vec<(String, String)> = stmt_req_profiles
            .query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read required verification profiles: {}", e))?;

        let mut missing_required_checks = Vec::new();
        for (prof_id, check_type) in req_profiles {
            let has_passing_run: bool = conn
                .query_row(
                    "SELECT 1 FROM verification_runs
                     WHERE task_id = ?1 AND commit_sha = ?2 AND check_id = ?3 AND is_passed = 1 AND is_stale = 0
                     LIMIT 1",
                    rusqlite::params![task_id, current_head_sha, prof_id],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !has_passing_run {
                missing_required_checks.push(format!(
                    "Missing required verification check: {} ({})",
                    prof_id, check_type
                ));
            }
        }

        // 4. Evaluator Results Gate (Verifies Machine Evaluators for commit SHA)
        let mut stmt_evals = conn
            .prepare("SELECT evaluator_name, passed, exit_code FROM evaluator_results WHERE task_id = ?1 AND commit_sha = ?2")
            .map_err(|e| e.to_string())?;

        let evals_iter = stmt_evals
            .query_map(rusqlite::params![task_id, current_head_sha], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i32>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut failed_evaluators = Vec::new();
        for ev in evals_iter {
            let (name, passed, exit_code) =
                ev.map_err(|e| format!("Failed to read evaluator result row: {}", e))?;
            if !passed {
                failed_evaluators.push(format!(
                    "Machine evaluator '{}' failed (exit code: {})",
                    name, exit_code
                ));
            }
        }

        let mut rejection_reasons = Vec::new();
        if total_runs_count == 0 {
            rejection_reasons.push("UNVERIFIED: Zero coordinator verification checks were executed against submitted commit HEAD. At least one passing check is required.".to_string());
        }
        for step in &missing_steps {
            rejection_reasons.push(format!("Mandatory step '{}' is not marked COMPLETED", step));
        }
        for file in &unresolved_violations {
            rejection_reasons.push(format!("Unresolved out-of-scope modification: {}", file));
        }
        for check in &failed_required_checks {
            rejection_reasons.push(format!(
                "Coordinator verification check '{}' failed or is stale",
                check
            ));
        }
        for missing in &missing_required_checks {
            rejection_reasons.push(missing.clone());
        }
        for eval_err in &failed_evaluators {
            rejection_reasons.push(eval_err.clone());
        }

        let is_valid = rejection_reasons.is_empty();

        Ok(VerificationResult {
            is_valid,
            missing_mandatory_steps: missing_steps,
            missing_evidence_step_ids: Vec::new(),
            unresolved_scope_violations: unresolved_violations,
            failed_coordinator_checks: failed_required_checks,
            rejection_reasons,
        })
    }

    /// Generates a deterministic, immutable Proof-of-Completion bundle with canonical SHA-256 digest
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
            .prepare("SELECT id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at, timed_out FROM verification_runs WHERE task_id = ?1 AND commit_sha = ?2 ORDER BY id ASC")
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
                    timed_out: row.get(14)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let verification_runs: Vec<VerificationRun> = runs_iter
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read verification run row: {}", e))?;

        // Retrieve active attempt ID and attempt number. When no attempt row exists the
        // digest must stay deterministic, so the fallback is a fixed empty identity (the
        // same value persisted in proof_bundles.attempt_id) rather than a fresh UUID.
        let (attempt_id, attempt_num): (String, i32) = conn
            .query_row(
                "SELECT id, attempt_number FROM task_attempts WHERE task_id = ?1 ORDER BY attempt_number DESC LIMIT 1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| (String::new(), 1));

        // Fetch task criteria and steps snapshots for audit log
        let mut criteria_stmt = conn
            .prepare(
                "SELECT id, criterion, is_satisfied FROM acceptance_criteria WHERE task_id = ?1 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let criteria_rows: Vec<serde_json::Value> = criteria_stmt
            .query_map([task_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "criterion": r.get::<_, String>(1)?,
                    "is_satisfied": r.get::<_, bool>(2)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read criterion row: {}", e))?;
        let criteria_json = serde_json::to_string(&criteria_rows).map_err(|e| e.to_string())?;
        drop(criteria_stmt);

        let mut steps_stmt = conn
            .prepare("SELECT id, title, description, is_mandatory, status FROM task_steps WHERE task_id = ?1 ORDER BY id ASC")
            .map_err(|e| e.to_string())?;
        let steps_rows: Vec<serde_json::Value> = steps_stmt
            .query_map([task_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "description": r.get::<_, String>(2)?,
                    "is_mandatory": r.get::<_, bool>(3)?,
                    "status": r.get::<_, String>(4)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read task step row: {}", e))?;
        let steps_json = serde_json::to_string(&steps_rows).map_err(|e| e.to_string())?;
        drop(steps_stmt);

        // Fetch machine evaluator results (identity, version and full result payload) for
        // the same commit HEAD the bundle seals, in a deterministic order.
        let mut evals_stmt = conn
            .prepare(
                "SELECT id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at FROM evaluator_results WHERE task_id = ?1 AND commit_sha = ?2 AND attempt_id = ?3 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let evaluator_results: Vec<EvaluatorResult> = evals_stmt
            .query_map(rusqlite::params![task_id, head_sha, attempt_id], |r| {
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
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read evaluator result row: {}", e))?;
        drop(evals_stmt);

        // Scope violations bound to this attempt, in a deterministic order.
        let mut violations_stmt = conn
            .prepare(
                "SELECT id, task_id, agent_id, file_path, violation_type, detected_at, resolved FROM scope_violations WHERE task_id = ?1 AND attempt_id = ?2 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let scope_violations: Vec<crate::models::ScopeViolation> = violations_stmt
            .query_map(rusqlite::params![task_id, attempt_id], |r| {
                Ok(crate::models::ScopeViolation {
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
        drop(violations_stmt);

        // Canonical ordering: sort files so the digest is independent of input order and
        // the persisted files_changed_json matches exactly what was hashed.
        let mut files_changed = files_changed.to_vec();
        files_changed.sort();

        // Canonical deterministic SHA256 digest across all verified package attributes.
        // Every scalar is hashed as a u64 big-endian length prefix followed by its UTF-8
        // bytes (integers and booleans as their decimal string form); every list carries a
        // u64 big-endian element count. Fixed field order + length prefixes make the
        // concatenation unambiguous, and every value is the exact one persisted in the
        // proof_bundles row or re-derivable from the evidence tables with the same
        // deterministic ORDER BY.
        fn push_field(hasher: &mut Sha256, value: &[u8]) {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value);
        }
        fn push_list_count(hasher: &mut Sha256, len: usize) {
            hasher.update((len as u64).to_be_bytes());
        }

        let mut hasher = Sha256::new();
        push_field(&mut hasher, task_id.as_bytes());
        push_field(&mut hasher, project_id.as_bytes());
        push_field(&mut hasher, attempt_id.as_bytes());
        push_field(&mut hasher, base_sha.as_bytes());
        push_field(&mut hasher, head_sha.as_bytes());
        push_list_count(&mut hasher, files_changed.len());
        for f in &files_changed {
            push_field(&mut hasher, f.as_bytes());
        }
        push_field(&mut hasher, diff_summary.as_bytes());
        push_field(&mut hasher, criteria_json.as_bytes());
        push_field(&mut hasher, steps_json.as_bytes());
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
        let proof_hash = hex::encode(hasher.finalize());

        let bundle = ProofBundle {
            task_id: task_id.to_string(),
            project_id: project_id.to_string(),
            agent_id: agent_id.map(|s| s.to_string()),
            prompt: prompt.to_string(),
            base_sha: base_sha.to_string(),
            head_sha: head_sha.to_string(),
            files_changed: files_changed.clone(),
            diff_summary: diff_summary.to_string(),
            verification_runs,
            scope_violations,
            proof_hash: proof_hash.clone(),
            generated_at: Utc::now().to_rfc3339(),
        };

        let files_json = serde_json::to_string(&bundle.files_changed).unwrap_or("[]".to_string());
        let verification_runs_json =
            serde_json::to_string(&bundle.verification_runs).unwrap_or("[]".to_string());
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO proof_bundles (id, task_id, project_id, agent_id, attempt_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                id, bundle.task_id, bundle.project_id, bundle.agent_id, attempt_id, attempt_num,
                bundle.prompt, bundle.base_sha, bundle.head_sha, files_json, bundle.diff_summary,
                verification_runs_json, criteria_json, steps_json, bundle.proof_hash, bundle.generated_at
            ],
        ).map_err(|e| format!("Failed to record proof bundle: {}", e))?;

        Ok(bundle)
    }

    /// Queries all historical proof bundles generated for a task
    pub fn list_proof_bundles(&self, task_id: &str) -> Result<Vec<ProofBundle>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, task_id, project_id, agent_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, proof_hash, generated_at FROM proof_bundles WHERE task_id = ?1 ORDER BY generated_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([task_id], |r| {
                let files_json: String = r.get(7)?;
                let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
                Ok(ProofBundle {
                    task_id: r.get(1)?,
                    project_id: r.get(2)?,
                    agent_id: r.get(3)?,
                    prompt: r.get(4)?,
                    base_sha: r.get(5)?,
                    head_sha: r.get(6)?,
                    files_changed: files,
                    diff_summary: r.get(8)?,
                    verification_runs: Vec::new(),
                    scope_violations: Vec::new(),
                    proof_hash: r.get(9)?,
                    generated_at: r.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.flatten().collect())
    }
}

#[cfg(test)]
mod tests;
