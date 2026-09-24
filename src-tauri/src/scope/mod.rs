use chrono::{Duration, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use tracing::{error, info};
use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{CollisionRisk, ScopeLease, ScopeViolation};

#[derive(Debug, Clone)]
pub struct ScopeManager {
    db: DbPool,
}

impl ScopeManager {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }

    /// Layer 1: Atomically checks for collisions and acquires exclusive or shared scope leases
    pub fn acquire_scope_tx(
        &self,
        tx: &rusqlite::Transaction,
        task_id: &str,
        agent_id: &str,
        raw_patterns: Vec<String>,
        access_type: &str,
    ) -> Result<Vec<ScopeLease>, String> {
        let mut normalized_patterns = Vec::new();
        for raw in &raw_patterns {
            let pats = Self::normalize_patterns(raw)?;
            normalized_patterns.extend(pats);
        }

        if normalized_patterns.is_empty() {
            return Ok(Vec::new());
        }

        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_at = (now + Duration::hours(4)).to_rfc3339();

        // 0. Validate task existence and ownership inside the transaction
        let assigned_agent: Option<String> = tx
            .query_row(
                "SELECT assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

        if !agent_id.is_empty()
            && agent_id != "master"
            && agent_id != "system"
            && agent_id != "coordinator"
        {
            if let Some(ref assigned) = assigned_agent {
                let (canon_assigned, ..) =
                    crate::core::CoordinatorEngine::canonicalize_ide_identity(assigned, "");
                let (canon_caller, ..) =
                    crate::core::CoordinatorEngine::canonicalize_ide_identity(agent_id, "");
                if assigned != agent_id && canon_assigned != canon_caller {
                    return Err(format!(
                        "Scope ownership violation: Task '{}' is assigned to agent '{}', caller is '{}'",
                        task_id, assigned, agent_id
                    ));
                }
            } else {
                return Err(format!(
                    "Scope ownership violation: Cannot acquire scope lease for unassigned task '{}'",
                    task_id
                ));
            }
        }

        // 1. Clean up expired leases
        tx.execute("DELETE FROM scope_leases WHERE expires_at < ?1", [&now_str])
            .map_err(|e| e.to_string())?;

        // 2. Query all currently active incompatible leases
        let mut stmt = tx
            .prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id != ?1 AND expires_at > ?2")
            .map_err(|e| e.to_string())?;

        let active_leases_iter = stmt
            .query_map(rusqlite::params![task_id, now_str], |row| {
                Ok(ScopeLease {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    pattern: row.get(3)?,
                    access_type: row.get(4)?,
                    expires_at: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut active_leases = Vec::new();
        for l in active_leases_iter {
            let lease = l.map_err(|e| format!("Failed to decode active scope lease row: {}", e))?;
            active_leases.push(lease);
        }
        drop(stmt);

        // 3. Conservative collision check
        for req_pat in &normalized_patterns {
            for existing in &active_leases {
                if (access_type == "EXCLUSIVE_WRITE" || existing.access_type == "EXCLUSIVE_WRITE")
                    && self.globs_might_overlap(req_pat, &existing.pattern)
                {
                    return Err(format!(
                        "Scope collision: Pattern '{}' overlaps with active lease '{}' held by agent '{}' for task '{}'",
                        req_pat, existing.pattern, existing.agent_id, existing.task_id
                    ));
                }
            }
        }

        // 4. Insert all requested leases atomically
        let mut granted = Vec::new();
        for norm_pat in normalized_patterns {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, task_id, agent_id, norm_pat, access_type, expires_at, now_str],
            ).map_err(|e| format!("Failed to insert scope lease: {}", e))?;

            granted.push(ScopeLease {
                id,
                task_id: task_id.to_string(),
                agent_id: agent_id.to_string(),
                pattern: norm_pat,
                access_type: access_type.to_string(),
                expires_at: expires_at.clone(),
                created_at: now_str.clone(),
            });
        }

        Ok(granted)
    }

    pub fn acquire_scope(
        &self,
        task_id: &str,
        agent_id: &str,
        raw_patterns: Vec<String>,
        access_type: &str,
    ) -> Result<Vec<ScopeLease>, String> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start scope transaction: {}", e))?;

        let granted = self.acquire_scope_tx(&tx, task_id, agent_id, raw_patterns, access_type)?;

        tx.commit()
            .map_err(|e| format!("Failed to commit scope reservation: {}", e))?;
        info!(
            "Successfully acquired {} scope leases for task '{}'",
            granted.len(),
            task_id
        );
        Ok(granted)
    }

    /// Splits and normalizes pattern strings (handling commas, semicolons, whitespace)
    pub fn normalize_patterns(raw: &str) -> Result<Vec<String>, String> {
        let mut result = Vec::new();
        for piece in raw.split([',', ';', '\n']) {
            let trimmed = piece.trim();
            if trimmed.is_empty() {
                continue;
            }
            let normalized = trimmed.replace('\\', "/");
            let clean = normalized.trim_start_matches("./").trim_start_matches('/');
            if clean.is_empty() {
                continue;
            }
            if Glob::new(clean).is_err() {
                return Err(format!("Invalid glob pattern: '{}'", clean));
            }
            result.push(clean.to_string());
        }
        if result.is_empty() && !raw.trim().is_empty() {
            let clean = raw.trim().replace('\\', "/");
            let norm = clean
                .trim_start_matches("./")
                .trim_start_matches('/')
                .to_string();
            if Glob::new(&norm).is_err() {
                return Err(format!("Invalid glob pattern: '{}'", norm));
            }
            result.push(norm);
        }
        Ok(result)
    }

    pub fn normalize_pattern(raw: &str) -> Result<String, String> {
        let list = Self::normalize_patterns(raw)?;
        list.into_iter()
            .next()
            .ok_or_else(|| "Empty pattern".to_string())
    }

    /// Returns true if a pattern represents a broad glob lease (e.g. `**`, `*`, `src/**`, `app/**`)
    pub fn is_broad_pattern(pattern: &str) -> bool {
        let trimmed = pattern.trim().replace('\\', "/");
        let clean = trimmed.trim_start_matches("./").trim_start_matches('/');
        clean == "**"
            || clean == "*"
            || clean.ends_with("/**")
            || clean.ends_with("/*")
            || !clean.contains('/')
    }

    /// Layer 2 (real-time collision analysis) was removed: `check_scope_overlap`
    /// had no callers, and its only logic was a DB query delegating to
    /// `globs_might_overlap` — the exact algorithm `acquire_scope`'s pre-check
    /// already uses. Both paths share one implementation, so the dead duplicate
    /// was deleted (hardening D25).
    ///
    /// Layer 3: Actual mutation audit comparing real Git diff changed files against granted scope leases
    pub fn audit_actual_mutations(
        &self,
        task_id: &str,
        agent_id: &str,
        changed_files: &[String],
    ) -> Result<Vec<ScopeViolation>, String> {
        self.audit_attempt_mutations(task_id, None, agent_id, changed_files)
    }

    /// Attempt-aware mutation audit that records attempt_id and auto-resolves previously covered files
    pub fn audit_attempt_mutations(
        &self,
        task_id: &str,
        attempt_id: Option<&str>,
        agent_id: &str,
        changed_files: &[String],
    ) -> Result<Vec<ScopeViolation>, String> {
        let conn = self.db.lock();
        let now_str = Utc::now().to_rfc3339();

        let mut stmt = conn
            .prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1 AND expires_at > ?2")
            .map_err(|e| e.to_string())?;

        let patterns_iter = stmt
            .query_map(rusqlite::params![task_id, now_str], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;

        let mut builder = GlobSetBuilder::new();
        let mut has_patterns = false;
        for p in patterns_iter.flatten() {
            let glob = Glob::new(&p)
                .map_err(|e| format!("Malformed scope lease glob pattern '{}': {}", p, e))?;
            builder.add(glob);
            has_patterns = true;
        }

        let globset = if has_patterns {
            builder
                .build()
                .map_err(|e| format!("Failed to compile scope lease globset: {}", e))?
        } else {
            GlobSet::empty()
        };

        let mut violations = Vec::new();

        for file in changed_files {
            let normalized = file.replace('\\', "/");
            let clean = normalized
                .trim_start_matches("./")
                .trim_start_matches('/')
                .to_string();

            if !globset.is_match(&clean) {
                let v_id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO scope_violations (id, task_id, agent_id, file_path, violation_type, detected_at, resolved, attempt_id)
                     VALUES (?1, ?2, ?3, ?4, 'UNRESERVED_WRITE', ?5, 0, ?6)",
                    rusqlite::params![v_id, task_id, agent_id, clean, now_str, attempt_id],
                )
                .map_err(|e| {
                    format!(
                        "Failed to record scope violation for '{}': {}",
                        clean, e
                    )
                })?;

                violations.push(ScopeViolation {
                    id: v_id,
                    task_id: task_id.to_string(),
                    agent_id: agent_id.to_string(),
                    file_path: clean,
                    violation_type: "UNRESERVED_WRITE".to_string(),
                    detected_at: now_str.clone(),
                    resolved: false,
                });
            }
        }

        // Auto-resolve any previous violations for files that are either no longer in changed_files or are now covered by scope.
        // Auto-resolve is non-fatal: failures are logged instead of silently leaving stale unresolved records.
        let mut unres_stmt = match conn.prepare(
            "SELECT id, file_path FROM scope_violations WHERE task_id = ?1 AND resolved = 0",
        ) {
            Ok(stmt) => stmt,
            Err(e) => {
                error!(
                    "Failed to load unresolved scope violations for auto-resolve: {}",
                    e
                );
                return Ok(violations);
            }
        };
        let unres_iter = match unres_stmt.query_map([task_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(iter) => iter,
            Err(e) => {
                error!(
                    "Failed to query unresolved scope violations for auto-resolve: {}",
                    e
                );
                return Ok(violations);
            }
        };
        let unres_list: Vec<(String, String)> = unres_iter.flatten().collect();
        for (v_id, path) in unres_list {
            let normalized = path.replace('\\', "/");
            let clean = normalized
                .trim_start_matches("./")
                .trim_start_matches('/')
                .to_string();
            let is_still_changed = changed_files.iter().any(|f| {
                let f_clean = f
                    .replace('\\', "/")
                    .trim_start_matches("./")
                    .trim_start_matches('/')
                    .to_string();
                f_clean == clean
            });
            if !is_still_changed || globset.is_match(&clean) {
                if let Err(e) = conn.execute(
                    "UPDATE scope_violations SET resolved = 1 WHERE id = ?1",
                    [&v_id],
                ) {
                    error!("Failed to auto-resolve scope violation '{}': {}", v_id, e);
                }
            }
        }

        Ok(violations)
    }

    /// Semantic Collision Risk Scoring
    pub fn calculate_collision_risk(
        &self,
        task_a_id: &str,
        task_b_id: &str,
    ) -> Result<CollisionRisk, String> {
        let conn = self.db.lock();

        let mut stmt_a = conn
            .prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;
        let patterns_a: Vec<String> = stmt_a
            .query_map([task_a_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        let mut stmt_b = conn
            .prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1")
            .map_err(|e| e.to_string())?;
        let patterns_b: Vec<String> = stmt_b
            .query_map([task_b_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        let mut overlapping = Vec::new();
        let mut semantic_factors = Vec::new();
        let mut risk_score: f64 = 0.0;

        for pa in &patterns_a {
            for pb in &patterns_b {
                if self.globs_might_overlap(pa, pb) {
                    overlapping.push(format!("{} <-> {}", pa, pb));
                    risk_score += 0.5;
                }
            }

            if pa.contains("package.json") || pa.contains("Cargo.toml") || pa.contains("lock") {
                semantic_factors.push("Package manifest / lockfile modified".to_string());
                risk_score += 0.3;
            }
            if pa.contains("migration") || pa.contains("schema") {
                semantic_factors.push("Database schema / migration modified".to_string());
                risk_score += 0.4;
            }
        }

        risk_score = risk_score.min(1.0);

        Ok(CollisionRisk {
            task_a_id: task_a_id.to_string(),
            task_b_id: task_b_id.to_string(),
            risk_score,
            overlapping_patterns: overlapping,
            semantic_risk_factors: semantic_factors,
        })
    }

    /// Releases scope leases for a task, verifying owner if specified
    pub fn release_scope(&self, task_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM scope_leases WHERE task_id = ?1", [task_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn release_scope_by_agent(&self, task_id: &str, agent_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        let affected = conn
            .execute(
                "DELETE FROM scope_leases WHERE task_id = ?1 AND agent_id = ?2",
                rusqlite::params![task_id, agent_id],
            )
            .map_err(|e| e.to_string())?;

        if affected == 0 {
            // Check if leases existed under another agent
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scope_leases WHERE task_id = ?1",
                    [task_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if count > 0 {
                return Err(format!(
                    "Scope lease release rejected: Leases for task '{}' belong to another agent.",
                    task_id
                ));
            }
        }
        Ok(())
    }

    pub fn renew_task_leases(&self, task_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        let now = Utc::now();
        let expires_at = (now + Duration::hours(4)).to_rfc3339();
        conn.execute(
            "UPDATE scope_leases SET expires_at = ?1 WHERE task_id = ?2",
            rusqlite::params![expires_at, task_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Conservative glob overlap detection:
    /// Any partial, direct, prefix, wildcard, or uncertain relationship blocks collision.
    pub fn globs_might_overlap(&self, pattern_a: &str, pattern_b: &str) -> bool {
        // Windows filesystems are case-insensitive, so compare in lowercase there.
        #[cfg(target_os = "windows")]
        let (pattern_a, pattern_b) = (pattern_a.to_lowercase(), pattern_b.to_lowercase());

        if pattern_a == pattern_b {
            return true;
        }
        if pattern_a == "**" || pattern_b == "**" || pattern_a == "*" || pattern_b == "*" {
            return true;
        }

        // Segment-aware comparison: split both patterns on '/'. Inputs are already
        // normalized (backslashes converted to '/', "./" stripped) by normalize_patterns.
        let segments_a: Vec<&str> = pattern_a.split('/').collect();
        let segments_b: Vec<&str> = pattern_b.split('/').collect();

        let (shorter, longer) = if segments_a.len() <= segments_b.len() {
            (&segments_a, &segments_b)
        } else {
            (&segments_b, &segments_a)
        };

        for (seg_a, seg_b) in shorter.iter().zip(longer.iter()) {
            if !Self::segments_compatible(seg_a, seg_b) {
                return false;
            }
        }

        if longer.len() > shorter.len() {
            // A deeper pattern overlaps when the shorter one ends in a wildcard
            // (matches arbitrary depth), or the boundary is prefix-compatible: the
            // longer pattern's segment at the shorter's last index starts with the
            // shorter's last segment (e.g. "src" vs "src/auth", "src/foo" vs
            // "src/foo*/x" — conservative).
            let last_short = shorter.last().expect("patterns are non-empty");
            let boundary = longer[shorter.len() - 1];
            boundary.starts_with(last_short) || last_short.contains('*') || boundary.contains('*')
        } else {
            true
        }
    }

    /// Segments at the same path position are compatible when equal, or when either
    /// side contains a wildcard (`*`/`**`): conservatively, any wildcard could match
    /// the other segment (a literal that is a prefix of a wildcard segment's fixed
    /// part is subsumed by this rule, e.g. "foo" vs "foo*"). A literal-vs-literal
    /// mismatch in a shared position means the paths are disjoint.
    fn segments_compatible(a: &str, b: &str) -> bool {
        a == b || a.contains('*') || b.contains('*')
    }
}

#[cfg(test)]
mod tests;
