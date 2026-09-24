#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::models::{DecomposedStepInput, TaskState, TaskSubstate};
use chrono::Utc;
use rusqlite::params;
use std::path::Path;
use std::process::Command;

fn setup_temp_git_repo(prefix: &str) -> std::path::PathBuf {
    let temp_dir =
        std::env::temp_dir().join(format!("axf_hard_{}_{}", prefix, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Test Repo\n").unwrap();

    let run_cmd = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&temp_dir)
            .output()
            .expect("Failed to run git command");
        if !out.status.success() {
            eprintln!(
                "Git cmd {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    };

    run_cmd(&["init"]);
    run_cmd(&["config", "user.name", "AgentXFlow Test Runner"]);
    run_cmd(&["config", "user.email", "runner@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

// =========================================================================
// TEST 1: Successful Git Merge + Failed/Interrupted SQLite Finalization
// Section 3.I Invariant:
// When Git target ref was advanced before an interruption, reconciliation
// (both startup and task_reconcile) MUST authoritatively detect that the commit
// is an ancestor, converge SQLite to MERGED/DONE/COMPLETED, and prevent duplicate merges.
// =========================================================================
#[test]
fn test_post_git_merge_interruption_reconciliation_converges() {
    let repo_dir = setup_temp_git_repo("test_merge_recon");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Reconciliation Proj",
            &repo_dir.to_string_lossy(),
            "Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Spec", 4, 4)
        .unwrap();
    let steps = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "Step 1".to_string(),
            description: "Desc 1".to_string(),
            suggested_scope: Some("src/a.txt".to_string()),
            acceptance_criteria: Some("Criteria 1".to_string()),
        },
        DecomposedStepInput {
            step_index: 2,
            title: "Step 2".to_string(),
            description: "Desc 2".to_string(),
            suggested_scope: Some("src/b.txt".to_string()),
            acceptance_criteria: Some("Criteria 2".to_string()),
        },
    ];
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    let task = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", Some(2))
        .unwrap();

    // Commit a change inside the task worktree
    let wt_path = Path::new(task.worktree_path.as_ref().unwrap());
    std::fs::create_dir_all(wt_path.join("src")).unwrap();
    std::fs::write(wt_path.join("src/a.txt"), "feature A").unwrap();

    let run_cmd = |dir: &Path, args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("Failed to run git command");
        assert!(out.status.success(), "Git cmd {:?} failed", args);
    };
    run_cmd(wt_path, &["add", "."]);
    run_cmd(wt_path, &["commit", "-m", "Implement feature A"]);

    let head_sha = engine.git.get_worktree_head_sha(wt_path).unwrap();

    // Fast-forward main to include this commit (simulating that Git CAS update-ref succeeded)
    run_cmd(&repo_dir, &["merge", "--ff-only", &head_sha]);

    // Verify commit is indeed an ancestor of main
    let is_anc = engine
        .git
        .is_ancestor(&repo_dir, &head_sha, "main")
        .unwrap();
    assert_eq!(is_anc, agent_x_flow_lib::git::GitAncestorResult::Ancestor);

    // Now simulate an interruption where SQLite still has the task in MERGE_READY
    // and merge_queue in RUNNING_CHECKS (the write failed/crashed before finalization)
    let queue_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE tasks SET state = 'MERGE_READY', head_sha = ?1 WHERE id = ?2",
            [&head_sha, &task.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, base_sha, head_sha, status, queued_at)
             VALUES (?1, ?2, ?3, 'agentxflow/task', 'main', 1, 'base', ?4, 'RUNNING_CHECKS', ?5)",
            params![&queue_id, &proj.id, &task.id, &head_sha, &now],
        )
        .unwrap();
    }

    // Call reconcile_task (as an agent would via MCP tool `task_reconcile`)
    let recon_val = engine.reconcile_task(&task.id).unwrap();
    assert_eq!(recon_val["state"], "DONE");
    assert_eq!(recon_val["merge_queue_status"], "MERGED");

    // Verify database state converged completely
    let task_fresh = engine.get_task(&task.id).unwrap();
    assert_eq!(task_fresh.state, TaskState::Done);
    assert_eq!(task_fresh.substate, TaskSubstate::None);

    let steps_fresh = engine.list_masterplan_steps(&proj.id).unwrap();
    for s in steps_fresh {
        assert_eq!(s.status, "COMPLETED");
    }

    // Verify startup reconciliation on this state also runs safely and idempotently
    engine.reconcile_on_startup();
    let task_after_startup = engine.get_task(&task.id).unwrap();
    assert_eq!(task_after_startup.state, TaskState::Done);
}

