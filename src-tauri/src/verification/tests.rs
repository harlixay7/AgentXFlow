use std::path::PathBuf;
use std::time::Duration;

use crate::db::DbPool;
use crate::verification::VerificationEngine;

fn setup_engine_and_worktree_with_pool() -> (VerificationEngine, DbPool, PathBuf, String) {
    let temp_dir =
        std::env::temp_dir().join(format!("agentxflow_verify_unit_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let pool = DbPool::new_in_memory().expect("Failed to initialize in-memory SQLite pool");
    let engine = VerificationEngine::new(pool.clone());

    // verification_runs/evidence_records carry FK references to projects/tasks,
    // so the harness must seed real rows for execute_check to record runs.
    let now = chrono::Utc::now().to_rfc3339();
    let project_id = format!("proj-{}", uuid::Uuid::new_v4());
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    let conn = pool.lock();
    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            project_id,
            "Verification Unit Test Project",
            temp_dir.to_string_lossy().as_ref(),
            "Spec",
            now,
            now
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            task_id,
            project_id,
            "Verification Unit Test Task",
            "Unit task description",
            now,
            now
        ],
    )
    .unwrap();
    drop(conn);

    (engine, pool, temp_dir, task_id)
}

fn setup_engine_and_worktree() -> (VerificationEngine, PathBuf, String) {
    let (engine, _pool, temp_dir, task_id) = setup_engine_and_worktree_with_pool();
    (engine, temp_dir, task_id)
}

#[test]
fn test_execute_check_respects_configured_timeout() {
    // "cmd /c ping -n 6 127.0.0.1 >nul" runs ~5s; a 2s timeout must kill it.
    let (engine, worktree, task_id) = setup_engine_and_worktree();
    let run = engine
        .execute_check(
            &task_id,
            "chk-ping-slow",
            "Slow Ping",
            &worktree,
            "sha-test",
            "cmd /c ping -n 6 127.0.0.1 >nul",
            Duration::from_secs(2),
        )
        .expect("execute_check should record a run");

    assert_eq!(
        run.exit_code, -1,
        "timed-out check must report exit_code -1"
    );
    assert!(!run.is_passed, "timed-out check must not pass");
    assert!(
        run.stderr.contains("timed out"),
        "stderr should describe the timeout, got: {}",
        run.stderr
    );
}

#[test]
fn test_execute_check_accepts_longer_configured_timeout() {
    // "cmd /c ping -n 4 127.0.0.1 >nul" runs ~3s; a 20s timeout lets it finish.
    let (engine, worktree, task_id) = setup_engine_and_worktree();
    let run = engine
        .execute_check(
            &task_id,
            "chk-ping-fast",
            "Fast Ping",
            &worktree,
            "sha-test",
            "cmd /c ping -n 4 127.0.0.1 >nul",
            Duration::from_secs(20),
        )
        .expect("execute_check should complete within the generous timeout");

    assert_eq!(run.exit_code, 0, "ping must exit 0 when not timed out");
    assert!(run.is_passed, "successful non-timed-out check must pass");
    assert!(
        run.duration_ms < 20_000,
        "duration must stay under the configured timeout, got {} ms",
        run.duration_ms
    );
}

#[test]
fn test_execute_check_captures_large_output_without_false_timeout() {
    // ~160KB of stdout far exceeds the OS pipe buffer (between 16KB and 64KB on
    // this system); the old code polled try_wait without draining, so the child
    // blocked forever on the full pipe and the 60s timeout fired, losing output.
    let (engine, worktree, task_id) = setup_engine_and_worktree();
    let run = engine
        .execute_check(
            &task_id,
            "chk-large-output",
            "Large Output",
            &worktree,
            "sha-test",
            "cmd /c for /L %i in (1,1,20000) do @echo line-%i",
            Duration::from_secs(60),
        )
        .expect("execute_check should record a run");

    assert_eq!(
        run.exit_code, 0,
        "large-output check must complete normally, got exit_code {}",
        run.exit_code
    );
    assert!(run.is_passed, "large-output check must pass");
    assert!(
        run.stdout.starts_with("line-1"),
        "stdout must contain the emitted output (64KB head kept), got {} bytes: {:?}",
        run.stdout.len(),
        &run.stdout[..run.stdout.len().min(60)]
    );
    assert!(
        run.duration_ms < 60_000,
        "must not false-timeout on pipe-buffer backpressure, took {} ms",
        run.duration_ms
    );
    assert!(
        !run.stderr.contains("timed out"),
        "must not report a timeout, got stderr: {}",
        run.stderr
    );
}

