use crate::db::DbPool;
use crate::scope::ScopeManager;

/// Runs both patterns through the engine's normalization (backslash conversion,
/// "./" stripping) and checks whether they might overlap, mirroring how
/// acquire_scope compares normalized request patterns against stored leases.
fn overlap(a: &str, b: &str) -> bool {
    let manager = ScopeManager::new(DbPool::new_in_memory().expect("in-memory DB pool"));
    let norm_a = ScopeManager::normalize_pattern(a).unwrap_or_else(|_| a.to_string());
    let norm_b = ScopeManager::normalize_pattern(b).unwrap_or_else(|_| b.to_string());
    manager.globs_might_overlap(&norm_a, &norm_b)
}

#[test]
fn test_overlap_exact_path() {
    assert!(overlap("src/a.rs", "src/a.rs"));
}

#[test]
fn test_overlap_same_subtree() {
    assert!(overlap("src/**", "src/auth/mod.rs"));
}

#[test]
fn test_overlap_ancestor_descendant() {
    assert!(overlap("src", "src/auth"));
}

#[test]
fn test_overlap_midstring_wildcard() {
    assert!(overlap("src/*/mod.rs", "src/auth/mod.rs"));
}

#[test]
fn test_overlap_wildcard_vs_specific() {
    assert!(overlap("*.rs", "src/**"));
}

#[test]
fn test_no_overlap_unrelated() {
    assert!(!overlap("src/a", "tests/b"));
}

#[test]
fn test_overlap_multiple_segments() {
    assert!(overlap("src/auth/**", "src/auth/handlers"));
}

#[test]
fn test_overlap_windows_separators() {
    assert!(overlap("src\\auth\\**", "src/auth/mod.rs"));
}

#[test]
fn test_overlap_boundary_prefix() {
    // Deeper pattern's segment at the shorter pattern's last index starts with the
    // shorter's last literal segment -> conservative overlap (brief Step 3:
    // "prefix-compatible" boundary rule).
    assert!(overlap("src/foo", "src/foo*/x"));
}

#[test]
fn test_no_overlap_prefix_sibling_dirs() {
    // Controller ruling R5: a literal-vs-literal mismatch in a shared position
    // means no overlap, even when one segment is a string prefix of the other
    // ("src/auth" vs "src/auth-2"). The pre-check is advisory; the authoritative
    // globset audit is unchanged, so this only relaxes collision conservativeness.
    assert!(!overlap("src/auth", "src/auth-2"));
}

#[cfg(windows)]
#[test]
fn test_overlap_case_insensitive_windows() {
    assert!(overlap("SRC/**", "src/auth"));
}

#[test]
fn test_normalize_patterns_rejects_invalid_glob() {
    let res = ScopeManager::normalize_patterns("src/[a-z");
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Invalid glob pattern"));
}

#[test]
fn test_audit_attempt_mutations_fails_closed_on_malformed_glob() {
    let pool = DbPool::new_in_memory().expect("in-memory DB pool");
    let manager = ScopeManager::new(pool.clone());
    let conn = pool.lock();
    let now = chrono::Utc::now().to_rfc3339();
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();

    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
        [&now],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t1', 'p1', 'T1', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a1', 'A1', 'CODER', '{}', ?1, ?1, 'sess_1')",
        [&now],
    ).unwrap();

    // Directly insert a malformed glob into scope_leases
    conn.execute(
        "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at)
         VALUES ('lease-malformed', 't1', 'a1', 'src/[invalid-unclosed', 'EXCLUSIVE_WRITE', ?1, ?2)",
        [&expires, &now],
    ).unwrap();
    drop(conn);

    let res = manager.audit_attempt_mutations("t1", None, "a1", &["src/file.rs".to_string()]);
    assert!(
        res.is_err(),
        "Scope audit must fail closed on malformed glob patterns"
    );
    assert!(res
        .unwrap_err()
        .contains("Malformed scope lease glob pattern"));
}

#[test]
fn test_matrix_exact_file_vs_exact_file() {
    assert!(overlap(
        "src/components/Button.tsx",
        "src/components/Button.tsx"
    ));
}

#[test]
fn test_matrix_sibling_files_do_not_collide() {
    assert!(!overlap("src/auth.rs", "src/user.rs"));
    assert!(!overlap("src/auth/login.ts", "src/auth/signup.ts"));
}

#[test]
fn test_matrix_directory_scope_vs_file_scope() {
    assert!(overlap("src/**", "src/auth.rs"));
    assert!(overlap("src/models/*", "src/models/user.rs"));
}

#[test]
fn test_matrix_src_vs_src_audio() {
    assert!(overlap("src/**", "src/audio/**"));
}

#[test]
fn test_matrix_unrelated_subtrees_do_not_collide() {
    assert!(!overlap("src/audio/**", "src/video/**"));
    assert!(!overlap("src/api/v1/**", "src/api/v2/**"));
}