// =========================================================================
// TEST 2: Submission Rejected When Zero Verification Checks Executed
// Section 3.F Invariant:
// When zero coordinator verification checks ran against submitted commit HEAD,
// submission MUST fail closed with UNVERIFIED rejection.
// =========================================================================
#[test]
fn test_zero_verification_runs_fails_submission() {
    let repo_dir = setup_temp_git_repo("test_zero_verif");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("Zero Verif", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let task = engine
        .create_task(&proj.id, "Test Task", "Desc", "HIGH", vec![], vec![])
        .unwrap();

    // Verify submission against a commit SHA with 0 verification runs
    let result = engine
        .verify
        .verify_task_submission(&task.id, "abc123commit")
        .unwrap();

    assert!(!result.is_valid);
    assert!(result
        .rejection_reasons
        .iter()
        .any(|r| r.contains("Zero coordinator verification checks were executed")));
}

// =========================================================================
// TEST 3: Startup Reconciliation Preserves Recovery Pointers On Cleanup Refusal
// Section 3.J Invariant:
// When worktree cleanup is refused (e.g. unmanaged path), recovery pointers
// (worktree_path, branch_name) MUST be preserved, and task must transition to
// BLOCKED with substate RECOVERABLE.
// =========================================================================
// TEST 3: Startup Reconciliation Preserves Recovery Pointers On Cleanup Failure
// Section 3.J Invariant:
// When worktree cleanup fails, recovery pointers (worktree_path, branch_name)
// MUST be preserved, and task must transition to BLOCKED with substate RECOVERABLE.
// =========================================================================
#[test]
fn test_startup_reconciliation_preserves_pointers_on_cleanup_failure() {
    let repo_dir = setup_temp_git_repo("test_cleanup_fail");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("Cleanup Fail", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let task_id = uuid::Uuid::new_v4().to_string();
    let managed_path = repo_dir
        .join(".agentxflow")
        .join("worktrees")
        .join(format!("task-{}", task_id));
    std::fs::create_dir_all(&managed_path).unwrap();

    // Lock a file inside the managed worktree with share_mode(0) so safe_remove_dir_all fails on deletion
    let lock_file_path = managed_path.join("in_use.bin");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(0); // 0 = EXCLUSIVE: no read, write, or delete sharing
    }
    let _locked_file = opts.open(&lock_file_path).unwrap();

    let now = Utc::now().to_rfc3339();

    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, assigned_agent_id, worktree_path, branch_name, created_at, updated_at)
             VALUES (?1, ?2, 'Interrupted Claim', 'Desc', 'CLAIMING', 'CLAIMING', 'HIGH', 0, 'agent_x', ?3, 'agentxflow/task-x', ?4, ?4)",
            params![task_id, proj.id, managed_path.to_string_lossy().to_string(), now],
        ).unwrap();
    }

    // Run startup reconciliation (safe_remove_dir_all will fail due to file in use)
    engine.reconcile_on_startup();

    // Task must be BLOCKED with RECOVERABLE substate, and pointers preserved
    let task = engine.get_task(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Blocked);
    assert_eq!(task.substate, TaskSubstate::Recoverable);
    assert_eq!(
        task.worktree_path.as_deref(),
        Some(managed_path.to_string_lossy().as_ref())
    );
    assert_eq!(task.branch_name.as_deref(), Some("agentxflow/task-x"));

    // Release locked file and clean up
    drop(_locked_file);
    let _ = std::fs::remove_dir_all(&managed_path);
}

