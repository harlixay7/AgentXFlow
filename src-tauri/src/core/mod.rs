use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::{info, warn};
use uuid::Uuid;

use crate::acp::AcpRuntime;
use crate::dag::DagEngine;
use crate::db::DbPool;
use crate::git::GitService;
use crate::merge::MergeEngine;
use crate::models::{
    AcceptanceCriteria, Agent, AgentCapabilitySet, ContextPack, CurrentContext, DecomposedStepInput,
    EvaluatorResult, EventItem, EvidenceRecord, IntegrationAttempt, Masterplan, MasterplanStep, MasterplanSummary,
    MergeQueueItem, PreparedMasterplanSnapshot, Project, ProofBundle, ScopeLease, ScopeViolation,
    Task, TaskAttempt, TaskDependency, TaskDetails, TaskState, TaskStep, TaskSubstate,
    VerificationResult, VerificationRun,
};
use crate::policies::PolicyEngine;
use crate::scheduler::{SchedulerConfig, SchedulerEngine};
use crate::scope::ScopeManager;
use crate::verification::VerificationEngine;

#[derive(Debug, Clone)]
pub struct CoordinatorEngine {
    pub db: DbPool,
    pub git: GitService,
    pub scope: ScopeManager,
    pub verify: VerificationEngine,
    pub merge: MergeEngine,
    pub dag: DagEngine,
    pub acp: AcpRuntime,
    pub policy: PolicyEngine,
    pub scheduler: SchedulerEngine,
    pub worktrees_root: std::path::PathBuf,
}

impl CoordinatorEngine {
    pub fn new(db: DbPool) -> Self {
        let worktrees_root = dirs_next::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("AgentXFlow")
            .join("worktrees");
        Self::new_with_worktree_root(db, worktrees_root)
    }

    pub fn new_with_worktree_root(db: DbPool, worktrees_root: std::path::PathBuf) -> Self {
        let git = GitService::new();
        let scope = ScopeManager::new(db.clone());
        let verify = VerificationEngine::new(db.clone());
        let merge = MergeEngine::new(db.clone(), git.clone());
        let dag = DagEngine::new(db.clone());
        let acp = AcpRuntime::new(db.clone());
        let policy = PolicyEngine::new(db.clone());
        let scheduler = SchedulerEngine::new(db.clone(), dag.clone(), scope.clone(), SchedulerConfig::default());

        let engine = Self {
            db,
            git,
            scope,
            verify,
            merge,
            dag,
            acp,
            policy,
            scheduler,
            worktrees_root,
        };

        engine.reconcile_on_startup();
        engine
    }

    /// Sequence-numbered event emitter
    pub fn emit_event(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let conn = self.db.lock();
        let event_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let payload_str = payload.to_string();

        conn.execute(
            "INSERT INTO events (event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event_id, project_id, task_id, agent_id, event_type, payload_str, now],
        ).ok();
    }

    pub fn get_events_after(&self, last_sequence: i64) -> Result<Vec<EventItem>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT sequence, event_id, project_id, task_id, agent_id, event_type, payload_json, timestamp FROM events WHERE sequence > ?1 ORDER BY sequence ASC LIMIT 500")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([last_sequence], |row| {
                Ok(EventItem {
                    sequence: row.get(0)?,
                    event_id: row.get(1)?,
                    project_id: row.get(2)?,
                    task_id: row.get(3)?,
                    agent_id: row.get(4)?,
                    event_type: row.get(5)?,
                    payload_json: row.get(6)?,
                    timestamp: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut res = Vec::new();
        for r in rows.flatten() {
            res.push(r);
        }
        Ok(res)
    }

    /// Self-healing startup reconciliation to repair interrupted claims and unclosed integrations
    pub fn reconcile_on_startup(&self) {
        info!("Running AgentXFlow startup reconciliation...");
        let conn = self.db.lock();

        // 1. Reset tasks left in CLAIMING state back to READY
        let mut stmt_interrupted = match conn.prepare("SELECT id, project_id, worktree_path FROM tasks WHERE state = 'CLAIMING'") {
            Ok(s) => s,
            Err(_) => return,
        };

        let interrupted_tasks: Vec<(String, String, Option<String>)> = stmt_interrupted
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|iter| iter.flatten().collect())
            .unwrap_or_default();
        drop(stmt_interrupted);

        for (t_id, _p_id, wt_path) in interrupted_tasks {
            if let Some(path_str) = wt_path {
                let path = std::path::PathBuf::from(path_str);
                if path.exists() {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
            conn.execute(
                "UPDATE tasks SET state = 'READY', substate = 'NONE', assigned_agent_id = NULL, worktree_path = NULL, branch_name = NULL WHERE id = ?1",
                [&t_id],
            ).ok();
            conn.execute("DELETE FROM scope_leases WHERE task_id = ?1", [&t_id]).ok();
            info!("Reconciled interrupted claiming task '{}' -> reset to READY", t_id);
        }

        // 2. Reset merge queue items interrupted during checks
        conn.execute("UPDATE merge_queue SET status = 'READY' WHERE status = 'RUNNING_CHECKS'", []).ok();

        // 3. Mark expired scope leases
        let now = Utc::now().to_rfc3339();
        conn.execute("DELETE FROM scope_leases WHERE expires_at < ?1", [&now]).ok();

        // 4. Reconcile orphaned claimed masterplan steps whose tasks are missing or cancelled
        conn.execute(
            "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL
             WHERE status = 'CLAIMED' AND (claimed_task_id IS NULL OR claimed_task_id IN (SELECT id FROM tasks WHERE state = 'CANCELLED' OR is_stale = 1))",
            [],
        ).ok();
    }

    // --- Projects ---
    pub fn create_project(
        &self,
        name: &str,
        path: &str,
        master_spec: &str,
        target_branch: &str,
    ) -> Result<Project, String> {
        let repo_path = Path::new(path);
        if !repo_path.exists() {
            return Err(format!("Path '{}' does not exist", path));
        }

        if !self.git.is_git_repo(repo_path) {
            info!("Directory '{}' is not a Git repository. Auto-initializing...", path);
            self.git.init_repo(repo_path)?;
        }

        let conn = self.db.lock();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, name, path, master_spec, target_branch, now],
        ).map_err(|e| e.to_string())?;

        // Initialize default project contract
        let contract_id = Uuid::new_v4().to_string();
        let mut hasher = Sha256::new();
        hasher.update(master_spec.as_bytes());
        let contract_hash = hex::encode(hasher.finalize());

        conn.execute(
            "INSERT INTO project_contracts (id, project_id, version, overview, architecture, rules_json, commands_json, testing_json, repo_map, security_constraints, contract_hash, created_at)
             VALUES (?1, ?2, 1, ?3, 'Standard Architecture', '[]', '[]', '[]', '', '[]', ?4, ?5)",
            params![contract_id, id, master_spec, contract_hash, now],
        ).ok();

        // Initialize baseline project rules
        let rule_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO project_rules (id, project_id, category, rule_text, strictness, created_at)
             VALUES (?1, ?2, 'SYSTEM', 'All mutations must occur inside assigned Git worktrees and within granted scope leases.', 'MANDATORY', ?3)",
            params![rule_id, id, now],
        ).ok();

        let proj = Project {
            id: id.clone(),
            name: name.to_string(),
            path: path.to_string(),
            master_spec: master_spec.to_string(),
            target_branch: target_branch.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        drop(conn);
        self.emit_event(Some(&id), None, None, "PROJECT_CREATED", json!({ "name": name, "path": path }));
        Ok(proj)
    }

    pub fn create_example_project(&self, root_dir: &str) -> Result<Project, String> {
        let example_path = Path::new(root_dir).join("example-repo");
        std::fs::create_dir_all(&example_path).map_err(|e| e.to_string())?;
        self.git.init_repo(&example_path)?;

        let proj = self.create_project(
            "AgentXFlow Example Project",
            example_path.to_str().unwrap(),
            "Example project demonstrating multi-agent coordination with worktrees and scope locks",
            "main",
        )?;

        // Create sample tasks
        self.create_task(
            &proj.id,
            "Implement User Authentication Service",
            "Build JWT-based authentication service in src/auth/",
            "HIGH",
            vec![
                ("Implement Token Generation".to_string(), "Generate signed JWT tokens".to_string(), true),
                ("Add Auth Unit Tests".to_string(), "Run cargo test --test auth_test".to_string(), true),
            ],
            vec!["Token signature matches secret".to_string(), "All tests pass".to_string()],
        )?;

        self.create_task(
            &proj.id,
            "Implement SQLite Database Migration Runner",
            "Build robust migration runner for 24 tables",
            "MEDIUM",
            vec![
                ("Write Migration SQL".to_string(), "Create initial schema".to_string(), true),
                ("Verify Foreign Keys".to_string(), "Test cascading deletes".to_string(), true),
            ],
            vec!["Schema migrations are idempotent".to_string()],
        )?;

        Ok(proj)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, path, master_spec, target_branch, created_at, updated_at FROM projects ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    master_spec: row.get(3)?,
                    target_branch: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut res = Vec::new();
        for r in rows.flatten() {
            res.push(r);
        }
        Ok(res)
    }

    // --- Tasks ---
    #[allow(clippy::too_many_arguments)]
    pub fn create_task_internal(
        &self,
        project_id: &str,
        masterplan_id: Option<&str>,
        masterplan_revision_id: Option<&str>,
        title: &str,
        description: &str,
        priority: &str,
        steps: Vec<(String, String, bool)>,
        criteria: Vec<String>,
    ) -> Result<Task, String> {
        let conn = self.db.lock();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO tasks (id, project_id, masterplan_id, masterplan_revision_id, title, description, state, substate, priority, is_stale, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'BACKLOG', 'NONE', ?7, 0, ?8, ?8)",
            params![id, project_id, masterplan_id, masterplan_revision_id, title, description, priority, now],
        ).map_err(|e| e.to_string())?;