fn count_ping_n_30_processes() -> usize {
    let script = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'ping.exe' -and $_.CommandLine -like '*-n 30 127.0.0.1*' } | Measure-Object | Select-Object -ExpandProperty Count";
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .expect("failed to run powershell ping detection");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn kill_ping_n_30_processes() {
    let script = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'ping.exe' -and $_.CommandLine -like '*-n 30 127.0.0.1*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }";
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output();
}

#[test]
fn test_execute_check_timeout_kills_descendant_processes() {
    // The direct child (cmd) runs `start /b` to detach a grandchild (cmd -> ping -n 30)
    // and then keeps itself alive with `ping -n 5` so the 3s timeout fires while the
    // detached ping is still running. The timeout must kill the whole tree, not just
    // the direct child.
    let (engine, worktree, task_id) = setup_engine_and_worktree();
    kill_ping_n_30_processes();

    let run = engine
        .execute_check(
            &task_id,
            "chk-tree-kill",
            "Tree Kill",
            &worktree,
            "sha-test",
            "cmd /c start /b cmd /c ping -n 30 127.0.0.1 >nul & ping -n 5 127.0.0.1 >nul",
            Duration::from_secs(3),
        )
        .expect("execute_check should record a run");

    let survivors = count_ping_n_30_processes();
    kill_ping_n_30_processes();
    assert_eq!(
        run.exit_code, -1,
        "timed-out check must report exit_code -1"
    );
    assert!(!run.is_passed, "timed-out check must not pass");
    assert_eq!(
        survivors, 0,
        "descendant ping must be terminated with the process tree, {} survivor(s) found",
        survivors
    );
}

#[test]
fn test_execute_check_rejects_zero_and_negative_timeouts() {
    // Negative durations are not representable in std::time::Duration (E0600), so
    // the invalid forms are the zero-valued ones; each must be rejected with Err.
    let (engine, worktree, task_id) = setup_engine_and_worktree();
    for bad_timeout in [
        Duration::ZERO,
        Duration::from_secs(0),
        Duration::from_millis(0),
    ] {
        let result = engine.execute_check(
            &task_id,
            "chk-invalid",
            "Invalid Timeout",
            &worktree,
            "sha-test",
            "cmd /c exit 0",
            bad_timeout,
        );
        assert!(
            result.is_err(),
            "timeout {:?} must be rejected with Err",
            bad_timeout
        );
        assert_eq!(
            result.unwrap_err(),
            "timeout must be >= 1 second",
            "rejection must carry the canonical error message"
        );
    }
}

#[test]
fn test_timeout_produces_structured_evidence() {
    // "cmd /c ping -n 4 127.0.0.1 >nul" runs ~3s; a 1s timeout must kill it and
    // persist durable structured evidence: timed_out in the verification_run row
    // and "timed_out": true in the COORDINATOR_OBSERVED evidence payload.
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let run = engine
        .execute_check(
            &task_id,
            "chk-timeout-evidence",
            "Timeout Evidence",
            &worktree,
            "sha-test",
            "cmd /c ping -n 4 127.0.0.1 >nul",
            Duration::from_secs(1),
        )
        .expect("execute_check should record a timed-out run");

    assert!(
        run.timed_out,
        "timed-out check must report timed_out = true"
    );

    let conn = pool.lock();
    let persisted_timed_out: bool = conn
        .query_row(
            "SELECT timed_out FROM verification_runs WHERE task_id = ?1 AND check_id = 'chk-timeout-evidence'",
            [&task_id],
            |r| r.get(0),
        )
        .expect("verification_runs row must expose the timed_out column");
    assert!(
        persisted_timed_out,
        "persisted verification_run must have timed_out = 1"
    );

    let payload: String = conn
        .query_row(
            "SELECT payload_json FROM evidence_records WHERE task_id = ?1 AND evidence_type = 'TEST_RESULT' ORDER BY recorded_at DESC LIMIT 1",
            [&task_id],
            |r| r.get(0),
        )
        .expect("evidence_records payload must be queryable");
    assert!(
        payload.contains("\"timed_out\":true"),
        "evidence payload must contain \"timed_out\":true, got: {}",
        payload
    );
}

