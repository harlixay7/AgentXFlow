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
        ).map_err(|e| e.to_string())?;

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
        if target.contains("git reset --hard") || target.contains("rm -rf /") || target.contains("format C:") {
            return Ok(("DENY", Some("Destructive operation is strictly prohibited by security policy".to_string())));
        }

        if target.contains("git push") {
            return Ok(("REQUIRE_APPROVAL", Some("Direct Git push requires human approval".to_string())));
        }

        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT condition_pattern, action, reason FROM policies WHERE project_id = ?1 AND hook = ?2")
            .map_err(|e| e.to_string())?;

        let rules = stmt
            .query_map(rusqlite::params![project_id, hook], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|e| e.to_string())?;

        for r in rules.flatten() {
            let (pattern, action, reason) = r;
            if target.contains(&pattern) {
                return match action.as_str() {
                    "DENY" => Ok(("DENY", Some(reason))),
                    "REQUIRE_APPROVAL" => Ok(("REQUIRE_APPROVAL", Some(reason))),
                    _ => Ok(("ALLOW", None)),
                };
            }
        }

        Ok(("ALLOW", None))
    }
}