// =========================================================================
// TEST 4: Git is_ancestor Distinguishes Ancestor, NotAncestor, and Command Error
// Section 3.H Invariant:
// Git merge-base authority MUST distinguish true Ancestor, NotAncestor, and Error.
// =========================================================================
#[test]
fn test_git_is_ancestor_distinguishes_error() {
    let repo_dir = setup_temp_git_repo("test_ancestor_err");
    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool);

    let head_sha = engine.git.get_ref_sha(&repo_dir, "main").unwrap();

    // 1. True ancestor: main against main
    let res_anc = engine.git.is_ancestor(&repo_dir, &head_sha, "main");
    assert_eq!(
        res_anc,
        Ok(agent_x_flow_lib::git::GitAncestorResult::Ancestor)
    );

    // 2. Not ancestor: create an orphan commit or detached branch
    let run_cmd = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo_dir)
            .output()
            .expect("Failed to run git command");
        assert!(out.status.success());
    };
    run_cmd(&["checkout", "--orphan", "orphan_branch"]);
    std::fs::write(repo_dir.join("orphan.txt"), "orphan content").unwrap();
    run_cmd(&["add", "orphan.txt"]);
    run_cmd(&["commit", "-m", "Orphan commit"]);
    run_cmd(&["checkout", "main"]);

    let orphan_sha = engine.git.get_ref_sha(&repo_dir, "orphan_branch").unwrap();
    let res_not_anc = engine.git.is_ancestor(&repo_dir, &orphan_sha, "main");
    assert_eq!(
        res_not_anc,
        Ok(agent_x_flow_lib::git::GitAncestorResult::NotAncestor)
    );

    // 3. Command error: invalid/corrupt commit sha
    let res_err = engine
        .git
        .is_ancestor(&repo_dir, "nonexistent_sha_xyz", "main");
    assert!(res_err.is_err());
    let err_msg = res_err.unwrap_err();
    assert!(err_msg.contains("failed") || err_msg.contains("Not a valid commit name"));
}

// =========================================================================
// TEST 5: Verification Profile DB Failure Fails Closed (Defect A)
// =========================================================================
#[test]
fn test_verification_profile_db_failure_fails_closed() {
    let repo_dir = setup_temp_git_repo("test_profile_db_err");
    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool.clone());

    // Drop table to simulate authoritative DB query failure
    {
        let conn = pool.lock();
        conn.execute("DROP TABLE verification_profiles", [])
            .unwrap();
    }

    let res = engine
        .verify
        .get_verification_profiles("proj_1", None, &repo_dir);
    assert!(
        res.is_err(),
        "Must fail closed on DB query failure, not infer defaults"
    );
    let err = res.unwrap_err();
    assert!(err.contains("Failed to prepare") || err.contains("no such table"));
}

// =========================================================================
// TEST 6: Verification Evidence Persistence Failure Aborts Check (Defect B)
// =========================================================================
#[test]
fn test_verification_evidence_write_failure_aborts_check() {
    let repo_dir = setup_temp_git_repo("test_evidence_fail");
    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool.clone());
    let proj = engine
        .create_project("Evidence Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let task_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    // Insert valid task, then drop evidence_records table to force evidence write failure specifically
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at)
             VALUES (?1, ?2, 'Test Task', 'Desc', 'RUNNING', 'NONE', 'HIGH', 0, ?3, ?3)",
            params![task_id, proj.id, now],
        ).unwrap();
        conn.execute("DROP TABLE evidence_records", []).unwrap();
    }

    let res = engine.verify.execute_check(
        &task_id,
        "chk_1",
        "TYPECHECK",
        &repo_dir,
        "dummy_commit_sha",
        "git status",
        std::time::Duration::from_secs(5),
    );

    assert!(
        res.is_err(),
        "execute_check must fail closed when evidence write fails"
    );
    let err = res.unwrap_err();
    assert!(err.contains("Failed to record verification evidence"));

    // Verify verification_runs was also rolled back
    {
        let conn = pool.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM verification_runs WHERE task_id = ?1",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "verification_runs must be rolled back on evidence failure"
        );
    }
}

