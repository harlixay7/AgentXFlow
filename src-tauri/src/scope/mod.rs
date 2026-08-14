use chrono::{Duration, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use tracing::info;
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
    pub fn acquire_scope(
        &self,
        task_id: &str,
        agent_id: &str,
        patterns: Vec<String>,
        access_type: &str,
    ) -> Result<Vec<ScopeLease>, String> {
        if patterns.is_empty() {
            return Ok(Vec::new());
        }

        // Normalize and expand all requested patterns (splitting multi-glob strings)
        let mut normalized_patterns = Vec::new();
        for p in &patterns {
            let norms = Self::normalize_patterns(p)?;
            normalized_patterns.extend(norms);
        }

        if normalized_patterns.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.db.lock();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_at = (now + Duration::hours(4)).to_rfc3339();

        // Use transaction for atomic check & lease reservation
        let tx = conn.transaction().map_err(|e| format!("Failed to start scope transaction: {}", e))?;

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
        for l in active_leases_iter.flatten() {
            active_leases.push(l);
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

        tx.commit().map_err(|e| format!("Failed to commit scope reservation: {}", e))?;
        info!("Successfully acquired {} scope leases for task '{}'", granted.len(), task_id);
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
            let norm = clean.trim_start_matches("./").trim_start_matches('/').to_string();
            if Glob::new(&norm).is_err() {
                return Err(format!("Invalid glob pattern: '{}'", norm));
            }
            result.push(norm);
        }
        Ok(result)
    }

    pub fn normalize_pattern(raw: &str) -> Result<String, String> {
        let list = Self::normalize_patterns(raw)?;
        list.into_iter().next().ok_or_else(|| "Empty pattern".to_string())
    }

    /// Layer 2: Real-time collision analysis against concurrent active tasks
    pub fn check_scope_overlap(
        &self,
        target_task_id: &str,
        patterns: &[String],
    ) -> Result<Vec<ScopeLease>, String> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let mut stmt = conn
            .prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id != ?1 AND expires_at > ?2 AND access_type = 'EXCLUSIVE_WRITE'")
            .map_err(|e| e.to_string())?;

        let leases_iter = stmt
            .query_map(rusqlite::params![target_task_id, now], |row| {
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

        let mut overlapping = Vec::new();
        for lease_res in leases_iter.flatten() {
            for requested in patterns {
                let norm = Self::normalize_pattern(requested).unwrap_or_else(|_| requested.clone());
                if self.globs_might_overlap(&norm, &lease_res.pattern) {
                    overlapping.push(lease_res.clone());
                    break;
                }
            }
        }

        Ok(overlapping)
    }

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
            .query_map(rusqlite::params![task_id, now_str], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let mut builder = GlobSetBuilder::new();
        let mut has_patterns = false;
        for p in patterns_iter.flatten() {
            if let Ok(glob) = Glob::new(&p) {
                builder.add(glob);
                has_patterns = true;
            }
        }

        let globset = if has_patterns {
            builder.build().unwrap_or_else(|_| GlobSet::empty())
        } else {
            GlobSet::empty()
        };

        let mut violations = Vec::new();

        for file in changed_files {
            let normalized = file.replace('\\', "/");
            let clean = normalized.trim_start_matches("./").trim_start_matches('/').to_string();

            if !globset.is_match(&clean) {
                let v_id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO scope_violations (id, task_id, agent_id, file_path, violation_type, detected_at, resolved, attempt_id)
                     VALUES (?1, ?2, ?3, ?4, 'UNRESERVED_WRITE', ?5, 0, ?6)",
                    rusqlite::params![v_id, task_id, agent_id, clean, now_str, attempt_id],
                ).ok();

                violations.push(ScopeViolation {
                    id: v_id,
                    task_id: task_id.to_string(),
                    agent_id: agent_id.to_string(),
                    file_path: clean,
                    violation_type: "UNRESERVED_WRITE".to_string(),
                    detected_at: now_str.clone(),
                    resolved: false,
                });
            } else {
                // If the file is now covered by active scope leases, mark previous violations resolved
                conn.execute(
                    "UPDATE scope_violations SET resolved = 1 WHERE task_id = ?1 AND file_path = ?2",
                    rusqlite::params![task_id, clean],
                ).ok();
            }
        }

        Ok(violations)
    }

    /// Semantic Collision Risk Scoring
    pub fn calculate_collision_risk(&self, task_a_id: &str, task_b_id: &str) -> Result<CollisionRisk, String> {
        let conn = self.db.lock();

        let mut stmt_a = conn.prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let patterns_a: Vec<String> = stmt_a.query_map([task_a_id], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_b = conn.prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let patterns_b: Vec<String> = stmt_b.query_map([task_b_id], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();

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
        let affected = conn.execute(
            "DELETE FROM scope_leases WHERE task_id = ?1 AND agent_id = ?2",
            rusqlite::params![task_id, agent_id],
        ).map_err(|e| e.to_string())?;

        if affected == 0 {
            // Check if leases existed under another agent
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM scope_leases WHERE task_id = ?1",
                [task_id],
                |r| r.get(0),
            ).unwrap_or(0);
            if count > 0 {
                return Err(format!("Scope lease release rejected: Leases for task '{}' belong to another agent.", task_id));
            }
        }
        Ok(())
    }

    pub fn renew_task_leases(&self, task_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        let now = Utc::now();
        let expires_at = (now + Duration::hours(4)).to_rfc3339();
        conn.execute("UPDATE scope_leases SET expires_at = ?1 WHERE task_id = ?2", rusqlite::params![expires_at, task_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Conservative glob overlap detection:
    /// Any partial, direct, prefix, wildcard, or uncertain relationship blocks collision.
    pub fn globs_might_overlap(&self, pattern_a: &str, pattern_b: &str) -> bool {
        if pattern_a == pattern_b {
            return true;
        }
        if pattern_a == "**" || pattern_b == "**" || pattern_a == "*" || pattern_b == "*" {
            return true;
        }

        let clean_a = pattern_a.trim_end_matches('*').trim_end_matches('/');
        let clean_b = pattern_b.trim_end_matches('*').trim_end_matches('/');

        if clean_a.is_empty() || clean_b.is_empty() {
            return true;
        }

        clean_a.starts_with(clean_b) || clean_b.starts_with(clean_a)
    }
}