#[test]
fn test_post_merge_verification_records_exact_integration_head_sha() {
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let project_id = {
        let conn = pool.lock();
        conn.query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    let fake_integration_sha = "integration-head-sha-abc-123";
    let outcome = engine
        .execute_post_merge_verification(
            &project_id,
            &task_id,
            &worktree,
            fake_integration_sha,
            Duration::from_secs(30),
        )
        .expect("execute_post_merge_verification should succeed");

    assert!(!outcome.runs.is_empty(), "must produce verification runs");
    for run in &outcome.runs {
        assert_eq!(
            run.commit_sha, fake_integration_sha,
            "Invariant 2: VerificationRun must record the exact integration HEAD commit SHA"
        );
    }
}

#[test]
fn test_post_merge_verification_honors_configured_required_profile() {
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let project_id = {
        let conn = pool.lock();
        conn.query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    // Insert custom required profile
    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('prof-custom-1', ?1, ?2, 'CUSTOM_UNIT', 'echo custom_test_passed', '[]', 30, 1, ?3)",
            rusqlite::params![project_id, task_id, now],
        ).unwrap();
    }

    let outcome = engine
        .execute_post_merge_verification(
            &project_id,
            &task_id,
            &worktree,
            "sha-int-head",
            Duration::from_secs(30),
        )
        .expect("execute_post_merge_verification should run custom profile");

    assert!(
        outcome.passed,
        "required custom passing test must pass the merge gate"
    );
    assert_eq!(outcome.runs.len(), 1);
    assert_eq!(outcome.runs[0].check_name, "CUSTOM_UNIT");
}

#[test]
fn test_post_merge_verification_optional_check_failure_does_not_block_merge() {
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let project_id = {
        let conn = pool.lock();
        conn.query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    // Insert optional profile that exits with non-zero (failure)
    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('prof-opt-fail', ?1, ?2, 'OPTIONAL_LINT', 'exit 1', '[]', 30, 0, ?3)",
            rusqlite::params![project_id, task_id, now],
        ).unwrap();
    }

    let outcome = engine
        .execute_post_merge_verification(
            &project_id,
            &task_id,
            &worktree,
            "sha-int-head",
            Duration::from_secs(30),
        )
        .expect("execute_post_merge_verification should run");

    assert_eq!(outcome.runs.len(), 1);
    assert!(!outcome.runs[0].is_passed, "optional check failed");
    assert!(
        outcome.passed,
        "Invariant 3: optional check failure (required = 0) must NOT block the merge"
    );
}

#[test]
fn test_post_merge_verification_required_check_failure_blocks_merge() {
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let project_id = {
        let conn = pool.lock();
        conn.query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    // Insert required profile that exits with non-zero (failure)
    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('prof-req-fail', ?1, ?2, 'REQUIRED_SUITE', 'exit 1', '[]', 30, 1, ?3)",
            rusqlite::params![project_id, task_id, now],
        ).unwrap();
    }

    let outcome = engine
        .execute_post_merge_verification(
            &project_id,
            &task_id,
            &worktree,
            "sha-int-head",
            Duration::from_secs(30),
        )
        .expect("execute_post_merge_verification should run");

    assert_eq!(outcome.runs.len(), 1);
    assert!(!outcome.runs[0].is_passed, "required check failed");
    assert!(
        !outcome.passed,
        "Invariant 3: required check failure (required = 1) MUST block the merge"
    );
}

#[test]
fn test_post_merge_verification_honors_args_json() {
    let (engine, pool, worktree, task_id) = setup_engine_and_worktree_with_pool();
    let project_id = {
        let conn = pool.lock();
        conn.query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('prof-args-1', ?1, ?2, 'ARGS_TEST', 'echo', '[\"alpha\", \"beta\"]', 30, 1, ?3)",
            rusqlite::params![project_id, task_id, now],
        ).unwrap();
    }

    let outcome = engine
        .execute_post_merge_verification(
            &project_id,
            &task_id,
            &worktree,
            "sha-int-head",
            Duration::from_secs(30),
        )
        .expect("execute_post_merge_verification should run with args");

    assert!(outcome.passed);
    assert_eq!(outcome.runs.len(), 1);
    assert!(
        outcome.runs[0].command.contains("alpha beta")
            || outcome.runs[0].stdout.contains("alpha beta")
            || outcome.runs[0].stdout.contains("alpha"),
        "command with structured args must execute args, got command: {}, stdout: {}",
        outcome.runs[0].command,
        outcome.runs[0].stdout
    );
}

#[test]
fn test_post_merge_verification_fallback_to_repo_markers() {
    let (engine, worktree, _task_id) = setup_engine_and_worktree();
    std::fs::write(
        worktree.join("Cargo.toml"),
        "[package]\nname = \"dummy\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let profiles = engine
        .get_verification_profiles("proj-fallback", None, &worktree)
        .unwrap();
    assert_eq!(
        profiles.len(),
        2,
        "Cargo repo must fallback to cargo check + cargo test"
    );
    assert_eq!(profiles[0].command, "cargo check");
    assert_eq!(profiles[1].command, "cargo test");
    assert!(profiles[0].required);
    assert!(profiles[1].required);
}
