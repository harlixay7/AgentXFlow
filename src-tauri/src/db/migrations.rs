use chrono::Utc;
use rusqlite::{Connection, Result, Transaction};
use tracing::info;

pub struct Migration {
    pub version: i32,
    pub name: &'static str,
    pub run: fn(&Transaction) -> Result<()>,
}

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    info!("Running versioned database schema migrations for AgentXFlow...");

    // Enable WAL mode & foreign keys for concurrency and durability
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;"
    )?;

    // Ensure migration history tracker table exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );"
    )?;

    let migrations = get_all_migrations();

    for m in migrations {
        let already_applied: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM _schema_migrations WHERE version = ?1",
            [m.version],
            |row| row.get(0),
        )?;

        if !already_applied {
            info!("Applying database migration #{:04}: {}...", m.version, m.name);
            let tx = conn.transaction()?;
            (m.run)(&tx)?;
            let now = Utc::now().to_rfc3339();
            tx.execute(
                "INSERT INTO _schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![m.version, m.name, now],
            )?;
            tx.commit()?;
            info!("Migration #{:04}: {} successfully applied.", m.version, m.name);
        }
    }

    // Defensive schema verification before accepting coordinator operations
    verify_schema_integrity(conn).map_err(|e| rusqlite::Error::SqlInputError {
        error: rusqlite::ffi::Error::new(1),
        msg: e,
        sql: String::new(),
        offset: 0,
    })?;

    Ok(())
}

fn get_all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "core_control_plane_entities",
            run: migration_0001_core_entities,
        },
        Migration {
            version: 2,
            name: "authoritative_session_security",
            run: migration_0002_authoritative_session_security,
        },
        Migration {
            version: 3,
            name: "immutable_proofs_and_masterplan_revisions",
            run: migration_0003_immutable_proofs_and_revisions,
        },
        Migration {
            version: 4,
            name: "crash_safe_claim_and_merge_metadata",
            run: migration_0004_claim_and_merge_metadata,
        },
        Migration {
            version: 5,
            name: "task_attempts_and_machine_evaluators",
            run: migration_0005_task_attempts_and_machine_evaluators,
        },
        Migration {
            version: 6,
            name: "task_masterplan_lifecycle_and_stale_invalidation",
            run: migration_0006_task_masterplan_lifecycle_and_stale_invalidation,
        },
        Migration {
            version: 7,
            name: "normalize_proof_bundles_task_attempts_and_evaluators",
            run: migration_0007_normalize_proof_bundles_task_attempts_and_evaluators,
        },
        Migration {
            version: 8,
            name: "task_attempt_worktree_paths",
            run: migration_0008_task_attempt_worktree_paths,
        },
        Migration {
            version: 9,
            name: "seed_canonical_ide_profiles_and_cleanup",
            run: migration_0009_seed_canonical_ide_profiles_and_cleanup,
        },
        Migration {
            version: 10,
            name: "masterplan_milestone_approval_toggle",
            run: migration_0010_masterplan_milestone_approval_toggle,
        },
    ]
}

