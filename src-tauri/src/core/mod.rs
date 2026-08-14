use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::path::Path;
use tracing::info;
use uuid::Uuid;

use crate::acp::AcpRuntime;
use crate::dag::DagEngine;
use crate::db::DbPool;
use crate::error::CoordinatorError;
use crate::git::GitService;
use crate::merge::MergeEngine;
use crate::models::{
    AcceptanceCriteria, Agent, AgentCapabilitySet, ContextPack, DecomposedStepInput, EventItem,
    EvidenceRecord, Masterplan, MasterplanStep, ProofBundle, Project, ScopeLease, ScopeViolation,
    Task, TaskDependency, TaskDetails, TaskState, TaskStep, TaskSubstate, VerificationResult,
    VerificationRun,
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

        let mut items = Vec::new();
        for r in rows.flatten() {
            items.push(r);
        }
        Ok(items)
    }

    /// Startup Crash Recovery & Reconciliation
    pub fn reconcile_on_startup(&self) {
        info!("Running startup crash recovery & reconciliation...");
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        // 1. Expire stale scope leases
        conn.execute("DELETE FROM scope_leases WHERE expires_at < ?1", [&now]).ok();

        // 2. Mark active agent runs whose agents are no longer reachable as paused
        conn.execute(
            "UPDATE agent_runs SET status = 'PAUSED', finished_at = ?1 WHERE status = 'ACTIVE'",
            [&now],
        ).ok();

        // 3. Mark interrupted merge queue items as STALE
        conn.execute(
            "UPDATE merge_queue SET status = 'STALE' WHERE status = 'RUNNING_CHECKS'",
            [],
        ).ok();

        info!("Crash recovery reconciliation complete.");
    }

    /// Canonical state machine transition service
    pub fn transition_task_state(
        &self,
        task_id: &str,
        next_state: TaskState,
        next_substate: TaskSubstate,
        reason: Option<&str>,
    ) -> Result<Task, CoordinatorError> {
        let mut task = self.get_task(task_id).map_err(CoordinatorError::TaskNotFound)?;

        if !task.state.can_transition_to(&next_state) && task.state != next_state {
            return Err(CoordinatorError::InvalidStateTransition {
                from: task.state.as_str().to_string(),
                to: next_state.as_str().to_string(),
                reason: reason.unwrap_or("Transition not allowed by state machine rules").to_string(),
            });
        }

        let prev_state = task.state.as_str().to_string();
        let next_state_str = next_state.as_str().to_string();

        let now = Utc::now().to_rfc3339();
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET state = ?1, substate = ?2, updated_at = ?3 WHERE id = ?4",
            params![next_state.as_str(), next_substate.as_str(), now, task_id],
        ).map_err(|e| CoordinatorError::Database(e.to_string()))?;

        task.state = next_state;
        task.substate = next_substate;
        task.updated_at = now;

        drop(conn);
        self.emit_event(
            Some(&task.project_id),
            Some(task_id),
            task.assigned_agent_id.as_deref(),
            "TASK_STATE_TRANSITIONED",
            json!({
                "from_state": prev_state,
                "to_state": next_state_str,
                "reason": reason.unwrap_or("")
            }),
        );

        Ok(task)
    }

    // --- Projects ---
    pub fn create_project(&self, name: &str, path: &str, master_spec: &str, target_branch: &str) -> Result<Project, String> {
        let repo_path = Path::new(path);
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
    pub fn create_task(
        &self,
        project_id: &str,
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
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'BACKLOG', 'NONE', ?5, ?6, ?6)",
            params![id, project_id, title, description, priority, now],
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
            created_at: now.clone(),
            updated_at: now,
        };

        drop(conn);
        self.emit_event(Some(project_id), Some(&id), None, "TASK_CREATED", json!({ "title": title, "priority": priority }));
        Ok(task)
    }

    pub fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>, String> {
        let conn = self.db.lock();
        let query = if project_id.is_empty() {
            "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, created_at, updated_at FROM tasks ORDER BY created_at DESC"
        } else {
            "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, created_at, updated_at FROM tasks WHERE project_id = ?1 ORDER BY created_at DESC"
        };

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
                created_at: row.get(16)?,
                updated_at: row.get(17)?,
            })
        };

        let mut res = Vec::new();
        if project_id.is_empty() {
            let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
            for r in rows.flatten() {
                res.push(r);
            }
        } else {
            let rows = stmt.query_map([project_id], map_row).map_err(|e| e.to_string())?;
            for r in rows.flatten() {
                res.push(r);
            }
        }

        Ok(res)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Task, String> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, project_id, parent_id, epic_id, title, description, state, substate, assigned_agent_id, priority, risk_score, estimated_scope, worktree_path, branch_name, base_sha, head_sha, created_at, updated_at FROM tasks WHERE id = ?1",
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
                    created_at: row.get(16)?,
                    updated_at: row.get(17)?,
                })
            },
        ).map_err(|e| format!("Task '{}' not found: {}", task_id, e))
    }

    /// Comprehensive task details query for TaskWorkspace
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
            "SELECT task_id, project_id, agent_id, prompt, base_sha, head_sha, files_changed_json, diff_summary, proof_hash, generated_at FROM proof_bundles WHERE task_id = ?1",
            [task_id],
            |r| {
                let files_json: String = r.get(6)?;
                let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
                Ok(ProofBundle {
                    task_id: r.get(0)?,
                    project_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    prompt: r.get(3)?,
                    base_sha: r.get(4)?,
                    head_sha: r.get(5)?,
                    files_changed: files,
                    diff_summary: r.get(7)?,
                    verification_runs: Vec::new(),
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
        })
    }

    /// Atomic Task Claiming with Compare-and-Swap & Transactional Lock
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

        // 1. Transactional check & claim
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

        // Atomically update task to RUNNING and assign agent
        tx.execute(
            "UPDATE tasks SET state = 'RUNNING', substate = 'ANALYZING', assigned_agent_id = ?1, worktree_path = ?2, branch_name = ?3, base_sha = ?4, updated_at = ?5 WHERE id = ?6",
            params![agent_id, worktree_path_str, branch_name, base_sha, now, task_id],
        ).map_err(|e| format!("Failed to claim task in database: {}", e))?;

        // Lock acceptance criteria upon claiming
        tx.execute("UPDATE acceptance_criteria SET is_locked = 1 WHERE task_id = ?1", [task_id]).ok();

        tx.commit().map_err(|e| format!("Failed to commit claim transaction: {}", e))?;
        drop(conn);

        // 2. Cut Git worktree on disk
        if let Err(e) = self.git.create_worktree(repo_path, &worktree_dir, &branch_name, &target_branch) {
            // Revert state if worktree creation failed
            let conn = self.db.lock();
            conn.execute("UPDATE tasks SET state = 'BACKLOG', substate = 'NONE', assigned_agent_id = NULL WHERE id = ?1", [task_id]).ok();
            return Err(format!("Failed to create isolated Git worktree: {}", e));
        }

        self.emit_event(Some(&project_id), Some(task_id), Some(agent_id), "TASK_CLAIMED", json!({ "agent": agent_id }));
        self.get_task(task_id)
    }

    pub fn complete_step(&self, step_id: &str, evidence_json: Option<&str>) -> Result<TaskStep, String> {
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

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
        self.emit_event(None, Some(&step.task_id), None, "STEP_COMPLETED", json!({ "step_id": step_id, "title": step.title }));
        Ok(step)
    }

    pub fn submit_task(&self, task_id: &str, agent_id: &str) -> Result<VerificationResult, String> {
        let task = self.get_task(task_id)?;

        // 1. Ownership enforcement
        if let Some(ref assigned) = task.assigned_agent_id {
            if assigned != agent_id {
                return Err(format!("Task ownership violation: Task '{}' is owned by agent '{}', not '{}'", task_id, assigned, agent_id));
            }
        }

        let proj = self.list_projects()?.into_iter().find(|p| p.id == task.project_id).ok_or("Project not found")?;
        let _repo_path = Path::new(&proj.path);

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

        // 3. Exact worktree HEAD SHA is authoritative
        let head_sha = self.git.get_worktree_head_sha(worktree_dir)?;

        // 4. Perform actual Git mutation audit against held scope leases
        if let Some(ref base) = task.base_sha {
            if let Ok(mutations) = self.git.get_worktree_mutations(worktree_dir, base) {
                self.scope.audit_actual_mutations(task_id, agent_id, &mutations).ok();
            }
        }

        // 5. Verification checks gate
        let verify_res = self.verify.verify_task_submission(task_id, &head_sha)?;

        if verify_res.is_valid {
            let now = Utc::now().to_rfc3339();
            let conn = self.db.lock();
            conn.execute(
                "UPDATE tasks SET state = 'REVIEW', substate = 'NONE', head_sha = ?1, updated_at = ?2 WHERE id = ?3",
                params![head_sha, now, task_id],
            ).map_err(|e| e.to_string())?;

            drop(conn);

            // Generate deterministic Proof-of-Completion bundle
            let changed_files = task.base_sha.as_deref().and_then(|base| {
                self.git.get_worktree_mutations(worktree_dir, base).ok()
            }).unwrap_or_default();

            self.verify.generate_proof_bundle(
                task_id,
                &task.project_id,
                Some(agent_id),
                &task.description,
                task.base_sha.as_deref().unwrap_or(""),
                &head_sha,
                &changed_files,
                "Authoritative Coordinator Verification Passed",
            ).ok();

            self.emit_event(Some(&task.project_id), Some(task_id), Some(agent_id), "TASK_SUBMITTED_FOR_REVIEW", json!({ "head_sha": head_sha }));
        }

        Ok(verify_res)
    }

    // --- Agents ---
    pub fn register_agent(&self, name: &str, agent_type: &str) -> Result<Agent, String> {
        let conn = self.db.lock();
        let id = Uuid::new_v4().to_string();
        let session_token = format!("axf_sess_{}", Uuid::new_v4().simple());
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + chrono::Duration::hours(24)).to_rfc3339();

        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, capabilities_json, last_heartbeat, created_at)
             VALUES (?1, ?2, ?3, 'Implementer', 'IDLE', '{}', ?4, ?4)",
            params![id, name, agent_type, now_str],
        ).map_err(|e| e.to_string())?;

        let sess_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
            params![sess_id, id, session_token, now_str, expires_str],
        ).map_err(|e| e.to_string())?;

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
        let proj = self.list_projects()?.into_iter().find(|p| p.id == project_id || p.id == task.project_id).ok_or("Project not found")?;

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

        Ok(ContextPack {
            project_id: proj.id,
            project_name: proj.name,
            contract_hash: "contract-v2".to_string(),
            contract_overview: proj.master_spec,
            project_rules: vec!["Execute exclusively inside assigned Git worktree".to_string(), "Reserve scope locks before writing files".to_string()],
            project_memory: Vec::new(),
            task_id: task.id,
            task_title: task.title,
            task_prompt: task.description,
            task_state: task.state.as_str().to_string(),
            task_substate: task.substate.as_str().to_string(),
            acceptance_criteria: criteria,
            required_steps: steps,
            dependencies: Vec::new(),
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
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM masterplans WHERE project_id = ?1",
                [project_id],
                |r| r.get(0),
            )
            .ok();

        let plan_id = if let Some(id) = existing_id {
            conn.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&id])
                .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE masterplans SET raw_text = ?1, status = 'UNSORTED', target_step_count = ?2, max_steps_per_agent = ?3, updated_at = ?4 WHERE id = ?5",
                params![raw_text, target_step_count, max_steps_per_agent, now, id],
            ).map_err(|e| e.to_string())?;
            id
        } else {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO masterplans (id, project_id, raw_text, status, target_step_count, max_steps_per_agent, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'UNSORTED', ?4, ?5, ?6, ?6)",
                params![id, project_id, raw_text, target_step_count, max_steps_per_agent, now],
            ).map_err(|e| e.to_string())?;
            id
        };

        drop(conn);
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

        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();

        conn.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&plan.id])
            .map_err(|e| e.to_string())?;

        let mut inserted_steps = Vec::new();

        for s in steps {
            let step_id = Uuid::new_v4().to_string();
            let suggested_scope = s.suggested_scope.unwrap_or_else(|| "src/**".to_string());
            let criteria = s.acceptance_criteria.unwrap_or_else(|| "All automated tests pass".to_string());

            conn.execute(
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

        conn.execute(
            "UPDATE masterplans SET status = 'RESORTED', updated_at = ?1 WHERE id = ?2",
            params![now, plan.id],
        ).map_err(|e| e.to_string())?;

        drop(conn);
        self.emit_event(Some(project_id), None, None, "MASTERPLAN_DECOMPOSED", json!({ "total_steps": inserted_steps.len(), "status": "RESORTED" }));

        Ok(inserted_steps)
    }

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

        // Check active anti-hoarding limit across cumulative active claims
        let conn = self.db.lock();
        let currently_active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM masterplan_steps WHERE claimed_agent_id = ?1 AND status = 'CLAIMED'",
                [agent_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        drop(conn);

        if currently_active >= plan.max_steps_per_agent as i64 {
            return Err(format!(
                "Anti-hoarding cap reached: Agent '{}' already has {} active claimed steps. Submit or complete your current chunk before claiming more.",
                agent_id, currently_active
            ));
        }

        let allowed = (plan.max_steps_per_agent as i64 - currently_active).max(1) as i32;
        let count = requested_count.unwrap_or(allowed).min(allowed).max(1);

        let all_steps = self.list_masterplan_steps(project_id)?;
        let pending_steps: Vec<MasterplanStep> = all_steps
            .into_iter()
            .filter(|s| s.status == "PENDING")
            .take(count as usize)
            .collect();

        if pending_steps.is_empty() {
            return Err("No pending steps available in masterplan. All steps are either claimed or completed.".to_string());
        }

        let first_idx = pending_steps.first().unwrap().step_index;
        let last_idx = pending_steps.last().unwrap().step_index;

        let task_title = format!("Masterplan Chunk: Steps {}-{} ({})", first_idx, last_idx, pending_steps.first().unwrap().title);
        let task_desc = pending_steps
            .iter()
            .map(|s| format!("Step #{}: {}\n{}", s.step_index, s.title, s.description))
            .collect::<Vec<String>>()
            .join("\n\n---\n\n");

        let task_steps: Vec<(String, String, bool)> = pending_steps
            .iter()
            .map(|s| (format!("Step #{}: {}", s.step_index, s.title), s.description.clone(), true))
            .collect();

        let criteria: Vec<String> = pending_steps
            .iter()
            .map(|s| s.acceptance_criteria.clone())
            .collect();

        // 1. Create Task in Backlog
        let task = self.create_task(project_id, &task_title, &task_desc, "HIGH", task_steps, criteria)?;

        // 2. Claim Task (cuts Git worktree)
        let claimed_task = match self.claim_task(&task.id, agent_id) {
            Ok(t) => t,
            Err(e) => {
                let conn = self.db.lock();
                conn.execute("DELETE FROM tasks WHERE id = ?1", [&task.id]).ok();
                return Err(e);
            }
        };

        // 3. Lock Scopes automatically for all scopes in chunk
        let mut all_patterns = Vec::new();
        for s in &pending_steps {
            for pat in s.suggested_scope.split(',') {
                let trimmed = pat.trim();
                if !trimmed.is_empty() {
                    all_patterns.push(trimmed.to_string());
                }
            }
        }
        if all_patterns.is_empty() {
            all_patterns.push("src/**".to_string());
        }

        if let Err(scope_err) = self.scope.acquire_scope(&task.id, agent_id, all_patterns, "EXCLUSIVE_WRITE") {
            // Roll back claimed task on scope collision
            let conn = self.db.lock();
            conn.execute("DELETE FROM tasks WHERE id = ?1", [&task.id]).ok();
            return Err(format!("Chunk claim aborted due to scope collision: {}", scope_err));
        }

        // 4. Update masterplan steps to CLAIMED
        let conn = self.db.lock();
        let now = Utc::now().to_rfc3339();
        for s in &pending_steps {
            conn.execute(
                "UPDATE masterplan_steps SET status = 'CLAIMED', claimed_agent_id = ?1, claimed_task_id = ?2, updated_at = ?3 WHERE id = ?4",
                params![agent_id, task.id, now, s.id],
            ).ok();
        }

        conn.execute(
            "UPDATE masterplans SET status = 'EXECUTING', updated_at = ?1 WHERE id = ?2",
            params![now, plan.id],
        ).ok();

        drop(conn);
        self.emit_event(Some(project_id), Some(&task.id), Some(agent_id), "MASTERPLAN_CHUNK_CLAIMED", json!({ "task_id": task.id, "from_step": first_idx, "to_step": last_idx }));

        Ok(claimed_task)
    }

    pub fn reset_masterplan(&self, project_id: &str) -> Result<(), String> {
        let plan = self.get_masterplan(project_id)?;
        if let Some(p) = plan {
            let conn = self.db.lock();
            let now = Utc::now().to_rfc3339();
            conn.execute("DELETE FROM masterplan_steps WHERE masterplan_id = ?1", [&p.id])
                .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE masterplans SET status = 'UNSORTED', updated_at = ?1 WHERE id = ?2",
                params![now, p.id],
            ).map_err(|e| e.to_string())?;
            drop(conn);
            self.emit_event(Some(project_id), None, None, "MASTERPLAN_RESET", json!({ "project_id": project_id }));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests;
