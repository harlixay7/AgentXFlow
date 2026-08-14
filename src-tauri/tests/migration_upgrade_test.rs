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
