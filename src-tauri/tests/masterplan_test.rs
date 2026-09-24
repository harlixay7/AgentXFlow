#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::models::DecomposedStepInput;
use std::process::Command;

fn setup_temp_git_repo() -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("viducia_mp_git_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Masterplan Test Repo\n").unwrap();

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
    run_cmd(&["config", "user.name", "Viducia Test"]);
    run_cmd(&["config", "user.email", "test@viducia.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

#[test]
fn test_masterplan_lifecycle_and_chunked_claims() {
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_db.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Masterplan Autopilot Repo",
            &temp_repo.to_string_lossy(),
            "Decomposed Masterplan Spec",
            "main",
        )
        .expect("Failed to create test project");
    let proj_id = &proj.id;

    // 1. Create Raw Masterplan (Unsorted)
    let raw_plan_text = "
    1. Setup SQLite database schemas for auth, sessions, and permissions.
    2. Implement password hashing with Argon2 and JWT creation.
    3. Build REST login and signup route handlers.
    4. Write integration unit tests for auth workflows.
    5. Setup Webhook listener for external events.
    6. Implement event filtering and validation middleware.
    7. Build Redis queue connector for async background jobs.
    8. Write unit tests for event webhook ingestion.
    9. Build React login and registration form components.
    10. Implement token refresh interceptors in Axios/Fetch client.
    11. Build User profile settings modal and session list.
    12. Write React component unit tests with Jest/Vitest.
    ";

    println!("[TEST] 1. Creating masterplan...");
    let plan = engine
        .create_or_update_masterplan(proj_id, raw_plan_text, 12, 4)
        .expect("Failed to create masterplan");

    assert_eq!(plan.status, "UNSORTED");
    assert_eq!(plan.target_step_count, 12);
    assert_eq!(plan.max_steps_per_agent, 4);

    println!("[TEST] 2. Decomposing masterplan...");
    let mut steps_input = Vec::new();
    for i in 1..=12 {
        steps_input.push(DecomposedStepInput {
            step_index: i,
            title: format!("Specification Step #{:02}", i),
            description: format!("Execute mandatory requirements for milestone step #{}.", i),
            suggested_scope: Some(if i <= 4 {
                "src/db/**, src/auth/**".to_string()
            } else if i <= 8 {
                "src/api/**, src/events/**".to_string()
            } else {
                "src/ui/**, src/components/**".to_string()
            }),
            acceptance_criteria: Some(format!("All tests pass for step #{}.", i)),
        });
    }

    let decomposed = engine
        .decompose_masterplan(proj_id, steps_input, None, None)
        .expect("Failed to decompose masterplan");

    assert_eq!(decomposed.len(), 12);

    let updated_plan = engine.get_masterplan(proj_id).unwrap().unwrap();
    assert_eq!(updated_plan.status, "RESORTED");

    println!("[TEST] 3. Registering agents...");
    let agent1 = engine
        .register_agent("Antigravity-Lead", "Antigravity")
        .unwrap();
    let agent2 = engine
        .register_agent("Claude-Code-Backend", "Claude")
        .unwrap();
    let agent3 = engine.register_agent("Cursor-Frontend", "Cursor").unwrap();

    println!("[TEST] 4. Agent 1 claiming chunk 1...");
    let task1 = engine
        .claim_masterplan_chunk(proj_id, &agent1.id, Some(4))
        .expect("Agent 1 failed to claim chunk 1");

    assert!(task1.title.contains("Steps 1-4"));
    assert_eq!(task1.assigned_agent_id, Some(agent1.id.clone()));
    assert!(task1.worktree_path.is_some());

    println!("[TEST] 4b. Hostile invariant check...");
    let hostile_decompose = engine.decompose_masterplan(
        proj_id,
        vec![DecomposedStepInput {
            step_index: 1,
            title: "Hostile Overwrite".to_string(),
            description: "Attempt to wipe claims".to_string(),
            suggested_scope: None,
            acceptance_criteria: None,
        }],
        None,
        None,
    );
    assert!(
        hostile_decompose.is_err(),
        "Re-decomposition must be blocked when steps are claimed"
    );

    println!("[TEST] 5. Agent 2 claiming chunk 2...");
    let task2 = engine
        .claim_masterplan_chunk(proj_id, &agent2.id, Some(10))
        .expect("Agent 2 failed to claim chunk 2");

    assert!(task2.title.contains("Steps 5-8"));
    assert_eq!(task2.assigned_agent_id, Some(agent2.id.clone()));

    println!("[TEST] 6. Agent 3 claiming chunk 3...");
    let task3 = engine
        .claim_masterplan_chunk(proj_id, &agent3.id, Some(4))
        .expect("Agent 3 failed to claim chunk 3");

    assert!(task3.title.contains("Steps 9-12"));
    assert_eq!(task3.assigned_agent_id, Some(agent3.id.clone()));

    println!("[TEST] 7. Verifying all steps claimed...");
    let steps = engine.list_masterplan_steps(proj_id).unwrap();
    assert_eq!(steps.len(), 12);
    for s in &steps {
        assert_eq!(s.status, "CLAIMED");
        assert!(s.claimed_agent_id.is_some());
        assert!(s.claimed_task_id.is_some());
    }

    let executing_plan = engine.get_masterplan(proj_id).unwrap().unwrap();
    assert_eq!(executing_plan.status, "EXECUTING");

    println!("[TEST] 8. Claiming beyond available...");
    let no_more = engine.claim_masterplan_chunk(proj_id, &agent1.id, Some(4));
    assert!(no_more.is_err());

    println!("[TEST] 9. Re-registering agent...");
    let re_agent1 = engine
        .register_agent("Antigravity-Lead", "Antigravity")
        .unwrap();
    assert_eq!(re_agent1.id, agent1.id);
    // D11: re-registration must rotate to a fresh random token, never reuse the
    // forgeable deterministic axf_sess_<id> value.
    assert_ne!(
        re_agent1.session_token, agent1.session_token,
        "Re-registration must rotate the session token"
    );
    assert!(re_agent1.session_token.is_some());

    println!("[TEST] 10. Resetting masterplan...");
    let reset_res = engine.reset_masterplan(proj_id, None);
    assert!(
        reset_res.is_ok(),
        "Resetting masterplan must succeed without deadlock: {:?}",
        reset_res.err()
    );

    let plan_after_reset = engine.get_masterplan(proj_id).unwrap();
    assert!(plan_after_reset.is_none());

    let steps_after_reset = engine.list_masterplan_steps(proj_id).unwrap();
    assert_eq!(steps_after_reset.len(), 0);
    println!("[TEST] SUCCESS!");
}

#[test]
fn test_decompose_idempotency_key_returns_stored_result() {
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_db.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Idempotency Test",
            &temp_repo.to_string_lossy(),
            "Spec",
            "main",
        )
        .expect("Failed to create test project");

    engine
        .create_or_update_masterplan(&proj.id, "Plan text", 4, 2)
        .unwrap();

    let steps_input: Vec<DecomposedStepInput> = (1..=4)
        .map(|i| DecomposedStepInput {
            step_index: i,
            title: format!("Step {}", i),
            description: format!("Desc {}", i),
            suggested_scope: None,
            acceptance_criteria: None,
        })
        .collect();

    let first = engine
        .decompose_masterplan(
            &proj.id,
            steps_input.clone(),
            None,
            Some("key-abc".to_string()),
        )
        .expect("First decompose failed");
    assert_eq!(first.len(), 4);

    let second = engine
        .decompose_masterplan(
            &proj.id,
            steps_input.clone(),
            None,
            Some("key-abc".to_string()),
        )
        .expect("Second decompose with same key failed");
    assert_eq!(second.len(), 4);
    assert_eq!(first[0].title, second[0].title);

    let steps = engine.list_masterplan_steps(&proj.id).unwrap();
    assert_eq!(steps.len(), 4, "Masterplan_steps must not have duplicates");
}