// =========================================================================
// TEST 7: Chunk Claim RUNNING Finalization Failure Preserves Recovery (Defect C)
// =========================================================================
#[test]
fn test_claim_finalization_failure_preserves_recoverable_state() {
    let repo_dir = setup_temp_git_repo("test_claim_fin_fail");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Claim Finalization Proj",
            &repo_dir.to_string_lossy(),
            "Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Spec", 2, 2)
        .unwrap();
    let steps = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "Step 1".to_string(),
            description: "Desc 1".to_string(),
            suggested_scope: Some("src/*".to_string()),
            acceptance_criteria: Some("Criteria 1".to_string()),
        },
        DecomposedStepInput {
            step_index: 2,
            title: "Step 2".to_string(),
            description: "Desc 2".to_string(),
            suggested_scope: Some("src/*".to_string()),
            acceptance_criteria: Some("Criteria 2".to_string()),
        },
    ];
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();

    // Trigger that forces the UPDATE tasks SET state = 'RUNNING' to abort
    {
        let conn = pool.lock();
        conn.execute(
            "CREATE TRIGGER fail_running_update BEFORE UPDATE OF state ON tasks
             WHEN NEW.state = 'RUNNING'
             BEGIN
                 SELECT RAISE(ABORT, 'Simulated DB failure during RUNNING transition');
             END;",
            [],
        )
        .unwrap();
    }

    let claim_res = engine.claim_masterplan_chunk(&proj.id, "antigravity", Some(2));
    assert!(
        claim_res.is_err(),
        "Claim must fail when RUNNING finalization fails"
    );
    let err_msg = claim_res.unwrap_err();
    assert!(
        err_msg.contains("Failed to transition task claim to RUNNING")
            || err_msg.contains("Simulated DB failure")
    );

    // Verify task is in BLOCKED state with RECOVERABLE substate, and worktree remains
    let conn = pool.lock();
    let (state, substate, wt_path): (String, String, Option<String>) = conn
        .query_row(
            "SELECT state, substate, worktree_path FROM tasks WHERE project_id = ?1 LIMIT 1",
            [&proj.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "BLOCKED");
    assert_eq!(substate, "RECOVERABLE");
    assert!(wt_path.is_some());
    assert!(
        Path::new(&wt_path.unwrap()).exists(),
        "Worktree directory must be preserved for recovery"
    );
}

// =========================================================================
// TEST 8: list_all_masterplans Surfaces DB Error (Defect G)
// =========================================================================
#[test]
fn test_list_all_masterplans_surfaces_db_error() {
    let repo_dir = setup_temp_git_repo("test_list_mp_err");
    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool.clone());

    let _proj = engine
        .create_project("MP Error Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    // Drop masterplans table to simulate DB failure
    {
        let conn = pool.lock();
        conn.execute("DROP TABLE masterplans", []).unwrap();
    }

    let res = engine.list_all_masterplans();
    assert!(res.is_err(), "Must surface DB error, not return empty list");
    let err = res.unwrap_err();
    assert!(err.contains("Failed to prepare masterplans query") || err.contains("no such table"));
}

// =========================================================================
// TEST 9: Workspace Symlink Escape Prohibited (Defect H)
// =========================================================================
#[test]
fn test_workspace_symlink_escape_prohibited() {
    let repo_dir = setup_temp_git_repo("test_symlink_escape");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool);

    let proj = engine
        .create_project("Symlink Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Spec", 1, 1)
        .unwrap();
    let steps = vec![DecomposedStepInput {
        step_index: 1,
        title: "Step 1".to_string(),
        description: "Desc 1".to_string(),
        suggested_scope: Some("**".to_string()),
        acceptance_criteria: Some("Criteria 1".to_string()),
    }];
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    let task = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", Some(1))
        .unwrap();

    let wt_path = Path::new(task.worktree_path.as_ref().unwrap());

    // Create an outside secret file
    let outside_file = repo_dir.join("outside_secret.txt");
    std::fs::write(&outside_file, "TOP_SECRET_DATA").unwrap();

    // Attempting to read outside the worktree via traversal
    let read_traversal =
        engine.task_workspace_read(&task.id, "antigravity", "../outside_secret.txt");
    assert!(read_traversal.is_err());
    assert!(read_traversal
        .unwrap_err()
        .contains("Path traversal prohibited"));

    // Attempting to write outside the worktree via traversal
    let write_traversal =
        engine.task_workspace_write(&task.id, "antigravity", "../outside_secret.txt", "hacked");
    assert!(write_traversal.is_err());
    assert!(write_traversal
        .unwrap_err()
        .contains("Path traversal prohibited"));

    // Verify path_is_under_root rejects escaping paths
    assert!(!agent_x_flow_lib::git::path_is_under_root(
        &outside_file,
        wt_path
    ));
}