        for (idx, (step_title, step_desc, is_mand)) in steps.into_iter().enumerate() {
            let step_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO task_steps (id, task_id, order_index, title, description, is_mandatory, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING')",
                params![step_id, id, idx as i32 + 1, step_title, step_desc, is_mand],
            ).ok();
        }

        for crit in criteria {
            let crit_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO acceptance_criteria (id, task_id, criterion, is_satisfied, is_locked)
                 VALUES (?1, ?2, ?3, 0, 0)",
                params![crit_id, id, crit],
            ).ok();
        }

        let task = Task {
            id: id.clone(),
            project_id: project_id.to_string(),
            parent_id: None,
            epic_id: None,
            title: title.to_string(),
            description: description.to_string(),
            state: TaskState::Backlog,
            substate: TaskSubstate::None,
            assigned_agent_id: None,
            priority: priority.to_string(),
            risk_score: 0.0,
            estimated_scope: None,
            worktree_path: None,
            branch_name: None,
            base_sha: None,
            head_sha: None,
            masterplan_id: masterplan_id.map(|s| s.to_string()),
            masterplan_revision_id: masterplan_revision_id.map(|s| s.to_string()),
            is_stale: false,
            created_at: now.clone(),
            updated_at: now,
        };

        drop(conn);
        self.emit_event(Some(project_id), Some(&id), None, "TASK_CREATED", json!({ "title": title, "priority": priority }));
        Ok(task)
    }

    pub fn create_task(
        &self,
        project_id: &str,
        title: &str,
        description: &str,
        priority: &str,
        steps: Vec<(String, String, bool)>,
        criteria: Vec<String>,
    ) -> Result<Task, String> {
        self.create_task_internal(project_id, None, None, title, description, priority, steps, criteria)
    }

    pub fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>, String> {
        if project_id.trim().is_empty() {
            return Err("project_id is required for list_tasks. Query 'project_list' or 'agentxflow_current_context' to obtain valid project IDs.".to_string());
        }
        let conn = self.db.lock();
        let query = "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, masterplan_id, masterplan_revision_id, is_stale, created_at, updated_at FROM tasks WHERE project_id = ?1 ORDER BY created_at DESC";

        let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;

        let map_row = |row: &rusqlite::Row| {
            Ok(Task {
                id: row.get(0)?,
                project_id: row.get(1)?,
                parent_id: row.get(2)?,
                epic_id: row.get(3)?,
                title: row.get(4)?,
                description: row.get(5)?,
                state: TaskState::parse(&row.get::<_, String>(6)?),
                substate: TaskSubstate::parse(&row.get::<_, String>(7)?),
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
        };

        let rows = stmt.query_map([project_id], map_row).map_err(|e| e.to_string())?;
        let mut res = Vec::new();
        for r in rows.flatten() {
            res.push(r);
        }

        Ok(res)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Task, String> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, masterplan_id, masterplan_revision_id, is_stale, created_at, updated_at FROM tasks WHERE id = ?1",
            [task_id],
            |row| {
                Ok(Task {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    epic_id: row.get(3)?,
                    title: row.get(4)?,
                    description: row.get(5)?,
                    state: TaskState::parse(&row.get::<_, String>(6)?),
                    substate: TaskSubstate::parse(&row.get::<_, String>(7)?),
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
            },
        ).map_err(|e| format!("Task '{}' not found: {}", task_id, e))
    }

    /// Explicitly cancel a task, releasing all write scope leases and returning masterplan steps to PENDING
    pub fn cancel_task(&self, task_id: &str, caller_agent_id: Option<&str>, reason: Option<&str>) -> Result<Task, String> {
        let task = self.get_task(task_id)?;

        if let Some(caller) = caller_agent_id {
            if let Some(ref assigned) = task.assigned_agent_id {
                if !assigned.is_empty() && assigned != caller {
                    return Err(format!("Authorization error: Caller agent '{}' is not the owner of task '{}' (assigned to '{}')", caller, task_id, assigned));
                }
            }
        }

        if task.state == TaskState::Done {
            return Err(format!("Cannot cancel task '{}': Task is already DONE (merged)", task_id));
        }

        let now = Utc::now().to_rfc3339();

        // 1. Release all scope leases held by this task
        let _ = self.scope.release_scope(task_id);

        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| format!("Failed to start cancel transaction: {}", e))?;

        // 2. Return any claimed masterplan steps back to PENDING
        tx.execute(
            "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL, updated_at = ?1 WHERE claimed_task_id = ?2",
            params![now, task_id],
        ).map_err(|e| e.to_string())?;

        // 3. Remove from merge queue if present
        tx.execute("DELETE FROM merge_queue WHERE task_id = ?1", [task_id]).map_err(|e| e.to_string())?;

        // 4. Update task state to CANCELLED and mark is_stale = 1
        tx.execute(
            "UPDATE tasks SET state = 'CANCELLED', substate = 'NONE', is_stale = 1, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        ).map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| format!("Failed to commit task cancellation: {}", e))?;
        drop(conn);

        // 5. Cleanup worktree if present
        if let Some(ref wt_path) = task.worktree_path {
            let p = Path::new(wt_path);
            if p.exists() {
                if let Ok(projs) = self.list_projects() {
                    if let Some(proj) = projs.into_iter().find(|p| p.id == task.project_id) {
                        let _ = self.git.remove_worktree(Path::new(&proj.path), p);
                    }
                }
                let _ = std::fs::remove_dir_all(p);
            }
        }

        self.emit_event(
            Some(&task.project_id),
            Some(task_id),
            caller_agent_id,
            "TASK_CANCELLED",
            json!({ "task_id": task_id, "reason": reason.unwrap_or("Task cancelled explicitly") }),
        );

        self.get_task(task_id)
    }

    /// Requeues a task chunk back to the masterplan: releases scope leases and sets step status = PENDING
    pub fn requeue_task(&self, task_id: &str, caller_agent_id: Option<&str>) -> Result<(), String> {
        self.cancel_task(task_id, caller_agent_id, Some("Task requeued to masterplan pending backlog"))?;
        Ok(())
    }

    pub fn get_task_details(&self, task_id: &str) -> Result<TaskDetails, String> {
        let task = self.get_task(task_id)?;
        let conn = self.db.lock();

        let mut stmt_steps = conn.prepare("SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE task_id = ?1 ORDER BY order_index ASC").map_err(|e| e.to_string())?;
        let steps: Vec<TaskStep> = stmt_steps.query_map([task_id], |r| {
            Ok(TaskStep { id: r.get(0)?, task_id: r.get(1)?, order_index: r.get(2)?, title: r.get(3)?, description: r.get(4)?, is_mandatory: r.get(5)?, status: r.get(6)?, completed_at: r.get(7)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_crit = conn.prepare("SELECT id, task_id, criterion, is_satisfied, is_locked FROM acceptance_criteria WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let criteria: Vec<AcceptanceCriteria> = stmt_crit.query_map([task_id], |r| {
            Ok(AcceptanceCriteria { id: r.get(0)?, task_id: r.get(1)?, criterion: r.get(2)?, is_satisfied: r.get(3)?, is_locked: r.get(4)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_leases = conn.prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let leases: Vec<ScopeLease> = stmt_leases.query_map([task_id], |r| {
            Ok(ScopeLease { id: r.get(0)?, task_id: r.get(1)?, agent_id: r.get(2)?, pattern: r.get(3)?, access_type: r.get(4)?, expires_at: r.get(5)?, created_at: r.get(6)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_deps = conn.prepare("SELECT id, task_id, depends_on_task_id, dependency_type, created_at FROM task_dependencies WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let dependencies: Vec<TaskDependency> = stmt_deps.query_map([task_id], |r| {
            Ok(TaskDependency { id: r.get(0)?, task_id: r.get(1)?, depends_on_task_id: r.get(2)?, dependency_type: r.get(3)?, created_at: r.get(4)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_runs = conn.prepare("SELECT id, task_id, run_id, check_id, check_name, commit_sha, command, exit_code, stdout, stderr, duration_ms, is_passed, is_stale, executed_at FROM verification_runs WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let verification_runs: Vec<VerificationRun> = stmt_runs.query_map([task_id], |r| {
            Ok(VerificationRun { id: r.get(0)?, task_id: r.get(1)?, run_id: r.get(2)?, check_id: r.get(3)?, check_name: r.get(4)?, commit_sha: r.get(5)?, command: r.get(6)?, exit_code: r.get(7)?, stdout: r.get(8)?, stderr: r.get(9)?, duration_ms: r.get(10)?, is_passed: r.get(11)?, is_stale: r.get(12)?, executed_at: r.get(13)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_violations = conn.prepare("SELECT id, task_id, agent_id, file_path, violation_type, detected_at, resolved FROM scope_violations WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let violations: Vec<ScopeViolation> = stmt_violations.query_map([task_id], |r| {
            Ok(ScopeViolation { id: r.get(0)?, task_id: r.get(1)?, agent_id: r.get(2)?, file_path: r.get(3)?, violation_type: r.get(4)?, detected_at: r.get(5)?, resolved: r.get(6)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_evidence = conn.prepare("SELECT id, task_id, step_id, evidence_type, source, payload_json, recorded_at FROM evidence_records WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let evidence_records: Vec<EvidenceRecord> = stmt_evidence.query_map([task_id], |r| {
            Ok(EvidenceRecord { id: r.get(0)?, task_id: r.get(1)?, step_id: r.get(2)?, evidence_type: r.get(3)?, source: r.get(4)?, payload_json: r.get(5)?, recorded_at: r.get(6)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let proof_bundle = conn.query_row(
            "SELECT task_id, project_id, agent_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, proof_hash, generated_at, verification_runs_json FROM proof_bundles WHERE task_id = ?1 ORDER BY generated_at DESC LIMIT 1",
            [task_id],
            |r| {
                let files_json: String = r.get(6)?;
                let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
                let runs_json: String = r.get(10).unwrap_or_else(|_| "[]".to_string());
                let runs: Vec<VerificationRun> = serde_json::from_str(&runs_json).unwrap_or_default();
                Ok(ProofBundle {
                    task_id: r.get(0)?,
                    project_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    prompt: r.get(3)?,
                    base_sha: r.get(4)?,
                    head_sha: r.get(5)?,
                    files_changed: files,
                    diff_summary: r.get(7)?,
                    verification_runs: runs,
                    scope_violations: Vec::new(),
                    proof_hash: r.get(8)?,
                    generated_at: r.get(9)?,
                })
            },
        ).ok();

        let assigned_agent = if let Some(ref aid) = task.assigned_agent_id {
            conn.query_row(
                "SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents WHERE id = ?1",
                [aid],
                |r| {
                    Ok(Agent {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        agent_type: r.get(2)?,
                        profile: r.get(3)?,
                        status: r.get(4)?,
                        capabilities: AgentCapabilitySet::default(),
                        last_heartbeat: r.get(5)?,
                        created_at: r.get(6)?,
                        session_token: None,
                    })
                },
            ).ok()
        } else {
            None
        };

        let active_attempt = conn.query_row(
            "SELECT id, task_id, agent_id, attempt_number, base_sha, head_sha, status, rejection_reasons, started_at, finished_at FROM task_attempts WHERE task_id = ?1 ORDER BY attempt_number DESC LIMIT 1",
            [task_id],
            |r| {
                Ok(TaskAttempt {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    attempt_number: r.get(3)?,
                    base_sha: r.get(4)?,
                    head_sha: r.get(5)?,
                    status: r.get(6)?,
                    rejection_reasons: r.get(7)?,
                    started_at: r.get(8)?,
                    finished_at: r.get(9)?,
                })
            },
        ).ok();

        let mut stmt_eval = conn.prepare("SELECT id, task_id, attempt_id, criterion_id, evaluator_name, evaluator_type, evaluator_version, commit_sha, exit_code, stdout_output, stderr_output, output_sha256, duration_ms, passed, evaluated_at FROM evaluator_results WHERE task_id = ?1 ORDER BY evaluated_at DESC").map_err(|e| e.to_string())?;
        let evaluator_results: Vec<EvaluatorResult> = stmt_eval.query_map([task_id], |r| {
            Ok(EvaluatorResult {
                id: r.get(0)?,
                task_id: r.get(1)?,
                attempt_id: r.get(2)?,
                criterion_id: r.get(3)?,
                evaluator_name: r.get(4)?,
                evaluator_type: r.get(5)?,
                evaluator_version: r.get(6)?,
                commit_sha: r.get(7)?,
                exit_code: r.get(8)?,
                stdout_output: r.get(9)?,
                stderr_output: r.get(10)?,
                output_sha256: r.get(11)?,
                duration_ms: r.get(12)?,
                passed: r.get(13)?,
                evaluated_at: r.get(14)?,
            })
        }).map_err(|e| e.to_string())?.flatten().collect();

        Ok(TaskDetails {
            task,
            steps,
            criteria,
            leases,
            dependencies,
            verification_runs,
            violations,
            evidence_records,
            proof_bundle,
            assigned_agent,
            active_attempt,
            evaluator_results,
        })
    }

    /// Crash-safe Task Claiming with Compare-and-Swap & Transactional Lock
    pub fn claim_task(&self, task_id: &str, agent_id: &str) -> Result<Task, String> {
        if !self.is_agent_registered(agent_id) {
            return Err(format!("Agent registration required: Agent ID '{}' is not registered.", agent_id));
        }

        // Dependency gate check
        if !self.dag.are_dependencies_satisfied(task_id)? {
            return Err(format!("Cannot claim task '{}': Prerequisite dependencies are not yet DONE", task_id));
        }

        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // 1. Transactional check & reserve state as CLAIMING
        let tx = conn.transaction().map_err(|e| format!("Failed to start transaction: {}", e))?;

        let (project_id, current_state, current_assigned): (String, String, Option<String>) = tx
            .query_row(
                "SELECT project_id, state, assigned_agent_id FROM tasks WHERE id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| format!("Task '{}' not found: {}", task_id, e))?;

        if current_state != "BACKLOG" && current_state != "READY" {
            return Err(format!("Cannot claim task '{}': Task is already in state '{}'", task_id, current_state));
        }

        if let Some(existing_agent) = current_assigned {
            if !existing_agent.is_empty() && existing_agent != agent_id {
                return Err(format!("Cannot claim task '{}': Already assigned to agent '{}'", task_id, existing_agent));
            }
        }

        let (proj_path, target_branch): (String, String) = tx
            .query_row(
                "SELECT path, target_branch FROM projects WHERE id = ?1",
                [&project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Project '{}' not found: {}", project_id, e))?;

        let repo_path = Path::new(&proj_path);
        let branch_name = format!("agentxflow/task-{}", task_id);
        let worktree_dir = self.worktrees_root.join(&project_id).join(format!("task-{}", task_id));
        let worktree_path_str = worktree_dir.to_string_lossy().to_string();

        let base_sha = self.git.get_ref_sha(repo_path, &target_branch).ok();

        // Set state = 'CLAIMING'
        tx.execute(
            "UPDATE tasks SET state = 'CLAIMING', substate = 'CLAIMING', assigned_agent_id = ?1, worktree_path = ?2, branch_name = ?3, base_sha = ?4, updated_at = ?5 WHERE id = ?6",
            params![agent_id, worktree_path_str, branch_name, base_sha, now, task_id],
        ).map_err(|e| format!("Failed to record task claim: {}", e))?;

        tx.commit().map_err(|e| format!("Failed to commit claim reservation: {}", e))?;
        drop(conn);

        // 2. Cut isolated Git worktree on disk
        if worktree_dir.exists() {
            let _ = self.git.remove_worktree(repo_path, &worktree_dir);
            let _ = std::fs::remove_dir_all(&worktree_dir);
        }

        if let Err(e) = self.git.create_worktree(repo_path, &worktree_dir, &branch_name, &target_branch) {
            // Full compensation on failure
            let conn = self.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'READY', substate = 'NONE', assigned_agent_id = NULL, worktree_path = NULL, branch_name = NULL WHERE id = ?1",
                [task_id],
            ).ok();
            return Err(format!("Failed to create isolated Git worktree: {}", e));
        }

        // 3. Mark state = 'RUNNING' and lock criteria
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET state = 'RUNNING', substate = 'ANALYZING', updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        ).ok();
        conn.execute("UPDATE acceptance_criteria SET is_locked = 1 WHERE task_id = ?1", [task_id]).ok();
        drop(conn);

        self.emit_event(Some(&project_id), Some(task_id), Some(agent_id), "TASK_CLAIMED", json!({ "agent": agent_id }));
        self.get_task(task_id)
    }

    pub fn complete_step(&self, step_id: &str, agent_id: Option<&str>, evidence_json: Option<&str>) -> Result<TaskStep, String> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // Verify step and caller task ownership
        let (task_id, assigned_agent): (String, Option<String>) = conn
            .query_row(
                "SELECT t.id, t.assigned_agent_id FROM task_steps s JOIN tasks t ON s.task_id = t.id WHERE s.id = ?1",
                [step_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("Step '{}' not found: {}", step_id, e))?;

        if let Some(caller) = agent_id {
            if let Some(ref assigned) = assigned_agent {
                if assigned != caller {
                    return Err(format!(
                        "Step ownership violation: Step belongs to task '{}' assigned to agent '{}', caller is '{}'",
                        task_id, assigned, caller
                    ));
                }
            }
        }

        conn.execute(
            "UPDATE task_steps SET status = 'COMPLETED', completed_at = ?1 WHERE id = ?2",
            params![now, step_id],
        ).map_err(|e| e.to_string())?;

        let step = conn.query_row(
            "SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE id = ?1",
            [step_id],
            |row| {
                Ok(TaskStep {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    order_index: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    is_mandatory: row.get(5)?,
                    status: row.get(6)?,
                    completed_at: row.get(7)?,
                })
            },
        ).map_err(|e| e.to_string())?;

        if let Some(ev) = evidence_json {
            let ev_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
                 VALUES (?1, ?2, ?3, 'AGENT_NOTE', 'AGENT_REPORTED', ?4, ?5)",
                params![ev_id, step.task_id, step_id, ev, now],
            ).ok();
        }

        drop(conn);
        self.emit_event(None, Some(&step.task_id), agent_id, "STEP_COMPLETED", json!({ "step_id": step_id, "title": step.title }));
        Ok(step)
    }

    /// Complete Authoritative Task Submission & Automated Machine Verification Gate
    pub fn submit_task(&self, task_id: &str, agent_id: &str) -> Result<VerificationResult, String> {
        let task = self.get_task(task_id)?;

        // 1. Ownership enforcement
        if let Some(ref assigned) = task.assigned_agent_id {
            if assigned != agent_id {
                return Err(format!("Task ownership violation: Task '{}' is owned by agent '{}', not '{}'", task_id, assigned, agent_id));
            }
        }

        let _proj = self.list_projects()?.into_iter().find(|p| p.id == task.project_id).ok_or("Project not found")?;
        let worktree_dir = match task.worktree_path.as_deref() {
            Some(path) => Path::new(path),
            None => return Err("Task has no worktree path allocated".to_string()),
        };

        if !worktree_dir.exists() {
            return Err(format!("Task worktree does not exist at {:?}", worktree_dir));
        }

        // 2. Cleanliness check (reject dirty worktree with uncommitted changes)
        if let Err(dirty_files) = self.git.check_worktree_cleanliness(worktree_dir) {
            return Ok(VerificationResult {
                is_valid: false,
                missing_mandatory_steps: Vec::new(),
                missing_evidence_step_ids: Vec::new(),
                unresolved_scope_violations: Vec::new(),
                failed_coordinator_checks: Vec::new(),
                rejection_reasons: vec![format!("Worktree has uncommitted modifications. Please commit all changes before submission: {:?}", dirty_files)],
            });
        }

        // Transition state to VERIFYING
        let now = Utc::now().to_rfc3339();
        let conn = self.db.lock();
        conn.execute("UPDATE tasks SET state = 'VERIFYING', substate = 'VERIFYING', updated_at = ?1 WHERE id = ?2", [&now, task_id])
            .map_err(|e| format!("Failed to set task to VERIFYING: {}", e))?;

        // Retrieve or create active task attempt with strict error propagation and run_number compatibility
        let attempt_opt: Option<(String, i32)> = conn
            .query_row(
                "SELECT id, attempt_number FROM task_attempts WHERE task_id = ?1 AND status = 'ACTIVE' ORDER BY attempt_number DESC LIMIT 1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        let (attempt_id, _attempt_num) = if let Some(att) = attempt_opt {
            att
        } else {
            let new_id = Uuid::new_v4().to_string();
            let new_num: i32 = conn.query_row(
                "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM task_attempts WHERE task_id = ?1",
                [task_id],
                |r| r.get(0),
            ).unwrap_or(1);
            conn.execute(
                "INSERT INTO task_attempts (id, task_id, agent_id, attempt_number, run_number, base_sha, worktree_path, status, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ACTIVE', ?8)",
                rusqlite::params![
                    new_id,
                    task_id,
                    agent_id,
                    new_num,
                    new_num,
                    task.base_sha.as_deref().unwrap_or(""),
                    worktree_dir.to_string_lossy().as_ref(),
                    now,
                ],
            ).map_err(|e| format!("Failed to create task attempt: {}", e))?;
            (new_id, new_num)
        };
        drop(conn);

        // 3. Exact worktree HEAD SHA is authoritative
        let head_sha = self.git.get_worktree_head_sha(worktree_dir)?;

        // 4. Invalidate stale verifications from previous commit SHAs
        self.verify.invalidate_stale_verifications(task_id, &head_sha)?;

        // 5. Automatically execute comprehensive verification profile and machine evaluators
        self.verify.execute_profile_for_attempt(
            task_id,
            &attempt_id,
            &task.project_id,
            worktree_dir,
            &head_sha,
        )?;

        // 6. Perform actual Git mutation audit against held scope leases for this attempt
        let changed_files = if let Some(ref base) = task.base_sha {
            self.git.get_worktree_mutations(worktree_dir, base).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.scope.audit_attempt_mutations(task_id, Some(&attempt_id), agent_id, &changed_files)?;

        // 7. Verification checks gate evaluation
        let verify_res = self.verify.verify_task_submission(task_id, &head_sha)?;

        let now_finished = Utc::now().to_rfc3339();

        if verify_res.is_valid {
            // 8. Generate deterministic Proof-of-Completion bundle (strictly required before MERGE_READY)
            self.verify.generate_proof_bundle(
                task_id,
                &task.project_id,
                Some(agent_id),
                &task.description,
                task.base_sha.as_deref().unwrap_or(""),
                &head_sha,
                &changed_files,
                "Authoritative Coordinator Automated Verification Passed",
            ).map_err(|e| format!("Failed to generate proof bundle: {}", e))?;

            // 9. Auto-enqueue for serialized merge queue (strictly required before MERGE_READY)
            self.enqueue_task_by_id(&task.project_id, task_id)
                .map_err(|e| format!("Failed to enqueue task in merge queue: {}", e))?;

            // 10. Atomically transition state to MERGE_READY & attempt to VERIFIED
            let conn = self.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'MERGE_READY', substate = 'NONE', head_sha = ?1, updated_at = ?2 WHERE id = ?3",
                params![head_sha, now_finished, task_id],
            ).map_err(|e| format!("Failed to transition task to MERGE_READY: {}", e))?;

            conn.execute(
                "UPDATE task_attempts SET status = 'VERIFIED', head_sha = ?1, finished_at = ?2 WHERE id = ?3",
                params![head_sha, now_finished, attempt_id],
            ).map_err(|e| format!("Failed to update task attempt status: {}", e))?;
            drop(conn);

            self.emit_event(Some(&task.project_id), Some(task_id), Some(agent_id), "TASK_VERIFIED", json!({ "head_sha": head_sha }));
        } else {
            let reasons_json = serde_json::to_string(&verify_res.rejection_reasons).unwrap_or_else(|_| "[]".to_string());
            let conn = self.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'FAILED', substate = 'NONE', updated_at = ?1 WHERE id = ?2",
                params![now_finished, task_id],
            ).map_err(|e| format!("Failed to set task to FAILED: {}", e))?;

            conn.execute(
                "UPDATE task_attempts SET status = 'FAILED', rejection_reasons = ?1, finished_at = ?2 WHERE id = ?3",
                params![reasons_json, now_finished, attempt_id],
            ).map_err(|e| format!("Failed to update task attempt status: {}", e))?;
            drop(conn);

            self.emit_event(Some(&task.project_id), Some(task_id), Some(agent_id), "TASK_VERIFICATION_FAILED", json!({ "reasons": verify_res.rejection_reasons }));
        }

        Ok(verify_res)
    }

    /// Backend-Authoritative Enqueue by Task ID
    pub fn enqueue_task_by_id(&self, project_id: &str, task_id: &str) -> Result<MergeQueueItem, String> {
        let task = self.get_task(task_id)?;
        if task.project_id != project_id {
            return Err(format!("Task '{}' does not belong to project '{}'", task_id, project_id));
        }

        if task.state != TaskState::Review && task.state != TaskState::MergeReady && task.state != TaskState::Verifying {
            return Err(format!("Task '{}' is in state '{:?}'. Only tasks in VERIFYING, REVIEW, or MERGE_READY state can be enqueued for merge.", task_id, task.state));
        }

        let proj = self.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;
        let repo_path = Path::new(&proj.path);

        let branch_name = task.branch_name.ok_or("Task has no branch name allocated")?;
        let worktree_dir = match task.worktree_path.as_deref() {
            Some(p) => Path::new(p),
            None => return Err("Task has no worktree allocated".to_string()),
        };

        let head_sha = self.git.get_worktree_head_sha(worktree_dir)?;

        // Ensure proof bundle exists for this HEAD
        let conn = self.db.lock();
        let has_proof: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proof_bundles WHERE task_id = ?1 AND head_sha = ?2",
            rusqlite::params![task_id, head_sha],
            |r| r.get(0),
        ).unwrap_or(0);
        drop(conn);

        if has_proof == 0 {
            return Err(format!("No valid proof bundle found for task '{}' at commit HEAD {}. Verification is required.", task_id, head_sha));
        }

        // The queue base is the target branch state observed when this item is
        // enqueued, not the task's original claim base. Earlier queued merges
        // are serialized ahead of this item and may have advanced the target
        // branch in the meantime.
        let base_sha = self
            .git
            .get_ref_sha(repo_path, &proj.target_branch)
            .or_else(|_| Ok::<String, String>(task.base_sha.unwrap_or_default()))?;

        let item = self.merge.enqueue_task(
            project_id,
            task_id,
            &branch_name,
            &proj.target_branch,
            &base_sha,
            &head_sha,
        )?;

        // Mark task as MERGE_READY
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute("UPDATE tasks SET state = 'MERGE_READY', updated_at = ?1 WHERE id = ?2", [&now, task_id]).ok();
        drop(conn);

        self.emit_event(Some(project_id), Some(task_id), None, "TASK_ENQUEUED_FOR_MERGE", json!({ "position": item.position, "branch": branch_name }));
        Ok(item)
    }

    // --- Agents ---
    pub fn register_agent(&self, name: &str, agent_type: &str) -> Result<Agent, String> {
        let conn = self.db.lock();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + chrono::Duration::hours(24)).to_rfc3339();

        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id, session_token FROM agents WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        let (id, session_token) = if let Some((existing_id, existing_token)) = existing {
            // Idempotent: refresh heartbeat, type, and session lease for existing agent
            conn.execute(
                "UPDATE agents SET last_heartbeat = ?1, status = 'IDLE', agent_type = ?2 WHERE id = ?3",
                params![now_str, agent_type, existing_id],
            ).ok();
            conn.execute(
                "UPDATE agent_sessions SET expires_at = ?1, last_activity_at = ?2 WHERE agent_id = ?3",
                params![expires_str, now_str, existing_id],
            ).ok();
            (existing_id, existing_token)
        } else {
            let new_id = Uuid::new_v4().to_string();
            let new_token = format!("axf_sess_{}", Uuid::new_v4().simple());
            conn.execute(
                "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at, session_token)
                 VALUES (?1, ?2, ?3, 'Implementer', 'IDLE', ?4, ?4, ?5)",
                params![new_id, name, agent_type, now_str, new_token],
            ).map_err(|e| e.to_string())?;

            let sess_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
                params![sess_id, new_id, new_token, now_str, expires_str],
            ).map_err(|e| e.to_string())?;

            (new_id, new_token)
        };

        let agent = Agent {
            id: id.clone(),
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: now_str.clone(),
            created_at: now_str,
            session_token: Some(session_token),
        };

        drop(conn);
        self.emit_event(None, None, Some(&id), "AGENT_REGISTERED", json!({ "name": name, "type": agent_type }));
        Ok(agent)
    }

    pub fn get_agent_by_session(&self, token: &str) -> Option<Agent> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        let agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM agent_sessions WHERE session_token = ?1 AND expires_at > ?2",
                rusqlite::params![token, now],
                |r| r.get(0),
            )
            .ok()?;

        let mut stmt = conn
            .prepare("SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents WHERE id = ?1")
            .ok()?;

        let agent = stmt
            .query_row([&agent_id], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    agent_type: row.get(2)?,
                    profile: row.get(3)?,
                    status: row.get(4)?,
                    capabilities: AgentCapabilitySet::default(),
                    last_heartbeat: row.get(5)?,
                    created_at: row.get(6)?,
                    session_token: Some(token.to_string()),
                })
            })
            .ok();

        if agent.is_some() {
            conn.execute(
                "UPDATE agent_sessions SET last_activity_at = ?1 WHERE session_token = ?2",
                rusqlite::params![now, token],
            ).ok();
        }

        agent
    }

    pub fn satisfy_acceptance_criterion(
        &self,
        task_id: &str,
        criterion_id: &str,
        evidence: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let rows_affected = conn.execute(
            "UPDATE acceptance_criteria SET is_satisfied = 1 WHERE id = ?1 AND task_id = ?2",
            rusqlite::params![criterion_id, task_id],
        ).map_err(|e| e.to_string())?;

        if rows_affected == 0 {
            return Err(format!("Criterion '{}' for task '{}' not found", criterion_id, task_id));
        }

        let ev_id = Uuid::new_v4().to_string();
        let note = evidence.unwrap_or("Manual User / Verification Sign-off");
        conn.execute(
            "INSERT INTO evidence_records (id, task_id, step_id, evidence_type, source, payload_json, recorded_at)
             VALUES (?1, ?2, NULL, 'USER_APPROVAL', 'COORDINATOR_OBSERVED', ?3, ?4)",
            rusqlite::params![ev_id, task_id, note, now],
        ).ok();

        drop(conn);
        self.emit_event(None, Some(task_id), None, "CRITERIA_SATISFIED", json!({ "criterion_id": criterion_id }));
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, agent_type, profile, status, last_heartbeat, created_at FROM agents ORDER BY name")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    agent_type: row.get(2)?,
                    profile: row.get(3)?,
                    status: row.get(4)?,
                    capabilities: AgentCapabilitySet::default(),
                    last_heartbeat: row.get(5)?,
                    created_at: row.get(6)?,
                    session_token: None,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut res = Vec::new();
        for r in rows.flatten() {
            res.push(r);
        }
        Ok(res)
    }

    pub fn unregister_agent(&self, agent_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        conn.execute("UPDATE tasks SET assigned_agent_id = NULL WHERE assigned_agent_id = ?1", [agent_id]).ok();
        conn.execute("DELETE FROM scope_leases WHERE agent_id = ?1", [agent_id]).ok();
        conn.execute("DELETE FROM agents WHERE id = ?1", [agent_id]).map_err(|e| e.to_string())?;
        drop(conn);

        self.emit_event(None, None, Some(agent_id), "AGENT_UNREGISTERED", json!({ "agent_id": agent_id }));
        Ok(())
    }

    pub fn is_agent_registered(&self, agent_id: &str) -> bool {
        if agent_id.trim().is_empty() {
            return false;
        }
        let conn = self.db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agents WHERE id = ?1", [agent_id], |r| r.get(0))
            .unwrap_or(0);
        count > 0
    }

    pub fn agent_heartbeat(&self, agent_id: &str) -> Result<(), String> {
        let conn = self.db.lock();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + chrono::Duration::hours(4)).to_rfc3339();

        let updated = conn.execute(
            "UPDATE agents SET last_heartbeat = ?1, status = 'WORKING' WHERE id = ?2",
            params![now_str, agent_id],
        ).map_err(|e| e.to_string())?;

        if updated == 0 {
            return Err(format!("Agent '{}' not found", agent_id));
        }

        // Transactionally renew active scope leases for this agent
        conn.execute("UPDATE scope_leases SET expires_at = ?1 WHERE agent_id = ?2", params![expires_str, agent_id]).ok();

        Ok(())
    }

    pub fn get_context_pack(&self, project_id: &str, task_id: &str) -> Result<ContextPack, String> {
        let task = self.get_task(task_id)?;
        if task.project_id != project_id {
            return Err(format!("Task '{}' belongs to project '{}', not '{}'", task_id, task.project_id, project_id));
        }

        let proj = self.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;

        let conn = self.db.lock();

        // 1. Contract & Hash
        let (contract_hash, contract_overview): (String, String) = conn
            .query_row(
                "SELECT contract_hash, overview FROM project_contracts WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| {
                let mut hasher = Sha256::new();
                hasher.update(proj.master_spec.as_bytes());
                (hex::encode(hasher.finalize()), proj.master_spec.clone())
            });

        // 2. Real Project Rules
        let mut stmt_rules = conn
            .prepare("SELECT rule_text FROM project_rules WHERE project_id = ?1 ORDER BY created_at ASC")
            .map_err(|e| e.to_string())?;
        let project_rules: Vec<String> = stmt_rules
            .query_map([project_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        // 3. Real Project Memory
        let mut stmt_mem = conn
            .prepare("SELECT content FROM project_memory WHERE project_id = ?1 ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;
        let project_memory: Vec<String> = stmt_mem
            .query_map([project_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        // 4. Steps & Criteria
        let mut stmt_steps = conn.prepare("SELECT id, task_id, order_index, title, description, is_mandatory, status, completed_at FROM task_steps WHERE task_id = ?1 ORDER BY order_index ASC").map_err(|e| e.to_string())?;
        let steps: Vec<TaskStep> = stmt_steps.query_map([task_id], |r| {
            Ok(TaskStep { id: r.get(0)?, task_id: r.get(1)?, order_index: r.get(2)?, title: r.get(3)?, description: r.get(4)?, is_mandatory: r.get(5)?, status: r.get(6)?, completed_at: r.get(7)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        let mut stmt_crit = conn.prepare("SELECT id, task_id, criterion, is_satisfied, is_locked FROM acceptance_criteria WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let criteria: Vec<AcceptanceCriteria> = stmt_crit.query_map([task_id], |r| {
            Ok(AcceptanceCriteria { id: r.get(0)?, task_id: r.get(1)?, criterion: r.get(2)?, is_satisfied: r.get(3)?, is_locked: r.get(4)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        // 5. Blocking Dependencies
        let mut stmt_deps = conn.prepare("SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1 AND dependency_type = 'BLOCKS'").map_err(|e| e.to_string())?;
        let dependencies: Vec<String> = stmt_deps.query_map([task_id], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();

        // 6. Scope Leases
        let mut stmt_leases = conn.prepare("SELECT id, task_id, agent_id, pattern, access_type, expires_at, created_at FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
        let leases: Vec<ScopeLease> = stmt_leases.query_map([task_id], |r| {
            Ok(ScopeLease { id: r.get(0)?, task_id: r.get(1)?, agent_id: r.get(2)?, pattern: r.get(3)?, access_type: r.get(4)?, expires_at: r.get(5)?, created_at: r.get(6)? })
        }).map_err(|e| e.to_string())?.flatten().collect();

        Ok(ContextPack {
            project_id: proj.id,
            project_name: proj.name,
            contract_hash,
            contract_overview,
            project_rules,
            project_memory,
            task_id: task.id,
            task_title: task.title,
            task_prompt: task.description,
            task_state: task.state.as_str().to_string(),
            task_substate: task.substate.as_str().to_string(),
            acceptance_criteria: criteria,
            required_steps: steps,
            dependencies,
            reserved_scope: leases,
            current_worktree: task.worktree_path,
            current_branch: task.branch_name,
            base_sha: task.base_sha,
            head_sha: task.head_sha,
        })
    }

    pub fn create_or_update_masterplan(
        &self,
        project_id: &str,
        raw_text: &str,
        target_step_count: i32,
        max_steps_per_agent: i32,
    ) -> Result<Masterplan, String> {
        let now = Utc::now().to_rfc3339();

        let existing: Option<(String, String)> = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT id, raw_text FROM masterplans WHERE project_id = ?1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
        };

        // Cancel all unmerged in-flight tasks and release scopes for the project
        let uncompleted_tasks: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn
                .prepare("SELECT id FROM tasks WHERE project_id = ?1 AND state != 'DONE' AND state != 'CANCELLED'")
                .map_err(|e| e.to_string())?;
            let ids = stmt.query_map([project_id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .flatten()
                .collect();
            ids
        };

        for tid in &uncompleted_tasks {
            let _ = self.cancel_task(tid, None, Some("Masterplan created/updated with new specification text"));
        }

        let plan_id = if let Some((id, old_text)) = existing {
            let conn = self.db.lock();
            // Archive existing plan into masterplan_revisions before updating
            let rev_id = Uuid::new_v4().to_string();
            let rev_num: i32 = conn.query_row(
                "SELECT COALESCE(MAX(revision_number), 0) + 1 FROM masterplan_revisions WHERE masterplan_id = ?1",
                [&id],
                |r| r.get(0),
            ).unwrap_or(1);

            conn.execute(
                "INSERT INTO masterplan_revisions (id, masterplan_id, project_id, revision_number, raw_text, reason, steps_snapshot_json, archived_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'User edited masterplan specification', '[]', ?6)",
                rusqlite::params![rev_id, id, project_id, rev_num, old_text, now],
            ).ok();

            conn.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&id])
                .map_err(|e| e.to_string())?;

            conn.execute(
                "UPDATE masterplans SET raw_text = ?1, status = 'UNSORTED', target_step_count = ?2, max_steps_per_agent = ?3, updated_at = ?4 WHERE id = ?5",
                params![raw_text, target_step_count, max_steps_per_agent, now, id],
            ).map_err(|e| e.to_string())?;
            id
        } else {
            let id = Uuid::new_v4().to_string();
            let conn = self.db.lock();
            conn.execute(
                "INSERT INTO masterplans (id, project_id, raw_text, status, target_step_count, max_steps_per_agent, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'UNSORTED', ?4, ?5, ?6, ?6)",
                params![id, project_id, raw_text, target_step_count, max_steps_per_agent, now],
            ).map_err(|e| e.to_string())?;
            id
        };

        self.emit_event(Some(project_id), None, None, "MASTERPLAN_UPDATED", json!({ "status": "UNSORTED", "plan_id": plan_id }));

        Ok(Masterplan {
            id: plan_id,
            project_id: project_id.to_string(),
            raw_text: raw_text.to_string(),
            status: "UNSORTED".to_string(),
            target_step_count,
            max_steps_per_agent,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn get_masterplan(&self, project_id: &str) -> Result<Option<Masterplan>, String> {
        let conn = self.db.lock();
        let plan = conn
            .query_row(
                "SELECT id, project_id, raw_text, status, target_step_count, max_steps_per_agent, created_at, updated_at FROM masterplans WHERE project_id = ?1",
                [project_id],
                |r| {
                    Ok(Masterplan {
                        id: r.get(0)?,
                        project_id: r.get(1)?,
                        raw_text: r.get(2)?,
                        status: r.get(3)?,
                        target_step_count: r.get(4)?,
                        max_steps_per_agent: r.get(5)?,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                    })
                },
            )
            .ok();
        Ok(plan)
    }

    pub fn list_masterplan_steps(&self, project_id: &str) -> Result<Vec<MasterplanStep>, String> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT ms.id, ms.masterplan_id, ms.step_index, ms.title, ms.description, ms.suggested_scope, ms.acceptance_criteria, ms.status, ms.claimed_agent_id, ms.claimed_task_id, ms.completed_at, ms.created_at, ms.updated_at
                 FROM masterplan_steps ms
                 JOIN masterplans m ON ms.masterplan_id = m.id
                 WHERE m.project_id = ?1
                 ORDER BY ms.step_index ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([project_id], |r| {
                Ok(MasterplanStep {
                    id: r.get(0)?,
                    masterplan_id: r.get(1)?,
                    step_index: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    suggested_scope: r.get(5)?,
                    acceptance_criteria: r.get(6)?,
                    status: r.get(7)?,
                    claimed_agent_id: r.get(8)?,
                    claimed_task_id: r.get(9)?,
                    completed_at: r.get(10)?,
                    created_at: r.get(11)?,
                    updated_at: r.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;

        Ok(rows.flatten().collect())
    }

    pub fn decompose_masterplan(
        &self,
        project_id: &str,
        steps: Vec<DecomposedStepInput>,
    ) -> Result<Vec<MasterplanStep>, String> {
        if steps.is_empty() {
            return Err("Cannot decompose masterplan with empty step list".to_string());
        }

        let plan = self
            .get_masterplan(project_id)?
            .ok_or_else(|| format!("No masterplan found for project '{}'", project_id))?;

        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let tx = conn.transaction().map_err(|e| format!("Failed to start transaction: {}", e))?;

        // Invariant check 1: Reject re-decomposition if any steps in current plan are active
        let active_claims: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE masterplan_id = ?1 AND status != 'PENDING'",
                [&plan.id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if active_claims > 0 {
            return Err(format!(
                "Cannot re-decompose masterplan: {} step(s) are actively claimed, in-progress, or completed. Reset the plan first via 'reset_masterplan' or submit active chunks.",
                active_claims
            ));
        }

        // Invariant check 2: Reject re-decomposition if any active non-stale tasks are currently in flight for the project
        let active_project_tasks: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND state IN ('RUNNING', 'VERIFYING', 'VERIFIED', 'REVIEW', 'MERGE_READY') AND is_stale = 0",
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if active_project_tasks > 0 {
            return Err(format!(
                "Cannot re-decompose masterplan: {} active task(s) are currently in flight for project '{}'. Complete, submit, or cancel active tasks first (or call reset_masterplan).",
                active_project_tasks, project_id
            ));
        }

        tx.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&plan.id])
            .map_err(|e| e.to_string())?;

        let mut inserted_steps = Vec::new();

        for s in steps {
            let step_id = Uuid::new_v4().to_string();
            let suggested_scope = s.suggested_scope.unwrap_or_else(|| "src/**".to_string());
            let criteria = s.acceptance_criteria.unwrap_or_else(|| "All automated tests pass".to_string());

            tx.execute(
                "INSERT INTO masterplan_steps (id, masterplan_id, step_index, title, description, suggested_scope, acceptance_criteria, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PENDING', ?8, ?8)",
                params![step_id, plan.id, s.step_index, s.title, s.description, suggested_scope, criteria, now],
            ).map_err(|e| e.to_string())?;

            inserted_steps.push(MasterplanStep {
                id: step_id,
                masterplan_id: plan.id.clone(),
                step_index: s.step_index,
                title: s.title,
                description: s.description,
                suggested_scope,
                acceptance_criteria: criteria,
                status: "PENDING".to_string(),
                claimed_agent_id: None,
                claimed_task_id: None,
                completed_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }

        tx.execute(
            "UPDATE masterplans SET status = 'RESORTED', updated_at = ?1 WHERE id = ?2",
            params![now, plan.id],
        ).map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| format!("Failed to commit masterplan decomposition: {}", e))?;
        drop(conn);

        self.emit_event(Some(project_id), None, None, "MASTERPLAN_DECOMPOSED", json!({ "total_steps": inserted_steps.len(), "status": "RESORTED" }));

        Ok(inserted_steps)
    }

    /// Transactional, race-safe chunk reservation with complete compensation and atomic scope acquisition
    pub fn claim_masterplan_chunk(
        &self,
        project_id: &str,
        agent_id: &str,
        requested_count: Option<i32>,
    ) -> Result<Task, String> {
        if !self.is_agent_registered(agent_id) {
            return Err(format!(
                "Agent registration required: Agent ID '{}' is not registered. Call 'agent.register' first.",
                agent_id
            ));
        }

        let plan = self
            .get_masterplan(project_id)?
            .ok_or_else(|| format!("No masterplan found for project '{}'", project_id))?;

        if plan.status == "UNSORTED" {
            return Err("Cannot claim steps from an UNSORTED masterplan. Decompose the plan first via 'masterplan.decompose'.".to_string());
        }

        if let Some(rc) = requested_count {
            if rc <= 0 {
                return Err(format!("Invalid chunk count {}: requested count must be greater than 0.", rc));
            }
        }

        let mut conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // 1. Check active anti-hoarding limit inside transaction
        let tx = conn.transaction().map_err(|e| format!("Failed to start transaction: {}", e))?;

        let currently_active: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE claimed_agent_id = ?1 AND status = 'CLAIMED'",
                [agent_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if currently_active >= plan.max_steps_per_agent as i64 {
            return Err(format!(
                "Anti-hoarding cap reached: Agent '{}' already has {} active claimed steps. Submit or complete your current chunk before claiming more.",
                agent_id, currently_active
            ));
        }

        let allowed = (plan.max_steps_per_agent as i64 - currently_active).max(1) as i32;
        let count = requested_count.unwrap_or(allowed).min(allowed).max(1);

        // 2. Select and atomically reserve pending steps
        let mut stmt = tx
            .prepare("SELECT id, step_index, title, description, suggested_scope, acceptance_criteria FROM masterplan_steps WHERE masterplan_id = ?1 AND status = 'PENDING' ORDER BY step_index ASC LIMIT ?2")
            .map_err(|e| e.to_string())?;

        let reserved_steps_iter = stmt
            .query_map(rusqlite::params![plan.id, count], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut reserved_steps = Vec::new();
        for r in reserved_steps_iter.flatten() {
            reserved_steps.push(r);
        }
        drop(stmt);

        if reserved_steps.is_empty() {
            return Err("No pending steps available in masterplan. All steps are either claimed or completed.".to_string());
        }

        let step_ids: Vec<String> = reserved_steps.iter().map(|(id, ..)| id.clone()).collect();
        for id in &step_ids {
            tx.execute(
                "UPDATE masterplan_steps SET status = 'CLAIMED', claimed_agent_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![agent_id, now, id],
            ).map_err(|e| e.to_string())?;
        }

        tx.commit().map_err(|e| format!("Failed to commit step reservation: {}", e))?;
        drop(conn);

        let first_idx = reserved_steps.first().unwrap().1;
        let last_idx = reserved_steps.last().unwrap().1;

        let task_title = format!("Masterplan Chunk: Steps {}-{} ({})", first_idx, last_idx, reserved_steps.first().unwrap().2);
        let task_desc = reserved_steps
            .iter()
            .map(|(_, idx, title, desc, ..)| format!("Step #{}: {}\n{}", idx, title, desc))
            .collect::<Vec<String>>()
            .join("\n\n---\n\n");

        let task_steps: Vec<(String, String, bool)> = reserved_steps
            .iter()
            .map(|(_, idx, title, desc, ..)| (format!("Step #{}: {}", idx, title), desc.clone(), true))
            .collect();

        let criteria: Vec<String> = reserved_steps
            .iter()
            .map(|(.., crit)| crit.clone())
            .collect();

        // 3. Create and Claim Task with atomic scope lease acquisition and complete rollback on collision
        let mut created_task_id: Option<String> = None;
        let task_res = (|| -> Result<Task, String> {
            let task = self.create_task_internal(
                project_id,
                Some(&plan.id),
                None,
                &task_title,
                &task_desc,
                "HIGH",
                task_steps,
                criteria,
            )?;
            created_task_id = Some(task.id.clone());

            let claimed_task = self.claim_task(&task.id, agent_id)?;

            let scope_patterns: Vec<String> = reserved_steps
                .iter()
                .map(|(.., scope, _)| scope.clone())
                .filter(|s| !s.trim().is_empty())
                .collect();

            if !scope_patterns.is_empty() {
                // Authoritative atomic scope acquisition: failure bubbles immediately (NO .ok() silent ignore)
                self.scope.acquire_scope(&claimed_task.id, agent_id, scope_patterns, "EXCLUSIVE_WRITE")?;
            }

            Ok(claimed_task)
        })();

        match task_res {
            Ok(claimed_task) => {
                let conn = self.db.lock();
                for id in &step_ids {
                    conn.execute(
                        "UPDATE masterplan_steps SET claimed_task_id = ?1 WHERE id = ?2",
                        params![claimed_task.id, id],
                    ).ok();
                }
                conn.execute(
                    "UPDATE masterplans SET status = 'EXECUTING', updated_at = ?1 WHERE id = ?2",
                    params![now, plan.id],
                ).ok();
                Ok(claimed_task)
            }
            Err(err) => {
                // Complete Compensation:
                // 1. If task was created, release scopes, remove worktree, and cancel task
                if let Some(ref tid) = created_task_id {
                    let _ = self.cancel_task(tid, Some(agent_id), Some("Claim failed during scope acquisition or initialization"));
                }
                // 2. Revert reserved steps back to PENDING
                let conn = self.db.lock();
                for id in &step_ids {
                    conn.execute(
                        "UPDATE masterplan_steps SET status = 'PENDING', claimed_agent_id = NULL, claimed_task_id = NULL WHERE id = ?1",
                        [id],
                    ).ok();
                }
                Err(format!("Failed to claim masterplan chunk: {}", err))
            }
        }
    }

    pub fn reset_masterplan(&self, project_id: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();

        // 1. Invalidate and cancel all in-flight unmerged tasks for this project and release their scopes
        let active_tasks: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare(
                "SELECT id FROM tasks WHERE project_id = ?1 AND state != 'DONE' AND state != 'CANCELLED'"
            ).map_err(|e| e.to_string())?;
            let ids = stmt.query_map([project_id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .flatten()
                .collect();
            ids
        };

        for tid in &active_tasks {
            let _ = self.cancel_task(tid, None, Some("Masterplan was reset; active task invalidated"));
        }

        let conn = self.db.lock();
        let plan_opt: Option<(String, String)> = conn
            .query_row(
                "SELECT id, raw_text FROM masterplans WHERE project_id = ?1",
                [project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        if let Some((plan_id, raw_text)) = plan_opt {
            // Archive prior plan before reset
            let rev_id = Uuid::new_v4().to_string();
            let rev_num: i32 = conn.query_row(
                "SELECT COALESCE(MAX(revision_number), 0) + 1 FROM masterplan_revisions WHERE masterplan_id = ?1",
                [&plan_id],
                |r| r.get(0),
            ).unwrap_or(1);

            conn.execute(
                "INSERT INTO masterplan_revisions (id, masterplan_id, project_id, revision_number, raw_text, reason, steps_snapshot_json, archived_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'Masterplan reset to empty', '[]', ?6)",
                rusqlite::params![rev_id, plan_id, project_id, rev_num, raw_text, now],
            ).ok();

            conn.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&plan_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM masterplans WHERE id = ?1", [&plan_id]).map_err(|e| e.to_string())?;
        }
        drop(conn);
        self.emit_event(Some(project_id), None, None, "MASTERPLAN_RESET", json!({ "project_id": project_id }));
        Ok(())
    }

    pub fn list_all_masterplans(&self) -> Result<Vec<MasterplanSummary>, String> {
        let projects = self.list_projects()?;
        let conn = self.db.lock();
        let mut summaries = Vec::new();

        for proj in projects {
            let plan_res = conn.query_row(
                "SELECT id, status, target_step_count, max_steps_per_agent, updated_at FROM masterplans WHERE project_id = ?1",
                [&proj.id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i32>(2)?, r.get::<_, i32>(3)?, r.get::<_, String>(4)?)),
            ).ok();

            if let Some((plan_id, status, target_step_count, max_steps_per_agent, updated_at)) = plan_res {
                let mut stmt = match conn.prepare("SELECT status FROM masterplan_steps WHERE masterplan_id = ?1") {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let step_statuses: Vec<String> = stmt
                    .query_map([&plan_id], |r| r.get(0))
                    .map(|iter| iter.flatten().collect())
                    .unwrap_or_default();

                let total = step_statuses.len();
                let pending = step_statuses.iter().filter(|s| s.as_str() == "PENDING").count();
                let claimed = step_statuses.iter().filter(|s| s.as_str() == "CLAIMED" || s.as_str() == "IN_PROGRESS").count();
                let completed = step_statuses.iter().filter(|s| s.as_str() == "COMPLETED").count();

                let next_action = if status == "UNSORTED" {
                    "masterplan_decompose".to_string()
                } else if pending > 0 {
                    "masterplan_claim_chunk".to_string()
                } else {
                    "all_steps_claimed_or_completed".to_string()
                };

                let handoff_prompt = if status == "UNSORTED" {
                    format!("Decompose masterplan for project '{}' (ID: {}) located at '{}' using tool 'masterplan_decompose'.", proj.name, proj.id, proj.path)
                } else {
                    format!("Claim next available chunk for project '{}' (ID: {}) located at '{}' using tool 'masterplan_claim_chunk'.", proj.name, proj.id, proj.path)
                };

                summaries.push(MasterplanSummary {
                    project_id: proj.id,
                    project_name: proj.name,
                    repository_path: proj.path,
                    masterplan_id: plan_id,
                    status,
                    target_step_count,
                    max_steps_per_agent,
                    total_steps: total,
                    pending_steps: pending,
                    claimed_steps: claimed,
                    completed_steps: completed,
                    last_updated: updated_at,
                    next_action,
                    handoff_prompt,
                });
            }
        }

        Ok(summaries)
    }

    /// Automatically parses raw masterplan specification into structured execution steps
    pub fn parse_masterplan_text_to_steps(&self, raw_text: &str, _target_step_count: i32) -> Result<Vec<DecomposedStepInput>, String> {
        let infer_scope = |title: &str, desc: &str| -> String {
            let lower = format!("{} {}", title, desc).to_lowercase();
            if lower.contains("backend") || lower.contains("rust") || lower.contains("src-tauri") || lower.contains("tauri") || lower.contains("mcp") || lower.contains("coordinator") || lower.contains("sqlite") || lower.contains("migration") || lower.contains("database") {
                "src-tauri/**".to_string()
            } else if lower.contains("frontend") || lower.contains("ui") || lower.contains("component") || lower.contains("react") || lower.contains("css") || lower.contains("view") || lower.contains("workbench") || lower.contains("modal") {
                "src/**".to_string()
            } else if lower.contains("crates/") {
                "crates/**".to_string()
            } else if lower.contains("packages/") {
                "packages/**".to_string()
            } else if lower.contains("apps/") {
                "apps/**".to_string()
            } else if lower.contains("test") || lower.contains("tests/") {
                "tests/**".to_string()
            } else if lower.contains("doc") || lower.contains("readme") || lower.contains("specification") {
                "*.md".to_string()
            } else {
                "**".to_string()
            }
        };

        let mut steps = Vec::new();
        let lines: Vec<&str> = raw_text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        let mut current_title: Option<String> = None;
        let mut current_desc: Vec<String> = Vec::new();
        let mut step_index = 1;

        for line in lines {
            let is_header = line.starts_with("# ")
                || line.starts_with("## ")
                || line.starts_with("### ")
                || line.starts_with("Step ")
                || line.starts_with("Phase ")
                || (line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && (line.contains(". ") || line.contains(": ")))
                || line.starts_with("- [ ] ")
                || line.starts_with("- [x] ");

            if is_header {
                if let Some(title) = current_title.take() {
                    let desc = if current_desc.is_empty() {
                        title.clone()
                    } else {
                        current_desc.join("\n")
                    };
                    let scope = infer_scope(&title, &desc);
                    steps.push(DecomposedStepInput {
                        step_index,
                        title,
                        description: desc,
                        suggested_scope: Some(scope),
                        acceptance_criteria: Some("Code builds cleanly and all verification tests pass.".to_string()),
                    });
                    step_index += 1;
                    current_desc.clear();
                }

                let clean_title = line
                    .trim_start_matches('#')
                    .trim_start_matches('-')
                    .trim_start_matches('[')
                    .trim_start_matches(']')
                    .trim_start_matches('x')
                    .trim_start_matches(' ')
                    .trim();

                current_title = Some(clean_title.to_string());
            } else if current_title.is_some() {
                current_desc.push(line.to_string());
            } else {
                current_title = Some(line.to_string());
            }
        }

        if let Some(title) = current_title {
            let desc = if current_desc.is_empty() {
                title.clone()
            } else {
                current_desc.join("\n")
            };
            let scope = infer_scope(&title, &desc);
            steps.push(DecomposedStepInput {
                step_index,
                title,
                description: desc,
                suggested_scope: Some(scope),
                acceptance_criteria: Some("Code builds cleanly and all verification tests pass.".to_string()),
            });
        }

        if steps.is_empty() {
            steps.push(DecomposedStepInput {
                step_index: 1,
                title: "Execute Masterplan Specification".to_string(),
                description: raw_text.to_string(),
                suggested_scope: Some("**".to_string()),
                acceptance_criteria: Some("Code builds cleanly and all verification tests pass.".to_string()),
            });
        }

        Ok(steps)
    }

    /// Automatically processes the next serialized merge in queue for a project
    pub fn process_next_merge(&self, project_id: &str) -> Result<Option<IntegrationAttempt>, String> {
        let proj = self.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;
        let queue = self.merge.list_queue(project_id)?;
        let next_ready = queue.into_iter().find(|item| item.status == "READY");
        if let Some(item) = next_ready {
            let attempt = self.merge.process_merge_by_id(&item.id, Path::new(&proj.path))?;
            if let Err(error) = self.scope.release_scope(&item.task_id) {
                warn!(task_id = %item.task_id, %error, "Merged task scopes could not be released automatically");
            }
            Ok(Some(attempt))
        } else {
            Ok(None)
        }
    }

    /// Reconciles task status, attempt, proof bundle, and merge queue health
    pub fn reconcile_task(&self, task_id: &str) -> Result<serde_json::Value, String> {
        let mut task = self.get_task(task_id)?;
        let conn = self.db.lock();
        let attempt: Option<(String, String)> = conn.query_row(
            "SELECT id, status FROM task_attempts WHERE task_id = ?1 ORDER BY attempt_number DESC LIMIT 1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        let has_proof: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM proof_bundles WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        ).unwrap_or(false);
        let queue_item: Option<(String, String, String)> = conn.query_row(
            "SELECT id, status, target_branch FROM merge_queue WHERE task_id = ?1 AND processed_at IS NULL",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).ok();
        drop(conn);

        let mut queue_status = queue_item.as_ref().map(|(_, status, _)| status.clone());

        // A verified task can become stale while an earlier FIFO candidate is
        // merged. Rebase the queue expectation to the current target and let
        // the normal merge simulation detect real conflicts.
        if task.state == TaskState::Blocked
            && queue_status.as_deref() == Some("STALE")
            && attempt.as_ref().map(|(_, status)| status.as_str()) == Some("VERIFIED")
            && has_proof
        {
            if let Some((queue_id, _, target_branch)) = queue_item.as_ref() {
                let project = self
                    .list_projects()?
                    .into_iter()
                    .find(|project| project.id == task.project_id)
                    .ok_or("Project not found")?;
                let target_sha = self.git.get_ref_sha(Path::new(&project.path), target_branch)?;
                let now = Utc::now().to_rfc3339();
                let conn = self.db.lock();
                conn.execute(
                    "UPDATE merge_queue SET base_sha = ?1, status = 'READY' WHERE id = ?2 AND status = 'STALE'",
                    params![target_sha, queue_id],
                ).map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE tasks SET state = 'MERGE_READY', substate = 'NONE', updated_at = ?1 WHERE id = ?2",
                    params![now, task_id],
                ).map_err(|e| e.to_string())?;
                drop(conn);
                task.state = TaskState::MergeReady;
                queue_status = Some("READY".to_string());
            }
        }

        // Auto-heal if MERGE_READY but not enqueued
        if task.state == TaskState::MergeReady && queue_status.is_none() {
            let _ = self.enqueue_task_by_id(&task.project_id, task_id);
        }

        Ok(serde_json::json!({
            "task_id": task.id,
            "state": task.state.as_str(),
            "attempt": attempt.map(|(id, st)| serde_json::json!({ "attempt_id": id, "status": st })),
            "has_proof_bundle": has_proof,
            "merge_queue_status": queue_status.unwrap_or_else(|| "NOT_ENQUEUED".to_string()),
        }))
    }

    /// Single atomic backend operation: Saves revision, parses steps, normalizes scopes, decomposes, and emits event
    pub fn prepare_masterplan(
        &self,
        project_id: &str,
        raw_text: &str,
        target_step_count: i32,
        max_steps_per_agent: i32,
    ) -> Result<PreparedMasterplanSnapshot, String> {
        let plan = self.create_or_update_masterplan(project_id, raw_text, target_step_count, max_steps_per_agent)?;
        let parsed_steps = self.parse_masterplan_text_to_steps(raw_text, target_step_count)?;
        let steps = self.decompose_masterplan(project_id, parsed_steps)?;

        let proj = self.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;
        let handoff_prompt = format!(
            "Claim next available chunk for project '{}' (ID: {}) located at '{}' using tool 'masterplan_claim_chunk'.",
            proj.name, proj.id, proj.path
        );

        self.emit_event(Some(project_id), None, None, "MASTERPLAN_PREPARED", json!({ "plan_id": plan.id, "step_count": steps.len() }));

        Ok(PreparedMasterplanSnapshot {
            masterplan: plan,
            total_steps: steps.len(),
            steps,
            target_step_count,
            max_steps_per_agent,
            handoff_prompt,
            next_action: "masterplan_claim_chunk".to_string(),
        })
    }

    #[allow(clippy::type_complexity)]
    pub fn get_current_context(&self, caller_agent_id: Option<&str>, project_id_filter: Option<&str>) -> Result<CurrentContext, String> {
        let summaries = self.list_all_masterplans()?;
        let agents = self.list_agents()?;
        let conn = self.db.lock();

        // 1. If caller has an active running task, prioritize caller task context
        if let Some(agent_id) = caller_agent_id {
            let active_task_opt: Option<(String, String, String, String, Option<String>, Option<String>)> = conn
                .query_row(
                    "SELECT t.id, t.project_id, t.title, t.state, t.worktree_path, p.name FROM tasks t
                     JOIN projects p ON t.project_id = p.id
                     WHERE t.assigned_agent_id = ?1 AND t.state IN ('RUNNING', 'CLAIMING', 'VERIFYING') AND t.is_stale = 0
                     ORDER BY t.updated_at DESC LIMIT 1",
                    [agent_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )
                .ok();

            if let Some((task_id, project_id, task_title, task_state, worktree_path, project_name)) = active_task_opt {
                let mut stmt_scopes = conn.prepare("SELECT pattern FROM scope_leases WHERE task_id = ?1").map_err(|e| e.to_string())?;
                let rows = stmt_scopes.query_map([&task_id], |r| r.get(0)).map_err(|e| e.to_string())?;
                let mut active_scopes: Vec<String> = Vec::new();
                for pattern in rows.flatten() {
                    active_scopes.push(pattern);
                }
                drop(stmt_scopes);

                let active_attempt_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM task_attempts WHERE task_id = ?1 AND status = 'ACTIVE' ORDER BY attempt_number DESC LIMIT 1",
                        [&task_id],
                        |r| r.get(0),
                    )
                    .ok();

                let now_str = Utc::now().to_rfc3339();
                drop(conn);

                let wt = worktree_path.unwrap_or_else(|| "N/A".to_string());
                return Ok(CurrentContext {
                    active_project_id: Some(project_id.clone()),
                    project_name,
                    repository_path: Some(wt.clone()),
                    masterplan_id: None,
                    masterplan_status: Some("EXECUTING".to_string()),
                    masterplan_revision: None,
                    caller_agent_id: Some(agent_id.to_string()),
                    active_task_id: Some(task_id.clone()),
                    active_attempt_id,
                    active_scopes,
                    current_state: Some(task_state),
                    last_updated: Some(now_str),
                    next_recommended_action: "task_submit".to_string(),
                    handoff_prompt: format!(
                        "Continue active task '{}' (ID: {}) in worktree '{}'. Implement required steps and submit with 'task_submit(task_id=\"{}\", agent_id=\"{}\")'.",
                        task_title, task_id, wt, task_id, agent_id
                    ),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: format!("Work strictly inside worktree '{}'. Verify code and submit via task_submit.", wt),
                });
            }
        }
        drop(conn);

        // 2. Otherwise return targeted or primary project masterplan context
        let target = if let Some(pid) = project_id_filter {
            summaries.iter().find(|s| s.project_id == pid).cloned()
        } else {
            summaries.first().cloned()
        };

        if let Some(target) = target {
            let instructions = if target.status == "UNSORTED" {
                format!(
                    "1. Call 'project_context(project_id=\"{}\")' to fetch rules.\n2. Call 'masterplan_get(project_id=\"{}\")' to read specification.\n3. Call 'masterplan_decompose(project_id=\"{}\", steps=[...])' to structure the plan.",
                    target.project_id, target.project_id, target.project_id
                )
            } else {
                format!(
                    "1. Call 'agent_register(name=\"...\", agent_type=\"...\")' to get session token.\n2. Call 'masterplan_claim_chunk(project_id=\"{}\", agent_id=your_id, count={})' to allocate worktree.\n3. Acquire scope and implement steps.",
                    target.project_id, target.max_steps_per_agent
                )
            };

            let pending_tasks = target.pending_steps;

            Ok(CurrentContext {
                active_project_id: Some(target.project_id.clone()),
                project_name: Some(target.project_name.clone()),
                repository_path: Some(target.repository_path.clone()),
                masterplan_id: Some(target.masterplan_id.clone()),
                masterplan_status: Some(target.status.clone()),
                masterplan_revision: None,
                caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                active_task_id: None,
                active_attempt_id: None,
                active_scopes: Vec::new(),
                current_state: None,
                last_updated: Some(target.last_updated.clone()),
                next_recommended_action: target.next_action.clone(),
                handoff_prompt: target.handoff_prompt.clone(),
                active_agents_count: agents.len(),
                pending_tasks_count: pending_tasks,
                instructions,
            })
        } else {
            let projects = self.list_projects()?;
            if let Some(proj) = projects.first() {
                Ok(CurrentContext {
                    active_project_id: Some(proj.id.clone()),
                    project_name: Some(proj.name.clone()),
                    repository_path: Some(proj.path.clone()),
                    masterplan_id: None,
                    masterplan_status: None,
                    masterplan_revision: None,
                    caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                    active_task_id: None,
                    active_attempt_id: None,
                    active_scopes: Vec::new(),
                    current_state: None,
                    last_updated: Some(proj.created_at.clone()),
                    next_recommended_action: "create_masterplan".to_string(),
                    handoff_prompt: format!("Create masterplan for project '{}' ({}) in the AgentXFlow UI.", proj.name, proj.id),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: "Create a masterplan in the AgentXFlow Workbench or add tasks.".to_string(),
                })
            } else {
                Ok(CurrentContext {
                    active_project_id: None,
                    project_name: None,
                    repository_path: None,
                    masterplan_id: None,
                    masterplan_status: None,
                    masterplan_revision: None,
                    caller_agent_id: caller_agent_id.map(|s| s.to_string()),
                    active_task_id: None,
                    active_attempt_id: None,
                    active_scopes: Vec::new(),
                    current_state: None,
                    last_updated: None,
                    next_recommended_action: "create_project".to_string(),
                    handoff_prompt: "No projects created yet. Create a project in AgentXFlow.".to_string(),
                    active_agents_count: agents.len(),
                    pending_tasks_count: 0,
                    instructions: "Open AgentXFlow and create or import a Git repository.".to_string(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
