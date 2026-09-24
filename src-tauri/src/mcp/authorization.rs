use serde_json::Value;

use crate::core::CoordinatorEngine;
use crate::models::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum CallerAuthority {
    Master,
    Agent(Agent),
}

impl CallerAuthority {
    pub fn is_master(&self) -> bool {
        matches!(self, CallerAuthority::Master)
    }

    pub fn agent_id(&self) -> Option<&str> {
        match self {
            CallerAuthority::Master => None,
            CallerAuthority::Agent(agent) => Some(&agent.id),
        }
    }
}

pub struct McpAuthPolicy;

impl McpAuthPolicy {
    /// Evaluates whether the given authority is permitted to execute the requested MCP tool
    /// with the provided parameters.
    pub fn authorize(
        authority: &CallerAuthority,
        coordinator: &CoordinatorEngine,
        tool_name: &str,
        params: &Value,
    ) -> Result<(), String> {
        match tool_name {
            // 1. Read-only / discovery tools (Allowed for any authenticated caller)
            "agentxflow_current_context"
            | "context.current"
            | "project_list"
            | "project.list"
            | "project_context"
            | "project.context"
            | "masterplan_list"
            | "masterplan.list"
            | "masterplan_get"
            | "masterplan.get"
            | "masterplan_status"
            | "masterplan.status"
            | "task_list"
            | "task.list"
            | "task_get"
            | "task.get"
            | "task_details"
            | "task.details"
            | "dag_dependencies"
            | "dependency_list"
            | "dag.dependencies"
            | "merge_queue_status"
            | "merge.queue_status" => Ok(()),

            // 2. Agent registration (Option A: Master can register any; Agent can only self-refresh/re-register own identity)
            "agent_register" | "agent.register" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let req_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let req_type = params
                        .get("agent_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !req_name.is_empty() {
                        let (canon_req_id, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(req_name, req_type);
                        let (canon_agent_id, ..) = CoordinatorEngine::canonicalize_ide_identity(
                            &agent.id,
                            &agent.agent_type,
                        );
                        let (canon_agent_name_id, ..) = CoordinatorEngine::canonicalize_ide_identity(
                            &agent.name,
                            &agent.agent_type,
                        );
                        if canon_req_id != canon_agent_id
                            && canon_req_id != canon_agent_name_id
                            && req_name != agent.id
                            && req_name != agent.name
                        {
                            return Err(format!(
                                "Forbidden: Authenticated agent '{}' cannot register or impersonate a different agent identity '{}'",
                                agent.id, req_name
                            ));
                        }
                    }
                }
                Ok(())
            }

            // 3. Heartbeat: Agent may heartbeat for itself; Master may heartbeat for any
            "agent_heartbeat" | "agent.heartbeat" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let req_agent_id = params
                        .get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !req_agent_id.is_empty() && req_agent_id != agent.id {
                        let (canon_req, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(req_agent_id, "");
                        let (canon_agent, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if canon_req != canon_agent && req_agent_id != agent.id {
                            return Err(format!(
                                "Agent impersonation rejected: Authenticated session belongs to '{}', cannot act on behalf of '{}'",
                                agent.id, req_agent_id
                            ));
                        }
                    }
                }
                Ok(())
            }

            // 4. Coordinator / Master-only administrative tools
            "merge_process"
            | "merge.process"
            | "masterplan_decompose"
            | "masterplan.decompose"
            | "masterplan_reset"
            | "masterplan.reset"
            | "prepare_masterplan"
            | "masterplan.prepare"
            | "unclaim_agent_tasks"
            | "agent.unclaim_tasks"
            | "force_agent_idle"
            | "agent.force_idle"
            | "task_reconcile"
            | "task.reconcile"
            | "criteria_satisfy"
            | "criteria.satisfy" => {
                if !authority.is_master() {
                    return Err(format!(
                        "Forbidden: Tool '{}' requires Master/Coordinator authority.",
                        tool_name
                    ));
                }
                Ok(())
            }

            // 5. Task mutation tools: Caller must own the task or be Master
            "task_complete_step"
            | "task.complete_step"
            | "task_submit"
            | "task.submit"
            | "task_cancel"
            | "task.cancel"
            | "task_requeue"
            | "task.requeue"
            | "task_workspace_path"
            | "task_workspace_read"
            | "task_workspace_write"
            | "task_workspace_exec" => {
                if let CallerAuthority::Agent(agent) = authority {
                    if let Some(req_agent) = params.get("agent_id").and_then(|v| v.as_str()) {
                        if !req_agent.trim().is_empty() && req_agent != agent.id {
                            let (canon_req, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(req_agent, "");
                            let (canon_agent, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                            if canon_req != canon_agent {
                                return Err(format!(
                                    "Forbidden: Authenticated agent '{}' cannot act on behalf of agent '{}'",
                                    agent.id, req_agent
                                ));
                            }
                        }
                    }

                    let task_id = if let Some(t_id) =
                        params.get("task_id").and_then(|v| v.as_str())
                    {
                        t_id.to_string()
                    } else if let Some(s_id) = params.get("step_id").and_then(|v| v.as_str()) {
                        let conn = coordinator.db.lock();
                        let found_task_id: Option<String> = conn
                            .query_row(
                                "SELECT task_id FROM task_steps WHERE id = ?1",
                                [s_id],
                                |r| r.get(0),
                            )
                            .ok();
                        drop(conn);
                        found_task_id.ok_or_else(|| format!("Step '{}' not found", s_id))?
                    } else {
                        return Err(
                            "Missing required parameter 'task_id' or 'step_id'".to_string()
                        );
                    };

                    let task = coordinator
                        .get_task(&task_id)
                        .map_err(|e| format!("Task not found: {}", e))?;

                    if let Some(ref assigned_agent) = task.assigned_agent_id {
                        let (canon_req, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(assigned_agent, "");
                        let (canon_agent, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if assigned_agent != &agent.id && canon_req != canon_agent {
                            return Err(format!(
                                "Authorization error: Agent '{}' is not the owner of task '{}' (assigned to '{}')",
                                agent.id, task_id, assigned_agent
                            ));
                        }
                    } else {
                        return Err(format!(
                            "Forbidden: Task '{}' is not assigned to any agent",
                            task_id
                        ));
                    }
                }
                Ok(())
            }

            // 6. Scope Release: Agent can only release its own leases; unassigned tasks explicitly denied
            "scope_release" | "scope.release" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let lease_id = params.get("lease_id").and_then(|v| v.as_str());
                    let task_id = params.get("task_id").and_then(|v| v.as_str());

                    if let Some(lid) = lease_id {
                        let conn = coordinator.db.lock();
                        let lease_owner: Option<String> = conn
                            .query_row(
                                "SELECT agent_id FROM scope_leases WHERE id = ?1",
                                [lid],
                                |r| r.get(0),
                            )
                            .ok();
                        drop(conn);

                        if let Some(owner) = lease_owner {
                            let (canon_req, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(&owner, "");
                            let (canon_agent, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                            if owner != agent.id && canon_req != canon_agent {
                                return Err(format!(
                                    "Forbidden: Agent '{}' cannot release lease '{}' owned by '{}'",
                                    agent.id, lid, owner
                                ));
                            }
                        } else {
                            return Err(format!("Lease '{}' not found", lid));
                        }
                    } else if let Some(tid) = task_id {
                        let task = coordinator
                            .get_task(tid)
                            .map_err(|e| format!("Task not found: {}", e))?;
                        if let Some(ref assigned_agent) = task.assigned_agent_id {
                            let (canon_req, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(assigned_agent, "");
                            let (canon_agent, ..) =
                                CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                            if assigned_agent != &agent.id && canon_req != canon_agent {
                                return Err(format!(
                                    "Forbidden: Agent '{}' cannot release scope for task '{}' owned by '{}'",
                                    agent.id, tid, assigned_agent
                                ));
                            }
                        } else {
                            return Err(format!(
                                "Forbidden: Agent '{}' cannot release scope for unassigned task '{}'",
                                agent.id, tid
                            ));
                        }
                    } else {
                        return Err(
                            "Missing required parameter 'task_id' or 'lease_id'".to_string()
                        );
                    }
                }
                Ok(())
            }

            // 7. Scope Acquire: Agent can only acquire leases for itself and tasks it owns
            "scope_acquire" | "scope.acquire" | "scope.propose" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let req_agent_id = params
                        .get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !req_agent_id.is_empty() && req_agent_id != agent.id {
                        let (canon_req, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(req_agent_id, "");
                        let (canon_agent, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if canon_req != canon_agent && req_agent_id != agent.id {
                            return Err(format!(
                                "Forbidden: Agent '{}' cannot acquire scope leases for '{}'",
                                agent.id, req_agent_id
                            ));
                        }
                    }

                    let task_id = params
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter 'task_id'".to_string())?;

                    let task = coordinator
                        .get_task(task_id)
                        .map_err(|e| format!("Task not found: {}", e))?;

                    if let Some(ref assigned_agent) = task.assigned_agent_id {
                        let (canon_assigned, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(assigned_agent, "");
                        let (canon_caller, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if assigned_agent != &agent.id && canon_assigned != canon_caller {
                            return Err(format!(
                                "Forbidden: Agent '{}' cannot acquire scope leases for task '{}' assigned to '{}'",
                                agent.id, task_id, assigned_agent
                            ));
                        }
                    } else {
                        return Err(format!(
                            "Forbidden: Agent '{}' cannot acquire scope leases for unassigned task '{}'",
                            agent.id, task_id
                        ));
                    }
                }
                Ok(())
            }

            // 8. Merge enqueuing: Task owner or Master
            "merge_enqueue" | "merge.enqueue" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let task_id = params
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter 'task_id'".to_string())?;

                    let task = coordinator
                        .get_task(task_id)
                        .map_err(|e| format!("Task not found: {}", e))?;

                    if let Some(ref assigned_agent) = task.assigned_agent_id {
                        let (canon_req, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(assigned_agent, "");
                        let (canon_agent, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if assigned_agent != &agent.id && canon_req != canon_agent {
                            return Err(format!(
                                "Forbidden: Agent '{}' cannot enqueue task '{}' owned by '{}'",
                                agent.id, task_id, assigned_agent
                            ));
                        }
                    } else {
                        return Err(format!(
                            "Forbidden: Task '{}' is not assigned to any agent",
                            task_id
                        ));
                    }
                }
                Ok(())
            }

            // 9. Task & Masterplan claiming: Agent for itself, or Master for any
            "task_claim"
            | "task.claim"
            | "masterplan_claim_chunk"
            | "masterplan.claim_chunk" => {
                if let CallerAuthority::Agent(agent) = authority {
                    let req_agent_id = params
                        .get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !req_agent_id.is_empty() && req_agent_id != agent.id {
                        let (canon_req, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(req_agent_id, "");
                        let (canon_agent, ..) =
                            CoordinatorEngine::canonicalize_ide_identity(&agent.id, "");
                        if canon_req != canon_agent && req_agent_id != agent.id {
                            return Err(format!(
                                "Forbidden: Agent '{}' cannot claim work on behalf of '{}'",
                                agent.id, req_agent_id
                            ));
                        }
                    }
                }
                Ok(())
            }

            // 10. Fail-closed: any unclassified or unknown tool is strictly denied
            _ => Err(format!(
                "Forbidden: Tool '{}' is unclassified or unknown. Access denied by fail-closed security policy.",
                tool_name
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbPool;
    use crate::models::AgentCapabilitySet;

    fn setup_test_coordinator() -> CoordinatorEngine {
        let temp_dir =
            std::env::temp_dir().join(format!("agentxflow_auth_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = DbPool::new(&db_path).unwrap();
        let wt_root = temp_dir.join("worktrees");
        CoordinatorEngine::new_with_worktree_root(pool, wt_root)
    }

    #[test]
    fn test_master_authority_can_execute_admin_tools() {
        let coordinator = setup_test_coordinator();
        let authority = CallerAuthority::Master;

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "masterplan_decompose",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(result.is_ok(), "Master must be authorized for decompose");

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "masterplan_reset",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(result.is_ok(), "Master must be authorized for reset");
    }

    #[test]
    fn test_agent_authority_forbidden_from_admin_tools() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "masterplan_decompose",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(result.is_err(), "Agent must be forbidden from decompose");

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "masterplan_reset",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(result.is_err(), "Agent must be forbidden from reset");

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "unclaim_agent_tasks",
            &serde_json::json!({ "agent_id": "other_agent" }),
        );
        assert!(
            result.is_err(),
            "Agent must be forbidden from unclaim_agent_tasks"
        );
    }

    #[test]
    fn test_agent_authority_permitted_for_discovery() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "project_list",
            &serde_json::json!({}),
        );
        assert!(result.is_ok(), "Agent must be allowed to list projects");

        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "task_list",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(result.is_ok(), "Agent must be allowed to list tasks");
    }

    #[test]
    fn test_unknown_unclassified_tool_is_denied_by_fail_closed_policy() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        // Unknown fake future tool
        let result = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "future_privileged_admin_tool",
            &serde_json::json!({}),
        );
        assert!(result.is_err(), "Unknown tool must fail closed for agent");
        assert!(result.unwrap_err().contains("fail-closed security policy"));

        // Unknown tool for Master authority must also fail closed
        let master_res = McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "future_privileged_admin_tool",
            &serde_json::json!({}),
        );
        assert!(
            master_res.is_err(),
            "Unknown tool must fail closed even for Master"
        );
        assert!(master_res
            .unwrap_err()
            .contains("fail-closed security policy"));
    }

    #[test]
    fn test_agent_register_option_a_self_refresh_and_impersonation_rejection() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "antigravity".to_string(),
            name: "Antigravity".to_string(),
            agent_type: "IDE".to_string(),
            profile: "Architect".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        // Self-register / refresh same name
        let self_res = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "agent_register",
            &serde_json::json!({ "name": "Antigravity" }),
        );
        assert!(
            self_res.is_ok(),
            "Agent must be allowed to self-refresh: {:?}",
            self_res.err()
        );

        // Self-register / refresh with lowercase canonical name
        let self_canon_res = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "agent_register",
            &serde_json::json!({ "name": "antigravity" }),
        );
        assert!(
            self_canon_res.is_ok(),
            "Agent must be allowed to self-refresh with canonical ID"
        );

        // Impersonation: attempting to register a different agent
        let imp_res = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "agent_register",
            &serde_json::json!({ "name": "Claude Code" }),
        );
        assert!(
            imp_res.is_err(),
            "Agent must be forbidden from registering another identity"
        );
        assert!(imp_res
            .unwrap_err()
            .contains("cannot register or impersonate"));

        // Impersonation via agent_type manipulation: Claude agent attempting to pass agent_type: antigravity
        let claude_agent = Agent {
            id: "claude-code".to_string(),
            name: "Claude Code".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let claude_authority = CallerAuthority::Agent(claude_agent);

        let type_hijack_res = McpAuthPolicy::authorize(
            &claude_authority,
            &coordinator,
            "agent_register",
            &serde_json::json!({ "name": "Helper", "agent_type": "antigravity" }),
        );
        assert!(
            type_hijack_res.is_err(),
            "Agent must be forbidden from hijacking another identity via agent_type"
        );
        assert!(type_hijack_res
            .unwrap_err()
            .contains("cannot register or impersonate"));

        // Master authority can register any agent with any type
        let master_res = McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "agent_register",
            &serde_json::json!({ "name": "Claude Code", "agent_type": "CLI" }),
        );
        assert!(
            master_res.is_ok(),
            "Master must be allowed to register any agent"
        );
    }

    #[test]
    fn test_merge_process_denied_for_agent_allowed_for_master() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        let agent_res = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "merge_process",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(
            agent_res.is_err(),
            "Agent must be forbidden from merge_process"
        );
        assert!(agent_res
            .unwrap_err()
            .contains("requires Master/Coordinator authority"));

        let agent_alias_res = McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "merge.process",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(
            agent_alias_res.is_err(),
            "Agent must be forbidden from merge.process alias"
        );

        let master_res = McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "merge_process",
            &serde_json::json!({ "project_id": "proj_1" }),
        );
        assert!(
            master_res.is_ok(),
            "Master must be allowed to execute merge_process"
        );
    }

