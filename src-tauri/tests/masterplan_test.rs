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
            eprintln!("Git cmd {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
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

    let plan = engine
        .create_or_update_masterplan(proj_id, raw_plan_text, 12, 4)
        .expect("Failed to create masterplan");

    assert_eq!(plan.status, "UNSORTED");
    assert_eq!(plan.target_step_count, 12);
    assert_eq!(plan.max_steps_per_agent, 4);

    // 2. Decompose Masterplan (Organizer Phase)
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
        .decompose_masterplan(proj_id, steps_input)
        .expect("Failed to decompose masterplan");

    assert_eq!(decomposed.len(), 12);

    let updated_plan = engine.get_masterplan(proj_id).unwrap().unwrap();
    assert_eq!(updated_plan.status, "RESORTED");

    // 3. Register 3 Distinct AI Agents
    let agent1 = engine.register_agent("Antigravity-Lead", "Antigravity").unwrap();
    let agent2 = engine.register_agent("Claude-Code-Backend", "Claude").unwrap();
    let agent3 = engine.register_agent("Cursor-Frontend", "Cursor").unwrap();

    // 4. Agent 1 Claims Chunk 1 (Steps 1 to 4)
    let task1 = engine
        .claim_masterplan_chunk(proj_id, &agent1.id, Some(4))
        .expect("Agent 1 failed to claim chunk 1");

    assert!(task1.title.contains("Steps 1-4"));
    assert_eq!(task1.assigned_agent_id, Some(agent1.id.clone()));
    assert!(task1.worktree_path.is_some());

    // 4b. Hostile Invariant Verification: An agent attempts to re-decompose the masterplan
    // while Agent 1 has active claims. This MUST be rejected to prevent claim wiping.
    let hostile_decompose = engine.decompose_masterplan(
        proj_id,
        vec![DecomposedStepInput {
            step_index: 1,
            title: "Hostile Overwrite".to_string(),
            description: "Attempt to wipe claims".to_string(),
            suggested_scope: None,
            acceptance_criteria: None,
        }],
    );
    assert!(hostile_decompose.is_err(), "Re-decomposition must be blocked when steps are claimed");
    let err_msg = hostile_decompose.unwrap_err();
    assert!(err_msg.contains("Cannot re-decompose masterplan") || err_msg.contains("actively claimed"));

    // Verify Agent 1's claims are completely intact
    let active_steps = engine.list_masterplan_steps(proj_id).unwrap();
    assert_eq!(active_steps.len(), 12);
    assert_eq!(active_steps[0].status, "CLAIMED");
    assert_eq!(active_steps[0].claimed_agent_id, Some(agent1.id.clone()));

    // 5. Agent 2 Claims Chunk 2 with Anti-Hoarding Cap (requests 10, should get max 4 -> Steps 5 to 8)
    let task2 = engine
        .claim_masterplan_chunk(proj_id, &agent2.id, Some(10))
        .expect("Agent 2 failed to claim chunk 2");

    assert!(task2.title.contains("Steps 5-8"));
    assert_eq!(task2.assigned_agent_id, Some(agent2.id.clone()));

    // 6. Agent 3 Claims Chunk 3 (Steps 9 to 12)
    let task3 = engine
        .claim_masterplan_chunk(proj_id, &agent3.id, Some(4))
        .expect("Agent 3 failed to claim chunk 3");

    assert!(task3.title.contains("Steps 9-12"));
    assert_eq!(task3.assigned_agent_id, Some(agent3.id.clone()));

    // 7. Verify all steps are claimed and plan is EXECUTING
    let steps = engine.list_masterplan_steps(proj_id).unwrap();
    assert_eq!(steps.len(), 12);
    for s in &steps {
        assert_eq!(s.status, "CLAIMED");
        assert!(s.claimed_agent_id.is_some());
        assert!(s.claimed_task_id.is_some());
    }

    let executing_plan = engine.get_masterplan(proj_id).unwrap().unwrap();
    assert_eq!(executing_plan.status, "EXECUTING");

    // 8. Attempting to claim further should return clean error
    let no_more = engine.claim_masterplan_chunk(proj_id, &agent1.id, Some(4));
    assert!(no_more.is_err());

    // 9. Verify Agent Registration Idempotency: Re-registering with the same name returns identical agent_id
    let re_agent1 = engine.register_agent("Antigravity-Lead", "Antigravity").unwrap();
    assert_eq!(re_agent1.id, agent1.id);
    assert_eq!(re_agent1.session_token, agent1.session_token);

    // 10. Verify Masterplan Reset: Executes cleanly without deadlock, clearing all steps and plan
    let reset_res = engine.reset_masterplan(proj_id);
    assert!(reset_res.is_ok(), "Resetting masterplan must succeed without deadlock: {:?}", reset_res.err());

    let plan_after_reset = engine.get_masterplan(proj_id).unwrap();
    assert!(plan_after_reset.is_none());

    let steps_after_reset = engine.list_masterplan_steps(proj_id).unwrap();
    assert_eq!(steps_after_reset.len(), 0);
}