fn migration_0001_core_entities(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
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
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            risk_score REAL DEFAULT 0.0,
            estimated_scope TEXT,
            assigned_agent_id TEXT,
            assigned_profile_id TEXT,
            allocated_budget_usd REAL,
            spent_budget_usd REAL DEFAULT 0.0,
            worktree_path TEXT,
            branch_name TEXT,
            base_sha TEXT,
            head_sha TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(parent_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Linear Steps within a Task
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

        -- Authoritative Acceptance Criteria
        CREATE TABLE IF NOT EXISTS acceptance_criteria (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            criterion TEXT NOT NULL,
            is_satisfied BOOLEAN NOT NULL DEFAULT 0,
            is_locked BOOLEAN NOT NULL DEFAULT 0,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Task Dependencies (DAG)
        CREATE TABLE IF NOT EXISTS task_dependencies (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            depends_on_task_id TEXT NOT NULL,
            dependency_type TEXT NOT NULL DEFAULT 'BLOCKS',
            created_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
            FOREIGN KEY(depends_on_task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Exclusive & Shared Scope Leases
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

        -- Recorded Scope Violations
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

        -- Autonomous Verification Runs
        CREATE TABLE IF NOT EXISTS verification_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            run_id TEXT,
            check_id TEXT NOT NULL,
            check_name TEXT NOT NULL,
            commit_sha TEXT NOT NULL,
            command TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            stdout TEXT,
            stderr TEXT,
            duration_ms INTEGER NOT NULL,
            is_passed BOOLEAN NOT NULL,
            is_stale BOOLEAN NOT NULL DEFAULT 0,
            source TEXT NOT NULL DEFAULT 'COORDINATOR_OBSERVED',
            executed_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Registered Autonomous Agents
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            agent_type TEXT NOT NULL,
            profile TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'IDLE',
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

        -- Agent Execution Runs
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

        -- Ordered Event Sequence Stream
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

        -- Base Indexes
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
        "
    )?;

    Ok(())
}

fn migration_0002_authoritative_session_security(tx: &Transaction) -> Result<()> {
    // 1. Inspect existing agent_sessions schema if present
    let mut check_sessions_stmt = tx.prepare("PRAGMA table_info(agent_sessions)")?;
    let session_cols = check_sessions_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();

    if session_cols.is_empty() {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_sessions (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                session_token TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                last_activity_at TEXT NOT NULL,
                FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
            );"
        )?;
    } else if !session_cols.contains(&"session_token".to_string()) {
        info!("Upgrading legacy agent_sessions table schema to authoritative session format...");
        tx.execute_batch(
            "DROP TABLE IF EXISTS agent_sessions;
             CREATE TABLE agent_sessions (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                session_token TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                last_activity_at TEXT NOT NULL,
                FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
             );"
        )?;
    }

    // 2. Ensure agents has session_token
    let mut check_agents_stmt = tx.prepare("PRAGMA table_info(agents)")?;
    let agent_cols = check_agents_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();

    if !agent_cols.contains(&"session_token".to_string()) {
        tx.execute_batch("ALTER TABLE agents ADD COLUMN session_token TEXT;")?;
    }

    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_token ON agent_sessions(session_token);
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_agent ON agent_sessions(agent_id);
        "
    )?;

    Ok(())
}

fn migration_0003_immutable_proofs_and_revisions(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        -- Immutable Proof Bundles per Task Attempt & Commit HEAD
        CREATE TABLE IF NOT EXISTS proof_bundles (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            agent_id TEXT,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            prompt TEXT NOT NULL,
            base_sha TEXT NOT NULL,
            head_sha TEXT NOT NULL,
            files_changed_json TEXT NOT NULL,
            diff_summary TEXT NOT NULL,
            verification_runs_json TEXT NOT NULL,
            criteria_json TEXT NOT NULL,
            steps_json TEXT NOT NULL,
            proof_hash TEXT NOT NULL UNIQUE,
            generated_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        -- Masterplan Historical Revisions (Preserves Plan Edits & Resets)
        CREATE TABLE IF NOT EXISTS masterplan_revisions (
            id TEXT PRIMARY KEY,
            masterplan_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            revision_number INTEGER NOT NULL,
            raw_text TEXT NOT NULL,
            reason TEXT NOT NULL,
            steps_snapshot_json TEXT NOT NULL,
            archived_at TEXT NOT NULL,
            FOREIGN KEY(masterplan_id) REFERENCES masterplans(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_proof_bundles_task_sha ON proof_bundles(task_id, head_sha);
        CREATE INDEX IF NOT EXISTS idx_masterplan_revisions_plan ON masterplan_revisions(masterplan_id, revision_number);
        "
    )?;

    Ok(())
}

fn migration_0004_claim_and_merge_metadata(tx: &Transaction) -> Result<()> {
    // Ensure tasks table has attempt_count, risk_score, estimated_scope
    let mut check_tasks_stmt = tx.prepare("PRAGMA table_info(tasks)")?;
    let task_cols = check_tasks_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();

    if !task_cols.contains(&"attempt_count".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;")?;
    }
    if !task_cols.contains(&"risk_score".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN risk_score REAL DEFAULT 0.0;")?;
    }
    if !task_cols.contains(&"estimated_scope".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN estimated_scope TEXT;")?;
    }

    // Ensure acceptance_criteria table has is_locked
    let mut check_crit_stmt = tx.prepare("PRAGMA table_info(acceptance_criteria)")?;
    let crit_cols = check_crit_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();

    if !crit_cols.contains(&"is_locked".to_string()) {
        tx.execute_batch("ALTER TABLE acceptance_criteria ADD COLUMN is_locked BOOLEAN NOT NULL DEFAULT 0;")?;
    }

    // Ensure merge_queue table has active worker tracking
    let mut check_mq_stmt = tx.prepare("PRAGMA table_info(merge_queue)")?;
    let mq_cols = check_mq_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();

    if !mq_cols.contains(&"claimed_by_worker".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN claimed_by_worker TEXT;")?;
    }
    if !mq_cols.contains(&"worker_heartbeat".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN worker_heartbeat TEXT;")?;
    }

    Ok(())
}

fn migration_0005_task_attempts_and_machine_evaluators(tx: &Transaction) -> Result<()> {
    // 1. Ensure task_attempts table exists with all required columns
    let mut check_ta_stmt = tx.prepare("PRAGMA table_info(task_attempts)")?;
    let ta_cols = check_ta_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_ta_stmt);

    if ta_cols.is_empty() {
        tx.execute_batch(
            "CREATE TABLE task_attempts (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                attempt_number INTEGER NOT NULL,
                base_sha TEXT NOT NULL,
                head_sha TEXT,
                status TEXT NOT NULL,
                rejection_reasons TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
            );"
        )?;
    } else {
        if !ta_cols.contains(&"attempt_number".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1;")?;
        }
        if !ta_cols.contains(&"agent_id".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN agent_id TEXT;")?;
        }
        if !ta_cols.contains(&"base_sha".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN base_sha TEXT NOT NULL DEFAULT '';")?;
        }
        if !ta_cols.contains(&"head_sha".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN head_sha TEXT;")?;
        }
        if !ta_cols.contains(&"status".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN status TEXT NOT NULL DEFAULT 'ACTIVE';")?;
        }
        if !ta_cols.contains(&"rejection_reasons".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN rejection_reasons TEXT;")?;
        }
        if !ta_cols.contains(&"started_at".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN started_at TEXT NOT NULL DEFAULT '';")?;
        }
        if !ta_cols.contains(&"finished_at".to_string()) {
            tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN finished_at TEXT;")?;
        }
    }

    // 2. Ensure evaluator_results table exists with all required columns
    let mut check_er_stmt = tx.prepare("PRAGMA table_info(evaluator_results)")?;
    let er_cols = check_er_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_er_stmt);

    if er_cols.is_empty() {
        tx.execute_batch(
            "CREATE TABLE evaluator_results (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                criterion_id TEXT,
                evaluator_name TEXT NOT NULL,
                evaluator_type TEXT NOT NULL,
                evaluator_version TEXT NOT NULL DEFAULT '1.0.0',
                commit_sha TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                stdout_output TEXT NOT NULL,
                stderr_output TEXT NOT NULL,
                output_sha256 TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                passed BOOLEAN NOT NULL,
                evaluated_at TEXT NOT NULL,
                FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE CASCADE
            );"
        )?;
    } else {
        if !er_cols.contains(&"attempt_id".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN attempt_id TEXT;")?;
        }
        if !er_cols.contains(&"criterion_id".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN criterion_id TEXT;")?;
        }
        if !er_cols.contains(&"evaluator_name".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN evaluator_name TEXT NOT NULL DEFAULT '';")?;
        }
        if !er_cols.contains(&"evaluator_type".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN evaluator_type TEXT NOT NULL DEFAULT 'COMMAND';")?;
        }
        if !er_cols.contains(&"evaluator_version".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN evaluator_version TEXT NOT NULL DEFAULT '1.0.0';")?;
        }
        if !er_cols.contains(&"commit_sha".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN commit_sha TEXT NOT NULL DEFAULT '';")?;
        }
        if !er_cols.contains(&"exit_code".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN exit_code INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !er_cols.contains(&"stdout_output".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN stdout_output TEXT NOT NULL DEFAULT '';")?;
        }
        if !er_cols.contains(&"stderr_output".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN stderr_output TEXT NOT NULL DEFAULT '';")?;
        }
        if !er_cols.contains(&"output_sha256".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN output_sha256 TEXT NOT NULL DEFAULT '';")?;
        }
        if !er_cols.contains(&"duration_ms".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !er_cols.contains(&"passed".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN passed BOOLEAN NOT NULL DEFAULT 0;")?;
        }
        if !er_cols.contains(&"evaluated_at".to_string()) {
            tx.execute_batch("ALTER TABLE evaluator_results ADD COLUMN evaluated_at TEXT NOT NULL DEFAULT '';")?;
        }
    }

    // 3. Ensure verification_profiles table exists with all required columns
    let mut check_vp_stmt = tx.prepare("PRAGMA table_info(verification_profiles)")?;
    let vp_cols = check_vp_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_vp_stmt);

    if vp_cols.is_empty() {
        tx.execute_batch(
            "CREATE TABLE verification_profiles (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_id TEXT,
                check_type TEXT NOT NULL,
                command TEXT NOT NULL,
                args_json TEXT NOT NULL DEFAULT '[]',
                timeout_secs INTEGER NOT NULL DEFAULT 60,
                required BOOLEAN NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );"
        )?;
    } else {
        if !vp_cols.contains(&"project_id".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN project_id TEXT;")?;
        }
        if !vp_cols.contains(&"task_id".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN task_id TEXT;")?;
        }
        if !vp_cols.contains(&"check_type".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN check_type TEXT NOT NULL DEFAULT 'UNIT_TESTS';")?;
        }
        if !vp_cols.contains(&"command".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN command TEXT NOT NULL DEFAULT '';")?;
        }
        if !vp_cols.contains(&"args_json".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN args_json TEXT NOT NULL DEFAULT '[]';")?;
        }
        if !vp_cols.contains(&"timeout_secs".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN timeout_secs INTEGER NOT NULL DEFAULT 60;")?;
        }
        if !vp_cols.contains(&"required".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN required BOOLEAN NOT NULL DEFAULT 1;")?;
        }
        if !vp_cols.contains(&"created_at".to_string()) {
            tx.execute_batch("ALTER TABLE verification_profiles ADD COLUMN created_at TEXT NOT NULL DEFAULT '';")?;
        }
    }

    // 4. Ensure scope_violations table has attempt_id
    let mut check_sv_stmt = tx.prepare("PRAGMA table_info(scope_violations)")?;
    let sv_cols = check_sv_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_sv_stmt);

    if !sv_cols.contains(&"attempt_id".to_string()) {
        tx.execute_batch("ALTER TABLE scope_violations ADD COLUMN attempt_id TEXT;")?;
    }

    // 5. Ensure proof_bundles table has attempt_id
    let mut check_pb_stmt = tx.prepare("PRAGMA table_info(proof_bundles)")?;
    let pb_cols = check_pb_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_pb_stmt);

    if !pb_cols.contains(&"attempt_id".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN attempt_id TEXT;")?;
    }

    // 6. Safe index creation
    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_task_attempts_task ON task_attempts(task_id, attempt_number);
        CREATE INDEX IF NOT EXISTS idx_evaluator_results_attempt ON evaluator_results(attempt_id, passed);
        CREATE INDEX IF NOT EXISTS idx_evaluator_results_task ON evaluator_results(task_id, criterion_id);
        "
    )?;

    Ok(())
}

