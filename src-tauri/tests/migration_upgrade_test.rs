#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::db::migrations::{run_migrations, verify_schema_integrity};
use rusqlite::Connection;

#[test]
fn test_legacy_database_migration_upgrade() {
    let temp_db_path =
        std::env::temp_dir().join(format!("legacy_test_{}.sqlite", uuid::Uuid::new_v4()));
    let mut conn = Connection::open(&temp_db_path).expect("Failed to open SQLite db");

    // 1. Create legacy schema (as it existed in older beta releases)
    conn.execute_batch(
        "
        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            master_spec TEXT NOT NULL,
            target_branch TEXT NOT NULL DEFAULT 'main',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_id TEXT,
            epic_id TEXT,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'BACKLOG',
            substate TEXT NOT NULL DEFAULT 'NONE',
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            assigned_agent_id TEXT,
            assigned_profile_id TEXT,
            allocated_budget_usd REAL,
            spent_budget_usd REAL DEFAULT 0.0,
            worktree_path TEXT,
            branch_name TEXT,
            base_sha TEXT,
            head_sha TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            agent_type TEXT NOT NULL,
            profile TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'IDLE',
            last_heartbeat TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        -- Old obsolete agent_sessions table schema
        CREATE TABLE agent_sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            task_id TEXT,
            token TEXT NOT NULL UNIQUE,
            connected_at TEXT NOT NULL,
            expires_at TEXT NOT NULL
        );
        ",
    )
    .expect("Failed to create legacy tables");

    // 2. Insert existing project, task, and agent data
    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
         VALUES ('proj-legacy-1', 'Legacy App', '/tmp/legacy', 'Build spec', 'main', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, created_at, updated_at)
         VALUES ('task-legacy-1', 'proj-legacy-1', 'Legacy Task', 'Fix bugs', 'BACKLOG', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at)
         VALUES ('agent-legacy-1', 'Legacy Agent', 'Antigravity', 'Lead', 'IDLE', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    ).unwrap();

    // 3. Run versioned migrations against legacy database
    run_migrations(&mut conn).expect("Migrations must succeed on legacy database");

    // 4. Verify existing project, task, and agent data are preserved
    let proj_name: String = conn
        .query_row(
            "SELECT name FROM projects WHERE id = 'proj-legacy-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(proj_name, "Legacy App");

    let task_title: String = conn
        .query_row(
            "SELECT title FROM tasks WHERE id = 'task-legacy-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_title, "Legacy Task");

    let agent_name: String = conn
        .query_row(
            "SELECT name FROM agents WHERE id = 'agent-legacy-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(agent_name, "Legacy Agent");

    // 5. Verify migrations tracker has recorded versions
    let applied_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM _schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert!(
        applied_count >= 4,
        "All versioned migrations must be recorded as applied"
    );

    // 6. Verify agent_sessions has new session_token schema and works cleanly
    conn.execute(
        "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
         VALUES ('sess-1', 'agent-legacy-1', 'axf_sess_test123', '2026-08-14T00:00:00Z', '2026-08-15T00:00:00Z', '2026-08-14T00:00:00Z')",
        [],
    ).expect("Inserting session into upgraded agent_sessions table must succeed");

    let retrieved_token: String = conn
        .query_row(
            "SELECT session_token FROM agent_sessions WHERE id = 'sess-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retrieved_token, "axf_sess_test123");

    // 7. Verify migrations v12–v16 schema changes
    verify_v12_to_v16(&conn);

    // 8. Cleanup
    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

/// Verifies that migrations v12–v16 each applied correctly against a fully
/// upgraded database. Called from the legacy-database upgrade test so that a
/// break in any later migration is caught even if no dedicated per-migration
/// test exists yet.
fn verify_v12_to_v16(conn: &Connection) {
    // ── v12: timed_out column on verification_runs ──────────────────────
    let vr_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(verification_runs)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        vr_cols.contains(&"timed_out".to_string()),
        "v12 must add timed_out column to verification_runs"
    );

    // ── v13: CHECK constraint on tasks.state ────────────────────────────
    let err = conn
        .execute(
            "INSERT INTO tasks (id, project_id, title, description, state, created_at, updated_at)
             VALUES ('task-exp', 'proj-legacy-1', 'Bad', 'Bad', 'EXPLODED', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("CHECK"),
        "v13 CHECK must reject illegal state values, got: {}",
        err
    );

    // ── v14: proof_bundles allows duplicate proof_hash (append-only) ────
    conn.execute(
        "INSERT INTO proof_bundles (id, task_id, project_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
         VALUES ('pb-dup-1', 'task-legacy-1', 'proj-legacy-1', 1, '', '', '', '[]', '', '[]', '[]', '[]', 'dup-hash', '2026-01-01T00:00:00Z')",
        [],
    )
    .expect("v14 must allow inserting into proof_bundles");
    conn.execute(
        "INSERT INTO proof_bundles (id, task_id, project_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
         VALUES ('pb-dup-2', 'task-legacy-1', 'proj-legacy-1', 1, '', '', '', '[]', '', '[]', '[]', '[]', 'dup-hash', '2026-01-01T00:01:00Z')",
        [],
    )
    .expect("v14 must allow duplicate proof_hash (append-only semantics)");

    // ── v15: masterplan_operations table exists and accepts rows ─────────
    let mo_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'masterplan_operations'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(mo_exists, "v15 must create masterplan_operations table");
    conn.execute(
        "INSERT INTO masterplan_operations (idempotency_key, masterplan_id, result_json, created_at)
         VALUES ('test-key', 'mp-1', '{}', '2026-01-01T00:00:00Z')",
        [],
    )
    .expect("v15 masterplan_operations must accept rows");

    // ── v16: session tokens rotated to fresh random values ──────────────
    let token: String = conn
        .query_row(
            "SELECT session_token FROM agents WHERE id = 'agent-legacy-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        token.starts_with("axf_sess_") && token.len() > 20,
        "v16 must rotate session tokens to unpredictable axf_sess_ values, got: {}",
        token
    );

    // ── v17: request_hash column on masterplan_operations ────────────────
    let mo_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(masterplan_operations)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        mo_cols.contains(&"request_hash".to_string()),
        "v17 must add request_hash column to masterplan_operations"
    );
}

#[test]
fn test_migration_0005_with_partially_existing_tables() {
    let temp_db_path =
        std::env::temp_dir().join(format!("test_partial_0005_{}.sqlite", uuid::Uuid::new_v4()));
    let mut conn = Connection::open(&temp_db_path).expect("Failed to open SQLite db");

    // Simulate an existing database where task_attempts was previously created without attempt_number
    conn.execute_batch(
        "
        CREATE TABLE _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES (1, 'core_entities', '2026-01-01');
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES (2, 'agent_sessions_and_token_rotation', '2026-01-01');
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES (3, 'evidence_proofs_and_revisions', '2026-01-01');
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES (4, 'claim_and_merge_metadata', '2026-01-01');

        CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL UNIQUE, master_spec TEXT NOT NULL, target_branch TEXT NOT NULL DEFAULT 'main', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE tasks (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, title TEXT NOT NULL, description TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'BACKLOG', substate TEXT NOT NULL DEFAULT 'NONE', priority TEXT NOT NULL DEFAULT 'MEDIUM', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE agents (id TEXT PRIMARY KEY, name TEXT NOT NULL, agent_type TEXT NOT NULL, profile TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'IDLE', last_heartbeat TEXT NOT NULL, created_at TEXT NOT NULL);
        CREATE TABLE scope_violations (id TEXT PRIMARY KEY, task_id TEXT NOT NULL, file_path TEXT NOT NULL, detected_at TEXT NOT NULL);
        CREATE TABLE proof_bundles (id TEXT PRIMARY KEY, task_id TEXT NOT NULL, project_id TEXT NOT NULL, proof_hash TEXT NOT NULL, created_at TEXT NOT NULL);

        -- Existing older task_attempts table without attempt_number
        CREATE TABLE task_attempts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );
        "
    ).unwrap();

    // Run migrations - must succeed and add attempt_number column and create index
    run_migrations(&mut conn)
        .expect("Migration 0005 must handle existing task_attempts table without panicking");

    // Verify attempt_number column exists and can be queried
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // Insert into upgraded task_attempts with attempt_number
    conn.execute(
        "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, base_sha, status, started_at)
         VALUES ('att-1', 't-1', 'ag-1', 1, 'base_sha', 'ACTIVE', '2026-08-14T00:00:00Z')",
        [],
    ).unwrap();

    let att_num: i64 = conn
        .query_row(
            "SELECT attempt_number FROM task_attempts WHERE id = 'att-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(att_num, 1);

    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

#[test]
fn test_v13_rebuild_preserves_tasks_and_child_rows() {
    let temp_db_path =
        std::env::temp_dir().join(format!("test_v13_rebuild_{}.sqlite", uuid::Uuid::new_v4()));
    let mut conn = Connection::open(&temp_db_path).expect("Failed to open SQLite db");

    // Simulate a pre-v13 database (migrations 1-12 already applied) with rows in
    // `tasks` and in child tables referencing tasks(id). The v13 rebuild must
    // preserve every row and install the CHECK constraint on tasks.state.
    conn.execute_batch(
        "
        CREATE TABLE _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES
            (1, 'core_control_plane_entities', '2026-01-01'),
            (2, 'authoritative_session_security', '2026-01-01'),
            (3, 'immutable_proofs_and_masterplan_revisions', '2026-01-01'),
            (4, 'crash_safe_claim_and_merge_metadata', '2026-01-01'),
            (5, 'task_attempts_and_machine_evaluators', '2026-01-01'),
            (6, 'task_masterplan_lifecycle_and_stale_invalidation', '2026-01-01'),
            (7, 'normalize_proof_bundles_task_attempts_and_evaluators', '2026-01-01'),
            (8, 'task_attempt_worktree_paths', '2026-01-01'),
            (9, 'seed_canonical_ide_profiles_and_cleanup', '2026-01-01'),
            (10, 'masterplan_milestone_approval_toggle', '2026-01-01'),
            (11, 'multiple_masterplans_and_active_toggle', '2026-01-01'),
            (12, 'timeout_evidence', '2026-01-01');

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            master_spec TEXT NOT NULL,
            target_branch TEXT NOT NULL DEFAULT 'main',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_id TEXT,
            epic_id TEXT,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'BACKLOG',
            substate TEXT NOT NULL DEFAULT 'NONE',
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            risk_score REAL DEFAULT 0.0,
            estimated_scope TEXT,
            assigned_agent_id TEXT,
            assigned_profile_id TEXT,
            allocated_budget_usd REAL,
            spent_budget_usd REAL DEFAULT 0.0,
            worktree_path TEXT,
            branch_name TEXT,
            base_sha TEXT,
            head_sha TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            masterplan_id TEXT,
            masterplan_revision_id TEXT,
            is_stale BOOLEAN NOT NULL DEFAULT 0,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(parent_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        CREATE TABLE task_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            order_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            is_mandatory BOOLEAN NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'PENDING',
            completed_at TEXT,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        CREATE TABLE evidence_records (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            step_id TEXT,
            evidence_type TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'AGENT_REPORTED',
            payload_json TEXT NOT NULL,
            recorded_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Minimal versions of the tables checked by verify_schema_integrity
        CREATE TABLE proof_bundles (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            verification_runs_json TEXT NOT NULL DEFAULT '[]',
            criteria_json TEXT NOT NULL DEFAULT '[]',
            steps_json TEXT NOT NULL DEFAULT '[]',
            proof_hash TEXT NOT NULL,
            generated_at TEXT NOT NULL
        );
        CREATE TABLE task_attempts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            run_number INTEGER NOT NULL DEFAULT 1,
            worktree_path TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );
        CREATE TABLE evaluator_results (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL,
            evaluator_name TEXT NOT NULL,
            passed BOOLEAN NOT NULL,
            evaluated_at TEXT NOT NULL
        );
        ",
    )
    .unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
         VALUES ('proj-13', 'V13 App', '/tmp/v13', 'Spec', 'main', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    // One row in a state the transition primitive never writes ('CLAIMING') and one
    // in a legacy alias state ('WORKING'): both must survive the rebuild.
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, created_at, updated_at)
         VALUES ('task-13-a', 'proj-13', 'Claiming Task', 'Desc', 'CLAIMING', 'CLAIMING', 'HIGH', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                ('task-13-b', 'proj-13', 'Legacy State Task', 'Desc', 'WORKING', 'NONE', 'MEDIUM', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    // Child rows that would be cascade-deleted by a naive DROP TABLE rebuild
    conn.execute(
        "INSERT INTO task_steps (id, task_id, order_index, title, description)
         VALUES ('step-13-a', 'task-13-a', 1, 'Step 1', 'Desc')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO evidence_records (id, task_id, evidence_type, payload_json, recorded_at)
         VALUES ('ev-13-a', 'task-13-a', 'TEST_PASS', '{}', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    // 3. Run migrations: only v13 is pending; it must rebuild `tasks` and preserve rows
    run_migrations(&mut conn).expect("Migration v13 must succeed on a pre-v13 database");

    // 4. Verify task rows survive with their exact state values
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(task_count, 2, "All task rows must survive the v13 rebuild");

    let state_a: String = conn
        .query_row("SELECT state FROM tasks WHERE id = 'task-13-a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state_a, "CLAIMING");
    let state_b: String = conn
        .query_row("SELECT state FROM tasks WHERE id = 'task-13-b'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state_b, "WORKING");

    // 5. Verify child rows survive (the rebuild must not cascade-delete them)
    let step_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_steps", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        step_count, 1,
        "task_steps rows must survive the v13 rebuild"
    );
    let ev_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM evidence_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        ev_count, 1,
        "evidence_records rows must survive the v13 rebuild"
    );

    // 6. Verify the CHECK constraint is enforced and indexes were recreated
    let err = conn
        .execute(
            "UPDATE tasks SET state = 'EXPLODED' WHERE id = 'task-13-a'",
            [],
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("CHECK"),
        "Unknown state write must fail with CHECK constraint, got: {}",
        err
    );

    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'tasks' AND name IN ('idx_tasks_project_state', 'idx_tasks_project_masterplan', 'idx_tasks_state_stale')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        idx_count, 3,
        "All tasks indexes must be recreated after the rebuild"
    );

    // 7. Legal state writes must still succeed after the rebuild
    conn.execute(
        "UPDATE tasks SET state = 'REVIEW' WHERE id = 'task-13-a'",
        [],
    )
    .unwrap();

    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

#[test]
fn test_v14_rebuild_makes_proof_bundles_append_only() {
    let temp_db_path =
        std::env::temp_dir().join(format!("test_v14_rebuild_{}.sqlite", uuid::Uuid::new_v4()));
    let mut conn = Connection::open(&temp_db_path).expect("Failed to open SQLite db");

    // Simulate a pre-v14 database (migrations 1-13 already applied) with rows in
    // `proof_bundles`. The pre-v14 table carries the UNIQUE constraint on proof_hash
    // from migration 0003 (plus the attempt_id column added by 0005/0007). The v14
    // rebuild must preserve every row and drop the UNIQUE constraint so duplicate
    // digests append new rows.
    conn.execute_batch(
        "
        CREATE TABLE _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        INSERT INTO _schema_migrations (version, name, applied_at) VALUES
            (1, 'core_control_plane_entities', '2026-01-01'),
            (2, 'authoritative_session_security', '2026-01-01'),
            (3, 'immutable_proofs_and_masterplan_revisions', '2026-01-01'),
            (4, 'crash_safe_claim_and_merge_metadata', '2026-01-01'),
            (5, 'task_attempts_and_machine_evaluators', '2026-01-01'),
            (6, 'task_masterplan_lifecycle_and_stale_invalidation', '2026-01-01'),
            (7, 'normalize_proof_bundles_task_attempts_and_evaluators', '2026-01-01'),
            (8, 'task_attempt_worktree_paths', '2026-01-01'),
            (9, 'seed_canonical_ide_profiles_and_cleanup', '2026-01-01'),
            (10, 'masterplan_milestone_approval_toggle', '2026-01-01'),
            (11, 'multiple_masterplans_and_active_toggle', '2026-01-01'),
            (12, 'timeout_evidence', '2026-01-01'),
            (13, 'task_state_check', '2026-01-01');

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            master_spec TEXT NOT NULL,
            target_branch TEXT NOT NULL DEFAULT 'main',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'BACKLOG',
            substate TEXT NOT NULL DEFAULT 'NONE',
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- The effective pre-v14 proof_bundles schema: migration 0003 columns plus the
        -- attempt_id column added by 0005/0007, with proof_hash UNIQUE.
        CREATE TABLE proof_bundles (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            agent_id TEXT,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            prompt TEXT NOT NULL,
            base_sha TEXT NOT NULL,
            head_sha TEXT NOT NULL,
            files_changed_json TEXT NOT NULL,
            diff_summary TEXT NOT NULL,
            verification_runs_json TEXT NOT NULL,
            criteria_json TEXT NOT NULL,
            steps_json TEXT NOT NULL,
            proof_hash TEXT NOT NULL UNIQUE,
            generated_at TEXT NOT NULL,
            attempt_id TEXT,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Minimal versions of the tables checked by verify_schema_integrity
        CREATE TABLE task_attempts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            run_number INTEGER NOT NULL DEFAULT 1,
            worktree_path TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );
        CREATE TABLE evaluator_results (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL,
            evaluator_name TEXT NOT NULL,
            passed BOOLEAN NOT NULL,
            evaluated_at TEXT NOT NULL
        );
        ",
    )
    .unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
         VALUES ('proj-14', 'V14 App', '/tmp/v14', 'Spec', 'main', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, created_at, updated_at)
         VALUES ('task-14-a', 'proj-14', 'Proof Task', 'Desc', 'VERIFIED', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    // Two proof rows with distinct digests (the pre-v14 UNIQUE constraint allows that).
    conn.execute(
        "INSERT INTO proof_bundles (id, task_id, project_id, agent_id, attempt_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
         VALUES ('proof-14-a', 'task-14-a', 'proj-14', 'agent-14', NULL, 1, 'prompt', 'base-0', 'head-1', '[]', 'd1', '[]', '[]', '[]', 'hash-aaaa', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO proof_bundles (id, task_id, project_id, agent_id, attempt_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
         VALUES ('proof-14-b', 'task-14-a', 'proj-14', 'agent-14', NULL, 1, 'prompt', 'base-0', 'head-1', '[]', 'd1', '[]', '[]', '[]', 'hash-bbbb', '2026-01-01T00:01:00Z')",
        [],
    )
    .unwrap();

    // 3. Run migrations: only v14 is pending; it must rebuild proof_bundles and preserve rows
    run_migrations(&mut conn).expect("Migration v14 must succeed on a pre-v14 database");

    // 4. Verify proof rows survive with their exact digests
    let proof_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM proof_bundles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        proof_count, 2,
        "All proof_bundles rows must survive the v14 rebuild"
    );

    let hash_a: String = conn
        .query_row(
            "SELECT proof_hash FROM proof_bundles WHERE id = 'proof-14-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash_a, "hash-aaaa");
    let hash_b: String = conn
        .query_row(
            "SELECT proof_hash FROM proof_bundles WHERE id = 'proof-14-b'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash_b, "hash-bbbb");

    // 5. Verify the UNIQUE constraint is gone: inserting a row with a DUPLICATE
    //    proof_hash must succeed (append-only semantics).
    conn.execute(
        "INSERT INTO proof_bundles (id, task_id, project_id, agent_id, attempt_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
         VALUES ('proof-14-c', 'task-14-a', 'proj-14', 'agent-14', NULL, 1, 'prompt', 'base-0', 'head-1', '[]', 'd1', '[]', '[]', '[]', 'hash-aaaa', '2026-01-01T00:02:00Z')",
        [],
    )
    .expect("Inserting a duplicate proof_hash must succeed after the v14 rebuild");

    let final_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM proof_bundles WHERE task_id = 'task-14-a' AND head_sha = 'head-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(final_count, 3, "Duplicate digest rows must accumulate");

    // 6. Verify indexes were recreated and the task_id FK is still enforced
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'proof_bundles' AND name IN ('idx_proof_bundles_task_sha', 'idx_proof_bundles_task_attempt')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        idx_count, 2,
        "All proof_bundles indexes must be recreated after the rebuild"
    );

    let fk_err = conn
        .execute(
            "INSERT INTO proof_bundles (id, task_id, project_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
             VALUES ('proof-14-x', 'missing-task', 'proj-14', 'p', 'b', 'h', '[]', 'd', '[]', '[]', '[]', 'hash-xxxx', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap_err();
    assert!(
        fk_err.to_string().contains("FOREIGN KEY"),
        "task_id FK must be enforced after the rebuild, got: {}",
        fk_err
    );

    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

#[test]
fn test_real_disk_database_initialization_if_present() {
    if let Some(data_dir) = dirs_next::data_dir() {
        let db_path = data_dir.join("AgentXFlow").join("agentxflow_v2.db");
        if db_path.exists() {
            println!(
                "Testing connection and migration against existing disk db: {:?}",
                db_path
            );
            let pool_res = agent_x_flow_lib::db::DbPool::new(&db_path);
            match pool_res {
                Ok(_) => (),
                Err(agent_x_flow_lib::error::CoordinatorError::Database(ref msg))
                    if msg
                        .contains("Another AgentXFlow coordinator instance is already running") =>
                {
                    println!(
                        "Real disk database is locked by active coordinator instance; single-instance lock verified."
                    );
                }
                Err(e) => panic!("Opening existing user on-disk DB must succeed: {:?}", e),
            }
        }
    }
}

#[test]
fn test_fresh_install_reopen_round_trip() {
    let temp_db_path =
        std::env::temp_dir().join(format!("round_trip_{}.sqlite", uuid::Uuid::new_v4()));

    // ── Phase 1: fresh install ──────────────────────────────────────────
    {
        let mut conn = Connection::open(&temp_db_path).expect("Failed to open SQLite db");
        run_migrations(&mut conn).expect("Fresh-install migrations must succeed");

        // Insert representative data across key tables
        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
             VALUES ('proj-rt', 'Round Trip App', '/tmp/rt', 'Spec', 'main', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, created_at, updated_at)
             VALUES ('task-rt', 'proj-rt', 'RT Task', 'Desc', 'BACKLOG', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at)
             VALUES ('agent-rt', 'RT Agent', 'Antigravity', 'Lead', 'IDLE', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
             VALUES ('sess-rt', 'agent-rt', 'axf_sess_rt_token', '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO proof_bundles (id, task_id, project_id, attempt_number, prompt, base_sha, head_sha, files_changed_json, diff_summary, verification_runs_json, criteria_json, steps_json, proof_hash, generated_at)
             VALUES ('pb-rt', 'task-rt', 'proj-rt', 1, 'prompt', 'base', 'head', '[]', 'summary', '[]', '[]', '[]', 'rt-hash', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, base_sha, run_number, worktree_path, status, started_at)
             VALUES ('att-rt', 'task-rt', 'agent-rt', 1, 'base-sha', 1, '/tmp/rt-wt', 'COMPLETED', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO evaluator_results (id, task_id, attempt_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at)
             VALUES ('ev-rt', 'task-rt', 'att-rt', 'schema_check', 'lint', '1.0.0', 'abc123', 0, 'ok', '', 'sha256', 100, 1, '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Record applied migration count
        let applied_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert!(
            applied_count >= 18,
            "Fresh install must apply all migrations, got {}",
            applied_count
        );

        // Verify schema integrity on the freshly installed DB
        verify_schema_integrity(&conn).expect("Schema integrity must pass on fresh install");
    }
    // Connection dropped here — DB file persists on disk

    // ── Phase 2: reopen and re-run migrations (idempotent no-op) ────────
    {
        let mut conn = Connection::open(&temp_db_path).expect("Failed to reopen SQLite db");
        run_migrations(&mut conn)
            .expect("Re-running migrations on an existing DB must succeed (idempotent)");

        // Migration count must not have changed
        let applied_count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM _schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            applied_count_after, 18,
            "Re-running migrations must not add new rows to _schema_migrations"
        );

        // Verify schema integrity after reopen
        verify_schema_integrity(&conn)
            .expect("Schema integrity must pass after reopen and idempotent re-run");

        // Verify every inserted row can be read back (deserialized)
        let proj_name: String = conn
            .query_row("SELECT name FROM projects WHERE id = 'proj-rt'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(proj_name, "Round Trip App");

        let task_title: String = conn
            .query_row("SELECT title FROM tasks WHERE id = 'task-rt'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(task_title, "RT Task");

        let agent_name: String = conn
            .query_row("SELECT name FROM agents WHERE id = 'agent-rt'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(agent_name, "RT Agent");

        let token: String = conn
            .query_row(
                "SELECT session_token FROM agent_sessions WHERE id = 'sess-rt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(token, "axf_sess_rt_token");

        let proof_hash: String = conn
            .query_row(
                "SELECT proof_hash FROM proof_bundles WHERE id = 'pb-rt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(proof_hash, "rt-hash");

        let att_status: String = conn
            .query_row(
                "SELECT status FROM task_attempts WHERE id = 'att-rt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(att_status, "COMPLETED");

        let ev_name: String = conn
            .query_row(
                "SELECT evaluator_name FROM evaluator_results WHERE id = 'ev-rt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ev_name, "schema_check");

        // Verify the CHECK constraint on tasks.state is enforced
        let err = conn
            .execute(
                "INSERT INTO tasks (id, project_id, title, description, state, created_at, updated_at)
                 VALUES ('task-bad', 'proj-rt', 'Bad', 'Bad', 'EXPLODED', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("CHECK"),
            "CHECK constraint must be enforced after reopen, got: {}",
            err
        );
    }

    // ── Cleanup ─────────────────────────────────────────────────────────
    std::fs::remove_file(temp_db_path).ok();
}
