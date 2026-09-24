#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::models::{DecomposedStepInput, TaskState};
use std::process::Command;

fn setup_temp_git_repo(prefix: &str) -> std::path::PathBuf {
    let temp_dir =
        std::env::temp_dir().join(format!("axf_mrg_{}_{}", prefix, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Merge Authority Repo\n").unwrap();

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

#[test]
fn test_merge_pre_finalization_target_authority_guard() {
    let repo_dir = setup_temp_git_repo("authority");
    let pool = DbPool::new_in_memory().expect("in-memory pool");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Target Authority Test Project",
            repo_dir.to_str().unwrap(),
            "Spec",
            "main",
        )
        .unwrap();

    let agent = engine.register_agent("agent-dev", "CODER").unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Spec", 1, 1)
        .unwrap();

    let steps = vec![DecomposedStepInput {
        step_index: 1,
        title: "Feature A".to_string(),
        description: "Feature A implementation".to_string(),
        suggested_scope: Some("src/feat_a/**".to_string()),
        acceptance_criteria: Some("Criteria A".to_string()),
    }];

    let _decomposed = engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    let task = engine
        .claim_masterplan_chunk(&proj.id, &agent.id, Some(1))
        .unwrap();

    // Write file and commit inside task worktree
    let wt_path = engine.get_task_workspace_path(&task.id, &agent.id).unwrap();
    let file_path = wt_path.join("src").join("feat_a").join("mod.rs");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "// Feature A").unwrap();

    let run_git = |args: &[&str], dir: &std::path::Path| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "Git cmd failed: {:?}", args);
    };

    run_git(&["add", "src/feat_a/mod.rs"], &wt_path);
    run_git(&["commit", "-m", "Implement feature A"], &wt_path);

    // Complete task steps in DB
    let conn = pool.lock();
    conn.execute(
        "UPDATE task_steps SET status = 'COMPLETED' WHERE task_id = ?1",
        [&task.id],
    )
    .unwrap();
    drop(conn);

    // Submit task
    let submit_res = engine.submit_task(&task.id, &agent.id);
    assert!(
        submit_res.is_ok(),
        "Submit should succeed: {:?}",
        submit_res
    );

    // Now, before merge processing completes, simulate another commit landing on the target branch (main)
    let extra_file = repo_dir.join("EXTRA.md");
    std::fs::write(&extra_file, "Concurrent change on main\n").unwrap();
    run_git(&["add", "EXTRA.md"], &repo_dir);
    run_git(&["commit", "-m", "Concurrent commit on main"], &repo_dir);

    // Now process the merge queue
    let queue = engine.merge.list_queue(&proj.id).unwrap();
    assert_eq!(queue.len(), 1);
    let q_item = &queue[0];

    // Attempting to process this merge must detect that the target branch moved from the enqueued base!
    let merge_res = engine.merge.process_merge_by_id(&q_item.id, &repo_dir);
    assert!(
        merge_res.is_err(),
        "Merge must detect moved target branch and reject stale base"
    );
    let err = merge_res.unwrap_err();
    assert!(
        err.contains("Candidate base is STALE")
            || err.contains("moved during post-merge verification")
            || err.contains("Base SHA mismatch"),
        "Error must explain stale base / moved target: {}",
        err
    );

    // The task should transition to BLOCKED
    let updated_task = engine.get_task(&task.id).unwrap();
    assert_eq!(updated_task.state, TaskState::Blocked);

    // Reconcile should rebase the task queue entry to the new target HEAD
    let recon_res = engine.reconcile_task(&task.id);
    assert!(
        recon_res.is_ok(),
        "Reconciliation should succeed: {:?}",
        recon_res
    );

    // Clean up
    let _ = std::fs::remove_dir_all(repo_dir);
}
