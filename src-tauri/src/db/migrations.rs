use rusqlite::{Connection, Result};
use tracing::info;

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    info!("Running database schema migrations for AgentXFlow V2...");

    // Enable WAL mode & foreign keys for asynchronous local performance
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;"
    )?;

    conn.execute_batch(
        "
        -- Projects
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            master_spec TEXT NOT NULL,
            target_branch TEXT NOT NULL DEFAULT 'main',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Versioned Project Contracts
        CREATE TABLE IF NOT EXISTS project_contracts (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            overview TEXT NOT NULL,
            architecture TEXT NOT NULL,
            rules_json TEXT NOT NULL,
            commands_json TEXT NOT NULL,
            testing_json TEXT NOT NULL,
            repo_map TEXT NOT NULL,
            security_constraints TEXT NOT NULL,
            contract_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Project Rules
        CREATE TABLE IF NOT EXISTS project_rules (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            category TEXT NOT NULL,
            rule_text TEXT NOT NULL,
            strictness TEXT NOT NULL DEFAULT 'MANDATORY',
            created_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Curated Engineering Memory
        CREATE TABLE IF NOT EXISTS project_memory (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            memory_type TEXT NOT NULL,
            content TEXT NOT NULL,
            source_task_id TEXT,
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Tasks (Primary State + Internal Substate)
        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_id TEXT,
            epic_id TEXT,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'BACKLOG',
            substate TEXT NOT NULL DEFAULT 'NONE',
            assigned_agent_id TEXT,
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            risk_score REAL NOT NULL DEFAULT 0.0,
            estimated_scope TEXT,
            worktree_path TEXT,
            branch_name TEXT,
            base_sha TEXT,
            head_sha TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Task Dependency Graph (DAG)
        CREATE TABLE IF NOT EXISTS task_dependencies (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            depends_on_task_id TEXT NOT NULL,
            dependency_type TEXT NOT NULL DEFAULT 'BLOCKS',
            created_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
            FOREIGN KEY(depends_on_task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Task Attempts (Execution Runs History)
        CREATE TABLE IF NOT EXISTS task_attempts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            run_number INTEGER NOT NULL,
            base_sha TEXT NOT NULL,
            head_sha TEXT,
            worktree_path TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL DEFAULT 'RUNNING',
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Task Steps (Checklist)
        CREATE TABLE IF NOT EXISTS task_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            order_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            is_mandatory BOOLEAN NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'PENDING',
            completed_at TEXT,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Acceptance Criteria
        CREATE TABLE IF NOT EXISTS acceptance_criteria (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            criterion TEXT NOT NULL,
            is_satisfied BOOLEAN NOT NULL DEFAULT 0,
            is_locked BOOLEAN NOT NULL DEFAULT 0,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Agents
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            agent_type TEXT NOT NULL,
            profile TEXT NOT NULL DEFAULT 'Implementer',
            status TEXT NOT NULL DEFAULT 'IDLE',
            capabilities_json TEXT NOT NULL DEFAULT '{}',
            last_heartbeat TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        -- Agent Profiles
        CREATE TABLE IF NOT EXISTS agent_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            role_description TEXT NOT NULL,
            preferred_agent_type TEXT NOT NULL,
            required_capabilities_json TEXT NOT NULL,
            permission_policy TEXT NOT NULL
        );

        -- Authenticated Agent Sessions
        CREATE TABLE IF NOT EXISTS agent_sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            session_token TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            last_activity_at TEXT NOT NULL,
            FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );

        -- Agent Execution Runs & Subagents
        CREATE TABLE IF NOT EXISTS agent_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            parent_run_id TEXT,
            role TEXT NOT NULL,
            prompt TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'ACTIVE',
            started_at TEXT NOT NULL,
            finished_at TEXT,
            prompt_tokens INTEGER NOT NULL DEFAULT 0,
            completion_tokens INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Agent Permission Requests
        CREATE TABLE IF NOT EXISTS agent_permission_requests (
            id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            action_type TEXT NOT NULL,
            target TEXT NOT NULL,
            reason TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'PENDING',
            requested_at TEXT NOT NULL,
            responded_at TEXT,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Scope Leases (Exclusive / Shared File Locks)
        CREATE TABLE IF NOT EXISTS scope_leases (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            pattern TEXT NOT NULL,
            access_type TEXT NOT NULL DEFAULT 'EXCLUSIVE_WRITE',
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Scope Violations
        CREATE TABLE IF NOT EXISTS scope_violations (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            violation_type TEXT NOT NULL,
            detected_at TEXT NOT NULL,
            resolved BOOLEAN NOT NULL DEFAULT 0,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Worktrees Registry
        CREATE TABLE IF NOT EXISTS worktrees (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            worktree_path TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            base_sha TEXT NOT NULL,
            is_integration BOOLEAN NOT NULL DEFAULT 0,
            is_healthy BOOLEAN NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Verification Profiles
        CREATE TABLE IF NOT EXISTS verification_profiles (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            name TEXT NOT NULL,
            checks_json TEXT NOT NULL,
            is_default BOOLEAN NOT NULL DEFAULT 1,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Coordinator-Executed Verification Runs
        CREATE TABLE IF NOT EXISTS verification_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            run_id TEXT,
            check_id TEXT NOT NULL,
            check_name TEXT NOT NULL,
            commit_sha TEXT NOT NULL,
            command TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            stdout TEXT NOT NULL,
            stderr TEXT NOT NULL,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            is_passed BOOLEAN NOT NULL,
            is_stale BOOLEAN NOT NULL DEFAULT 0,
            executed_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Proof Bundles
        CREATE TABLE IF NOT EXISTS proof_bundles (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL UNIQUE,
            project_id TEXT NOT NULL,
            agent_id TEXT,
            prompt TEXT NOT NULL,
            base_sha TEXT NOT NULL,
            head_sha TEXT NOT NULL,
            files_changed_json TEXT NOT NULL,
            diff_summary TEXT NOT NULL,
            proof_hash TEXT NOT NULL,
            generated_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Serialized Merge Queue
        CREATE TABLE IF NOT EXISTS merge_queue (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_id TEXT NOT NULL UNIQUE,
            branch_name TEXT NOT NULL,
            target_branch TEXT NOT NULL DEFAULT 'main',
            position INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'READY',
            base_sha TEXT NOT NULL,
            head_sha TEXT NOT NULL,
            queued_at TEXT NOT NULL,
            processed_at TEXT,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Integration Attempts
        CREATE TABLE IF NOT EXISTS integration_attempts (
            id TEXT PRIMARY KEY,
            merge_queue_id TEXT NOT NULL,
            simulation_passed BOOLEAN NOT NULL,
            conflicts_json TEXT,
            post_merge_verification_passed BOOLEAN NOT NULL,
            merge_strategy TEXT NOT NULL DEFAULT 'SQUASH',
            target_sha_before TEXT NOT NULL,
            target_sha_after TEXT,
            attempted_at TEXT NOT NULL,
            FOREIGN KEY(merge_queue_id) REFERENCES merge_queue(id) ON DELETE CASCADE
        );

        -- Ordered Event Sequence Stream (Replaces 4s Full Polling)
        CREATE TABLE IF NOT EXISTS events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            project_id TEXT,
            task_id TEXT,
            agent_id TEXT,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            timestamp TEXT NOT NULL
        );

        -- Deterministic Lifecycle Policy Rules
        CREATE TABLE IF NOT EXISTS policies (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            hook TEXT NOT NULL,
            condition_pattern TEXT NOT NULL,
            action TEXT NOT NULL DEFAULT 'ALLOW',
            reason TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Masterplans
        CREATE TABLE IF NOT EXISTS masterplans (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'UNSORTED',
            target_step_count INTEGER NOT NULL DEFAULT 20,
            max_steps_per_agent INTEGER NOT NULL DEFAULT 4,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        -- Masterplan Decomposed Steps
        CREATE TABLE IF NOT EXISTS masterplan_steps (
            id TEXT PRIMARY KEY,
            masterplan_id TEXT NOT NULL,
            step_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            suggested_scope TEXT NOT NULL,
            acceptance_criteria TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'PENDING',
            claimed_agent_id TEXT,
            claimed_task_id TEXT,
            completed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(masterplan_id) REFERENCES masterplans(id) ON DELETE CASCADE,
            FOREIGN KEY(claimed_agent_id) REFERENCES agents(id) ON DELETE SET NULL,
            FOREIGN KEY(claimed_task_id) REFERENCES tasks(id) ON DELETE SET NULL
        );

        -- First-Class Step & Coordinator Evidence Records
        CREATE TABLE IF NOT EXISTS evidence_records (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            step_id TEXT,
            evidence_type TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'AGENT_REPORTED',
            payload_json TEXT NOT NULL,
            recorded_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Indexes for High Performance Querying
        CREATE INDEX IF NOT EXISTS idx_tasks_project_state ON tasks(project_id, state);
        CREATE INDEX IF NOT EXISTS idx_task_deps_task_id ON task_dependencies(task_id);
        CREATE INDEX IF NOT EXISTS idx_task_deps_depends_on ON task_dependencies(depends_on_task_id);
        CREATE INDEX IF NOT EXISTS idx_scope_leases_task ON scope_leases(task_id);
        CREATE INDEX IF NOT EXISTS idx_scope_violations_task ON scope_violations(task_id);
        CREATE INDEX IF NOT EXISTS idx_verification_runs_task ON verification_runs(task_id, commit_sha);
        CREATE INDEX IF NOT EXISTS idx_merge_queue_project_pos ON merge_queue(project_id, position);
        CREATE INDEX IF NOT EXISTS idx_events_sequence ON events(sequence);
        CREATE INDEX IF NOT EXISTS idx_masterplans_project ON masterplans(project_id);
        CREATE INDEX IF NOT EXISTS idx_masterplan_steps_plan_idx ON masterplan_steps(masterplan_id, step_index);
        CREATE INDEX IF NOT EXISTS idx_evidence_records_task ON evidence_records(task_id);
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_token ON agent_sessions(session_token);
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_agent ON agent_sessions(agent_id);
        "
    )?;

    info!("Database schema migrations completed successfully.");
    Ok(())
}
