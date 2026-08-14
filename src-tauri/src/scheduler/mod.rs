use tracing::info;
use crate::dag::DagEngine;
use crate::db::DbPool;
use crate::models::{Task, TaskState};
use crate::scope::ScopeManager;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub max_active_agents: usize,
    pub max_verification_workers: usize,
    pub max_merges: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_active_agents: 4,
            max_verification_workers: 2,
            max_merges: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerEngine {
    db: DbPool,
    dag: DagEngine,
    scope: ScopeManager,
    config: SchedulerConfig,
}

impl SchedulerEngine {
    pub fn new(db: DbPool, dag: DagEngine, scope: ScopeManager, config: SchedulerConfig) -> Self {
        Self {
            db,
            dag,
            scope,
            config,
        }
    }

    pub fn scope(&self) -> &ScopeManager {
        &self.scope
    }

    /// Evaluates all READY tasks and returns candidate tasks eligible for immediate execution
    pub fn get_schedulable_tasks(&self, project_id: &str) -> Result<Vec<Task>, String> {
        let conn = self.db.lock();

        // 1. Check current concurrency load
        let active_count: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND state = 'RUNNING'",
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if active_count >= self.config.max_active_agents {
            info!("Max active agent concurrency limit ({}/{}) reached for project '{}'", active_count, self.config.max_active_agents, project_id);
            return Ok(Vec::new());
        }

        // 2. Fetch all READY tasks (filtering out stale tasks)
        let mut stmt = conn
            .prepare("SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, masterplan_id, masterplan_revision_id, is_stale, created_at, updated_at FROM tasks WHERE project_id = ?1 AND state = 'READY' AND is_stale = 0 ORDER BY priority DESC, created_at ASC")
            .map_err(|e| e.to_string())?;

        let tasks_iter = stmt
            .query_map([project_id], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    epic_id: row.get(3)?,
                    title: row.get(4)?,
                    description: row.get(5)?,
                    state: TaskState::parse(&row.get::<_, String>(6)?),
                    substate: crate::models::TaskSubstate::parse(&row.get::<_, String>(7)?),
                    assigned_agent_id: row.get(8)?,
                    priority: row.get(9)?,
                    risk_score: row.get(10)?,
                    estimated_scope: row.get(11)?,
                    worktree_path: row.get(12)?,
                    branch_name: row.get(13)?,
                    base_sha: row.get(14)?,
                    head_sha: row.get(15)?,
                    masterplan_id: row.get(16)?,
                    masterplan_revision_id: row.get(17)?,
                    is_stale: row.get(18)?,
                    created_at: row.get(19)?,
                    updated_at: row.get(20)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut schedulable = Vec::new();
        for task in tasks_iter.flatten() {
            // Check if dependencies are satisfied
            if self.dag.are_dependencies_satisfied(&task.id)? {
                schedulable.push(task);
            }
        }

        Ok(schedulable)
    }
}
