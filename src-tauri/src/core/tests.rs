#[cfg(test)]
#[allow(clippy::module_inception, clippy::bool_assert_comparison)]
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

        // Assign task to agent-1
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET assigned_agent_id = 'agent-1', state = 'RUNNING' WHERE id = ?1",
                [&task.id],
            )
            .unwrap();
        }

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
    fn test_proof_snapshot_historical_determinism() {
        let (engine, proj_id) = setup_test_engine();
        let proj = engine.get_project(&proj_id).unwrap();
        let head_sha = engine
            .git
            .get_head_sha(std::path::Path::new(&proj.path))
            .unwrap();

        let task_id = seed_proof_fixture(&engine, &proj_id);

        // Update fixture verification records to match actual repo HEAD
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE verification_runs SET commit_sha = ?1 WHERE task_id = ?2",
                rusqlite::params![head_sha, task_id],
            )
            .unwrap();
            conn.execute(
                "UPDATE evaluator_results SET commit_sha = ?1 WHERE task_id = ?2",
                rusqlite::params![head_sha, task_id],
            )
            .unwrap();
            conn.execute(
                "UPDATE tasks SET state = 'VERIFYING', branch_name = 'agentxflow/task-proof-digest', worktree_path = ?2 WHERE id = ?1",
                rusqlite::params![task_id, proj.path],
            ).unwrap();
        }

        let _bundle1 = engine
            .verify
            .generate_proof_bundle(
                &task_id,
                &proj_id,
                Some("agent-1"),
                "Test prompt",
                "base-sha-0",
                &head_sha,
                &["src/a.rs".to_string()],
                "+10 -2",
            )
            .unwrap();

        // Simulate later verification runs and evaluators added after proof P1 was generated
        {
            let conn = engine.db.lock();
            conn.execute(
                "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, source, executed_at, timed_out)
                 VALUES ('later-run-999', ?1, NULL, 'check-extra', 'Extra Check', ?2, 'npm run extra', 0, 'EXTRA STDOUT', '', 100, 1, 0, 'COORDINATOR_OBSERVED', '2026-08-31T20:00:00Z', 0)",
                rusqlite::params![task_id, head_sha],
            ).unwrap();
        }

        // Verify that the stored proof bundle verifies against its immutable snapshot
        let enqueue_res = engine.enqueue_task_by_id(&proj_id, &task_id);
        assert!(
            enqueue_res.is_ok(),
            "Enqueue must succeed using immutable snapshot proof verification even after later evidence is added: {:?}",
            enqueue_res.err()
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
            merge_events, 2,
            "expected exactly two MERGE_* lifecycle events (MERGE_STARTED + MERGE_BLOCKED_CONFLICT), got {}",
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

        // assert: the invalid BACKLOG -> BLOCKED write is rejected; the task is untouched but
        // MERGE_STARTED is emitted because the queue item did transition to RUNNING_CHECKS
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
            merge_events, 1,
            "expected exactly one MERGE_* event (MERGE_STARTED) for the rejected task-state transition, got {}",
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

    #[tokio::test]
    async fn test_merge_emits_lifecycle_events() {
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
                "Merge Lifecycle Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();
        let task_id = task.id.clone();
        let branch_name = format!("agentxflow/task-{}", task.id);

        drive_task_to_merge_ready(&engine, &task_id);

        // setup: clean repo with a trivial feature branch that merges cleanly
        git_safety::setup_repo_with_changes(&repo, false, false);
        std::fs::write(repo.join(".gitignore"), ".agentxflow/\n").unwrap();
        git_safety::git(&repo, &["add", ".gitignore"]);
        git_safety::git(&repo, &["commit", "-m", "baseline"]);
        let base_sha = git_safety::git(&repo, &["rev-parse", "main"]);

        git_safety::git(&repo, &["checkout", "-b", &branch_name]);
        std::fs::write(repo.join("feature.txt"), "feature").unwrap();
        git_safety::git(&repo, &["add", "feature.txt"]);
        git_safety::git(&repo, &["commit", "-m", "feature"]);
        let head_sha = git_safety::git(&repo, &["rev-parse", "HEAD"]);
        git_safety::git(&repo, &["checkout", "main"]);

        // record sequence before merge
        let seq_before: i64 = {
            let conn = engine.db.lock();
            conn.query_row("SELECT COALESCE(MAX(sequence), 0) FROM events", [], |r| {
                r.get(0)
            })
            .unwrap()
        };

        // act: enqueue and process the merge (success path)
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
        assert!(attempt.simulation_passed);

        // assert: MERGE_STARTED and MERGE_DONE both present in the events table
        let conn = engine.db.lock();
        let events: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT event_type FROM events WHERE task_id = ?1 AND sequence > ?2 ORDER BY sequence ASC",
                )
                .unwrap();
            stmt.query_map(rusqlite::params![task_id, seq_before], |r| r.get(0))
                .unwrap()
                .flatten()
                .collect()
        };
        drop(conn);

        assert!(
            events.contains(&"MERGE_STARTED".to_string()),
            "MERGE_STARTED must be emitted for a merge lifecycle transition, got: {:?}",
            events
        );
        assert!(
            events.contains(&"MERGE_DONE".to_string()),
            "MERGE_DONE must be emitted for a successful merge, got: {:?}",
            events
        );
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

    #[test]
    fn test_simultaneous_chunk_claims_select_disjoint_sets() {
        use std::sync::{Arc, Barrier};

        let (engine, proj_id) = setup_test_engine();
        let agent1 = engine.register_agent("Agent-Claim-1", "Coder").unwrap();
        let agent2 = engine.register_agent("Agent-Claim-2", "Coder").unwrap();

        // Prepare plan with 3 steps, each having a unique non-overlapping scope
        let raw_plan = "# Step 1: A\nDesc A\n# Step 2: B\nDesc B\n# Step 3: C\nDesc C";
        let plan = engine.prepare_masterplan(&proj_id, raw_plan, 3, 2).unwrap();

        // Assign unique scopes to avoid collision
        {
            let conn = engine.db.lock();
            let step_ids: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT id FROM masterplan_steps WHERE masterplan_id = ?1 ORDER BY step_index ASC")
                    .unwrap();
                stmt.query_map([&plan.masterplan.id], |r| r.get::<_, String>(0))
                    .unwrap()
                    .flatten()
                    .collect()
            };
            for (i, sid) in step_ids.iter().enumerate() {
                conn.execute(
                    "UPDATE masterplan_steps SET suggested_scope = ?1 WHERE id = ?2",
                    rusqlite::params![format!("src/step{}/**", i), sid],
                )
                .unwrap();
            }
        }

        let engine = Arc::new(engine);
        let barrier = Arc::new(Barrier::new(2));

        let engine1 = Arc::clone(&engine);
        let proj1 = proj_id.clone();
        let agent1_id = agent1.id.clone();
        let barrier1 = Arc::clone(&barrier);
        let t1 = std::thread::spawn(move || {
            barrier1.wait();
            engine1.claim_masterplan_chunk(&proj1, &agent1_id, Some(1))
        });

        let engine2 = Arc::clone(&engine);
        let proj2 = proj_id.clone();
        let agent2_id = agent2.id.clone();
        let barrier2 = Arc::clone(&barrier);
        let t2 = std::thread::spawn(move || {
            barrier2.wait();
            engine2.claim_masterplan_chunk(&proj2, &agent2_id, Some(1))
        });

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        assert!(r1.is_ok(), "First claim must succeed: {:?}", r1.err());
        assert!(r2.is_ok(), "Second claim must succeed: {:?}", r2.err());

        let task1 = r1.unwrap();
        let task2 = r2.unwrap();
        assert_ne!(
            task1.id, task2.id,
            "Claims must produce distinct tasks (no double-claim)"
        );

        // Verify the underlying masterplan steps are disjoint
        let steps = engine.list_masterplan_steps(&proj_id).unwrap();
        let claimed: Vec<_> = steps.iter().filter(|s| s.status == "CLAIMED").collect();
        assert_eq!(claimed.len(), 2, "Exactly two steps should be CLAIMED");
        assert_ne!(
            claimed[0].id, claimed[1].id,
            "Claimed steps must have different IDs"
        );

        // Now claim the remaining step
        let agent3 = engine.register_agent("Agent-Claim-3", "Coder").unwrap();
        let r3 = engine.claim_masterplan_chunk(&proj_id, &agent3.id, Some(1));
        assert!(r3.is_ok(), "Third claim must succeed (1 pending left)");

        // All 3 steps now claimed — requesting 1 more must fail
        let agent4 = engine.register_agent("Agent-Claim-4", "Coder").unwrap();
        let r4 = engine.claim_masterplan_chunk(&proj_id, &agent4.id, Some(1));
        assert!(r4.is_err(), "Claiming beyond available steps must fail");
    }

    #[test]
    fn test_reconcile_startup_recovers_applied_git_merge() {
        let (engine, proj_id) = setup_test_engine();
        let proj = engine.get_project(&proj_id).unwrap();
        let head_sha = engine
            .git
            .get_head_sha(std::path::Path::new(&proj.path))
            .unwrap();

        let task = engine
            .create_task(
                &proj_id,
                "Reconcile Merge Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();

        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE tasks SET state = 'MERGE_READY' WHERE id = ?1",
            [&task.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at)
             VALUES ('mq-applied-1', ?1, ?2, 'agentxflow/task-1', 'main', 1, 'RUNNING_CHECKS', 'base-1', ?3, ?4)",
            rusqlite::params![proj_id, task.id, head_sha, now],
        ).unwrap();
        drop(conn);

        // Run startup reconciliation
        engine.reconcile_on_startup();

        let conn = engine.db.lock();
        let queue_status: String = conn
            .query_row(
                "SELECT status FROM merge_queue WHERE id = 'mq-applied-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let task_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = ?1", [&task.id], |r| {
                r.get(0)
            })
            .unwrap();
        drop(conn);

        assert_eq!(
            queue_status, "MERGED",
            "Applied git merge must be finalized to MERGED in SQLite"
        );
        assert_eq!(
            task_state, "DONE",
            "Task state must be transitioned to DONE"
        );
    }

    #[test]
    fn test_reconcile_startup_resets_unapplied_merge_to_ready() {
        let (engine, proj_id) = setup_test_engine();
        let task = engine
            .create_task(
                &proj_id,
                "Unapplied Merge Task",
                "Desc",
                "HIGH",
                vec![],
                vec![],
            )
            .unwrap();

        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE tasks SET state = 'MERGE_READY' WHERE id = ?1",
            [&task.id],
        )
        .unwrap();
        // Insert with a non-existent commit sha (not in git target branch)
        conn.execute(
            "INSERT INTO merge_queue (id, project_id, task_id, branch_name, target_branch, position, status, base_sha, head_sha, queued_at)
             VALUES ('mq-unapplied-1', ?1, ?2, 'agentxflow/task-2', 'main', 1, 'RUNNING_CHECKS', 'base-1', 'deadbeef00000000000000000000000000000000', ?3)",
            rusqlite::params![proj_id, task.id, now],
        ).unwrap();
        drop(conn);

        // Run startup reconciliation
        engine.reconcile_on_startup();

        let conn = engine.db.lock();
        let queue_status: String = conn
            .query_row(
                "SELECT status FROM merge_queue WHERE id = 'mq-unapplied-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let task_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = ?1", [&task.id], |r| {
                r.get(0)
            })
            .unwrap();
        drop(conn);

        assert_eq!(
            queue_status, "READY",
            "Unapplied merge must be safely reset to READY"
        );
        assert_eq!(
            task_state, "MERGE_READY",
            "Task state remains MERGE_READY ready for next attempt"
        );
    }

    #[test]
    fn test_create_task_atomic_transaction() {
        let (engine, proj_id) = setup_test_engine();
        let steps = vec![
            ("Step 1".to_string(), "Desc 1".to_string(), true),
            ("Step 2".to_string(), "Desc 2".to_string(), false),
        ];
        let criteria = vec!["Criterion 1".to_string(), "Criterion 2".to_string()];

        let task = engine
            .create_task(
                &proj_id,
                "Atomic Task",
                "Atomic Desc",
                "HIGH",
                steps,
                criteria,
            )
            .unwrap();

        let conn = engine.db.lock();
        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_steps WHERE task_id = ?1",
                [&task.id],
                |r| r.get(0),
            )
            .unwrap();
        let crit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM acceptance_criteria WHERE task_id = ?1",
                [&task.id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        assert_eq!(
            step_count, 2,
            "Both task steps must be atomically persisted"
        );
        assert_eq!(
            crit_count, 2,
            "Both acceptance criteria must be atomically persisted"
        );
    }

    #[test]
    fn test_optional_verification_check_failure_does_not_block_submission() {
        let (engine, proj_id) = setup_test_engine();
        let proj = engine.get_project(&proj_id).unwrap();
        let head_sha = engine
            .git
            .get_head_sha(std::path::Path::new(&proj.path))
            .unwrap();

        let task = engine
            .create_task(
                &proj_id,
                "Contract Task",
                "Desc",
                "HIGH",
                vec![("Mandatory Step".to_string(), "".to_string(), true)],
                vec![],
            )
            .unwrap();

        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();
        // Mark mandatory step completed
        conn.execute(
            "UPDATE task_steps SET status = 'COMPLETED' WHERE task_id = ?1",
            [&task.id],
        )
        .unwrap();

        // Insert 1 required check config and 1 optional check config
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('chk-req', ?1, 'TYPECHECK', 'cargo check', '[]', 30, 1, ?2)",
            rusqlite::params![proj_id, now],
        ).unwrap();
        conn.execute(
            "INSERT INTO verification_profiles (id, project_id, check_type, command, args_json, timeout_secs, required, created_at)
             VALUES ('chk-opt', ?1, 'LINT_WARN', 'cargo clippy', '[]', 30, 0, ?2)",
            rusqlite::params![proj_id, now],
        ).unwrap();

        // Insert required check run as PASSED
        conn.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, source, executed_at, timed_out)
             VALUES ('run-req', ?1, NULL, 'chk-req', 'TYPECHECK', ?2, 'cargo check', 0, 'ok', '', 50, 1, 0, 'COORDINATOR_OBSERVED', ?3, 0)",
            rusqlite::params![task.id, head_sha, now],
        ).unwrap();

        // Insert optional check run as FAILED
        conn.execute(
            "INSERT INTO verification_runs (id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, source, executed_at, timed_out)
             VALUES ('run-opt', ?1, NULL, 'chk-opt', 'LINT_WARN', ?2, 'cargo clippy', 1, '', 'warning found', 50, 0, 0, 'COORDINATOR_OBSERVED', ?3, 0)",
            rusqlite::params![task.id, head_sha, now],
        ).unwrap();

        drop(conn);

        let outcome = engine
            .verify
            .verify_task_submission(&task.id, &head_sha)
            .unwrap();
        assert!(
            outcome.is_valid,
            "Submission must pass when optional check fails but all required checks pass: {:?}",
            outcome.rejection_reasons
        );
    }

    #[test]
    fn test_agent_heartbeat_atomic_renewal() {
        let (engine, proj_id) = setup_test_engine();
        let agent = engine.register_agent("Heartbeat Agent", "Coder").unwrap();

        // Create a task and assign to agent
        let task = engine
            .create_task(&proj_id, "HB Task", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
                rusqlite::params![agent.id, task.id],
            )
            .unwrap();
        }

        let leases = engine
            .scope
            .acquire_scope(
                &task.id,
                &agent.id,
                vec!["src/hb/**".to_string()],
                "EXCLUSIVE_WRITE",
            )
            .unwrap();
        assert_eq!(leases.len(), 1);
        let orig_expiry = leases[0].expires_at.clone();

        // Send heartbeat
        let hb_res = engine.agent_heartbeat(&agent.id);
        assert!(hb_res.is_ok(), "Heartbeat must succeed: {:?}", hb_res.err());

        let conn = engine.db.lock();
        let renewed_expiry: String = conn
            .query_row(
                "SELECT expires_at FROM scope_leases WHERE id = ?1",
                [&leases[0].id],
                |r| r.get(0),
            )
            .unwrap();
        let session_expiry: String = conn
            .query_row(
                "SELECT expires_at FROM agent_sessions WHERE agent_id = ?1",
                [&agent.id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        assert!(
            renewed_expiry > orig_expiry,
            "Heartbeat must advance scope lease expiry (orig: {}, renewed: {})",
            orig_expiry,
            renewed_expiry
        );
        assert!(!session_expiry.is_empty(), "Session expiry must be set");
    }

    #[test]
    fn test_scope_manager_acquire_scope_task_ownership_invariant() {
        let (engine, proj_id) = setup_test_engine();
        let agent_a = engine.register_agent("Agent Alpha", "IDE").unwrap();
        let agent_b = engine.register_agent("Agent Beta", "CLI").unwrap();

        // 1. Task owned by Agent A
        let task_a = engine
            .create_task(&proj_id, "Task Owned A", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
                rusqlite::params![agent_a.id, task_a.id],
            )
            .unwrap();
        }

        // 2. Unassigned task
        let task_unassigned = engine
            .create_task(&proj_id, "Task Unassigned", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        // Case 1: Agent A acquires scope for own task -> ALLOW
        let res_own = engine.scope.acquire_scope(
            &task_a.id,
            &agent_a.id,
            vec!["src/core/**".to_string()],
            "EXCLUSIVE_WRITE",
        );
        assert!(
            res_own.is_ok(),
            "Agent A must be allowed to acquire scope for own task: {:?}",
            res_own.err()
        );
        assert_eq!(res_own.unwrap().len(), 1);

        // Case 2: Agent B acquires scope for Agent A's task -> DENY
        let res_other = engine.scope.acquire_scope(
            &task_a.id,
            &agent_b.id,
            vec!["src/other/**".to_string()],
            "EXCLUSIVE_WRITE",
        );
        assert!(
            res_other.is_err(),
            "Agent B must be rejected from acquiring scope on Agent A's task"
        );
        assert!(res_other.unwrap_err().contains("Scope ownership violation"));

        // Case 3: Agent A acquires scope for unassigned task -> DENY
        let res_unassigned = engine.scope.acquire_scope(
            &task_unassigned.id,
            &agent_a.id,
            vec!["src/unassigned/**".to_string()],
            "EXCLUSIVE_WRITE",
        );
        assert!(
            res_unassigned.is_err(),
            "Agent must be rejected from acquiring scope on unassigned task"
        );
        assert!(res_unassigned.unwrap_err().contains("unassigned task"));

        // Case 4: Master ("master") acquires scope for unassigned task -> ALLOW
        let res_master = engine.scope.acquire_scope(
            &task_unassigned.id,
            "master",
            vec!["src/master/**".to_string()],
            "EXCLUSIVE_WRITE",
        );
        assert!(
            res_master.is_ok(),
            "Master must be allowed to acquire scope for unassigned task: {:?}",
            res_master.err()
        );
        assert_eq!(res_master.unwrap().len(), 1);
    }

    #[test]
    fn test_complete_step_ownership_domain_invariant() {
        let (engine, proj_id) = setup_test_engine();
        let agent_a = engine.register_agent("Agent Alpha", "IDE").unwrap();
        let agent_b = engine.register_agent("Agent Beta", "CLI").unwrap();

        // 1. Task assigned to Agent A
        let task_a = engine
            .create_task(
                &proj_id,
                "Task Assigned A",
                "Description",
                "HIGH",
                vec![("Step 1".to_string(), "Do Step 1".to_string(), true)],
                vec![],
            )
            .unwrap();

        // Assign task_a to agent_a
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
                rusqlite::params![agent_a.id, task_a.id],
            )
            .unwrap();
        }

        let task_a_details = engine.get_task_details(&task_a.id).unwrap();
        let step_a_id = &task_a_details.steps[0].id;

        // 2. Unassigned task
        let task_unassigned = engine
            .create_task(
                &proj_id,
                "Task Unassigned",
                "Description",
                "HIGH",
                vec![("Step Unassigned".to_string(), "Do Step".to_string(), true)],
                vec![],
            )
            .unwrap();
        let task_unassigned_details = engine.get_task_details(&task_unassigned.id).unwrap();
        let step_unassigned_id = &task_unassigned_details.steps[0].id;

        // Case 1: Assigned agent completes own step -> ALLOW
        let res_own = engine.complete_step(step_a_id, Some(&agent_a.id), Some("evidence-log"));
        assert!(
            res_own.is_ok(),
            "Assigned agent must be allowed to complete own step: {:?}",
            res_own.err()
        );

        // Case 2: Other agent completes step -> DENY
        let res_other = engine.complete_step(step_a_id, Some(&agent_b.id), Some("evidence-log"));
        assert!(
            res_other.is_err(),
            "Other agent must be rejected from completing another's step"
        );
        assert!(res_other.unwrap_err().contains("Step ownership violation"));

        // Case 3: Agent completes step on unassigned task -> DENY
        let res_unassigned =
            engine.complete_step(step_unassigned_id, Some(&agent_a.id), Some("evidence-log"));
        assert!(
            res_unassigned.is_err(),
            "Agent must be rejected from completing step on unassigned task"
        );
        assert!(res_unassigned.unwrap_err().contains("unassigned task"));

        // Case 4: Trusted system caller (agent_id = None) -> ALLOW
        let res_trusted = engine.complete_step(step_unassigned_id, None, Some("trusted-signoff"));
        assert!(
            res_trusted.is_ok(),
            "Trusted caller (agent_id: None) must be allowed to complete step: {:?}",
            res_trusted.err()
        );
    }

    #[test]
    fn test_claim_task_atomic_dependency_gate() {
        let (engine, proj_id, _) = setup_test_engine_with_worktree_root();
        let agent = engine.register_agent("Claim Agent", "IDE").unwrap();

        let t1 = engine
            .create_task(&proj_id, "T1 Prerequisite", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        let t2 = engine
            .create_task(&proj_id, "T2 Dependent", "Desc", "HIGH", vec![], vec![])
            .unwrap();

        // Add blocking dependency: T2 depends on T1
        engine.dag.add_dependency(&t2.id, &t1.id, "BLOCKS").unwrap();

        // Attempt claim of T2 while T1 is BACKLOG -> must fail
        let claim_blocked = engine.claim_task(&t2.id, &agent.id);
        assert!(
            claim_blocked.is_err(),
            "Claiming task with unsatisfied dependency must fail"
        );
        assert!(claim_blocked
            .unwrap_err()
            .contains("Prerequisite dependencies are not yet DONE"));

        // Transition T1 to DONE
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'DONE', substate = 'NONE' WHERE id = ?1",
                rusqlite::params![t1.id],
            )
            .unwrap();
        }

        // Attempt claim of T2 now -> must succeed
        let claim_allowed = engine.claim_task(&t2.id, &agent.id);
        assert!(
            claim_allowed.is_ok(),
            "Claiming task after dependencies are satisfied must succeed: {:?}",
            claim_allowed.err()
        );
        assert_eq!(claim_allowed.unwrap().state, TaskState::Running);
    }

    #[test]
    fn test_register_agent_first_registration() {
        let (engine, _) = setup_test_engine();
        let agent = engine.register_agent("New Worker", "CLI").unwrap();

        assert_eq!(agent.name, "New Worker");
        assert_eq!(agent.status, "IDLE");
        assert_eq!(agent.active_task_id, None);
        assert_eq!(agent.active_task_title, None);
        assert!(agent.session_token.is_some());
    }

    #[test]
    fn test_register_agent_reconnect_preserves_persisted_state_and_active_task() {
        let (engine, proj_id) = setup_test_engine();
        let initial_agent = engine.register_agent("Persistent Worker", "CLI").unwrap();

        // Create a task and assign it to the agent
        let task = engine
            .create_task(&proj_id, "In-Flight Work", "Desc", "HIGH", vec![], vec![])
            .unwrap();
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE tasks SET assigned_agent_id = ?1, state = 'RUNNING' WHERE id = ?2",
                rusqlite::params![initial_agent.id, task.id],
            )
            .unwrap();
            // Set past created_at to verify it is not fabricated on re-registration
            conn.execute(
                "UPDATE agents SET created_at = '2026-01-01T00:00:00Z', status = 'WORKING' WHERE id = ?1",
                [&initial_agent.id],
            )
            .unwrap();
        }

        // Re-register the exact same canonical agent (e.g. reconnect)
        let reconnected = engine.register_agent("Persistent Worker", "CLI").unwrap();

        // Authoritative verification: returned Agent reflects actual persisted and active state
        assert_eq!(reconnected.id, initial_agent.id);
        assert_eq!(
            reconnected.created_at, "2026-01-01T00:00:00Z",
            "Registration must preserve original creation timestamp"
        );
        assert_eq!(
            reconnected.status, "WORKING",
            "Status must reflect active task state rather than fabricated IDLE"
        );
        assert_eq!(
            reconnected.active_task_id,
            Some(task.id),
            "Active task ID must be populated from database"
        );
        assert_eq!(
            reconnected.active_task_title,
            Some("In-Flight Work".to_string()),
            "Active task title must match"
        );
        assert!(
            reconnected.session_token.is_some(),
            "Session token must be refreshed on registration"
        );
    }

    #[test]
    fn test_session_lifetime_and_expiration_gating() {
        let (engine, _) = setup_test_engine();
        let agent = engine.register_agent("Session Agent", "IDE").unwrap();
        let session_token = agent.session_token.expect("Session token must exist");

        // 1. Verify initial session expiry is ~365 days in the future
        let conn = engine.db.lock();
        let initial_expiry: String = conn
            .query_row(
                "SELECT expires_at FROM agent_sessions WHERE session_token = ?1",
                [&session_token],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        let initial_dt = chrono::DateTime::parse_from_rfc3339(&initial_expiry).unwrap();
        let now = chrono::Utc::now();
        let days_diff = (initial_dt.signed_duration_since(now)).num_days();
        assert!(
            (360..=366).contains(&days_diff),
            "Initial session expiry must be approximately 365 days (got {} days)",
            days_diff
        );

        // 2. Heartbeat preserves long-lived initial window without truncating it
        engine.agent_heartbeat(&agent.id).unwrap();
        let conn = engine.db.lock();
        let post_hb_expiry: String = conn
            .query_row(
                "SELECT expires_at FROM agent_sessions WHERE session_token = ?1",
                [&session_token],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert_eq!(
            post_hb_expiry, initial_expiry,
            "Heartbeat must not shorten a 365-day initial window"
        );

        // 3. Valid session lookup succeeds
        let lookup = engine.get_agent_by_session(&session_token);
        assert!(lookup.is_some(), "Valid session token must return Agent");
        assert_eq!(lookup.unwrap().id, agent.id);

        // 4. Expired session is strictly rejected
        {
            let conn = engine.db.lock();
            conn.execute(
                "UPDATE agent_sessions SET expires_at = '2020-01-01T00:00:00Z' WHERE session_token = ?1",
                [&session_token],
            )
            .unwrap();
        }
        let expired_lookup = engine.get_agent_by_session(&session_token);
        assert!(
            expired_lookup.is_none(),
            "Expired session must be rejected by get_agent_by_session"
        );

        // 5. Re-registering establishes a fresh valid session
        let re_registered = engine.register_agent("Session Agent", "IDE").unwrap();
        let new_token = re_registered.session_token.expect("New session token");
        assert_ne!(new_token, session_token);
        let valid_lookup = engine.get_agent_by_session(&new_token);
        assert!(
            valid_lookup.is_some(),
            "Re-registered agent must have valid session"
        );
    }

    #[test]
    fn test_create_project_atomicity() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_proj_atom_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = DbPool::new_in_memory().unwrap();
        let engine = CoordinatorEngine::new(pool.clone());

        let proj = engine
            .create_project(
                "Atomic Proj",
                temp_dir.to_str().unwrap(),
                "Master Spec Content",
                "main",
            )
            .unwrap();

        // Verify all 3 records were created atomically
        let conn = pool.lock();
        let proj_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                [&proj.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(proj_count, 1);

        let contract_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project_contracts WHERE project_id = ?1",
                [&proj.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(contract_count, 1);

        let rule_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project_rules WHERE project_id = ?1",
                [&proj.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rule_count, 1);
        drop(conn);

        let _ = crate::git::GitService::safe_remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_reset_masterplan_cross_project_rejection() {
        let (engine, p1_id) = setup_test_engine();
        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Create a second project
        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p2', 'P2', 'dummy/path2', 'Spec2', 'main', ?1, ?1)",
            [&now],
        ).unwrap();

        // 2. Create a masterplan belonging to Project 1
        conn.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, is_active, created_at, updated_at) VALUES ('mp_p1', ?1, 'P1 Plan', 'Raw plan', 'ACTIVE', 3, 2, 1, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s1', 'mp_p1', 0, 'Step 1', 'Desc', '[]', '[]', 'PENDING', ?1, ?1)",
            [&now],
        ).unwrap();
        drop(conn);

        // 3. Project 2 attempts to reset Project 1's masterplan -> REJECTED
        let cross_reset_res = engine.reset_masterplan("p2", Some("mp_p1"));
        assert!(
            cross_reset_res.is_err(),
            "Resetting a masterplan from another project must be rejected"
        );
        assert!(cross_reset_res.unwrap_err().contains("belongs to project"));

        // Verify Masterplan 1 and its steps still exist untouched
        let conn = engine.db.lock();
        let mp_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mp_count, 1, "Masterplan in P1 must not be deleted");

        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = 'mp_p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_count, 1, "Masterplan step in P1 must not be deleted");
        drop(conn);

        // 4. Nonexistent masterplan -> REJECTED
        let nonexist_res = engine.reset_masterplan(&p1_id, Some("mp_nonexistent"));
        assert!(
            nonexist_res.is_err(),
            "Resetting a nonexistent masterplan must return error"
        );
        assert!(nonexist_res.unwrap_err().contains("not found"));

        // 5. Resetting with correct project + masterplan -> ALLOWED
        let valid_reset_res = engine.reset_masterplan(&p1_id, Some("mp_p1"));
        assert!(
            valid_reset_res.is_ok(),
            "Resetting with matching project and masterplan must succeed: {:?}",
            valid_reset_res.err()
        );

        let conn = engine.db.lock();
        let mp_count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            mp_count_after, 0,
            "Masterplan in P1 must be deleted on valid reset"
        );
    }

    #[test]
    fn test_create_project_atomicity_rollback_on_failure() {
        let pool = DbPool::new_in_memory().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let target_proj_id = "p_atom_fail";

        let mut conn = pool.lock();
        let tx_res = (|| -> Result<(), String> {
            let tx = conn
                .transaction()
                .map_err(|e| format!("Failed to begin transaction: {}", e))?;

            // 1. Insert project
            tx.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES (?1, 'Fail Proj', 'dummy/path', 'Spec', 'main', ?2, ?2)",
                rusqlite::params![target_proj_id, now],
            ).map_err(|e| e.to_string())?;

            // 2. Insert contract
            tx.execute(
                "INSERT INTO project_contracts (id, project_id, version, overview, architecture, rules_json, commands_json, testing_json, repo_map, security_constraints, contract_hash, created_at) VALUES ('c1', ?1, 1, 'Spec', 'Arch', '[]', '[]', '[]', '', '[]', 'hash', ?2)",
                rusqlite::params![target_proj_id, now],
            ).map_err(|e| e.to_string())?;

            // 3. Force child insert error with invalid foreign key
            let err_res: Result<usize, _> = tx.execute(
                "INSERT INTO project_rules (id, project_id, category, rule_text, strictness, created_at) VALUES ('r1', 'nonexistent_project_id', 'SYSTEM', 'Rule', 'MANDATORY', ?1)",
                rusqlite::params![now],
            );
            if let Err(e) = err_res {
                return Err(format!("Child insert failed as expected: {}", e));
            }

            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        })();

        assert!(tx_res.is_err(), "Operation must fail");
        drop(conn);

        // Verify that 0 project, contract, or rule rows exist for target_proj_id
        let conn = pool.lock();
        let proj_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                [target_proj_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            proj_count, 0,
            "Projects table must have 0 rows after rollback"
        );

        let contract_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project_contracts WHERE project_id = ?1",
                [target_proj_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            contract_count, 0,
            "Contracts table must have 0 rows after rollback"
        );

        let rule_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project_rules WHERE project_id = ?1",
                [target_proj_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rule_count, 0, "Rules table must have 0 rows after rollback");
    }

    #[test]
    fn test_reset_masterplan_same_project_cross_masterplan_isolation() {
        let (engine, p1_id) = setup_test_engine();
        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Create Masterplan A and Masterplan B in the same project P1
        conn.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, is_active, created_at, updated_at) VALUES ('mp_a', ?1, 'Plan A', 'Raw A', 'ACTIVE', 3, 2, 1, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s_a1', 'mp_a', 0, 'Step A1', 'Desc', '[]', '[]', 'CLAIMED', ?1, ?1)",
            [&now],
        ).unwrap();

        conn.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, is_active, created_at, updated_at) VALUES ('mp_b', ?1, 'Plan B', 'Raw B', 'ACTIVE', 3, 2, 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s_b1', 'mp_b', 0, 'Step B1', 'Desc', '[]', '[]', 'CLAIMED', ?1, ?1)",
            [&now],
        ).unwrap();

        // 2. Create Task A1 belonging to Plan A and Task B1 belonging to Plan B
        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_a1', ?1, 'mp_a', 'Task A1', 'Desc A1', 'RUNNING', 'EXECUTING', 'HIGH', 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_b1', ?1, 'mp_b', 'Task B1', 'Desc B1', 'RUNNING', 'EXECUTING', 'HIGH', 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        drop(conn);

        // 3. Reset ONLY Masterplan A
        let reset_res = engine.reset_masterplan(&p1_id, Some("mp_a"));
        assert!(
            reset_res.is_ok(),
            "Resetting masterplan A must succeed: {:?}",
            reset_res.err()
        );

        // 4. Verify Plan A and its steps are deleted, and Task A1 is cancelled
        let conn = engine.db.lock();
        let mp_a_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mp_a_count, 0, "Masterplan A must be deleted");

        let step_a_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = 'mp_a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_a_count, 0, "Masterplan A steps must be deleted");

        let t_a1_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = 't_a1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(t_a1_state, "CANCELLED", "Task A1 must be cancelled");

        // 5. Verify Masterplan B, its steps, and Task B1 remain COMPLETELY UNTOUCHED and RUNNING
        let mp_b_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mp_b_count, 1, "Masterplan B must remain intact");

        let step_b_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = 'mp_b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_b_count, 1, "Masterplan B steps must remain intact");

        let t_b1_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = 't_b1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            t_b1_state, "RUNNING",
            "Task B1 must NOT be cancelled (must stay RUNNING)"
        );
    }

    #[test]
    fn test_reset_masterplan_aborts_and_preserves_state_on_cancellation_failure() {
        let (engine, p1_id) = setup_test_engine();
        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Create Masterplan A with Task A1 in project P1
        conn.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, is_active, created_at, updated_at) VALUES ('mp_a_fail', ?1, 'Plan A', 'Raw A', 'ACTIVE', 3, 2, 1, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s_a1_fail', 'mp_a_fail', 0, 'Step A1', 'Desc', '[]', '[]', 'CLAIMED', ?1, ?1)",
            [&now],
        ).unwrap();

        // Task A1 is marked DONE (cannot be cancelled -> cancel_task returns Err)
        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_a1_done', ?1, 'mp_a_fail', 'Task A1', 'Desc A1', 'DONE', 'READY_FOR_MERGE', 'HIGH', 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        drop(conn);

        // 2. Call reset_masterplan on mp_a_fail
        let reset_res = engine.reset_masterplan(&p1_id, Some("mp_a_fail"));
        assert!(
            reset_res.is_err(),
            "Reset must fail when task cancellation fails"
        );
        let err = reset_res.unwrap_err();
        assert!(
            err.contains("failed to cancel active task") || err.contains("Preflight rejection"),
            "Error must describe cancellation failure: {}",
            err
        );

        // 3. Verify Masterplan A and its steps STILL EXIST in the database (destructive cleanup aborted)
        let conn = engine.db.lock();
        let mp_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_a_fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            mp_count, 1,
            "Masterplan must NOT be deleted when cancellation fails"
        );

        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = 'mp_a_fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            step_count, 1,
            "Masterplan steps must NOT be deleted when cancellation fails"
        );
    }

    #[test]
    fn test_reset_masterplan_preflight_aborts_without_mutating_any_task() {
        let (engine, p1_id) = setup_test_engine();
        let conn = engine.db.lock();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Create Masterplan with two tasks: Task A (RUNNING, cancellable) and Task B (DONE, non-cancellable)
        conn.execute(
            "INSERT INTO masterplans (id, project_id, title, raw_text, status, target_step_count, max_steps_per_agent, is_active, created_at, updated_at) VALUES ('mp_preflight', ?1, 'Preflight Plan', 'Raw', 'ACTIVE', 2, 2, 1, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s_pf1', 'mp_preflight', 0, 'Step 1', 'Desc', '[]', '[]', 'CLAIMED', ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at) VALUES ('s_pf2', 'mp_preflight', 1, 'Step 2', 'Desc', '[]', '[]', 'CLAIMED', ?1, ?1)",
            [&now],
        ).unwrap();

        // Task A: RUNNING
        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_pf_a', ?1, 'mp_preflight', 'Task A', 'Desc A', 'RUNNING', 'EXECUTING', 'HIGH', 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        // Task B: DONE (non-cancellable)
        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_pf_b', ?1, 'mp_preflight', 'Task B', 'Desc B', 'DONE', 'READY_FOR_MERGE', 'HIGH', 0, ?2, ?2)",
            [&p1_id, &now],
        ).unwrap();
        drop(conn);

        // 2. Execute reset_masterplan
        let res = engine.reset_masterplan(&p1_id, Some("mp_preflight"));
        assert!(res.is_err(), "Reset must fail during preflight");
        let err = res.unwrap_err();
        assert!(
            err.contains("Preflight rejection") || err.contains("Cannot cancel task"),
            "Error must identify preflight rejection: {}",
            err
        );

        // 3. Prove that Task A was NOT cancelled and remains in RUNNING state
        let conn = engine.db.lock();
        let task_a_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = 't_pf_a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            task_a_state, "RUNNING",
            "Preflight failure must abort before cancelling Task A"
        );

        let task_b_state: String = conn
            .query_row("SELECT state FROM tasks WHERE id = 't_pf_b'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(task_b_state, "DONE", "Task B must remain in DONE state");

        // 4. Prove masterplan and steps still exist
        let mp_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplans WHERE id = 'mp_preflight'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mp_count, 1, "Masterplan must not be deleted");

        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = 'mp_preflight'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_count, 2, "Masterplan steps must not be deleted");
    }

    #[test]
    fn test_get_events_after_success_and_error_handling() {
        let (engine, p1_id) = setup_test_engine();
        engine.emit_event(
            Some(&p1_id),
            None,
            None,
            "CUSTOM_TEST_EVENT",
            serde_json::json!({ "key": "value" }),
        );

        let events = engine.get_events_after(0).unwrap();
        assert!(!events.is_empty(), "Must return emitted events");
        let found = events.iter().any(|e| e.event_type == "CUSTOM_TEST_EVENT");
        assert!(found, "Emitted event must be present in get_events_after");
    }
}
