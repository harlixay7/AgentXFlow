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
