use agent_x_flow_lib::db::migrations::run_migrations;
use rusqlite::Connection;

#[test]
fn test_legacy_database_migration_upgrade() {
    let temp_db_path = std::env::temp_dir().join(format!("legacy_test_{}.sqlite", uuid::Uuid::new_v4()));
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
        "
    ).expect("Failed to create legacy tables");

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
    let proj_name: String = conn.query_row("SELECT name FROM projects WHERE id = 'proj-legacy-1'", [], |r| r.get(0)).unwrap();
    assert_eq!(proj_name, "Legacy App");

    let task_title: String = conn.query_row("SELECT title FROM tasks WHERE id = 'task-legacy-1'", [], |r| r.get(0)).unwrap();
    assert_eq!(task_title, "Legacy Task");

    let agent_name: String = conn.query_row("SELECT name FROM agents WHERE id = 'agent-legacy-1'", [], |r| r.get(0)).unwrap();
    assert_eq!(agent_name, "Legacy Agent");

    // 5. Verify migrations tracker has recorded versions
    let applied_count: i64 = conn.query_row("SELECT COUNT(*) FROM _schema_migrations", [], |r| r.get(0)).unwrap();
    assert!(applied_count >= 4, "All versioned migrations must be recorded as applied");

    // 6. Verify agent_sessions has new session_token schema and works cleanly
    conn.execute(
        "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
         VALUES ('sess-1', 'agent-legacy-1', 'axf_sess_test123', '2026-08-14T00:00:00Z', '2026-08-15T00:00:00Z', '2026-08-14T00:00:00Z')",
        [],
    ).expect("Inserting session into upgraded agent_sessions table must succeed");

    let retrieved_token: String = conn.query_row(
        "SELECT session_token FROM agent_sessions WHERE id = 'sess-1'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(retrieved_token, "axf_sess_test123");

    // 7. Cleanup
    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

#[test]
fn test_migration_0005_with_partially_existing_tables() {
    let temp_db_path = std::env::temp_dir().join(format!("test_partial_0005_{}.sqlite", uuid::Uuid::new_v4()));
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
    run_migrations(&mut conn).expect("Migration 0005 must handle existing task_attempts table without panicking");

    // Verify attempt_number column exists and can be queried
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM task_attempts",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 0);

    // Insert into upgraded task_attempts with attempt_number
    conn.execute(
        "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, base_sha, status, started_at)
         VALUES ('att-1', 't-1', 'ag-1', 1, 'base_sha', 'ACTIVE', '2026-08-14T00:00:00Z')",
        [],
    ).unwrap();

    let att_num: i64 = conn.query_row(
        "SELECT attempt_number FROM task_attempts WHERE id = 'att-1'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(att_num, 1);

    drop(conn);
    std::fs::remove_file(temp_db_path).ok();
}

#[test]
fn test_real_disk_database_initialization_if_present() {
    if let Some(data_dir) = dirs_next::data_dir() {
        let db_path = data_dir.join("AgentXFlow").join("agentxflow_v2.db");
        if db_path.exists() {
            println!("Testing connection and migration against existing disk db: {:?}", db_path);
            let pool_res = agent_x_flow_lib::db::DbPool::new(&db_path);
            assert!(pool_res.is_ok(), "Opening existing user on-disk DB must succeed: {:?}", pool_res.err());
        }
    }
}
