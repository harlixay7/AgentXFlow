#[cfg(test)]
pub mod tests {
    use crate::core::{transition_task_state_on_conn, CoordinatorEngine};
    use crate::db::DbPool;
    use crate::models::TaskState;
    use serde_json::json;

    #[path = "../../../../tests/git_safety_test.rs"]
    mod git_safety;

    fn setup_test_engine() -> (CoordinatorEngine, String) {
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
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
                eprintln!(
                    "Git cmd {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&out.stderr)
                );
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
        let proj = engine
            .create_project(
                "Test Unit Project",
                &temp_dir.to_string_lossy(),
                "Spec",
                "main",
            )
            .unwrap();
        (engine, proj.id)
    }

    /// Drives a freshly created task through the legal claim/submit path into MERGE_READY,
    /// which is the state tasks are in when the merge engine processes them (enqueue_task_by_id
    /// normalizes VERIFYING/REVIEW to MERGE_READY before the queue worker picks the item up).
    fn drive_task_to_merge_ready(engine: &CoordinatorEngine, task_id: &str) {
        let conn = engine.db.lock();
        for (from, to) in [
            (&[TaskState::Backlog][..], TaskState::Ready),
            (&[TaskState::Ready][..], TaskState::Running),
            (&[TaskState::Running][..], TaskState::Verifying),
            (&[TaskState::Verifying][..], TaskState::MergeReady),
        ] {
            transition_task_state_on_conn(&conn, task_id, from, to).unwrap();
        }
    }