fn migration_0006_task_masterplan_lifecycle_and_stale_invalidation(tx: &Transaction) -> Result<()> {
    // 1. Defensively inspect and upgrade tasks table
    let mut check_tasks_stmt = tx.prepare("PRAGMA table_info(tasks)")?;
    let task_cols = check_tasks_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_tasks_stmt);

    if !task_cols.contains(&"masterplan_id".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN masterplan_id TEXT;")?;
    }
    if !task_cols.contains(&"masterplan_revision_id".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN masterplan_revision_id TEXT;")?;
    }
    if !task_cols.contains(&"is_stale".to_string()) {
        tx.execute_batch("ALTER TABLE tasks ADD COLUMN is_stale BOOLEAN NOT NULL DEFAULT 0;")?;
    }

    // 2. Safe index creation
    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_tasks_project_masterplan ON tasks(project_id, masterplan_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_state_stale ON tasks(state, is_stale);
        "
    )?;

    Ok(())
}

fn migration_0007_normalize_proof_bundles_task_attempts_and_evaluators(tx: &Transaction) -> Result<()> {
    // 1. Defensively normalize proof_bundles table
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS proof_bundles (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            agent_id TEXT,
            attempt_id TEXT,
            attempt_number INTEGER NOT NULL DEFAULT 1,
            prompt TEXT NOT NULL DEFAULT '',
            base_sha TEXT NOT NULL DEFAULT '',
            head_sha TEXT NOT NULL DEFAULT '',
            files_changed_json TEXT NOT NULL DEFAULT '[]',
            diff_summary TEXT NOT NULL DEFAULT '',
            verification_runs_json TEXT NOT NULL DEFAULT '[]',
            criteria_json TEXT NOT NULL DEFAULT '[]',
            steps_json TEXT NOT NULL DEFAULT '[]',
            proof_hash TEXT NOT NULL,
            generated_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );
        "
    )?;

    let mut check_pb_stmt = tx.prepare("PRAGMA table_info(proof_bundles)")?;
    let pb_cols = check_pb_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_pb_stmt);

    if !pb_cols.contains(&"agent_id".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN agent_id TEXT;")?;
    }
    if !pb_cols.contains(&"attempt_id".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN attempt_id TEXT;")?;
    }
    if !pb_cols.contains(&"attempt_number".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1;")?;
    }
    if !pb_cols.contains(&"prompt".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN prompt TEXT NOT NULL DEFAULT '';")?;
    }
    if !pb_cols.contains(&"base_sha".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN base_sha TEXT NOT NULL DEFAULT '';")?;
    }
    if !pb_cols.contains(&"head_sha".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN head_sha TEXT NOT NULL DEFAULT '';")?;
    }
    if !pb_cols.contains(&"files_changed_json".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN files_changed_json TEXT NOT NULL DEFAULT '[]';")?;
    }
    if !pb_cols.contains(&"diff_summary".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN diff_summary TEXT NOT NULL DEFAULT '';")?;
    }
    if !pb_cols.contains(&"verification_runs_json".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN verification_runs_json TEXT NOT NULL DEFAULT '[]';")?;
    }
    if !pb_cols.contains(&"criteria_json".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN criteria_json TEXT NOT NULL DEFAULT '[]';")?;
    }
    if !pb_cols.contains(&"steps_json".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN steps_json TEXT NOT NULL DEFAULT '[]';")?;
    }
    if !pb_cols.contains(&"generated_at".to_string()) {
        tx.execute_batch("ALTER TABLE proof_bundles ADD COLUMN generated_at TEXT NOT NULL DEFAULT '';")?;
    }

    // 2. Defensively normalize task_attempts table
    let mut check_ta_stmt = tx.prepare("PRAGMA table_info(task_attempts)")?;
    let ta_cols = check_ta_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_ta_stmt);

    if !ta_cols.contains(&"run_number".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN run_number INTEGER NOT NULL DEFAULT 1;")?;
    }
    if !ta_cols.contains(&"attempt_number".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1;")?;
    }
    if !ta_cols.contains(&"rejection_reasons".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN rejection_reasons TEXT;")?;
    }
    if !ta_cols.contains(&"base_sha".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN base_sha TEXT;")?;
    }
    if !ta_cols.contains(&"head_sha".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN head_sha TEXT;")?;
    }

    // Backfill run_number from attempt_number or vice versa where available
    tx.execute_batch(
        "
        UPDATE task_attempts SET run_number = attempt_number WHERE run_number IS NULL OR run_number = 0;
        UPDATE task_attempts SET attempt_number = run_number WHERE attempt_number IS NULL OR attempt_number = 0;
        "
    )?;

    // 3. Ensure evaluator_results table exists with all columns
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS evaluator_results (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL,
            criterion_id TEXT,
            evaluator_name TEXT NOT NULL,
            evaluator_type TEXT NOT NULL,
            evaluator_version TEXT NOT NULL,
            commit_sha TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            stdout_output TEXT NOT NULL,
            stderr_output TEXT NOT NULL,
            output_sha256 TEXT NOT NULL,
            duration_ms INTEGER NOT NULL,
            passed BOOLEAN NOT NULL,
            evaluated_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );
        "
    )?;

    // 4. Ensure merge_queue table has all tracking columns
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS merge_queue (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'READY',
            enqueued_at TEXT NOT NULL,
            base_sha TEXT,
            head_sha TEXT,
            queued_at TEXT,
            processed_at TEXT,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );
        "
    )?;

    let mut check_mq_stmt = tx.prepare("PRAGMA table_info(merge_queue)")?;
    let mq_cols = check_mq_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_mq_stmt);

    if !mq_cols.contains(&"base_sha".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN base_sha TEXT;")?;
    }
    if !mq_cols.contains(&"head_sha".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN head_sha TEXT;")?;
    }
    if !mq_cols.contains(&"queued_at".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN queued_at TEXT;")?;
    }
    if !mq_cols.contains(&"processed_at".to_string()) {
        tx.execute_batch("ALTER TABLE merge_queue ADD COLUMN processed_at TEXT;")?;
    }

    // 5. Create supporting indexes
    tx.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_proof_bundles_task_attempt ON proof_bundles(task_id, attempt_number);
        CREATE INDEX IF NOT EXISTS idx_evaluator_results_attempt_passed ON evaluator_results(attempt_id, passed);
        CREATE INDEX IF NOT EXISTS idx_merge_queue_project_status ON merge_queue(project_id, status);
        "
    )?;

    Ok(())
}

