use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{AgentCapabilitySet, AgentPermissionRequest, AgentRun};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentDescriptor {
    pub name: String,
    pub binary_name: String,
    pub agent_type: String,
    pub is_installed: bool,
    pub version: Option<String>,
    pub capabilities: AgentCapabilitySet,
}

#[derive(Debug, Clone)]
pub struct AcpRuntime {
    db: DbPool,
    active_sessions: Arc<Mutex<HashMap<String, String>>>,
}

impl AcpRuntime {
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Discovers locally available AI coding agents
    pub fn discover_installed_agents(&self) -> Vec<AcpAgentDescriptor> {
        let candidates = vec![
            ("Codex CLI", "codex", "Codex"),
            ("OpenCode", "opencode", "OpenCode"),
            ("Claude Code", "claude", "Claude"),
            ("Cline CLI", "cline", "Cline"),
            ("Antigravity Agent", "antigravity", "Antigravity"),
        ];

        let mut discovered = Vec::new();
        for (name, bin, agent_type) in candidates {
            #[cfg(target_os = "windows")]
            let check_cmd = format!("where {}", bin);
            #[cfg(not(target_os = "windows"))]
            let check_cmd = format!("which {}", bin);

            let is_installed = std::process::Command::new(if cfg!(target_os = "windows") { "cmd" } else { "sh" })
                .args(&[if cfg!(target_os = "windows") { "/c" } else { "-c" }, &check_cmd])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            discovered.push(AcpAgentDescriptor {
                name: name.to_string(),
                binary_name: bin.to_string(),
                agent_type: agent_type.to_string(),
                is_installed,
                version: if is_installed { Some("1.0.0".to_string()) } else { None },
                capabilities: AgentCapabilitySet::default(),
            });
        }

        discovered
    }

    /// Spawns an agent execution run
    pub async fn start_agent_run(
        &self,
        task_id: &str,
        agent_id: &str,
        role: &str,
        prompt: &str,
        parent_run_id: Option<&str>,
    ) -> Result<AgentRun, String> {
        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        info!("Starting ACP agent run '{}' for agent '{}' on task '{}' (Role: {})", run_id, agent_id, task_id, role);

        let run = AgentRun {
            id: run_id.clone(),
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            parent_run_id: parent_run_id.map(|s| s.to_string()),
            role: role.to_string(),
            prompt: prompt.to_string(),
            status: "ACTIVE".to_string(),
            started_at: now.clone(),
            finished_at: None,
            prompt_tokens: 0,
            completion_tokens: 0,
        };

        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO agent_runs (id, task_id, agent_id, parent_run_id, role, prompt, status, started_at, prompt_tokens, completion_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ACTIVE', ?7, 0, 0)",
            rusqlite::params![run.id, run.task_id, run.agent_id, run.parent_run_id, run.role, run.prompt, run.started_at],
        ).map_err(|e| e.to_string())?;

        let mut lock = self.active_sessions.lock().await;
        lock.insert(run_id.clone(), agent_id.to_string());

        Ok(run)
    }

    /// Handles bidirectional permission requests from running agents
    pub fn create_permission_request(
        &self,
        run_id: &str,
        task_id: &str,
        agent_id: &str,
        action_type: &str,
        target: &str,
        reason: &str,
    ) -> Result<AgentPermissionRequest, String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let req = AgentPermissionRequest {
            id: id.clone(),
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            action_type: action_type.to_string(),
            target: target.to_string(),
            reason: reason.to_string(),
            status: "PENDING".to_string(),
            requested_at: now.clone(),
            responded_at: None,
        };

        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO agent_permission_requests (id, run_id, task_id, agent_id, action_type, target, reason, status, requested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PENDING', ?8)",
            rusqlite::params![req.id, req.run_id, req.task_id, req.agent_id, req.action_type, req.target, req.reason, req.requested_at],
        ).map_err(|e| e.to_string())?;

        Ok(req)
    }

    pub fn respond_permission_request(&self, request_id: &str, is_approved: bool) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let status = if is_approved { "APPROVED" } else { "DENIED" };

        let conn = self.db.lock();
        conn.execute(
            "UPDATE agent_permission_requests SET status = ?1, responded_at = ?2 WHERE id = ?3",
            rusqlite::params![status, now, request_id],
        ).map_err(|e| e.to_string())?;

        Ok(())
    }
}