#[test]
fn test_decompose_duplicate_without_key_still_heuristic_guarded() {
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_db.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project("Guard Test", &temp_repo.to_string_lossy(), "Spec", "main")
        .expect("Failed to create test project");

    engine
        .create_or_update_masterplan(&proj.id, "Plan text", 2, 1)
        .unwrap();

    let steps1: Vec<DecomposedStepInput> = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "S1".into(),
            description: "D1".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
        DecomposedStepInput {
            step_index: 2,
            title: "S2".into(),
            description: "D2".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
    ];

    engine
        .decompose_masterplan(&proj.id, steps1, None, None)
        .unwrap();

    let steps2: Vec<DecomposedStepInput> = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "S1".into(),
            description: "D1".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
        DecomposedStepInput {
            step_index: 2,
            title: "S2".into(),
            description: "D2".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
    ];

    let res = engine.decompose_masterplan(&proj.id, steps2, None, None);
    assert!(
        res.is_ok(),
        "Content-equality guard should allow identical retry without claims"
    );
}

#[test]
fn test_idempotency_key_scoped_to_masterplan() {
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_db.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let dir_a = temp_repo.join("proj_a");
    let dir_b = temp_repo.join("proj_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let proj_a = engine
        .create_project("Scope Test A", &dir_a.to_string_lossy(), "Spec", "main")
        .expect("Failed to create test project");
    let proj_b = engine
        .create_project("Scope Test B", &dir_b.to_string_lossy(), "Spec", "main")
        .expect("Failed to create test project");

    engine
        .create_or_update_masterplan(&proj_a.id, "Plan text", 2, 1)
        .unwrap();
    engine
        .create_or_update_masterplan(&proj_b.id, "Plan text B", 3, 1)
        .unwrap();

    let steps1: Vec<DecomposedStepInput> = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "S1".into(),
            description: "D1".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
        DecomposedStepInput {
            step_index: 2,
            title: "S2".into(),
            description: "D2".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
    ];

    engine
        .decompose_masterplan(&proj_a.id, steps1, None, Some("shared-key".to_string()))
        .unwrap();

    let steps2: Vec<DecomposedStepInput> = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "A".into(),
            description: "B".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
        DecomposedStepInput {
            step_index: 2,
            title: "C".into(),
            description: "D".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
        DecomposedStepInput {
            step_index: 3,
            title: "E".into(),
            description: "F".into(),
            suggested_scope: None,
            acceptance_criteria: None,
        },
    ];

    let res = engine.decompose_masterplan(&proj_b.id, steps2, None, Some("shared-key".to_string()));
    assert!(
        res.is_err(),
        "Reusing an idempotency key with a different masterplan_id must fail"
    );
}

#[test]
fn test_decompose_crash_atomic_idempotency_lifecycle() {
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_atomic_mp.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Atomic MP Test",
            &temp_repo.to_string_lossy(),
            "Spec",
            "main",
        )
        .expect("Failed to create test project");

    let plan = engine
        .create_or_update_masterplan(&proj.id, "Plan text for atomic test", 3, 2)
        .unwrap();

    let steps = vec![
        DecomposedStepInput {
            step_index: 1,
            title: "Atomic Step 1".into(),
            description: "Desc 1".into(),
            suggested_scope: Some("src/**".into()),
            acceptance_criteria: Some("Criteria 1".into()),
        },
        DecomposedStepInput {
            step_index: 2,
            title: "Atomic Step 2".into(),
            description: "Desc 2".into(),
            suggested_scope: Some("src/**".into()),
            acceptance_criteria: Some("Criteria 2".into()),
        },
    ];

    let key = format!("atomic-idem-key-{}", uuid::Uuid::new_v4());

    // 1. First call: creates steps + idempotency record atomically in single transaction
    let result1 = engine
        .decompose_masterplan(&proj.id, steps.clone(), None, Some(key.clone()))
        .expect("Decomposition must succeed");
    assert_eq!(result1.len(), 2);

    // Verify both masterplan_steps and masterplan_operations rows exist
    {
        let conn = pool.lock();
        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1",
                [&plan.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_count, 2, "Database must contain exactly 2 steps");

        let op_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_operations WHERE idempotency_key = ?1 AND masterplan_id = ?2",
                [&key, &plan.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            op_count, 1,
            "Idempotency operation must be persisted in SQLite"
        );
    }

    // 2. Replay with same idempotency key returns stored result without mutating steps
    let result2 = engine
        .decompose_masterplan(&proj.id, steps, None, Some(key.clone()))
        .expect("Idempotent replay must succeed");
    assert_eq!(result2.len(), 2);
    assert_eq!(
        result1[0].id, result2[0].id,
        "Replay must return exact stored step records"
    );
    assert_eq!(
        result1[1].id, result2[1].id,
        "Replay must return exact stored step records"
    );

    // 3. Reusing the key with another project/plan fails closed
    let other_dir = temp_repo.join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let proj2 = engine
        .create_project(
            "Another Project",
            &other_dir.to_string_lossy(),
            "Spec",
            "main",
        )
        .unwrap();
    let plan2 = engine
        .create_or_update_masterplan(&proj2.id, "Plan 2", 2, 1)
        .unwrap();

    let hostile_steps = vec![DecomposedStepInput {
        step_index: 1,
        title: "Hostile".into(),
        description: "Desc".into(),
        suggested_scope: None,
        acceptance_criteria: None,
    }];

    let cross_plan_err = engine.decompose_masterplan(&proj2.id, hostile_steps, None, Some(key));
    assert!(
        cross_plan_err.is_err(),
        "Cross-plan key reuse must be rejected"
    );

    // Verify plan2 has no steps
    {
        let conn = pool.lock();
        let plan2_steps: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1",
                [&plan2.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plan2_steps, 0, "Failed transaction must leave no steps");
    }
}