/// Keeps task attempts self-contained for verification and recovery across schema generations.
fn migration_0008_task_attempt_worktree_paths(tx: &Transaction) -> Result<()> {
    let mut check_ta_stmt = tx.prepare("PRAGMA table_info(task_attempts)")?;
    let ta_cols = check_ta_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_ta_stmt);

    if !ta_cols.contains(&"worktree_path".to_string()) {
        tx.execute_batch("ALTER TABLE task_attempts ADD COLUMN worktree_path TEXT NOT NULL DEFAULT '';")?;
    }

    Ok(())
}

/// Seeds first-class canonical IDE profiles and normalizes legacy random UUID agents
fn migration_0009_seed_canonical_ide_profiles_and_cleanup(tx: &Transaction) -> Result<()> {
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();
    let expires_str = (now + chrono::Duration::days(365)).to_rfc3339();

    // 1. Ensure agents table has session_token column
    let mut check_ag_stmt = tx.prepare("PRAGMA table_info(agents)")?;
    let ag_cols = check_ag_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_ag_stmt);

    if !ag_cols.contains(&"session_token".to_string()) {
        tx.execute_batch("ALTER TABLE agents ADD COLUMN session_token TEXT;")?;
    }

    // 2. Ensure agent_sessions table exists
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS agent_sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            session_token TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            last_activity_at TEXT NOT NULL,
            FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        "
    )?;

    let canonical_roster = [
        ("antigravity", "Antigravity", "IDE", "Google Antigravity Advanced Agentic Coding Assistant"),
        ("claude-code", "Claude Code", "CLI", "Anthropic Claude Code Agentic Terminal Engine"),
        ("cursor", "Cursor", "IDE", "Cursor AI Coding Assistant"),
        ("opencode", "OpenCode", "IDE", "OpenCode Multi-Agent Orchestrator"),
        ("codex", "OpenAI Codex", "CLI", "OpenAI Codex Agentic Coding Engine"),
        ("gemini-cli", "Gemini CLI", "CLI", "Google Gemini Developer CLI"),
        ("copilot", "GitHub Copilot", "IDE", "GitHub Copilot / VS Code Agent"),
        ("windsurf", "Windsurf", "IDE", "Codeium Windsurf AI Cascade IDE"),
        ("junie", "Junie", "IDE", "JetBrains Junie AI Assistant"),
        ("aider", "Aider", "CLI", "Aider AI Pair Programmer"),
    ];

    for (id, name, agent_type, profile) in canonical_roster {
        let token = format!("axf_sess_{}", id.replace('-', "_"));
        tx.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at, session_token)
             VALUES (?1, ?2, ?3, ?4, 'IDLE', ?5, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 agent_type = excluded.agent_type,
                 profile = excluded.profile,
                 last_heartbeat = excluded.last_heartbeat,
                 session_token = excluded.session_token",
            rusqlite::params![id, name, agent_type, profile, now_str, token],
        )?;

        let sess_id = format!("sess_{}", id.replace('-', "_"));
        tx.execute(
            "INSERT INTO agent_sessions (id, agent_id, session_token, created_at, expires_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 expires_at = excluded.expires_at,
                 last_activity_at = excluded.last_activity_at,
                 session_token = excluded.session_token",
            rusqlite::params![sess_id, id, token, now_str, expires_str],
        )?;
    }

    Ok(())
}