    fn setup_test_engine_with_worktree_root() -> (CoordinatorEngine, String, std::path::PathBuf) {
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
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
                eprintln!(
                    "Git cmd {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&out.stderr)
                );
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
        let engine = CoordinatorEngine::new_with_worktree_root(pool, temp_dir.join("worktrees"));
        let proj = engine
            .create_project(
                "Test Unit Project",
                &temp_dir.to_string_lossy(),
                "Spec",
                "main",
            )
            .unwrap();
        (engine, proj.id, temp_dir)
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
    fn test_illegal_task_transitions_are_blocked() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Illegal Transition Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let ready_task = engine
            .create_task(&proj_id, "Ready Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let verifying_task = engine
            .create_task(&proj_id, "Verifying Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        let conn = engine.db.lock();

        // READY -> DONE must Err("Illegal transition")
        transition_task_state_on_conn(
            &conn,
            &ready_task.id,
            &[TaskState::Backlog],
            TaskState::Ready,
        )
        .unwrap();
        let err = transition_task_state_on_conn(
            &conn,
            &ready_task.id,
            &[TaskState::Ready],
            TaskState::Done,
        )
        .unwrap_err();
        assert!(err.contains("Illegal transition"), "got: {}", err);

        // BACKLOG -> DONE must Err (DONE is only reachable from MERGE_READY)
        let err =
            transition_task_state_on_conn(&conn, &task.id, &[TaskState::Backlog], TaskState::Done)
                .unwrap_err();
        assert!(err.contains("Illegal transition"), "got: {}", err);

        // VERIFYING -> READY must Err (no table entry and no legitimate caller)
        transition_task_state_on_conn(
            &conn,
            &verifying_task.id,
            &[TaskState::Backlog],
            TaskState::Running,
        )
        .unwrap();
        transition_task_state_on_conn(
            &conn,
            &verifying_task.id,
            &[TaskState::Running],
            TaskState::Verifying,
        )
        .unwrap();
        let err = transition_task_state_on_conn(
            &conn,
            &verifying_task.id,
            &[TaskState::Verifying],
            TaskState::Ready,
        )
        .unwrap_err();
        assert!(err.contains("Illegal transition"), "got: {}", err);

        // Transitioning a task that does not exist must Err
        let err = transition_task_state_on_conn(
            &conn,
            "missing-task-id",
            &[TaskState::Backlog],
            TaskState::Ready,
        )
        .unwrap_err();
        assert!(err.contains("not found"), "got: {}", err);
    }

    #[test]
    fn test_direct_sql_state_write_to_unknown_state_fails() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Unknown State Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let conn = engine.db.lock();

        // Direct UPDATE to an illegal state must fail at the DB layer (CHECK constraint)
        let err = conn
            .execute(
                "UPDATE tasks SET state = 'EXPLODED', updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, task.id],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK"),
            "UPDATE of unknown state must fail with CHECK constraint, got: {}",
            err
        );

        // Direct INSERT with an illegal state must also fail at the DB layer
        let err = conn
            .execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, created_at, updated_at)
                 VALUES ('task-exploded', ?1, 'Exploded', 'Desc', 'EXPLODED', 'NONE', 'MEDIUM', ?2, ?2)",
                rusqlite::params![proj_id, now],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK"),
            "INSERT of unknown state must fail with CHECK constraint, got: {}",
            err
        );

        // Legal state writes must still succeed
        conn.execute(
            "UPDATE tasks SET state = 'REVIEW', updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, task.id],
        )
        .unwrap();
    }

    #[test]
    fn test_legal_task_transitions_succeed_and_emit_events() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Legal Transition Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();

        let steps: Vec<(TaskState, Vec<TaskState>, &str, &str)> = vec![
            (
                TaskState::Ready,
                vec![TaskState::Backlog],
                "READY",
                "TASK_READY",
            ),
            (
                TaskState::Running,
                vec![TaskState::Ready],
                "RUNNING",
                "TASK_CLAIMED",
            ),
            (
                TaskState::Verifying,
                vec![TaskState::Running],
                "VERIFYING",
                "TASK_VERIFYING",
            ),
            (
                TaskState::MergeReady,
                vec![TaskState::Verifying],
                "MERGE_READY",
                "TASK_MERGE_READY",
            ),
            (
                TaskState::Done,
                vec![TaskState::MergeReady],
                "DONE",
                "TASK_DONE",
            ),
        ];

        let mut last_seq = 0;
        for (to, from, state_name, event_type) in &steps {
            engine
                .transition_task_state(
                    &task.id,
                    from,
                    to.clone(),
                    Some("agent-1"),
                    event_type,
                    json!({ "state": state_name }),
                )
                .unwrap();
            assert_eq!(engine.get_task(&task.id).unwrap().state, *to);
            let events = engine.get_events_after(last_seq).unwrap();
            assert!(
                events.iter().any(|e| e.event_type == *event_type),
                "expected event '{}' to be emitted after transition to {}",
                event_type,
                state_name
            );
            last_seq = events.iter().map(|e| e.sequence).max().unwrap_or(last_seq);
        }
        assert_eq!(engine.get_task(&task.id).unwrap().state, TaskState::Done);

        // Direct BACKLOG -> RUNNING is legal: claim_task accepts BACKLOG tasks
        let backlog_task = engine
            .create_task(
                &proj_id,
                "Backlog Claim Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        engine
            .transition_task_state(
                &backlog_task.id,
                &[TaskState::Backlog],
                TaskState::Running,
                None,
                "TASK_CLAIMED",
                json!({ "direct_backlog_claim": true }),
            )
            .unwrap();
        assert_eq!(
            engine.get_task(&backlog_task.id).unwrap().state,
            TaskState::Running
        );
    }

    #[test]
    fn test_dag_dependency_cycle_detection() {
        let (engine, proj_id) = setup_test_engine();

        // Create 3 tasks
        let t1 = engine
            .create_task(&proj_id, "Task 1", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let t2 = engine
            .create_task(&proj_id, "Task 2", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let t3 = engine
            .create_task(&proj_id, "Task 3", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        // T1 depends on T2
        assert!(engine.dag.add_dependency(&t1.id, &t2.id, "BLOCKS").is_ok());
        // T2 depends on T3
        assert!(engine.dag.add_dependency(&t2.id, &t3.id, "BLOCKS").is_ok());

        // Attempting T3 depends on T1 would form cycle T1 -> T2 -> T3 -> T1!
        let cycle_res = engine.dag.add_dependency(&t3.id, &t1.id, "BLOCKS");
        assert!(cycle_res.is_err(), "Cycle should be detected and rejected");

        // Dependencies satisfaction
        assert_eq!(
            engine.dag.are_dependencies_satisfied(&t1.id).unwrap(),
            false
        );
    }

    fn drive_task_to_done(engine: &CoordinatorEngine, task_id: &str) {
        drive_task_to_merge_ready(engine, task_id);
        let conn = engine.db.lock();
        transition_task_state_on_conn(&conn, task_id, &[TaskState::MergeReady], TaskState::Done)
            .unwrap();
    }

    #[test]
    fn test_blocks_dependency_blocks_scheduling() {
        let (engine, proj_id) = setup_test_engine();
        let a = engine
            .create_task(&proj_id, "Blocks A", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let b = engine
            .create_task(&proj_id, "Blocks B", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        engine.dag.add_dependency(&b.id, &a.id, "BLOCKS").unwrap();

        assert!(!engine.dag.are_dependencies_satisfied(&b.id).unwrap());

        drive_task_to_done(&engine, &a.id);
        assert!(engine.dag.are_dependencies_satisfied(&b.id).unwrap());
    }

    #[test]
    fn test_related_to_dependency_does_not_block() {
        let (engine, proj_id) = setup_test_engine();
        let a = engine
            .create_task(&proj_id, "Related A", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let b = engine
            .create_task(&proj_id, "Related B", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        engine
            .dag
            .add_dependency(&b.id, &a.id, "RELATED_TO")
            .unwrap();

        assert!(engine.dag.are_dependencies_satisfied(&b.id).unwrap());
    }

    #[test]
    fn test_parent_child_dependency_blocks_scheduling() {
        let (engine, proj_id) = setup_test_engine();
        let a = engine
            .create_task(&proj_id, "Parent A", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let b = engine
            .create_task(&proj_id, "Child B", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        engine
            .dag
            .add_dependency(&b.id, &a.id, "PARENT_CHILD")
            .unwrap();

        assert!(!engine.dag.are_dependencies_satisfied(&b.id).unwrap());

        drive_task_to_done(&engine, &a.id);
        assert!(engine.dag.are_dependencies_satisfied(&b.id).unwrap());
    }

    #[test]
    fn test_unknown_dependency_type_still_blocks() {
        let (engine, proj_id) = setup_test_engine();
        let a = engine
            .create_task(&proj_id, "Legacy A", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let b = engine
            .create_task(&proj_id, "Legacy B", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        engine
            .dag
            .add_dependency(&b.id, &a.id, "LEGACY_TYPO")
            .unwrap();

        assert!(!engine.dag.are_dependencies_satisfied(&b.id).unwrap());

        drive_task_to_done(&engine, &a.id);
        assert!(engine.dag.are_dependencies_satisfied(&b.id).unwrap());
    }

    #[test]
    fn test_scope_engine_v2_mutation_audit() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(&proj_id, "Scope Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        // Grant scope on src/auth/**
        engine
            .scope
            .acquire_scope(
                &task.id,
                "agent-1",
                vec!["src/auth/**".to_string()],
                "EXCLUSIVE_WRITE",
            )
            .unwrap();

        // Simulate changed files
        let files = vec![
            "src/auth/login.ts".to_string(),
            "src/payments/charge.ts".to_string(), // OUT OF SCOPE!
        ];

        let violations = engine
            .scope
            .audit_actual_mutations(&task.id, "agent-1", &files)
            .unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_path, "src/payments/charge.ts");
    }

    #[test]
    fn test_verification_engine_and_proof_bundle() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Verify Task",
                "Desc",
                "HIGH",
                vec![("Mandatory Step".to_string(), "Must do".to_string(), true)],
                vec!["Criteria 1".to_string()],
            )
            .unwrap();

        // Initial submission fails because step is pending
        let res = engine
            .verify
            .verify_task_submission(&task.id, "head-sha-1")
            .unwrap();
        assert_eq!(res.is_valid, false);

        // Generate Proof Bundle
        let bundle = engine
            .verify
            .generate_proof_bundle(
                &task.id,
                &proj_id,
                Some("agent-1"),
                "Test prompt",
                "base-sha-0",
                "head-sha-1",
                &["src/auth.rs".to_string()],
                "+10 -2",
            )
            .unwrap();

        assert!(!bundle.proof_hash.is_empty());
        assert_eq!(bundle.head_sha, "head-sha-1");
    }

    /// Seeds a fully-controlled task/attempt/run/evaluator/criteria fixture with fixed IDs
    /// so proof-digest tests can vary exactly one evidence field at a time.
    fn seed_proof_fixture(engine: &CoordinatorEngine, proj_id: &str) -> String {
        let task_id = "proof-digest-task-1".to_string();
        let attempt_id = "proof-digest-attempt-1".to_string();
        let now = "2026-01-01T00:00:00Z".to_string();
        let conn = engine.db.lock();
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, created_at, updated_at)
             VALUES (?1, ?2, 'Digest Task', 'Desc', 'BACKLOG', 'NONE', 'HIGH', ?3, ?3)",
            rusqlite::params![task_id, proj_id, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token)
             VALUES ('proof-digest-agent-1', 'Digest Agent', 'CODER', '{}', ?1, ?1, 'sess_proof_digest_1')",
            [now.clone()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, run_number, base_sha, worktree_path, status, started_at)
             VALUES (?1, ?2, 'proof-digest-agent-1', 1, 1, 'base-sha-0', '', 'VERIFIED', ?3)",
            rusqlite::params![attempt_id, task_id, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO acceptance_criteria (id, task_id, criterion, is_satisfied, is_locked)
             VALUES ('proof-digest-crit-1', ?1, 'Criteria 1', 1, 0)",
            [&task_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, source, executed_at, timed_out)
             VALUES ('proof-digest-run-1', ?1, NULL, 'chk-1', 'TYPECHECK', 'head-sha-1', 'cargo check', 0, 'out-1', 'err-1', 100, 1, 0, 'COORDINATOR_OBSERVED', ?2, 0)",
            rusqlite::params![task_id, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO evaluator_results (id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at)
             VALUES ('proof-digest-eval-1', ?1, ?2, NULL, 'TYPECHECK', 'COMMAND', '1.0.0', 'head-sha-1', 0, 'out-1', 'err-1', 'abc123', 100, 1, ?3)",
            rusqlite::params![task_id, attempt_id, now],
        )
        .unwrap();
        drop(conn);
        task_id
    }

    fn generate_fixture_proof_bundle(
        engine: &CoordinatorEngine,
        task_id: &str,
        proj_id: &str,
    ) -> crate::models::ProofBundle {
        engine
            .verify
            .generate_proof_bundle(
                task_id,
                proj_id,
                Some("agent-1"),
                "Test prompt",
                "base-sha-0",
                "head-sha-1",
                &["src/a.rs".to_string()],
                "+10 -2",
            )
            .unwrap()
    }

    /// D10: the proof digest must be canonical and deterministic — identical inputs (even in
    /// a different file order) must yield an identical SHA-256 digest.
    #[test]
    fn test_proof_hash_is_deterministic() {
        let (engine, proj_id) = setup_test_engine();
        let task_id = seed_proof_fixture(&engine, &proj_id);
        let gen = |files: &[String]| {
            engine
                .verify
                .generate_proof_bundle(
                    &task_id,
                    &proj_id,
                    Some("agent-1"),
                    "Test prompt",
                    "base-sha-0",
                    "head-sha-1",
                    files,
                    "+10 -2",
                )
                .unwrap()
                .proof_hash
        };
        let h1 = gen(&["src/b.rs".to_string(), "src/a.rs".to_string()]);
        let h2 = gen(&["src/b.rs".to_string(), "src/a.rs".to_string()]);
        assert_eq!(h1, h2, "identical inputs must produce an identical digest");
        let h3 = gen(&["src/a.rs".to_string(), "src/b.rs".to_string()]);
        assert_eq!(
            h1, h3,
            "the digest must be canonical: file order must not change the hash"
        );
    }

    /// D10: the proof digest must cover captured output, criteria snapshots and evaluator
    /// identity/version. Each mutation of exactly one evidence field must change the hash.
    #[test]
    fn test_proof_hash_covers_output_criteria_and_evaluator() {
        let (engine, proj_id) = setup_test_engine();
        let task_id = seed_proof_fixture(&engine, &proj_id);
        let gen = || generate_fixture_proof_bundle(&engine, &task_id, &proj_id).proof_hash;
        let baseline = gen();

        // stdout differs -> digest must differ
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE verification_runs SET stdout = 'out-2' WHERE id = 'proof-digest-run-1'",
                [],
            )
            .unwrap();
        }
        let stdout_diff = gen();
        assert_ne!(
            baseline, stdout_diff,
            "a change in captured stdout must change the digest"
        );

        // criteria differ -> digest must differ
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE verification_runs SET stdout = 'out-1' WHERE id = 'proof-digest-run-1'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE acceptance_criteria SET criterion = 'Other Criteria' WHERE id = 'proof-digest-crit-1'",
                [],
            )
            .unwrap();
        }
        let criteria_diff = gen();
        assert_ne!(
            baseline, criteria_diff,
            "a change in acceptance criteria must change the digest"
        );

        // evaluator version differs -> digest must differ
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE acceptance_criteria SET criterion = 'Criteria 1' WHERE id = 'proof-digest-crit-1'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE evaluator_results SET evaluator_version = '2.0.0' WHERE id = 'proof-digest-eval-1'",
                [],
            )
            .unwrap();
        }
        let eval_diff = gen();
        assert_ne!(
            baseline, eval_diff,
            "a change in evaluator version must change the digest"
        );
    }

    /// D10: flipping a single byte in stored stdout must be detected by the digest.
    #[test]
    fn test_proof_hash_detects_tampering() {
        let (engine, proj_id) = setup_test_engine();
        let task_id = seed_proof_fixture(&engine, &proj_id);
        let gen = || generate_fixture_proof_bundle(&engine, &task_id, &proj_id).proof_hash;
        let original = gen();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE verification_runs SET stdout = 'OUT-1' WHERE id = 'proof-digest-run-1'",
                [],
            )
            .unwrap();
        }
        let tampered = gen();
        assert_ne!(
            original, tampered,
            "flipping a byte in stored stdout must change the digest"
        );
    }

    /// D10: proof records are append-only — re-verifying the same task/head with identical
    /// outputs must append a NEW per-attempt row instead of overwriting the prior one
    /// (INSERT OR REPLACE + UNIQUE proof_hash collapses it to one row today).
    #[test]
    fn test_proof_records_are_append_only() {
        let (engine, proj_id) = setup_test_engine();
        let task_id = seed_proof_fixture(&engine, &proj_id);

        let bundle1 = generate_fixture_proof_bundle(&engine, &task_id, &proj_id);
        let bundle2 = generate_fixture_proof_bundle(&engine, &task_id, &proj_id);
        assert_eq!(
            bundle1.proof_hash, bundle2.proof_hash,
            "identical inputs must yield an identical digest"
        );
        assert_eq!(
            bundle1.head_sha, bundle2.head_sha,
            "identical inputs must yield the same head"
        );

        let conn = engine.db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proof_bundles WHERE task_id = ?1 AND head_sha = ?2",
                rusqlite::params![task_id, bundle1.head_sha],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        assert_eq!(
            count, 2,
            "re-verification with identical outputs must append a new proof row (got {} rows)",
            count
        );
    }

    #[test]
    fn test_serialized_merge_queue_operations() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(&proj_id, "Merge Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        let queue_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task.id,
                "agentxflow/task-1",
                "main",
                "base-sha-1",
                "head-sha-1",
            )
            .unwrap();

        assert_eq!(queue_item.position, 1);
        assert_eq!(queue_item.status, "READY");

        let list = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(list.len(), 1);
    }

    /// D19 regression: the MERGE_READY state write in enqueue_task_by_id must propagate
    /// errors. A failed enqueue must surface as Err (never swallowed by .ok()) and must not
    /// enqueue a queue row. The task's state is corrupted to CANCELLED - rejected by the
    /// enqueue gate and by the transition from-list ([Review, Verifying]) - so enqueue must
    /// fail loudly before anything is enqueued.
    #[test]
    fn test_enqueue_task_failure_surfaces() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Enqueue Failure Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();

        // Drive the task to VERIFYING (a state the enqueue gate accepts) via legal transitions.
        {
            let conn = engine.db.lock();
            for (from, to) in [
                (&[TaskState::Backlog][..], TaskState::Ready),
                (&[TaskState::Ready][..], TaskState::Running),
                (&[TaskState::Running][..], TaskState::Verifying),
            ] {
                transition_task_state_on_conn(&conn, &task_id, from, to).unwrap();
            }
        }

        // Give the task a valid proof bundle so it is fully enqueueable except for its state.
        engine
            .verify
            .generate_proof_bundle(
                &task_id,
                &proj_id,
                Some("agent-1"),
                "Test prompt",
                "base-sha-0",
                "head-sha-1",
                &["src/auth.rs".to_string()],
                "+10 -2",
            )
            .unwrap();

        // Corrupt the state to CANCELLED - rejected by the enqueue gate AND by the
        // transition from-list ([Review, Verifying]) - so enqueue must fail loudly.
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'CANCELLED' WHERE id = ?1",
                [&task_id],
            )
            .unwrap();
        }

        // act: enqueue must surface Err instead of swallowing the failed state write
        let err = engine.enqueue_task_by_id(&proj_id, &task_id).unwrap_err();
        assert!(
            err.contains("state"),
            "expected the rejection to mention the invalid task state, got: {}",
            err
        );

        // assert: nothing was enqueued and no state write flipped the task to MERGE_READY
        let conn = engine.db.lock();
        let queued: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM merge_queue WHERE task_id = ?1",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(
            queued, 0,
            "failed enqueue must not create a merge_queue row"
        );
        assert_eq!(
            engine.get_task(&task_id).unwrap().state,
            TaskState::Cancelled,
            "failed enqueue must not silently transition the task state"
        );
    }

    /// D28 regression: the merge-enqueue gate must re-verify the stored proof digest before
    /// enqueueing. The pre-fix gate only COUNTed proof rows, so a tampered row (diff_summary
    /// mutated in the DB) still enqueued. The integrity check must refuse the enqueue with
    /// PROOF_INTEGRITY_FAILURE and leave the queue untouched.
    #[test]
    fn test_enqueue_rejects_tampered_proof() {
        let (engine, proj_id, _temp_dir) = setup_test_engine_with_worktree_root();
        let agent = engine.register_agent("Agent-Tamper", "Coder").unwrap();
        let task = engine
            .create_task(&proj_id, "Tamper Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let task_id = task.id.clone();

        // claim -> real managed worktree in RUNNING state
        let claimed = engine.claim_task(&task_id, &agent.id).unwrap();
        assert_eq!(claimed.state, TaskState::Running);

        // drive RUNNING -> VERIFYING (a state the enqueue gate accepts)
        {
            let conn = engine.db.lock();
            transition_task_state_on_conn(
                &conn,
                &task_id,
                &[TaskState::Running],
                TaskState::Verifying,
            )
            .unwrap();
        }

        let worktree_path = claimed.worktree_path.as_deref().unwrap();
        let head_sha = engine
            .git
            .get_worktree_head_sha(std::path::Path::new(worktree_path))
            .unwrap();
        let base_sha = claimed.base_sha.as_deref().unwrap();

        // normal proof path: seal a bundle at the worktree HEAD
        engine
            .verify
            .generate_proof_bundle(
                &task_id,
                &proj_id,
                Some(&agent.id),
                "Test prompt",
                base_sha,
                &head_sha,
                &[],
                "Authoritative Coordinator Automated Verification Passed",
            )
            .unwrap();

        // sanity: the untampered proof enqueues cleanly (untampered flow unchanged)
        engine.enqueue_task_by_id(&proj_id, &task_id).unwrap();

        // tamper with the stored row the gate hashes (diff_summary is hashed AND stored)
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE proof_bundles SET diff_summary = 'TAMPERED' WHERE task_id = ?1 AND head_sha = ?2",
                rusqlite::params![task_id, head_sha],
            )
            .unwrap();
        }

        let queue_before: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM merge_queue WHERE task_id = ?1 AND status = 'READY'",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap()
        };

        // act: the tampered proof must be refused at the gate
        let err = engine.enqueue_task_by_id(&proj_id, &task_id).unwrap_err();
        assert!(
            err.contains("Proof integrity check failed"),
            "expected a proof integrity rejection, got: {}",
            err
        );

        // assert: task NOT enqueued (no new queue row), PROOF_INTEGRITY_FAILURE emitted,
        //         and the task state untouched by the refused enqueue
        let queue_after: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM merge_queue WHERE task_id = ?1 AND status = 'READY'",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(
            queue_after, queue_before,
            "tampered enqueue must not add a merge queue row"
        );

        let events = engine.get_events_after(0).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.event_type == "PROOF_INTEGRITY_FAILURE"),
            "expected a PROOF_INTEGRITY_FAILURE event, got event types: {:?}",
            events
                .iter()
                .map(|e| e.event_type.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            engine.get_task(&task_id).unwrap().state,
            TaskState::MergeReady,
            "refused enqueue must not change the task state"
        );
    }

    #[test]
    fn test_task_cancellation_and_scope_release() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Agent-Alpha", "Coder").unwrap();
        let task = engine
            .create_task(&proj_id, "Cancel Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        // Claim and acquire scope
        let claimed = engine.claim_task(&task.id, &agent.id).unwrap();
        assert_eq!(claimed.state, TaskState::Running);
        engine
            .scope
            .acquire_scope(
                &task.id,
                &agent.id,
                vec!["crates/engine/**".to_string()],
                "EXCLUSIVE_WRITE",
            )
            .unwrap();

        // Verify scope lease exists
        let leases = engine.get_task_details(&task.id).unwrap().leases;
        assert_eq!(leases.len(), 1);

        // Cancel task explicitly
        let cancelled = engine
            .cancel_task(&task.id, Some(&agent.id), Some("User requested stop"))
            .unwrap();
        assert_eq!(cancelled.state, TaskState::Cancelled);
        assert!(cancelled.is_stale);

        // Verify scope leases were fully released
        let leases_after = engine.get_task_details(&task.id).unwrap().leases;
        assert_eq!(leases_after.len(), 0);

        // Another task can now acquire the same scope pattern without collision
        let agent2 = engine.register_agent("Agent-Beta", "Coder").unwrap();
        let task2 = engine
            .create_task(&proj_id, "Task 2", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let claimed2 = engine.claim_task(&task2.id, &agent2.id).unwrap();
        assert!(engine
            .scope
            .acquire_scope(
                &claimed2.id,
                &agent2.id,
                vec!["crates/engine/**".to_string()],
                "EXCLUSIVE_WRITE"
            )
            .is_ok());
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
        let claimed_chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        assert_eq!(claimed_chunk.state, TaskState::Running);
        assert!(!claimed_chunk.is_stale);

        // 3. Reset masterplan
        assert!(engine.reset_masterplan(&proj_id, None).is_ok());

        // 4. Verify claimed task is now marked CANCELLED & is_stale = true, and its scopes released
        let task_after = engine.get_task(&claimed_chunk.id).unwrap();
        assert_eq!(task_after.state, TaskState::Cancelled);
        assert!(task_after.is_stale);

        let leases = engine.get_task_details(&claimed_chunk.id).unwrap().leases;
        assert_eq!(leases.len(), 0);
    }

    #[test]
    fn test_masterplan_reset_preserves_dirty_user_work() {
        let (engine, proj_id, temp_dir) = setup_test_engine_with_worktree_root();
        let repo = temp_dir.clone();
        let agent = engine.register_agent("Agent-Reset-Safe", "Coder").unwrap();

        // arrange: primary checkout has staged + unstaged + untracked files
        git_safety::setup_repo_with_changes(&repo, true, true);
        std::fs::write(repo.join("README.md"), "edited-unstaged").unwrap();
        let head_before = git_safety::git(&repo, &["rev-parse", "HEAD"]);

        // masterplan with claimed chunks
        let raw_plan = "# Step 1: Init\nInit project\n# Step 2: Core\nCore logic";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        let claimed_chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        assert_eq!(claimed_chunk.state, TaskState::Running);

        // act
        assert!(engine.reset_masterplan(&proj_id, None).is_ok());

        // assert 1: masterplan + steps deleted; tasks cancelled + stale
        assert!(engine.get_masterplan(&proj_id).unwrap().is_none());
        assert!(engine.list_masterplan_steps(&proj_id).unwrap().is_empty());
        let task_after = engine.get_task(&claimed_chunk.id).unwrap();
        assert_eq!(task_after.state, TaskState::Cancelled);
        assert!(task_after.is_stale);

        // assert 2: user file "a.txt" staged edit survives; untracked "user-note.txt" survives;
        //          unstaged README.md edit survives
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "v2-user"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            "edited-unstaged"
        );
        assert!(repo.join("user-note.txt").exists());

        // assert 3: HEAD of the primary checkout unchanged from pre-reset
        assert_eq!(git_safety::git(&repo, &["rev-parse", "HEAD"]), head_before);
    }

    #[test]
    fn test_masterplan_reset_removes_managed_worktrees_only() {
        let (engine, proj_id, temp_dir) = setup_test_engine_with_worktree_root();
        let repo = temp_dir.clone();
        let agent = engine.register_agent("Agent-WT-Safe", "Coder").unwrap();

        // arrange: dirty primary checkout (staged edit + untracked file)
        git_safety::setup_repo_with_changes(&repo, true, true);
        let head_before = git_safety::git(&repo, &["rev-parse", "HEAD"]);

        // claimed chunk -> managed task worktree at worktrees_root/<project>/task-<id>
        let raw_plan = "# Step 1: Init\nInit project\n# Step 2: Core\nCore logic";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        let claimed_chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        let managed_wt = temp_dir
            .join("worktrees")
            .join(&proj_id)
            .join(format!("task-{}", claimed_chunk.id));
        assert!(
            managed_wt.exists(),
            "managed task worktree should exist on disk after claim"
        );

        // a worktree outside worktrees_root (user-managed) must survive the reset
        let user_wt =
            std::env::temp_dir().join(format!("agentxflow_userwt_{}", uuid::Uuid::new_v4()));
        git_safety::git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "user-branch",
                user_wt.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(user_wt.join("keep.txt"), "user worktree file").unwrap();

        // act
        assert!(engine.reset_masterplan(&proj_id, None).is_ok());

        // assert: managed task worktrees removed from disk
        assert!(
            !managed_wt.exists(),
            "managed task worktree must be removed by reset"
        );

        // assert: anything outside worktrees_root untouched, including the primary checkout
        assert!(user_wt.exists());
        assert_eq!(
            std::fs::read_to_string(user_wt.join("keep.txt")).unwrap(),
            "user worktree file"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "v2-user"
        );
        assert!(repo.join("user-note.txt").exists());
        assert_eq!(git_safety::git(&repo, &["rev-parse", "HEAD"]), head_before);
    }

    #[test]
    fn test_merge_into_checked_out_branch_preserves_dirty_user_work() {
        let (engine, proj_id) = setup_test_engine();
        let repo = {
            let proj = engine
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == proj_id)
                .unwrap();
            std::path::PathBuf::from(&proj.path)
        };
        let task = engine
            .create_task(
                &proj_id,
                "Merge Safety Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        // The merge engine only writes task state from MERGE_READY (the state produced by the
        // enqueue flow); drive the task there so the DONE write is a legal validated transition.
        drive_task_to_merge_ready(&engine, &task_id);

        // Establish a.txt baseline commit on main (helper's init_repo commits it)
        git_safety::setup_repo_with_changes(&repo, false, false);

        // .gitignore for .agentxflow/ (mirrors GitService::init_repo) so the
        // integration worktree never dirties the primary checkout status
        std::fs::write(repo.join(".gitignore"), ".agentxflow/\n").unwrap();
        std::fs::write(repo.join("b.txt"), "b1").unwrap();
        git_safety::git(&repo, &["add", ".gitignore", "b.txt"]);
        git_safety::git(&repo, &["commit", "-m", "baseline"]);
        let base_sha = git_safety::git(&repo, &["rev-parse", "main"]);

        // Feature branch with a commit that only ADDS a new file, so a fast-forward
        // sync can proceed without touching the user's dirty files
        git_safety::git(&repo, &["checkout", "-b", &branch_name]);
        std::fs::write(repo.join("feature.txt"), "feature").unwrap();
        git_safety::git(&repo, &["add", "feature.txt"]);
        git_safety::git(&repo, &["commit", "-m", "feature"]);
        let head_sha = git_safety::git(&repo, &["rev-parse", "HEAD"]);
        git_safety::git(&repo, &["checkout", "main"]);

        // Dirty the primary checkout: staged edit, unstaged edit, untracked file
        std::fs::write(repo.join("a.txt"), "v2-user").unwrap();
        git_safety::git(&repo, &["add", "a.txt"]);
        std::fs::write(repo.join("b.txt"), "b2-user").unwrap();
        std::fs::write(repo.join("user-note.txt"), "keep me").unwrap();

        // Enqueue the verified task for merge and process it (background worker call)
        let q_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task_id,
                &branch_name,
                "main",
                &base_sha,
                &head_sha,
            )
            .unwrap();
        let attempt = engine.merge.process_merge_by_id(&q_item.id, &repo).unwrap();
        assert_eq!(attempt.simulation_passed, true);

        // 1. Merge still succeeds: queue item -> MERGED, task -> DONE
        let q_after = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(q_after.len(), 1);
        assert_eq!(q_after[0].status, "MERGED");
        assert_eq!(engine.get_task(&task_id).unwrap().state, TaskState::Done);

        // 2. Staged user edit survives
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "v2-user"
        );
        // 3. Untracked user file survives
        assert!(repo.join("user-note.txt").exists());
        // 3b. Unstaged user edit survives
        assert_eq!(
            std::fs::read_to_string(repo.join("b.txt")).unwrap(),
            "b2-user"
        );
        // 4. Ref advanced: main HEAD == merged integration head == checkout HEAD
        let main_sha = git_safety::git(&repo, &["rev-parse", "main"]);
        let checkout_head = git_safety::git(&repo, &["rev-parse", "HEAD"]);
        assert_eq!(main_sha, checkout_head);
        assert_eq!(main_sha, attempt.target_sha_after.unwrap());
    }

    #[test]
    fn test_merge_verification_hangs_do_not_stall_worker() {
        let (engine, proj_id) = setup_test_engine();
        let repo = {
            let proj = engine
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == proj_id)
                .unwrap();
            std::path::PathBuf::from(&proj.path)
        };
        let task = engine
            .create_task(
                &proj_id,
                "Hanging Merge Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        // The merge engine only writes BLOCKED from MERGE_READY (the state produced by the
        // enqueue flow); drive the task there so the BLOCKED write is a legal validated transition.
        drive_task_to_merge_ready(&engine, &task_id);

        // arrange: a Cargo project whose "cargo test" hangs (a #[test] that sleeps for an hour)
        // far longer than POST_MERGE_VERIFY_TIMEOUT, so the merge's post-merge verification can
        // never complete on its own and must be bounded/tree-killed by the executor.
        git_safety::setup_repo_with_changes(&repo, false, false);
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"hang_merge_proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let src_dir = repo.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            src_dir.join("lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn hangs_forever() {\n        std::thread::sleep(std::time::Duration::from_secs(3600));\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(repo.join(".gitignore"), ".agentxflow/\n").unwrap();
        git_safety::git(&repo, &["add", "Cargo.toml", "src/lib.rs", ".gitignore"]);
        git_safety::git(&repo, &["commit", "-m", "baseline with hanging test"]);
        let base_sha = git_safety::git(&repo, &["rev-parse", "main"]);

        // Feature branch with a commit that only ADDS a new file (fast-forwardable)
        git_safety::git(&repo, &["checkout", "-b", &branch_name]);
        std::fs::write(repo.join("feature.txt"), "feature").unwrap();
        git_safety::git(&repo, &["add", "feature.txt"]);
        git_safety::git(&repo, &["commit", "-m", "feature"]);
        let head_sha = git_safety::git(&repo, &["rev-parse", "HEAD"]);
        git_safety::git(&repo, &["checkout", "main"]);

        let q_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task_id,
                &branch_name,
                "main",
                &base_sha,
                &head_sha,
            )
            .unwrap();

        // act: run the merge on a worker thread and verify it returns within the post-merge
        // verification timeout + grace instead of stalling the worker forever.
        let worker_engine = engine.clone();
        let worker_repo = repo.clone();
        let worker_qid = q_item.id.clone();
        let start = std::time::Instant::now();
        let worker = std::thread::spawn(move || {
            worker_engine
                .merge
                .process_merge_by_id(&worker_qid, &worker_repo)
        });

        let grace = std::time::Duration::from_secs(60);
        let bound = crate::merge::POST_MERGE_VERIFY_TIMEOUT + grace;
        while start.elapsed() < bound && !worker.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let elapsed = start.elapsed();
        assert!(
            worker.is_finished(),
            "process_merge_by_id was still blocked after {:.1}s: post-merge verification is NOT bounded and the background merge worker would stall forever",
            elapsed.as_secs_f64()
        );
        assert!(
            elapsed < bound,
            "process_merge_by_id returned only after {:.1}s, exceeding the {}s timeout + {}s grace",
            elapsed.as_secs_f64(),
            crate::merge::POST_MERGE_VERIFY_TIMEOUT.as_secs(),
            grace.as_secs()
        );

        let result = worker.join().unwrap();

        // assert: hung post-merge verification is treated as verification failure -> BLOCKED,
        // never stuck in RUNNING_CHECKS and never silently MERGED.
        assert!(
            result.is_err(),
            "hung post-merge verification must fail the merge, got: {:?}",
            result
        );
        let q_after = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(q_after.len(), 1);
        assert_eq!(q_after[0].status, "FAILED_TESTS");
        assert_eq!(engine.get_task(&task_id).unwrap().state, TaskState::Blocked);
    }

    /// Forces a real 3-way merge conflict on `a.txt`: the feature branch edits it, then main
    /// moves past the feature's fork point with a conflicting edit. The recorded base SHA is the
    /// CURRENT main SHA (as enqueue_task_by_id records the task's base at enqueue time), so the
    /// stale check passes, but the merge simulation hits a genuine conflict on `a.txt`.
    fn setup_conflicting_repo(repo: &std::path::Path, branch_name: &str) -> (String, String) {
        git_safety::git(repo, &["config", "user.email", "t@t.t"]);
        git_safety::git(repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "v1").unwrap();
        std::fs::write(repo.join(".gitignore"), ".agentxflow/\n").unwrap();
        git_safety::git(repo, &["add", "a.txt", ".gitignore"]);
        git_safety::git(repo, &["commit", "-m", "baseline"]);

        git_safety::git(repo, &["checkout", "-b", branch_name]);
        std::fs::write(repo.join("a.txt"), "v2-feature").unwrap();
        git_safety::git(repo, &["add", "a.txt"]);
        git_safety::git(repo, &["commit", "-m", "feature edit"]);

        git_safety::git(repo, &["checkout", "main"]);
        std::fs::write(repo.join("a.txt"), "v2-main").unwrap();
        git_safety::git(repo, &["add", "a.txt"]);
        git_safety::git(repo, &["commit", "-m", "main edit"]);
        let base_sha = git_safety::git(repo, &["rev-parse", "main"]);
        let head_sha = git_safety::git(repo, &["rev-parse", branch_name]);
        (base_sha, head_sha)
    }

    #[test]
    fn test_merge_engine_state_writes_are_validated() {
        let (engine, proj_id) = setup_test_engine();
        let repo = {
            let proj = engine
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == proj_id)
                .unwrap();
            std::path::PathBuf::from(&proj.path)
        };
        let task = engine
            .create_task(
                &proj_id,
                "Merge Validation Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        // arrange: a VERIFYING task enqueued for merge. The enqueue flow normalizes
        // VERIFYING -> MERGE_READY before the queue worker runs, so the merge engine
        // legitimately writes BLOCKED from MERGE_READY only.
        drive_task_to_merge_ready(&engine, &task_id);
        assert_eq!(
            engine.get_task(&task_id).unwrap().state,
            TaskState::MergeReady
        );

        // Force the merge to fail with a genuine 3-way conflict
        let (base_sha, head_sha) = setup_conflicting_repo(&repo, &branch_name);
        let q_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task_id,
                &branch_name,
                "main",
                &base_sha,
                &head_sha,
            )
            .unwrap();

        // act: process the merge
        let attempt = engine.merge.process_merge_by_id(&q_item.id, &repo).unwrap();

        // assert: task -> BLOCKED is a legal transition and the queue item records the conflict
        assert_eq!(attempt.simulation_passed, false);
        let q_after = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(q_after[0].status, "BLOCKED_CONFLICT");
        assert_eq!(engine.get_task(&task_id).unwrap().state, TaskState::Blocked);

        // assert: the merge lifecycle event was written through the events table (no raw bypass)
        let conn = engine.db.lock();
        let merge_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND event_type LIKE 'MERGE_%'",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(
            merge_events, 1,
            "expected exactly one MERGE_* lifecycle event for the failed merge, got {}",
            merge_events
        );
    }

    #[test]
    fn test_merge_engine_rejects_illegal_state_write() {
        let (engine, proj_id) = setup_test_engine();
        let repo = {
            let proj = engine
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == proj_id)
                .unwrap();
            std::path::PathBuf::from(&proj.path)
        };
        let task = engine
            .create_task(
                &proj_id,
                "Illegal Merge Write Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        // arrange: the task stays in BACKLOG (no production path enqueues a non-MERGE_READY
        // task - enqueue_task_by_id rejects it - but a direct queue INSERT must not let the
        // merge engine silently clobber the state either)
        let (base_sha, head_sha) = setup_conflicting_repo(&repo, &branch_name);
        let q_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task_id,
                &branch_name,
                "main",
                &base_sha,
                &head_sha,
            )
            .unwrap();

        // act: process the merge; it hits a conflict, so the merge engine attempts BLOCKED
        let attempt = engine.merge.process_merge_by_id(&q_item.id, &repo).unwrap();
        assert_eq!(attempt.simulation_passed, false);

        // assert: the invalid BACKLOG -> BLOCKED write is rejected; the task is untouched and
        // no MERGE_* event is emitted for a transition that never happened
        assert_eq!(
            engine.get_task(&task_id).unwrap().state,
            TaskState::Backlog,
            "merge engine must not write BLOCKED from a state that makes it illegal"
        );
        let conn = engine.db.lock();
        let merge_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND event_type LIKE 'MERGE_%'",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(
            merge_events, 0,
            "no MERGE_* event may be emitted for a rejected transition, got {}",
            merge_events
        );
    }

    #[test]
    fn test_merge_aborts_when_integration_worktree_cannot_be_reset() {
        let (engine, proj_id) = setup_test_engine();
        let repo = {
            let proj = engine
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == proj_id)
                .unwrap();
            std::path::PathBuf::from(&proj.path)
        };
        let task = engine
            .create_task(
                &proj_id,
                "Reset Failure Merge Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        // The merge engine only writes BLOCKED from MERGE_READY (the state produced by the
        // enqueue flow); drive the task there so the BLOCKED write is a legal validated transition.
        drive_task_to_merge_ready(&engine, &task_id);

        // arrange: baseline + feature commit on the real repo
        let (base_sha, head_sha) = setup_conflicting_repo(&repo, &branch_name);

        // Pre-create the integration worktree directory as a non-repo: a `.git` file pointing
        // nowhere makes every git command inside it fail ("not a git repository"), so
        // ensure_integration_worktree's exists-check passes but the subsequent
        // `reset --hard` / `clean -fd` cannot run. Pre-fix these failures were .ok()-swallowed
        // and the merge proceeded on the broken tree.
        let integration_dir = repo.join(".agentxflow").join("integration").join(&proj_id);
        std::fs::create_dir_all(&integration_dir).unwrap();
        std::fs::write(
            integration_dir.join(".git"),
            "gitdir: /nonexistent/agentxflow-test-gitdir\n",
        )
        .unwrap();

        let q_item = engine
            .merge
            .enqueue_task(
                &proj_id,
                &task_id,
                &branch_name,
                "main",
                &base_sha,
                &head_sha,
            )
            .unwrap();

        // act: process the merge
        let result = engine.merge.process_merge_by_id(&q_item.id, &repo);

        // assert: the reset failure propagates as an Err (no silent merge on a stale tree)
        let err = result.expect_err(
            "integration worktree reset failure must abort the merge instead of proceeding on a stale tree",
        );
        assert!(
            err.contains("reset"),
            "expected the reset failure to be surfaced in the error, got: {}",
            err
        );

        // assert: queue item -> BLOCKED and the task -> BLOCKED
        let q_after = engine.merge.list_queue(&proj_id).unwrap();
        assert_eq!(q_after.len(), 1);
        assert_eq!(q_after[0].status, "BLOCKED");
        assert_eq!(engine.get_task(&task_id).unwrap().state, TaskState::Blocked);

        // assert: the error is recorded in the integration attempt
        let conn = engine.db.lock();
        let recorded_error: Option<String> = conn
            .query_row(
                "SELECT conflicts_json FROM integration_attempts WHERE merge_queue_id = ?1",
                [&q_item.id],
                |r| r.get(0),
            )
            .ok();
        drop(conn);
        assert!(
            recorded_error
                .as_deref()
                .map(|e| e.contains("not a git repository"))
                .unwrap_or(false),
            "expected the reset error to be recorded in the integration attempt, got: {:?}",
            recorded_error
        );

        // assert: target branch ref untouched
        let main_sha = git_safety::git(&repo, &["rev-parse", "main"]);
        assert_eq!(main_sha, base_sha);
    }

    #[test]
    fn test_decompose_blocked_while_tasks_active() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Agent-Decomp", "Coder").unwrap();

        // Prepare plan
        let raw_plan = "# Step 1: A\nDesc A\n# Step 2: B\nDesc B";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();

        // Claim a chunk so an active task exists
        let claimed = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        assert_eq!(claimed.state, TaskState::Running);

        // Attempting to re-decompose while task is active MUST fail
        let decomp_res = engine.decompose_masterplan(
            &proj_id,
            vec![crate::models::DecomposedStepInput {
                step_index: 1,
                title: "New 1".to_string(),
                description: "New desc".to_string(),
                suggested_scope: Some("src/**".to_string()),
                acceptance_criteria: Some("Pass".to_string()),
            }],
            None,
            None,
        );
        assert!(
            decomp_res.is_err(),
            "Decomposing while active tasks are running must be blocked"
        );

        // Cancel the active task
        engine
            .cancel_task(&claimed.id, Some(&agent.id), Some("Cancelled for test"))
            .unwrap();

        // Now re-decomposition succeeds
        let decomp_res2 = engine.decompose_masterplan(
            &proj_id,
            vec![crate::models::DecomposedStepInput {
                step_index: 1,
                title: "New 1".to_string(),
                description: "New desc".to_string(),
                suggested_scope: Some("src/**".to_string()),
                acceptance_criteria: Some("Pass".to_string()),
            }],
            None,
            None,
        );
        assert!(
            decomp_res2.is_ok(),
            "Decomposing after cancelling active task must succeed"
        );
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
        let task = engine
            .create_task(
                &proj_id,
                "Task A",
                "Task Prompt",
                "HIGH",
                vec![("Step 1".into(), "Do step 1".into(), true)],
                vec!["Criterion 1".into()],
            )
            .unwrap();

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
        let chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(2))
            .unwrap();

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

    #[test]
    fn test_cancel_task_refuses_to_delete_unmanaged_path() {
        // arrange: temp repo + project; INSERT task rows directly with hostile worktree_path values
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
        git_safety::init_repo(&temp_dir);
        let temp_db = temp_dir.join("test.db");
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite pool");
        let engine =
            CoordinatorEngine::new_with_worktree_root(pool.clone(), temp_dir.join("worktrees"));
        let proj = engine
            .create_project(
                "Test Unit Project",
                &temp_dir.to_string_lossy(),
                "Spec",
                "main",
            )
            .unwrap();
        drop(engine);

        // <repo>/some-unrelated-dir: real dir with a file
        let unrelated = temp_dir.join("some-unrelated-dir");
        std::fs::create_dir_all(&unrelated).unwrap();
        std::fs::write(unrelated.join("keep.txt"), "keep me").unwrap();

        // CLAIMING task pointing at the unrelated dir, RUNNING task pointing at the primary checkout itself
        let claiming_id = uuid::Uuid::new_v4().to_string();
        let running_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, worktree_path, created_at, updated_at)
                 VALUES (?1, ?2, 'Hostile Claiming', 'd', 'CLAIMING', 'CLAIMING', 'HIGH', ?3, ?4, ?4)",
                rusqlite::params![claiming_id, proj.id, unrelated.to_string_lossy(), now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, worktree_path, created_at, updated_at)
                 VALUES (?1, ?2, 'Hostile Running', 'd', 'RUNNING', 'NONE', 'HIGH', ?3, ?4, ?4)",
                rusqlite::params![running_id, proj.id, temp_dir.to_string_lossy(), now],
            )
            .unwrap();
        }

        // act: CoordinatorEngine::new runs reconcile_on_startup, then cancel_task on the RUNNING task
        let engine = CoordinatorEngine::new_with_worktree_root(pool, temp_dir.join("worktrees"));
        let head_before = git_safety::git(&temp_dir, &["rev-parse", "HEAD"]);
        let cancelled = engine.cancel_task(&running_id, None, None).unwrap();
        assert_eq!(cancelled.state, TaskState::Cancelled);

        // assert 1: the unrelated dir and its file still exist
        assert!(unrelated.exists());
        assert_eq!(
            std::fs::read_to_string(unrelated.join("keep.txt")).unwrap(),
            "keep me"
        );
        // assert 2: the primary checkout is unchanged
        assert!(temp_dir.join("a.txt").exists());
        assert_eq!(
            git_safety::git(&temp_dir, &["rev-parse", "HEAD"]),
            head_before
        );
        // assert 3: a WORKTREE_SAFETY_REFUSAL event was emitted
        let events = engine.get_events_after(0).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.event_type == "WORKTREE_SAFETY_REFUSAL"),
            "expected a WORKTREE_SAFETY_REFUSAL event"
        );
    }

    #[test]
    fn test_reconcile_on_startup_only_deletes_managed_worktrees() {
        // arrange: task row state='CLAIMING', worktree_path = <repo>/user-data (untracked user dir with files)
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
        git_safety::init_repo(&temp_dir);
        let temp_db = temp_dir.join("test.db");
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite pool");
        let engine =
            CoordinatorEngine::new_with_worktree_root(pool.clone(), temp_dir.join("worktrees"));
        let proj = engine
            .create_project(
                "Test Unit Project",
                &temp_dir.to_string_lossy(),
                "Spec",
                "main",
            )
            .unwrap();
        drop(engine);

        let user_data = temp_dir.join("user-data");
        std::fs::create_dir_all(&user_data).unwrap();
        std::fs::write(user_data.join("notes.txt"), "user data").unwrap();

        let hostile_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, worktree_path, created_at, updated_at)
                 VALUES (?1, ?2, 'Hostile Claiming', 'd', 'CLAIMING', 'CLAIMING', 'HIGH', ?3, ?4, ?4)",
                rusqlite::params![hostile_id, proj.id, user_data.to_string_lossy(), now],
            )
            .unwrap();
        }

        // act: CoordinatorEngine::new runs reconcile_on_startup
        let engine = CoordinatorEngine::new_with_worktree_root(pool, temp_dir.join("worktrees"));

        // assert 1: <repo>/user-data survives
        assert!(user_data.exists());
        assert_eq!(
            std::fs::read_to_string(user_data.join("notes.txt")).unwrap(),
            "user data"
        );
        // assert 2: the CLAIMING task's state was still reset to READY
        let task = engine.get_task(&hostile_id).unwrap();
        assert_eq!(task.state, TaskState::Ready);
    }

    #[test]
    fn test_reconcile_failures_are_logged_and_evented() {
        // arrange: force a reconcile write failure by dropping the merge_queue table
        // (the RUNNING_CHECKS -> READY reset then fails with "no such table")
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_unit_{}", uuid::Uuid::new_v4()));
        let temp_db = temp_dir.join("test.db");
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite pool");
        {
            let conn = pool.lock();
            conn.execute("DROP TABLE merge_queue", []).unwrap();
        }

        // act: CoordinatorEngine::new runs reconcile_on_startup
        let engine = CoordinatorEngine::new_with_worktree_root(pool, temp_dir.join("worktrees"));

        // assert: the engine still constructs (startup proceeds despite the failure)
        // and the partial reconcile is surfaced as a RECONCILE_PARTIAL event
        let events = engine.get_events_after(0).unwrap();
        let partial = events
            .iter()
            .find(|e| e.event_type == "RECONCILE_PARTIAL")
            .expect("expected a RECONCILE_PARTIAL event after a failed reconcile write");
        let payload: serde_json::Value = serde_json::from_str(&partial.payload_json).unwrap();
        assert_eq!(
            payload["by_category"]["merge_queue_reset"], 1,
            "merge_queue reset failure should be counted in the summary"
        );
    }

    /// D29: heartbeat must slide agent_sessions.expires_at forward to a 30-day
    /// window so active agents never expire; dead agents lose their session
    /// 30 days after their last heartbeat.
    #[test]
    fn test_heartbeat_renews_session_expiry() {
        let (engine, _proj_id) = setup_test_engine();
        let agent = engine
            .register_agent("Agent-Heartbeat-Expiry", "Coder")
            .unwrap();

        // Manually backdate the session expiry to now + 1s (about to expire)
        let soon = (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE agent_sessions SET expires_at = ?1 WHERE agent_id = ?2",
                rusqlite::params![soon, agent.id],
            )
            .unwrap();
        }

        engine.agent_heartbeat(&agent.id).unwrap();

        let expires_at: String = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT expires_at FROM agent_sessions WHERE agent_id = ?1",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap()
        };

        // Heartbeat slides expiry to now + 30 days (tolerance: test execution time)
        let parsed = chrono::DateTime::parse_from_rfc3339(&expires_at).unwrap();
        let diff = parsed.signed_duration_since(chrono::Utc::now());
        assert!(
            diff >= chrono::Duration::days(30) - chrono::Duration::minutes(5)
                && diff <= chrono::Duration::days(30) + chrono::Duration::minutes(5),
            "expected expiry ~30 days from now, got {}",
            expires_at
        );
    }

    /// D20: unregistering an agent must leave no orphaned state behind.
    /// RUNNING tasks are cancelled + stale with their worktrees removed from
    /// disk, CLAIMED masterplan steps revert to PENDING, scope leases are
    /// released, ACTIVE agent_runs terminate, and the agent + sessions delete.
    #[tokio::test]
    async fn test_unregister_agent_leaves_no_orphaned_state() {
        let (engine, proj_id, temp_dir) = setup_test_engine_with_worktree_root();
        let agent = engine.register_agent("Agent-Unreg", "Coder").unwrap();

        // arrange 1: claimed masterplan chunk -> RUNNING task with managed worktree on disk
        let raw_plan = "# Step 1: Init\nInit project\n# Step 2: Core\nCore logic";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        let chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        assert_eq!(chunk.state, TaskState::Running);
        let managed_wt = temp_dir
            .join("worktrees")
            .join(&proj_id)
            .join(format!("task-{}", chunk.id));
        assert!(
            managed_wt.exists(),
            "managed task worktree should exist on disk after claim"
        );
        let steps_before = engine.list_masterplan_steps(&proj_id).unwrap();
        assert_eq!(steps_before.len(), 2);
        assert_eq!(
            steps_before
                .iter()
                .filter(|s| s.status == "CLAIMED")
                .count(),
            1,
            "chunk claim must leave exactly one CLAIMED masterplan step"
        );

        // arrange 2: scope lease + merge queue entry + ACTIVE agent run for the agent
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = engine.db.lock();
            conn.execute(
                "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at)
                 VALUES (?1, ?2, ?3, 'src/**', 'EXCLUSIVE_WRITE', ?4, ?4)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    chunk.id,
                    agent.id,
                    now
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at)
                 VALUES (?1, ?2, ?3, ?4, 'main', 1, 'READY', ?5, ?5, ?6)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    proj_id,
                    chunk.id,
                    chunk.branch_name,
                    chunk.base_sha,
                    now
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO agent_runs (id, task_id, agent_id, parent_run_id, role, prompt, status, started_at, prompt_tokens, completion_tokens)
                 VALUES (?1, ?2, ?3, NULL, 'Implementer', 'p', 'ACTIVE', ?4, 0, 0)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), chunk.id, agent.id, now],
            )
            .unwrap();
        }

        // act
        engine.unregister_agent(&agent.id).unwrap();

        // assert 1: agent + sessions deleted
        let agent_count: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM agents WHERE id = ?1",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(agent_count, 0, "agent row must be deleted");
        let session_count: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE agent_id = ?1",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(session_count, 0, "agent sessions must cascade-delete");

        // assert 2: task CANCELLED + stale, merge queue entry removed, worktree removed from disk
        let task_after = engine.get_task(&chunk.id).unwrap();
        assert_eq!(task_after.state, TaskState::Cancelled);
        assert!(task_after.is_stale);
        let mq_count: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM merge_queue WHERE task_id = ?1",
                [&chunk.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(mq_count, 0, "merge queue entry must be removed");
        assert!(
            !managed_wt.exists(),
            "managed task worktree must be removed from disk"
        );

        // assert 3: masterplan steps back to PENDING with claimed_agent_id NULL
        let steps_after = engine.list_masterplan_steps(&proj_id).unwrap();
        assert_eq!(steps_after.len(), 2);
        assert!(steps_after.iter().all(|s| s.status == "PENDING"));
        assert!(steps_after.iter().all(|s| s.claimed_agent_id.is_none()));

        // assert 4: scope_leases empty for that agent
        let lease_count: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM scope_leases WHERE agent_id = ?1",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(lease_count, 0, "scope leases must be released");

        // assert 5: no ACTIVE agent_runs rows remain for the agent
        let active_run_count: i64 = {
            let conn = engine.db.lock();
            conn.query_row(
                "SELECT COUNT(*) FROM agent_runs WHERE agent_id = ?1 AND status = 'ACTIVE'",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(
            active_run_count, 0,
            "ACTIVE agent runs must be terminated (no orphaned ACTIVE rows)"
        );
    }

    #[tokio::test]
    async fn test_stale_agent_tasks_are_recovered_by_sweep() {
        let (engine, proj_id, temp_dir) = setup_test_engine_with_worktree_root();
        let agent = engine.register_agent("Agent-Stale", "Coder").unwrap();

        // arrange: claimed masterplan chunk -> RUNNING task with managed worktree on disk
        let raw_plan = "# Step 1: Init\nInit project\n# Step 2: Core\nCore logic";
        engine.prepare_masterplan(&proj_id, raw_plan, 2, 2).unwrap();
        let chunk = engine
            .claim_masterplan_chunk(&proj_id, &agent.id, Some(1))
            .unwrap();
        assert_eq!(chunk.state, TaskState::Running);
        let managed_wt = temp_dir
            .join("worktrees")
            .join(&proj_id)
            .join(format!("task-{}", chunk.id));
        assert!(
            managed_wt.exists(),
            "managed task worktree should exist on disk after claim"
        );

        // backdate agents.last_heartbeat to 10 minutes ago (older than STALE_AGENT_GRACE)
        let stale_ts = (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2",
                rusqlite::params![stale_ts, agent.id],
            )
            .unwrap();
        }

        // act
        engine.stale_recovery_sweep().unwrap();

        // assert: task CANCELLED + stale
        let task_after = engine.get_task(&chunk.id).unwrap();
        assert_eq!(task_after.state, TaskState::Cancelled);
        assert!(task_after.is_stale, "recovered task must be marked stale");
        // assert: masterplan steps back to PENDING with claimed_agent_id NULL
        let steps_after = engine.list_masterplan_steps(&proj_id).unwrap();
        assert!(steps_after.iter().all(|s| s.status == "PENDING"));
        assert!(steps_after.iter().all(|s| s.claimed_agent_id.is_none()));
        // assert: managed worktree removed from disk
        assert!(
            !managed_wt.exists(),
            "managed task worktree must be removed from disk"
        );
        // assert: STALE_RECOVERY event was emitted for the recovered task
        let events = engine.get_events_after(0).unwrap();
        assert!(
            events.iter().any(
                |e| e.event_type == "STALE_RECOVERY" && e.task_id.as_deref() == Some(&chunk.id)
            ),
            "expected a STALE_RECOVERY event for the recovered task"
        );
    }

    #[tokio::test]
    async fn test_sweep_spares_active_agents_and_merge_ready_tasks() {
        let (engine, proj_id, _temp_dir) = setup_test_engine_with_worktree_root();
        let agent = engine.register_agent("Agent-Fresh", "Coder").unwrap();

        // arrange: a MERGE_READY task with no assignee (recent agent heartbeat stays fresh)
        let task = engine
            .create_task(
                &proj_id,
                "Merge ready task",
                "Already done, no assignee",
                "MEDIUM",
                vec![],
                vec!["works".to_string()],
            )
            .unwrap();
        drive_task_to_merge_ready(&engine, &task.id);
        assert_eq!(
            engine.get_task(&task.id).unwrap().state,
            TaskState::MergeReady
        );

        // act
        engine.stale_recovery_sweep().unwrap();

        // assert: nothing recovered; task state unchanged
        let task_after = engine.get_task(&task.id).unwrap();
        assert_eq!(task_after.state, TaskState::MergeReady);
        assert!(
            !task_after.is_stale,
            "MERGE_READY task must not be marked stale"
        );
        let agent_after = engine
            .list_agents()
            .unwrap()
            .into_iter()
            .find(|a| a.id == agent.id)
            .unwrap();
        assert_eq!(agent_after.status, "IDLE");
        let events = engine.get_events_after(0).unwrap();
        assert!(
            !events.iter().any(|e| e.event_type == "STALE_RECOVERY"),
            "no STALE_RECOVERY events should be emitted when nothing is stale"
        );
    }
}
