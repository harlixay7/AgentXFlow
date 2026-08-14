#[cfg(test)]
pub mod tests {
    use crate::core::CoordinatorEngine;
    use crate::db::DbPool;
    use crate::models::TaskState;

    fn setup_test_engine() -> (CoordinatorEngine, String) {
        let temp_dir = std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
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
}