// =========================================================================
// TEST 10: check_is_git_repo Fails Closed (Defect J)
// =========================================================================
#[test]
fn test_check_is_git_repo_fails_closed() {
    let temp_dir = std::env::temp_dir().join(format!("axf_non_repo_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool);

    // 1. Non-git directory must return Ok(false)
    let is_repo = engine.git.check_is_git_repo(&temp_dir).unwrap();
    assert_eq!(
        is_repo, false,
        "Empty directory must not be recognized as a Git repo"
    );

    // 2. Non-existent path must return Err
    let non_existent = temp_dir.join("does_not_exist");
    let err_res = engine.git.check_is_git_repo(&non_existent);
    assert!(err_res.is_err(), "Non-existent directory must return Err");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

// =========================================================================
// TEST 11: Merge Queue Re-Enqueue Preserves Primary Key and Integration Attempts (D5)
// =========================================================================
#[test]
fn test_merge_queue_preserves_attempts_and_id_on_reenqueue() {
    let repo_dir = setup_temp_git_repo("test_mq_preserve");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("MQ Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let task = engine
        .create_task(&proj.id, "MQ Task", "Desc", "HIGH", vec![], vec![])
        .unwrap();

    // 1. First enqueue
    let item1 = engine
        .merge
        .enqueue_task(
            &proj.id,
            &task.id,
            "branch-1",
            "main",
            "base-sha-1",
            "head-sha-1",
        )
        .unwrap();

    // 2. Record an integration attempt referencing item1.id
    let attempt_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO integration_attempts (id, merge_queue_id, simulation_passed, conflicts_json, post_merge_verification_passed, merge_strategy, target_sha_before, target_sha_after, attempted_at)
             VALUES (?1, ?2, 0, '[]', 0, 'MERGE_COMMIT', 'base-sha-1', 'head-sha-1', ?3)",
            params![attempt_id, item1.id, now],
        ).unwrap();

        // Mark item as STALE to simulate target branch moving
        conn.execute(
            "UPDATE merge_queue SET status = 'STALE' WHERE id = ?1",
            params![item1.id],
        )
        .unwrap();
    }

    // 3. Re-enqueue with updated base/head SHAs
    let item2 = engine
        .merge
        .enqueue_task(
            &proj.id,
            &task.id,
            "branch-1",
            "main",
            "base-sha-2",
            "head-sha-2",
        )
        .unwrap();

    // Primary key must be preserved
    assert_eq!(
        item1.id, item2.id,
        "Merge queue re-enqueue must preserve existing primary key ID"
    );
    assert_eq!(item2.base_sha, "base-sha-2");
    assert_eq!(item2.head_sha, "head-sha-2");
    assert_eq!(item2.status, "READY");

    // Integration attempt MUST NOT have been cascade-deleted!
    {
        let conn = pool.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM integration_attempts WHERE id = ?1",
                params![attempt_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "Integration attempt history must survive re-enqueuing"
        );

        // Mark as MERGED
        conn.execute(
            "UPDATE merge_queue SET status = 'MERGED' WHERE id = ?1",
            params![item1.id],
        )
        .unwrap();
    }

    // 4. Re-enqueuing a MERGED task must be rejected
    let re_enqueue_err = engine.merge.enqueue_task(
        &proj.id,
        &task.id,
        "branch-1",
        "main",
        "base-sha-3",
        "head-sha-3",
    );
    assert!(
        re_enqueue_err.is_err(),
        "Re-enqueuing an already MERGED task must fail closed"
    );
    assert!(re_enqueue_err
        .unwrap_err()
        .contains("Cannot re-enqueue merged task"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}

// =========================================================================
// TEST 12: Corrupt package.json Fails Closed in Verification Discovery (D6)
// =========================================================================
#[test]
fn test_verification_corrupt_package_json_fails_closed() {
    let temp_dir = std::env::temp_dir().join(format!("axf_bad_pkg_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Write malformed JSON to package.json
    let pkg_file = temp_dir.join("package.json");
    std::fs::write(
        &pkg_file,
        "{ \"name\": \"broken\", \"scripts\": { missing_quotes: true }",
    )
    .unwrap();

    let pool = DbPool::new_in_memory().unwrap();
    let engine = CoordinatorEngine::new(pool);

    let res = engine
        .verify
        .get_verification_profiles("proj-1", None, &temp_dir);
    assert!(
        res.is_err(),
        "Corrupt package.json must fail closed with Err"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Failed to parse package.json"),
        "Error must clearly identify package.json parse failure: {}",
        err
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

// =========================================================================
// TEST 13: Missing Required Profile Check Blocks Task Submission (D7)
// =========================================================================
#[test]
fn test_verification_missing_required_check_fails_submission() {
    let repo_dir = setup_temp_git_repo("test_missing_check");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Missing Check Proj",
            &repo_dir.to_string_lossy(),
            "Spec",
            "main",
        )
        .unwrap();

    let task = engine
        .create_task(&proj.id, "Test Task", "Desc", "HIGH", vec![], vec![])
        .unwrap();

    let head_sha = "head1234567890abcdef";
    let now = Utc::now().to_rfc3339();

    // Configure 2 required checks in verification_profiles
    {
        let conn = pool.lock();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('chk-build', ?1, ?2, 'BUILD', 'npm run build', '[]', 60, 1, ?3)",
            params![proj.id, task.id, now],
        ).unwrap();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, task_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('chk-test', ?1, ?2, 'UNIT_TESTS', 'npm test', '[]', 60, 1, ?3)",
            params![proj.id, task.id, now],
        ).unwrap();

        // Record a passing run ONLY for 'chk-build'
        conn.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, source, executed_at, timed_out)
             VALUES ('run-1', ?1, NULL, 'chk-build', 'BUILD', ?2, 'npm run build', 0, 'ok', '', 100, 1, 0, 'COORDINATOR_OBSERVED', ?3, 0)",
            params![task.id, head_sha, now],
        ).unwrap();
    }

    // Verify submission: 'chk-test' was never executed, so verification must fail
    let outcome = engine
        .verify
        .verify_task_submission(&task.id, head_sha)
        .unwrap();
    assert!(
        !outcome.is_valid,
        "Submission must be rejected when a required profile check was never executed"
    );
    assert!(
        outcome
            .rejection_reasons
            .iter()
            .any(|r| r.contains("chk-test")),
        "Rejection reasons must identify missing required check 'chk-test': {:?}",
        outcome.rejection_reasons
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}

// =========================================================================
// TEST 14: Step Completion and Criteria Satisfaction Are Atomic With Evidence (D3 & D4)
// =========================================================================
#[test]
fn test_step_completion_and_criteria_satisfaction_atomic_with_evidence() {
    let repo_dir = setup_temp_git_repo("test_step_atomic");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("Step Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let agent = engine.register_agent("Step Agent", "IDE").unwrap();

    let task = engine
        .create_task(
            &proj.id,
            "Step Task",
            "Desc",
            "HIGH",
            vec![("Step 1".to_string(), "Desc 1".to_string(), true)],
            vec!["Crit 1".to_string()],
        )
        .unwrap();

    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
            params![agent.id, task.id],
        )
        .unwrap();
    }

    // Query step and criterion IDs
    let (step_id, crit_id) = {
        let conn = pool.lock();
        let sid: String = conn
            .query_row(
                "SELECT id FROM task_steps WHERE task_id = ?1",
                [&task.id],
                |r| r.get(0),
            )
            .unwrap();
        let cid: String = conn
            .query_row(
                "SELECT id FROM acceptance_criteria WHERE task_id = ?1",
                [&task.id],
                |r| r.get(0),
            )
            .unwrap();
        (sid, cid)
    };

    // 1. complete_step with evidence
    let completed_step = engine
        .complete_step(&step_id, Some(&agent.id), Some("{\"test\": \"pass\"}"))
        .unwrap();
    assert_eq!(completed_step.status, "COMPLETED");

    // Verify evidence record was inserted
    {
        let conn = pool.lock();
        let ev_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM evidence_records WHERE step_id = ?1 AND payload_json = '{\"test\": \"pass\"}'",
            [&step_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(
            ev_count, 1,
            "Evidence record must be atomically inserted for step completion"
        );
    }

    // 2. satisfy_acceptance_criterion with evidence
    engine
        .satisfy_acceptance_criterion(&task.id, &crit_id, Some("Manual verification verified"))
        .unwrap();

    // Verify criterion satisfied and evidence record inserted
    {
        let conn = pool.lock();
        let is_sat: bool = conn
            .query_row(
                "SELECT is_satisfied FROM acceptance_criteria WHERE id = ?1",
                [&crit_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(is_sat, "Criterion must be satisfied");

        let ev_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM evidence_records WHERE task_id = ?1 AND payload_json = 'Manual verification verified'",
            [&task.id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(
            ev_count, 1,
            "Evidence record must be atomically inserted for criterion satisfaction"
        );
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}

// =========================================================================
// TEST 15: Agent Cancel Authorization Enforced Inside Transaction (D9)
// =========================================================================
#[test]
fn test_agent_cancel_authorization_enforced() {
    let repo_dir = setup_temp_git_repo("test_cancel_auth");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("Cancel Proj", &repo_dir.to_string_lossy(), "Spec", "main")
        .unwrap();

    let agent_a = engine.register_agent("Agent A", "IDE").unwrap();
    let agent_b = engine.register_agent("Agent B", "IDE").unwrap();

    let task = engine
        .create_task(&proj.id, "Cancel Task", "Desc", "HIGH", vec![], vec![])
        .unwrap();

    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
            params![agent_a.id, task.id],
        )
        .unwrap();
    }

    // Agent B tries to cancel Agent A's task -> rejected
    let cancel_b_err = engine.cancel_task(&task.id, Some(&agent_b.id), Some("Malicious cancel"));
    assert!(
        cancel_b_err.is_err(),
        "Agent B must not be permitted to cancel Agent A's task"
    );
    assert!(cancel_b_err.unwrap_err().contains("Authorization error"));

    // Agent A cancels own task -> succeeds
    let cancel_a_res = engine.cancel_task(&task.id, Some(&agent_a.id), Some("Legitimate cancel"));
    assert!(
        cancel_a_res.is_ok(),
        "Owner agent must be permitted to cancel own task"
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}

// =========================================================================
// TEST 16: Masterplan Update Cancels In-Flight Tasks and Updates Atomically (D2)
// =========================================================================
#[test]
fn test_masterplan_update_cancels_inflight_and_updates_atomically() {
    let repo_dir = setup_temp_git_repo("test_mp_update_atomic");
    let db_path = repo_dir.join("test.sqlite");
    let pool = DbPool::new(&db_path).expect("Failed to init DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "MP Update Proj",
            &repo_dir.to_string_lossy(),
            "Initial Spec",
            "main",
        )
        .unwrap();

    let plan = engine
        .create_or_update_masterplan(&proj.id, "Initial Spec", 2, 2)
        .unwrap();

    // Create a task bound to this masterplan
    let agent = engine.register_agent("Worker", "IDE").unwrap();
    let task = engine
        .create_task(&proj.id, "Bound Task", "Desc", "HIGH", vec![], vec![])
        .unwrap();

    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE tasks SET masterplan_id = ?1, assigned_agent_id = ?2, state = 'RUNNING' WHERE id = ?3",
            params![plan.id, agent.id, task.id],
        ).unwrap();
    }

    // Update masterplan with new specification
    let updated_plan = engine
        .create_or_update_masterplan(&proj.id, "Updated Spec Content", 4, 4)
        .unwrap();

    assert_eq!(updated_plan.raw_text, "Updated Spec Content");
    assert_eq!(updated_plan.target_step_count, 4);

    // In-flight task must have been cancelled
    let task_after = engine.get_task(&task.id).unwrap();
    assert_eq!(task_after.state, TaskState::Cancelled);

    // Revision must have been created
    {
        let conn = pool.lock();
        let rev_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_revisions WHERE masterplan_id = ?1",
                [&plan.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            rev_count >= 1,
            "A revision archive must be created on masterplan update"
        );
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}
