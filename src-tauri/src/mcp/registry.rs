use serde_json::json;

pub fn get_all_tool_definitions() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "agentxflow_current_context",
            "description": "Get the tailored context, active task, assigned worktree, active scopes, and recommended next action for the requesting agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Optional requesting agent ID" },
                    "project_id": { "type": "string", "description": "Optional target project ID" }
                }
            }
        }),
        json!({
            "name": "project_list",
            "description": "List all managed projects with exact IDs, repository paths, and target branches.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "project_context",
            "description": "Get architectural rules, contract hashes, and project metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Target project ID" },
                    "task_id": { "type": "string", "description": "Optional task ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "masterplan_list",
            "description": "List all masterplans across all projects with status, step counts, and active handoffs.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "masterplan_get",
            "description": "Get masterplan specification, current status, project identity, and decomposition instructions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Target project ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "prepare_masterplan",
            "description": "Atomically save, parse, structure, and prepare a masterplan for agents.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Target project ID" },
                    "raw_text": { "type": "string", "description": "Masterplan specification text" },
                    "target_step_count": { "type": "integer", "description": "Desired number of execution steps" },
                    "max_steps_per_agent": { "type": "integer", "description": "Maximum steps claimable by one agent" }
                },
                "required": ["project_id", "raw_text"]
            }
        }),
        json!({
            "name": "masterplan_status",
            "description": "Query plan progress stats, total steps, and step statuses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Target project ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "masterplan_decompose",
            "description": "Decompose raw masterplan into structured execution steps. Returns compact summary by default.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID" },
                    "idempotency_key": { "type": "string", "description": "Optional client idempotency key (e.g. 'decompose:<plan_id>:<hash>') for retry protection" },
                    "compact": { "type": "boolean", "description": "When true (default), returns token-efficient summary instead of echoing all step records" },
                    "append": { "type": "boolean", "description": "When true, appends/merges steps for phased multi-chunk decomposition (e.g. 25 steps per phase) without wiping existing steps" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step_index": { "type": "integer" },
                                "title": { "type": "string" },
                                "description": { "type": "string" },
                                "suggested_scope": { "type": "string" },
                                "acceptance_criteria": { "type": "string" }
                            },
                            "required": ["step_index", "title", "description"]
                        }
                    }
                },
                "required": ["project_id", "steps"]
            }
        }),
        json!({
            "name": "masterplan_claim_chunk",
            "description": "Claim the next batch of steps from an organized masterplan and allocate an isolated Git worktree.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID" },
                    "agent_id": { "type": "string", "description": "Claiming agent ID" },
                    "count": { "type": "integer", "description": "Optional step count (capped by coordinator limit)" }
                },
                "required": ["project_id", "agent_id"]
            }
        }),
        json!({
            "name": "agent_register",
            "description": "Register or connect an AI IDE / agent session and obtain an authoritative agent_id and session token.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": [
                            "Antigravity",
                            "Claude Code",
                            "Cursor",
                            "OpenCode",
                            "OpenAI Codex",
                            "Gemini CLI",
                            "GitHub Copilot",
                            "Windsurf",
                            "Junie",
                            "Aider"
                        ],
                        "description": "Select your AI IDE / Agent platform"
                    },
                    "agent_type": {
                        "type": "string",
                        "enum": ["IDE", "CLI", "Autonomous Swarm", "Reviewer", "Implementer"],
                        "description": "Agent category type"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "agent_heartbeat",
            "description": "Keep agent session and active scope leases alive.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Unique agent identifier" }
                },
                "required": ["agent_id"]
            }
        }),
        json!({
            "name": "task_list",
            "description": "List all tasks in backlog or ready queue for a project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Target project ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "task_get",
            "description": "Get task details including prompt, status, acceptance criteria, and worktree path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "task_claim",
            "description": "Atomically claim a task and cut an isolated Git worktree on disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Claiming agent ID" }
                },
                "required": ["task_id", "agent_id"]
            }
        }),
        json!({
            "name": "scope_acquire",
            "description": "Atomically lock file glob patterns for exclusive write access.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Agent identifier" },
                    "patterns": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "File glob patterns (e.g. ['src/auth/**', 'tests/auth_test.rs'])"
                    }
                },
                "required": ["task_id", "agent_id", "patterns"]
            }
        }),
        json!({
            "name": "scope_release",
            "description": "Release held write locks back to the pool.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Optional agent identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "task_complete_step",
            "description": "Mark a required task step completed with test or build evidence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "step_id": { "type": "string", "description": "Step identifier" },
                    "agent_id": { "type": "string", "description": "Optional claiming agent identifier" },
                    "evidence": { "type": "string", "description": "Structured command output or test log" }
                },
                "required": ["step_id"]
            }
        }),
        json!({
            "name": "dag_dependencies",
            "description": "List blocker tasks that must finish before this task starts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "task_submit",
            "description": "Submit task for automatic coordinator verification profile execution, machine evaluation, and git mutation audit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Agent identifier" }
                },
                "required": ["task_id", "agent_id"]
            }
        }),
        json!({
            "name": "task_cancel",
            "description": "Cancel an active task, releasing all write scope leases, cleaning up worktrees, and reverting any masterplan steps back to PENDING.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Optional claiming agent identifier" },
                    "reason": { "type": "string", "description": "Optional cancellation rationale" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "task_requeue",
            "description": "Requeue a claimed chunk task back to masterplan pending steps, releasing held scope leases.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" },
                    "agent_id": { "type": "string", "description": "Optional claiming agent identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "task_details",
            "description": "Get complete task details including steps, acceptance criteria, active scope leases, attempts, and verification results.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "merge_queue_status",
            "description": "List all queued branch merges and their integration statuses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "merge_enqueue",
            "description": "Enqueue a verified or MERGE_READY task into the serialized merge queue.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID" },
                    "task_id": { "type": "string", "description": "Task identifier" }
                },
                "required": ["project_id", "task_id"]
            }
        }),
        json!({
            "name": "merge_process",
            "description": "Process the next ready serialized branch merge in queue for a project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID" }
                },
                "required": ["project_id"]
            }
        }),
        json!({
            "name": "task_reconcile",
            "description": "Reconcile task state, task attempt, proof bundle, and merge queue status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task identifier" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "unclaim_agent_tasks",
            "description": "Safely unclaim all active tasks for an agent, reverting masterplan steps to PENDING and releasing locks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Agent identifier" }
                },
                "required": ["agent_id"]
            }
        }),
        json!({
            "name": "force_agent_idle",
            "description": "Forces an agent status to IDLE, unclaiming active tasks and resetting session heartbeat.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Agent identifier" }
                },
                "required": ["agent_id"]
            }
        }),
    ]
}