#[test]
fn test_matrix_unrelated_directories_do_not_collide() {
    assert!(!overlap("src/**", "tests/**"));
    assert!(!overlap("docs/**", "scripts/**"));
}

#[test]
fn test_matrix_is_broad_pattern() {
    assert!(ScopeManager::is_broad_pattern("**"));
    assert!(ScopeManager::is_broad_pattern("*"));
    assert!(ScopeManager::is_broad_pattern("src/**"));
    assert!(ScopeManager::is_broad_pattern("src/*"));
    assert!(ScopeManager::is_broad_pattern("src"));
    assert!(!ScopeManager::is_broad_pattern("src/auth.rs"));
    assert!(!ScopeManager::is_broad_pattern("src/audio/track.wav"));
}

#[test]
fn test_matrix_expired_lease_cleanup_and_no_collision() {
    let pool = DbPool::new_in_memory().expect("in-memory DB pool");
    let manager = ScopeManager::new(pool.clone());
    let conn = pool.lock();
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();
    let expired_str = (now - chrono::Duration::hours(2)).to_rfc3339();

    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t1', 'p1', 'T1', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t2', 'p1', 'T2', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a1', 'A1', 'CODER', '{}', ?1, ?1, 'sess_1')",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a2', 'A2', 'CODER', '{}', ?1, ?1, 'sess_2')",
        [&now_str],
    ).unwrap();

    // Assign tasks to agents
    conn.execute(
        "UPDATE tasks SET assigned_agent_id = 'a1' WHERE id = 't1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET assigned_agent_id = 'a2' WHERE id = 't2'",
        [],
    )
    .unwrap();

    // Insert an expired lease for t1
    conn.execute(
        "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at)
         VALUES ('lease-expired', 't1', 'a1', 'src/auth/**', 'EXCLUSIVE_WRITE', ?1, ?2)",
        [&expired_str, &now_str],
    ).unwrap();
    drop(conn);

    // Agent 2 acquiring the same pattern succeeds because t1's lease has expired and gets cleaned up
    let res = manager.acquire_scope(
        "t2",
        "a2",
        vec!["src/auth/**".to_string()],
        "EXCLUSIVE_WRITE",
    );
    assert!(
        res.is_ok(),
        "Expired lease must be cleaned up and not block acquisition: {:?}",
        res
    );
}

#[test]
fn test_matrix_active_lease_collision_and_two_independent_chunks() {
    let pool = DbPool::new_in_memory().expect("in-memory DB pool");
    let manager = ScopeManager::new(pool.clone());
    let conn = pool.lock();
    let now_str = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t1', 'p1', 'T1', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t2', 'p1', 'T2', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t3', 'p1', 'T3', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, ?1, ?1)",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a1', 'A1', 'CODER', '{}', ?1, ?1, 'sess_1')",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a2', 'A2', 'CODER', '{}', ?1, ?1, 'sess_2')",
        [&now_str],
    ).unwrap();
    conn.execute(
        "INSERT INTO agents (id, name, agent_type, profile, last_heartbeat, created_at, session_token) VALUES ('a3', 'A3', 'CODER', '{}', ?1, ?1, 'sess_3')",
        [&now_str],
    ).unwrap();

    conn.execute(
        "UPDATE tasks SET assigned_agent_id = 'a1' WHERE id = 't1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET assigned_agent_id = 'a2' WHERE id = 't2'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET assigned_agent_id = 'a3' WHERE id = 't3'",
        [],
    )
    .unwrap();
    drop(conn);

    // Agent 1 acquires src/audio/**
    let res1 = manager.acquire_scope(
        "t1",
        "a1",
        vec!["src/audio/**".to_string()],
        "EXCLUSIVE_WRITE",
    );
    assert!(res1.is_ok());

    // Agent 2 acquires independent chunk src/video/** -> succeeds!
    let res2 = manager.acquire_scope(
        "t2",
        "a2",
        vec!["src/video/**".to_string()],
        "EXCLUSIVE_WRITE",
    );
    assert!(res2.is_ok(), "Independent chunks must succeed concurrently");

    // Agent 3 tries to acquire overlapping src/audio/codec.rs -> must collide with clear diagnostic
    let res3 = manager.acquire_scope(
        "t3",
        "a3",
        vec!["src/audio/codec.rs".to_string()],
        "EXCLUSIVE_WRITE",
    );
    assert!(res3.is_err());
    let err = res3.unwrap_err();
    assert!(
        err.contains("Scope collision"),
        "Collision message must contain Scope collision: {}",
        err
    );
    assert!(
        err.contains("src/audio/codec.rs"),
        "Collision message must state requested pattern: {}",
        err
    );
    assert!(
        err.contains("src/audio/**"),
        "Collision message must state existing lease: {}",
        err
    );
    assert!(
        err.contains("a1"),
        "Collision message must state holding agent: {}",
        err
    );
    assert!(
        err.contains("t1"),
        "Collision message must state conflicting task: {}",
        err
    );
}
