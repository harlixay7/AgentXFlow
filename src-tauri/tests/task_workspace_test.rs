#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::models::DecomposedStepInput;
use std::process::Command;

fn setup_temp_git_repo(prefix: &str) -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("axf_ws_{}_{}", prefix, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Workspace Test Repo\n").unwrap();

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
fn test_task_workspace_lifecycle_and_security() {
    let repo_dir = setup_temp_git_repo("lifecycle");
    let pool = DbPool::new_in_memory().expect("in-memory pool");
    let engine = CoordinatorEngine::new(pool);

    let proj = engine
        .create_project(
            "Workspace Test Project",
            repo_dir.to_str().unwrap(),
            "Spec",
            "main",
        )
        .unwrap();

    let agent_a = engine.register_agent("agent-alpha", "CODER").unwrap();
    let agent_b = engine.register_agent("agent-beta", "CODER").unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Spec", 1, 1)
        .unwrap();

    let steps = vec![DecomposedStepInput {
        step_index: 1,
        title: "Task Workspace Step 1".to_string(),
        description: "Implementation of module alpha".to_string(),
        suggested_scope: Some("src/alpha/**".to_string()),
        acceptance_criteria: Some("Criteria 1".to_string()),
    }];

    let _decomposed = engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    let task = engine
        .claim_masterplan_chunk(&proj.id, &agent_a.id, Some(1))
        .unwrap();
    let task_id = task.id.clone();

    // 1. Authoritative path resolution
    let ws_path = engine
        .get_task_workspace_path(&task_id, &agent_a.id)
        .expect("Must resolve authoritative worktree path");
    assert!(ws_path.exists(), "Worktree path must exist on disk");
    assert!(ws_path.to_string_lossy().contains(&task_id));

    // 2. Ownership enforcement (agent-beta cannot access agent-alpha's workspace)
    let unauthorized = engine.get_task_workspace_path(&task_id, &agent_b.id);
    assert!(unauthorized.is_err());
    assert!(unauthorized
        .unwrap_err()
        .contains("Task ownership violation"));

    // 3. Traversal prevention
    let traversal_read = engine.task_workspace_read(&task_id, &agent_a.id, "../../cargo.lock");
    assert!(traversal_read.is_err());
    assert!(traversal_read
        .unwrap_err()
        .contains("Path traversal prohibited"));

    let traversal_write =
        engine.task_workspace_write(&task_id, &agent_a.id, "../escape.txt", "forbidden content");
    assert!(traversal_write.is_err());
    assert!(traversal_write
        .unwrap_err()
        .contains("Path traversal prohibited"));

    // 4. Scope-enforced writing
    // Writing outside leased scope (e.g. src/beta/secret.txt when lease is src/alpha/**) fails closed
    let scope_violation = engine.task_workspace_write(
        &task_id,
        &agent_a.id,
        "src/beta/secret.txt",
        "unauthorized write",
    );
    assert!(scope_violation.is_err());
    assert!(scope_violation.unwrap_err().contains("Scope violation"));

    // Writing within leased scope succeeds
    let write_ok = engine.task_workspace_write(
        &task_id,
        &agent_a.id,
        "src/alpha/index.ts",
        "export const alpha = 42;",
    );
    assert!(
        write_ok.is_ok(),
        "Scoped write must succeed: {:?}",
        write_ok
    );

    // Reading file back from worktree
    let read_ok = engine.task_workspace_read(&task_id, &agent_a.id, "src/alpha/index.ts");
    assert!(read_ok.is_ok());
    assert_eq!(read_ok.unwrap(), "export const alpha = 42;");

    // 5. Execution strictly inside worktree
    let exec_res = engine.task_workspace_exec(&task_id, &agent_a.id, "git status", None, Some(30));
    assert!(
        exec_res.is_ok(),
        "Workspace execution must succeed: {:?}",
        exec_res
    );
    let run = exec_res.unwrap();
    assert!(run.is_passed);
    assert_eq!(run.exit_code, 0);

    // Clean up
    let _ = std::fs::remove_dir_all(repo_dir);
}