/// Adds require_milestone_approval column to masterplans for hybrid autonomous/milestone handoffs
fn migration_0010_masterplan_milestone_approval_toggle(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS masterplans (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'UNSORTED',
            target_step_count INTEGER NOT NULL DEFAULT 20,
            max_steps_per_agent INTEGER NOT NULL DEFAULT 4,
            require_milestone_approval BOOLEAN NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        "
    )?;

    let mut check_mp_stmt = tx.prepare("PRAGMA table_info(masterplans)")?;
    let mp_cols = check_mp_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect::<Vec<String>>())
        .unwrap_or_default();
    drop(check_mp_stmt);

    if !mp_cols.contains(&"require_milestone_approval".to_string()) {
        tx.execute_batch("ALTER TABLE masterplans ADD COLUMN require_milestone_approval BOOLEAN NOT NULL DEFAULT 1;")?;
    }

    Ok(())
}

/// Verifies that all required database tables and columns exist before accepting coordinator traffic
pub fn verify_schema_integrity(conn: &Connection) -> Result<(), String> {
    // 1. Verify proof_bundles
    let mut check_pb = conn.prepare("PRAGMA table_info(proof_bundles)").map_err(|e| e.to_string())?;
    let pb_cols: Vec<String> = check_pb.query_map([], |r| r.get(1)).map_err(|e| e.to_string())?.flatten().collect();
    drop(check_pb);
    for col in &["id", "task_id", "project_id", "attempt_number", "verification_runs_json", "criteria_json", "steps_json", "proof_hash", "generated_at"] {
        if !pb_cols.iter().any(|c| c == col) {
            return Err(format!("Schema verification failed: column '{}' missing from proof_bundles", col));
        }
    }

    // 2. Verify task_attempts
    let mut check_ta = conn.prepare("PRAGMA table_info(task_attempts)").map_err(|e| e.to_string())?;
    let ta_cols: Vec<String> = check_ta.query_map([], |r| r.get(1)).map_err(|e| e.to_string())?.flatten().collect();
    drop(check_ta);
    for col in &["id", "task_id", "attempt_number", "run_number", "worktree_path", "status", "started_at"] {
        if !ta_cols.iter().any(|c| c == col) {
            return Err(format!("Schema verification failed: column '{}' missing from task_attempts", col));
        }
    }

    // 3. Verify evaluator_results
    let mut check_er = conn.prepare("PRAGMA table_info(evaluator_results)").map_err(|e| e.to_string())?;
    let er_cols: Vec<String> = check_er.query_map([], |r| r.get(1)).map_err(|e| e.to_string())?.flatten().collect();
    drop(check_er);
    for col in &["id", "task_id", "attempt_id", "evaluator_name", "passed", "evaluated_at"] {
        if !er_cols.iter().any(|c| c == col) {
            return Err(format!("Schema verification failed: column '{}' missing from evaluator_results", col));
        }
    }

    Ok(())
}
