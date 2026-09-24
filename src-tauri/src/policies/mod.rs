use crate::db::DbPool;
use crate::models::PolicyRule;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    db: DbPool,
}

impl PolicyEngine {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }

    pub fn add_policy(
        &self,
        project_id: &str,
        hook: &str,
        condition_pattern: &str,
        action: &str,
        reason: &str,
    ) -> Result<PolicyRule, String> {
        let id = Uuid::new_v4().to_string();
        let conn = self.db.lock();

        conn.execute(
            "INSERT INTO policies (id, project_id, hook, condition_pattern, action, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, project_id, hook, condition_pattern, action, reason],
        )
        .map_err(|e| e.to_string())?;

        Ok(PolicyRule {
            id,
            project_id: project_id.to_string(),
            hook: hook.to_string(),
            condition_pattern: condition_pattern.to_string(),
            action: action.to_string(),
            reason: reason.to_string(),
        })
    }

    pub fn evaluate_hook(
        &self,
        project_id: &str,
        hook: &str,
        target: &str,
    ) -> Result<(&'static str, Option<String>), String> {
        // Hard-coded non-negotiable safety guardrails
        if target.contains("git reset --hard")
            || target.contains("rm -rf /")
            || target.contains("format C:")
        {
            return Ok((
                "DENY",
                Some("Destructive operation is strictly prohibited by security policy".to_string()),
            ));
        }

        if target.contains("git push") {
            return Ok((
                "REQUIRE_APPROVAL",
                Some("Direct Git push requires human approval".to_string()),
            ));
        }

        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, condition_pattern, action, reason FROM policies WHERE project_id = ?1 AND hook = ?2")
            .map_err(|e| e.to_string())?;

        let rules_iter = stmt
            .query_map(rusqlite::params![project_id, hook], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let rules: Vec<(String, String, String, String)> = rules_iter
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read policy rules: {}", e))?;

        let mut matching_rules: Vec<(String, String, String, String)> = rules
            .into_iter()
            .filter(|(_id, pattern, _action, _reason)| target.contains(pattern))
            .collect();

        // Sort by specificity: longest pattern length descending, then id ascending for tie-breaking
        matching_rules.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

        if let Some((_id, pattern, action, reason)) = matching_rules.into_iter().next() {
            return match action.as_str() {
                "DENY" => Ok(("DENY", Some(reason))),
                "REQUIRE_APPROVAL" => Ok(("REQUIRE_APPROVAL", Some(reason))),
                "ALLOW" => Ok(("ALLOW", Some(reason))),
                unknown => Err(format!(
                    "Invalid policy action '{}' configured for rule on pattern '{}'",
                    unknown, pattern
                )),
            };
        }

        Ok(("ALLOW", None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_engine_hardcoded_guardrails() {
        let pool = DbPool::new_in_memory().unwrap();
        let engine = PolicyEngine::new(pool);

        let res_reset = engine
            .evaluate_hook("p1", "pre-command", "git reset --hard HEAD")
            .unwrap();
        assert_eq!(res_reset.0, "DENY");

        let res_push = engine
            .evaluate_hook("p1", "pre-command", "git push origin main")
            .unwrap();
        assert_eq!(res_push.0, "REQUIRE_APPROVAL");
    }

    #[test]
    fn test_policy_engine_fail_closed_on_invalid_action() {
        let pool = DbPool::new_in_memory().unwrap();
        let engine = PolicyEngine::new(pool.clone());
        let now = chrono::Utc::now().to_rfc3339();

        // Insert project to satisfy foreign key
        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
                [&now],
            ).unwrap();
        }

        // Add valid ALLOW rule
        engine
            .add_policy("p1", "pre-mutation", "safe_dir", "ALLOW", "Safe directory")
            .unwrap();
        let res_allow = engine
            .evaluate_hook("p1", "pre-mutation", "safe_dir/file.txt")
            .unwrap();
        assert_eq!(res_allow.0, "ALLOW");

        // Add rule with invalid / unknown action
        engine
            .add_policy(
                "p1",
                "pre-mutation",
                "secret_dir",
                "UNKNOWN_ACTION",
                "Bad rule",
            )
            .unwrap();
        let res_invalid = engine.evaluate_hook("p1", "pre-mutation", "secret_dir/key.pem");
        assert!(
            res_invalid.is_err(),
            "Unknown policy action must fail closed with error"
        );
        assert!(res_invalid
            .unwrap_err()
            .contains("Invalid policy action 'UNKNOWN_ACTION'"));
    }

    #[test]
    fn test_policy_engine_most_specific_pattern_wins_broad_allow_specific_deny() {
        let pool = DbPool::new_in_memory().unwrap();
        let engine = PolicyEngine::new(pool.clone());
        let now = chrono::Utc::now().to_rfc3339();

        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
                [&now],
            ).unwrap();
        }

        // Broad rule: src/ -> ALLOW
        engine
            .add_policy("p1", "pre-mutation", "src/", "ALLOW", "Allow src directory")
            .unwrap();
        // Specific rule: src/secrets/ -> DENY
        engine
            .add_policy(
                "p1",
                "pre-mutation",
                "src/secrets/",
                "DENY",
                "Deny secrets directory",
            )
            .unwrap();

        // 1. Target in safe src/ -> ALLOW (only broad rule matches)
        let res_src = engine
            .evaluate_hook("p1", "pre-mutation", "src/components/button.tsx")
            .unwrap();
        assert_eq!(res_src.0, "ALLOW");

        // 2. Target in src/secrets/ -> DENY (both match, specific rule wins)
        let res_secrets = engine
            .evaluate_hook("p1", "pre-mutation", "src/secrets/private_key.pem")
            .unwrap();
        assert_eq!(res_secrets.0, "DENY");
        assert_eq!(res_secrets.1.as_deref(), Some("Deny secrets directory"));
    }

    #[test]
    fn test_policy_engine_most_specific_pattern_wins_broad_deny_specific_allow() {
        let pool = DbPool::new_in_memory().unwrap();
        let engine = PolicyEngine::new(pool.clone());
        let now = chrono::Utc::now().to_rfc3339();

        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
                [&now],
            ).unwrap();
        }

        // Broad rule: src/ -> DENY
        engine
            .add_policy(
                "p1",
                "pre-mutation",
                "src/",
                "DENY",
                "Deny src directory by default",
            )
            .unwrap();
        // Specific rule: src/public/ -> ALLOW
        engine
            .add_policy(
                "p1",
                "pre-mutation",
                "src/public/",
                "ALLOW",
                "Allow public directory",
            )
            .unwrap();

        // 1. Target in normal src/ -> DENY
        let res_src = engine
            .evaluate_hook("p1", "pre-mutation", "src/internal/core.rs")
            .unwrap();
        assert_eq!(res_src.0, "DENY");

        // 2. Target in src/public/ -> ALLOW (most specific wins)
        let res_public = engine
            .evaluate_hook("p1", "pre-mutation", "src/public/index.html")
            .unwrap();
        assert_eq!(res_public.0, "ALLOW");
    }

    #[test]
    fn test_policy_engine_equal_specificity_deterministic_tie_breaker() {
        let pool = DbPool::new_in_memory().unwrap();
        let engine = PolicyEngine::new(pool.clone());
        let now = chrono::Utc::now().to_rfc3339();

        {
            let conn = pool.lock();
            conn.execute(
                "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
                [&now],
            ).unwrap();

            // Insert two rules with identical pattern length: "dir_a" and "dir_b", or same pattern with different IDs
            // Rule with smaller ID "pol_01" (DENY) vs Rule with "pol_02" (ALLOW) on same pattern "target_pattern"
            conn.execute(
                "INSERT INTO policies (id, project_id, hook, condition_pattern, action, reason) VALUES ('pol_01', 'p1', 'pre-command', 'shared_pattern', 'DENY', 'First rule')",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO policies (id, project_id, hook, condition_pattern, action, reason) VALUES ('pol_02', 'p1', 'pre-command', 'shared_pattern', 'ALLOW', 'Second rule')",
                [],
            ).unwrap();
        }

        // Resolves deterministically to pol_01 (id ASC)
        let res = engine
            .evaluate_hook("p1", "pre-command", "execute with shared_pattern in path")
            .unwrap();
        assert_eq!(res.0, "DENY");
        assert_eq!(res.1.as_deref(), Some("First rule"));
    }
}
