#[cfg(test)]
pub mod tests {
    use crate::core::CoordinatorEngine;
    use crate::db::DbPool;
    use crate::models::TaskState;

    fn setup_test_engine() -> (CoordinatorEngine, String) {
        let temp_dir = std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let readme = temp_dir.join("README.md");
        std::fs::write(&readme, "# Test Unit Project\n").unwrap();

        let run_cmd = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&temp_dir)
                .output()
                .expect("Failed to run git command");
            if !out.status.success() {
                eprintln!("Git cmd {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
            }
        };

        run_cmd(&["init"]);
        run_cmd(&["config", "user.name", "AgentXFlow Unit Test"]);
        run_cmd(&["config", "user.email", "test@agentxflow.local"]);
        run_cmd(&["add", "README.md"]);
        run_cmd(&["commit", "-m", "Initial commit"]);
        run_cmd(&["branch", "-M", "main"]);

        let temp_db = temp_dir.join("test.db");
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite pool");
        let engine = CoordinatorEngine::new(pool);
        let proj = engine.create_project("Test Unit Project", &temp_dir.to_string_lossy(), "Spec", "main").unwrap();
        (engine, proj.id)
    }

    #[test]
    fn test_task_state_transitions() {
        assert!(TaskState::Backlog.can_transition_to(&TaskState::Ready));
        assert!(TaskState::Ready.can_transition_to(&TaskState::Running));
        assert!(TaskState::Running.can_transition_to(&TaskState::Review));
        assert!(TaskState::Review.can_transition_to(&TaskState::MergeReady));
        assert!(TaskState::MergeReady.can_transition_to(&TaskState::Done));

        // Illegal backward or bypass transitions
        assert!(!TaskState::Backlog.can_transition_to(&TaskState::Done));
        assert!(!TaskState::Ready.can_transition_to(&TaskState::Done));
        assert!(!TaskState::Done.can_transition_to(&TaskState::Running));
    }

    #[test]
    fn test_dag_dependency_cycle_detection() {
        let (engine, proj_id) = setup_test_engine();

        // Create 3 tasks
        let t1 = engine.create_task(&proj_id, "Task 1", "Desc", "HIGH", vec![], vec![]).unwrap();
        let t2 = engine.create_task(&proj_id, "Task 2", "Desc", "HIGH", vec![], vec![]).unwrap();
        let t3 = engine.create_task(&proj_id, "Task 3", "Desc", "HIGH", vec![], vec![]).unwrap();

        // T1 depends on T2
        assert!(engine.dag.add_dependency(&t1.id, &t2.id, "BLOCKS").is_ok());
        // T2 depends on T3
        assert!(engine.dag.add_dependency(&t2.id, &t3.id, "BLOCKS").is_ok());

        // Attempting T3 depends on T1 would form cycle T1 -> T2 -> T3 -> T1!
        let cycle_res = engine.dag.add_dependency(&t3.id, &t1.id, "BLOCKS");
        assert!(cycle_res.is_err(), "Cycle should be detected and rejected");

        // Dependencies satisfaction
        assert_eq!(engine.dag.are_dependencies_satisfied(&t1.id).unwrap(), false);
    }

    #[test]
    fn test_scope_engine_v2_mutation_audit() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine.create_task(&proj_id, "Scope Task", "Desc", "HIGH", vec![], vec![]).unwrap();

        // Grant scope on src/auth/**
        engine.scope.acquire_scope(&task.id, "agent-1", vec!["src/auth/**".to_string()], "EXCLUSIVE_WRITE").unwrap();

        // Simulate changed files
        let files = vec![
            "src/auth/login.ts".to_string(),
            "src/payments/charge.ts".to_string(), // OUT OF SCOPE!
        ];

        let violations = engine.scope.audit_actual_mutations(&task.id, "agent-1", &files).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_path, "src/payments/charge.ts");
    }

    #[test]
    fn test_verification_engine_and_proof_bundle() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine.create_task(
            &proj_id,
            "Verify Task",
            "Desc",
            "HIGH",
            vec![("Mandatory Step".to_string(), "Must do".to_string(), true)],
            vec!["Criteria 1".to_string()],
        ).unwrap();

        // Initial submission fails because step is pending
        let res = engine.verify.verify_task_submission(&task.id, "head-sha-1").unwrap();
        assert_eq!(res.is_valid, false);

        // Generate Proof Bundle
        let bundle = engine.verify.generate_proof_bundle(
            &task.id,
            &proj_id,
            Some("agent-1"),
            "Test prompt",
            "base-sha-0",
            "head-sha-1",
            &["src/auth.rs".to_string()],
            "+10 -2",
        ).unwrap();

        assert!(!bundle.proof_hash.is_empty());
        assert_eq!(bundle.head_sha, "head-sha-1");
    }

    #[test]
    fn test_serialized_merge_queue_operations() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine.create_task(&proj_id, "Merge Task", "Desc", "HIGH", vec![], vec![]).unwrap();

        let queue_item = engine.merge.enqueue_task(
            &proj_id,
            &task.id,
            "agentxflow/task-1",
            "main",
            "base-sha-1",
            "head-sha-1",
        ).unwrap();

        assert_eq!(queue_item.position, 1);
        assert_eq!(queue_item.status, "READY");

        let list = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_task_cancellation_and_scope_release() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Agent-Alpha", "Coder").unwrap();
        let task = engine.create_task(&proj_id, "Cancel Task", "Desc", "HIGH", vec![], vec![]).unwrap();

        // Claim and acquire scope
        let claimed = engine.claim_task(&task.id, &agent.id).unwrap();
        assert_eq!(claimed.state, TaskState::Running);
        engine.scope.acquire_scope(&task.id, &agent.id, vec!["crates/engine/**".to_string()], "EXCLUSIVE_WRITE").unwrap();

        // Verify scope lease exists
        let leases = engine.get_task_details(&task.id).unwrap().leases;
        assert_eq!(leases.len(), 1);

        // Cancel task explicitly
        let cancelled = engine.cancel_task(&task.id, Some(&agent.id), Some("User requested stop")).unwrap();
        assert_eq!(cancelled.state, TaskState::Cancelled);
        assert!(cancelled.is_stale);

        // Verify scope leases were fully released
        let leases_after = engine.get_task_details(&task.id).unwrap().leases;
        assert_eq!(leases_after.len(), 0);

        // Another task can now acquire the same scope pattern without collision
        let agent2 = engine.register_agent("Agent-Beta", "Coder").unwrap();
        let task2 = engine.create_task(&proj_id, "Task 2", "Desc", "HIGH", vec![], vec![]).unwrap();
        let claimed2 = engine.claim_task(&task2.id, &agent2.id).unwrap();
        assert!(engine.scope.acquire_scope(&claimed2.id, &agent2.id, vec!["crates/engine/**".to_string()], "EXCLUSIVE_WRITE").is_ok());
    }

    #[test]
    fn test_masterplan_lifecycle_and_reset_invalidation() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Agent-1", "Coder").unwrap();

        // 1. Prepare masterplan
        let raw_plan = "# Step 1: Init\nInit project\n# Step 2: Core\nCore logic";
        let prep = engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        assert_eq!(prep.steps.len(), 2);

        // 2. Claim chunk
        let claimed_chunk = engine.claim_masterplan_chunk(&proj_id, &agent.id, Some(1)).unwrap();
        assert_eq!(claimed_chunk.state, TaskState::Running);
        assert!(!claimed_chunk.is_stale);

        // 3. Reset masterplan
        assert!(engine.reset_masterplan(&proj_id).is_ok());

        // 4. Verify claimed task is now marked CANCELLED & is_stale = true, and its scopes released
        let task_after = engine.get_task(&claimed_chunk.id).unwrap();
        assert_eq!(task_after.state, TaskState::Cancelled);
        assert!(task_after.is_stale);

        let leases = engine.get_task_details(&claimed_chunk.id).unwrap().leases;
        assert_eq!(leases.len(), 0);
    }

    #[test]
    fn test_decompose_blocked_while_tasks_active() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Agent-Decomp", "Coder").unwrap();

        // Prepare plan
        let raw_plan = "# Step 1: A\nDesc A\n# Step 2: B\nDesc B";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();

        // Claim a chunk so an active task exists
        let claimed = engine.claim_masterplan_chunk(&proj_id, &agent.id, Some(1)).unwrap();
        assert_eq!(claimed.state, TaskState::Running);

        // Attempting to re-decompose while task is active MUST fail
        let decomp_res = engine.decompose_masterplan(&proj_id, vec![
            crate::models::DecomposedStepInput {
                step_index: 1,
                title: "New 1".to_string(),
                description: "New desc".to_string(),
                suggested_scope: Some("src/**".to_string()),
                acceptance_criteria: Some("Pass".to_string()),
            }
        ]);
        assert!(decomp_res.is_err(), "Decomposing while active tasks are running must be blocked");

        // Cancel the active task
        engine.cancel_task(&claimed.id, Some(&agent.id), Some("Cancelled for test")).unwrap();

        // Now re-decomposition succeeds
        let decomp_res2 = engine.decompose_masterplan(&proj_id, vec![
            crate::models::DecomposedStepInput {
                step_index: 1,
                title: "New 1".to_string(),
                description: "New desc".to_string(),
                suggested_scope: Some("src/**".to_string()),
                acceptance_criteria: Some("Pass".to_string()),
            }
        ]);
        assert!(decomp_res2.is_ok(), "Decomposing after cancelling active task must succeed");
    }

    #[test]
    fn test_get_project_context_without_task_id() {
        let (engine, proj_id) = setup_test_engine();

        let ctx = engine.get_project_context(&proj_id).unwrap();
        assert_eq!(ctx.project_id, proj_id);
        assert_eq!(ctx.project_name, "Test Unit Project");
        assert!(!ctx.contract_hash.is_empty());
        assert_eq!(ctx.contract_overview, "Spec");
        assert!(!ctx.project_rules.is_empty());
        assert!(ctx.project_rules[0].contains("Git worktrees"));
    }

    #[test]
    fn test_get_context_pack_with_task_id() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine.create_task(&proj_id, "Task A", "Task Prompt", "HIGH", vec![
            ("Step 1".into(), "Do step 1".into(), true),
        ], vec!["Criterion 1".into()]).unwrap();

        let pack = engine.get_context_pack(&proj_id, &task.id).unwrap();
        assert_eq!(pack.project_id, proj_id);
        assert_eq!(pack.project_name, "Test Unit Project");
        assert_eq!(pack.task_id, task.id);
        assert_eq!(pack.task_title, "Task A");
        assert_eq!(pack.task_prompt, "Task Prompt");
        assert_eq!(pack.required_steps.len(), 1);
        assert_eq!(pack.acceptance_criteria.len(), 1);
    }

    #[test]
    fn test_dynamic_agent_status_and_unclaim() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Antigravity", "IDE").unwrap();

        // 1. Initial status with 0 tasks should be IDLE
        let agents = engine.list_agents().unwrap();
        let ag = agents.iter().find(|a| a.id == agent.id).unwrap();
        assert_eq!(ag.status, "IDLE");
        assert_eq!(ag.active_task_id, None);

        // 2. Prepare masterplan and claim chunk -> status becomes WORKING
        let raw_plan = "# Step 1: Alpha\nDesc Alpha\n# Step 2: Beta\nDesc Beta";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        let chunk = engine.claim_masterplan_chunk(&proj_id, &agent.id, Some(2)).unwrap();

        let agents_working = engine.list_agents().unwrap();
        let ag_working = agents_working.iter().find(|a| a.id == agent.id).unwrap();
        assert_eq!(ag_working.status, "WORKING");
        assert_eq!(ag_working.active_task_id, Some(chunk.id.clone()));

        // 3. Unclaim agent tasks -> reverts steps to PENDING and returns agent to IDLE
        let unclaimed = engine.unclaim_agent_tasks(&agent.id).unwrap();
        assert_eq!(unclaimed.len(), 1);
        assert_eq!(unclaimed[0], chunk.id);

        let agents_idle = engine.list_agents().unwrap();
        let ag_idle = agents_idle.iter().find(|a| a.id == agent.id).unwrap();
        assert_eq!(ag_idle.status, "IDLE");
        assert_eq!(ag_idle.active_task_id, None);

        // Verify masterplan steps are PENDING again
        let steps = engine.list_masterplan_steps(&proj_id).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|s| s.status == "PENDING"));
    }
}