    #[test]
    fn test_scope_release_matrix_own_task_other_task_unassigned_task_leases() {
        let coordinator = setup_test_coordinator();
        let now = chrono::Utc::now().to_rfc3339();
        let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();

        let conn = coordinator.db.lock();
        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
            [&now],
        ).unwrap();
        // Task assigned to agent_1
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, assigned_agent_id, created_at, updated_at) VALUES ('t_own', 'p1', 'T1', 'Desc', 'WORKING', 'NONE', 'MEDIUM', 0, 'agent_1', ?1, ?1)",
            [&now],
        ).unwrap();
        // Task assigned to agent_2
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, assigned_agent_id, created_at, updated_at) VALUES ('t_other', 'p1', 'T2', 'Desc', 'WORKING', 'NONE', 'MEDIUM', 0, 'agent_2', ?1, ?1)",
            [&now],
        ).unwrap();
        // Unassigned task
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, assigned_agent_id, created_at, updated_at) VALUES ('t_unassigned', 'p1', 'T3', 'Desc', 'BACKLOG', 'NONE', 'MEDIUM', 0, NULL, ?1, ?1)",
            [&now],
        ).unwrap();
        // Lease owned by agent_1
        conn.execute(
            "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at) VALUES ('l_own', 't_own', 'agent_1', 'src/**', 'EXCLUSIVE_WRITE', ?1, ?2)",
            [&expires, &now],
        ).unwrap();
        // Lease owned by agent_2
        conn.execute(
            "INSERT INTO scope_leases (id, task_id, agent_id, pattern, access_type, expires_at, created_at) VALUES ('l_other', 't_other', 'agent_2', 'src/**', 'EXCLUSIVE_WRITE', ?1, ?2)",
            [&expires, &now],
        ).unwrap();
        drop(conn);

        let agent1 = Agent {
            id: "agent_1".to_string(),
            name: "Agent 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority1 = CallerAuthority::Agent(agent1);

        // 1. Agent releases own task scope -> ALLOW
        let res_own_task = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "task_id": "t_own" }),
        );
        assert!(
            res_own_task.is_ok(),
            "Agent must be allowed to release scope for own task"
        );

        // 2. Agent releases other agent's task scope -> DENY
        let res_other_task = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "task_id": "t_other" }),
        );
        assert!(
            res_other_task.is_err(),
            "Agent must be forbidden from releasing other task's scope"
        );
        assert!(res_other_task
            .unwrap_err()
            .contains("cannot release scope for task"));

        // 3. Agent releases unassigned task scope -> DENY
        let res_unassigned_task = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "task_id": "t_unassigned" }),
        );
        assert!(
            res_unassigned_task.is_err(),
            "Agent must be forbidden from releasing unassigned task scope"
        );
        assert!(res_unassigned_task
            .unwrap_err()
            .contains("cannot release scope for unassigned task"));

        // 4. Agent releases own lease -> ALLOW
        let res_own_lease = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "lease_id": "l_own" }),
        );
        assert!(
            res_own_lease.is_ok(),
            "Agent must be allowed to release own lease"
        );

        // 5. Agent releases other lease -> DENY
        let res_other_lease = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "lease_id": "l_other" }),
        );
        assert!(
            res_other_lease.is_err(),
            "Agent must be forbidden from releasing other lease"
        );

        // 6. Master authority can release any task or lease -> ALLOW
        let master_task_res = McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "scope_release",
            &serde_json::json!({ "task_id": "t_unassigned" }),
        );
        assert!(
            master_task_res.is_ok(),
            "Master must be allowed to release unassigned task scope"
        );
    }

    #[test]
    fn test_aliases_have_identical_authorization_semantics() {
        let coordinator = setup_test_coordinator();
        let agent = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority = CallerAuthority::Agent(agent);

        // Discovery alias
        assert!(McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "project.list",
            &serde_json::json!({})
        )
        .is_ok());
        assert!(McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "project_list",
            &serde_json::json!({})
        )
        .is_ok());

        // Admin alias
        assert!(McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "agent.force_idle",
            &serde_json::json!({ "agent_id": "a" })
        )
        .is_err());
        assert!(McpAuthPolicy::authorize(
            &authority,
            &coordinator,
            "force_agent_idle",
            &serde_json::json!({ "agent_id": "a" })
        )
        .is_err());

        // Master on Admin alias
        assert!(McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "agent.force_idle",
            &serde_json::json!({ "agent_id": "a" })
        )
        .is_ok());
        assert!(McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "force_agent_idle",
            &serde_json::json!({ "agent_id": "a" })
        )
        .is_ok());
    }

    #[test]
    fn test_scope_acquire_matrix_own_task_other_task_unassigned_task() {
        let coordinator = setup_test_coordinator();
        let conn = coordinator.db.lock();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO projects (id, name, path, master_spec, target_branch, created_at, updated_at) VALUES ('p1', 'P1', 'dummy/path', 'Spec', 'main', ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at) VALUES ('agent_worker_1', 'Worker 1', 'CLI', 'Imp', 'IDLE', ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO agents (id, name, agent_type, profile, status, last_heartbeat, created_at) VALUES ('agent_worker_2', 'Worker 2', 'CLI', 'Imp', 'IDLE', ?1, ?1)",
            [&now],
        ).unwrap();

        // 1. Task owned by agent_worker_1
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, assigned_agent_id, is_stale, created_at, updated_at) VALUES ('t_own', 'p1', 'Own Task', 'Desc', 'RUNNING', 'CLAIMING', 'HIGH', 'agent_worker_1', 0, ?1, ?1)",
            [&now],
        ).unwrap();

        // 2. Task owned by agent_worker_2
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, assigned_agent_id, is_stale, created_at, updated_at) VALUES ('t_other', 'p1', 'Other Task', 'Desc', 'RUNNING', 'CLAIMING', 'HIGH', 'agent_worker_2', 0, ?1, ?1)",
            [&now],
        ).unwrap();

        // 3. Unassigned task
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, description, state, substate, priority, is_stale, created_at, updated_at) VALUES ('t_unassigned', 'p1', 'Unassigned Task', 'Desc', 'BACKLOG', 'NONE', 'HIGH', 0, ?1, ?1)",
            [&now],
        ).unwrap();
        drop(conn);

        let agent1 = Agent {
            id: "agent_worker_1".to_string(),
            name: "Worker 1".to_string(),
            agent_type: "CLI".to_string(),
            profile: "Implementer".to_string(),
            status: "IDLE".to_string(),
            capabilities: AgentCapabilitySet::default(),
            last_heartbeat: "".to_string(),
            created_at: "".to_string(),
            session_token: None,
            active_task_id: None,
            active_task_title: None,
            last_seen_seconds: None,
        };
        let authority1 = CallerAuthority::Agent(agent1);

        // 1. Agent acquires scope for own task -> ALLOW
        let res_own = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_acquire",
            &serde_json::json!({
                "task_id": "t_own",
                "agent_id": "agent_worker_1",
                "patterns": ["src/**"]
            }),
        );
        assert!(
            res_own.is_ok(),
            "Agent must be allowed to acquire scope for own task: {:?}",
            res_own.err()
        );

        // 2. Agent acquires scope for other agent's task -> DENY
        let res_other = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_acquire",
            &serde_json::json!({
                "task_id": "t_other",
                "agent_id": "agent_worker_1",
                "patterns": ["src/**"]
            }),
        );
        assert!(
            res_other.is_err(),
            "Agent must be forbidden from acquiring scope for another agent's task"
        );
        assert!(res_other
            .unwrap_err()
            .contains("cannot acquire scope leases for task"));

        // 3. Agent acquires scope for unassigned task -> DENY
        let res_unassigned = McpAuthPolicy::authorize(
            &authority1,
            &coordinator,
            "scope_acquire",
            &serde_json::json!({
                "task_id": "t_unassigned",
                "agent_id": "agent_worker_1",
                "patterns": ["src/**"]
            }),
        );
        assert!(
            res_unassigned.is_err(),
            "Agent must be forbidden from acquiring scope for unassigned task"
        );
        assert!(res_unassigned.unwrap_err().contains("unassigned task"));

        // 4. Master authority can acquire scope for any task -> ALLOW
        let master_res = McpAuthPolicy::authorize(
            &CallerAuthority::Master,
            &coordinator,
            "scope_acquire",
            &serde_json::json!({
                "task_id": "t_unassigned",
                "agent_id": "agent_worker_1",
                "patterns": ["src/**"]
            }),
        );
        assert!(
            master_res.is_ok(),
            "Master must be allowed to acquire scope on behalf of agent"
        );
    }
}
