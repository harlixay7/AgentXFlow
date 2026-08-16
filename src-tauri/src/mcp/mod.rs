use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

pub mod registry;

use crate::core::CoordinatorEngine;
use crate::security::SecurityManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

pub struct McpServerState {
    pub coordinator: CoordinatorEngine,
    pub security: SecurityManager,
}

pub struct McpServer {
    pub port: u16,
    pub state: Arc<McpServerState>,
}

impl McpServer {
    pub fn new(coordinator: CoordinatorEngine, port: u16, security: SecurityManager) -> Self {
        let state = Arc::new(McpServerState {
            coordinator,
            security,
        });
        Self { port, state }
    }

    pub async fn start(&self) -> Result<SocketAddr, String> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/mcp", post(handle_mcp_streamable_http))
            .route("/mcp/sse", get(handle_mcp_legacy_sse))
            .route("/health", get(handle_health))
            .layer(cors)
            .with_state(self.state.clone());

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Binding Authoritative Standards-Compliant MCP Gateway to http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind TCP listener on {}: {}", addr, e))?;

        let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("MCP server error: {}", e);
            }
        });

        Ok(bound_addr)
    }
}

async fn handle_health() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "application/json")],
        Json(serde_json::json!({
            "status": "ok",
            "service": "AgentXFlow Authoritative MCP Gateway (Viducia)",
            "protocol_version": "2024-11-05",
            "supported_versions": ["2024-11-05", "2026-07-28"],
            "transport": "Streamable HTTP"
        })),
    )
}

async fn handle_mcp_legacy_sse() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "text/event-stream"), ("Cache-Control", "no-cache")],
        "event: endpoint\ndata: /mcp\n\n",
    )
}

/// Validates Host and Origin to prevent unauthorized cross-origin access
fn validate_security_headers(headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    if let Some(host) = headers.get("host") {
        if let Ok(host_str) = host.to_str() {
            let clean = host_str.split(':').next().unwrap_or("");
            if clean != "127.0.0.1" && clean != "localhost" {
                return Err((StatusCode::FORBIDDEN, format!("Forbidden: Host '{}' is not a permitted loopback address", host_str)));
            }
        }
    }

    if let Some(origin) = headers.get("origin") {
        if let Ok(origin_str) = origin.to_str() {
            let is_permitted = if origin_str == "tauri://localhost" || origin_str == "https://tauri.localhost" {
                true
            } else if let Ok(uri) = origin_str.parse::<axum::http::Uri>() {
                let host = uri.host().unwrap_or("");
                let scheme = uri.scheme_str().unwrap_or("");
                (scheme == "http" || scheme == "https" || scheme == "tauri") && (host == "127.0.0.1" || host == "localhost")
            } else {
                false
            };

            if !is_permitted {
                return Err((StatusCode::FORBIDDEN, format!("Forbidden: Origin '{}' is not permitted", origin_str)));
            }
        }
    }

    Ok(())
}

/// Standards-Compliant Model Context Protocol (MCP) HTTP Handler
async fn handle_mcp_streamable_http(
    State(state): State<Arc<McpServerState>>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    // 1. Host / Origin security check
    if let Err((status, msg)) = validate_security_headers(&headers) {
        return (
            status,
            HeaderMap::new(),
            Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32003,
                    message: msg,
                    data: None,
                }),
            }),
        );
    }

    // 2. Bearer Authentication validation (Master token or Agent Session token)
    let (is_authenticated, caller_agent) = if let Some(auth) = headers.get("authorization") {
        if let Ok(token_str) = auth.to_str() {
            let clean_token = token_str.trim_start_matches("Bearer ").trim();
            if state.security.validate_token(clean_token) {
                (true, None)
            } else if let Some(agent) = state.coordinator.get_agent_by_session(clean_token) {
                (true, Some(agent))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        }
    } else {
        (false, None)
    };

    if !is_authenticated {
        return (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32001,
                    message: "Unauthorized: Missing or invalid Bearer token in Authorization header".to_string(),
                    data: None,
                }),
            }),
        );
    }

    let method = req.method.as_str();
    let params = req.params.clone().unwrap_or(serde_json::json!({}));

    // Dynamic protocol version negotiation supporting standard 2024-11-05 and 2026-07-28
    let requested_version = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .or_else(|| headers.get("MCP-Protocol-Version").and_then(|h| h.to_str().ok()))
        .unwrap_or("2024-11-05");

    let negotiated_version = match requested_version {
        "2026-07-28" => "2026-07-28",
        _ => "2024-11-05",
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert("MCP-Protocol-Version", HeaderValue::from_str(negotiated_version).unwrap_or(HeaderValue::from_static("2024-11-05")));

    // Standard MCP Protocol Routing
    let response_result = match method {
        // --- 1. Standard MCP Protocol Handlers ---
        "initialize" => {
            Ok(serde_json::json!({
                "protocolVersion": negotiated_version,
                "serverInfo": {
                    "name": "AgentXFlow Coordinator",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    },
                    "prompts": {
                        "listChanged": false
                    },
                    "resources": {
                        "subscribe": false,
                        "listChanged": false
                    },
                    "logging": {}
                }
            }))
        }

        "notifications/initialized" | "notifications/cancelled" => {
            Ok(serde_json::json!({}))
        }

        "ping" => {
            Ok(serde_json::json!({}))
        }

        "prompts/list" => {
            Ok(serde_json::json!({
                "prompts": []
            }))
        }

        "resources/list" => {
            Ok(serde_json::json!({
                "resources": []
            }))
        }

        "resources/templates/list" => {
            Ok(serde_json::json!({
                "resourceTemplates": []
            }))
        }

        "logging/setLevel" => {
            Ok(serde_json::json!({}))
        }

        "tools/list" => {
            Ok(serde_json::json!({
                "tools": get_tool_definitions()
            }))
        }

        "tools/call" => {
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));
            execute_mcp_tool(&state, caller_agent.as_ref(), tool_name, &arguments)
                .map(|val| serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                    }],
                    "isError": false
                }))
        }

        // --- 2. Direct Tool Method Fallback Routing ---
        other => execute_mcp_tool(&state, caller_agent.as_ref(), other, &params),
    };

    match response_result {
        Ok(res_val) => (
            StatusCode::OK,
            response_headers,
            Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(res_val),
                error: None,
            }),
        ),
        Err(err_msg) => (
            StatusCode::OK,
            response_headers,
            Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: err_msg,
                    data: None,
                }),
            }),
        ),
    }
}

/// Executes individual tool logic with strict ownership and session checking
fn execute_mcp_tool(
    state: &Arc<McpServerState>,
    caller_agent: Option<&crate::models::Agent>,
    tool_name: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Transparent activity heartbeat: refresh liveness for caller whenever an agent is resolved
    if let Some(agent) = caller_agent {
        state.coordinator.touch_agent_activity(&agent.id);
    } else if let Some(raw_aid) = params.get("agent_id").and_then(|v| v.as_str()).or_else(|| params.get("caller_agent_id").and_then(|v| v.as_str())) {
        state.coordinator.touch_agent_activity(raw_aid);
    }

    let resolve_agent_id = |req_id: &str| -> Result<String, String> {
        if let Some(agent) = caller_agent {
            if !req_id.is_empty() && req_id != agent.id {
                return Err(format!(
                    "Agent impersonation rejected: Authenticated session belongs to '{}', cannot act on behalf of '{}'",
                    agent.id, req_id
                ));
            }
            Ok(agent.id.clone())
        } else if !req_id.is_empty() {
            let (canon_id, ..) = crate::core::CoordinatorEngine::canonicalize_ide_identity(req_id, "");
            if state.coordinator.is_agent_registered(&canon_id) {
                Ok(canon_id)
            } else if state.coordinator.is_agent_registered(req_id) {
                Ok(req_id.to_string())
            } else {
                Err(format!(
                    "Agent '{}' is not registered. Call 'agent_register' with your agent name first.",
                    req_id
                ))
            }
        } else {
            // Check if there is exactly one registered agent on the coordinator
            let agents = state.coordinator.list_agents().unwrap_or_default();
            if agents.len() == 1 {
                Ok(agents[0].id.clone())
            } else {
                Err("Missing 'agent_id' parameter. Pass 'agent_id' or call 'agent_register'.".to_string())
            }
        }
    };

    match tool_name {
        // Discovery tools
        "agentxflow_current_context" | "context.current" => {
            let caller_agent_id = params.get("agent_id").and_then(|v| v.as_str()).or_else(|| caller_agent.map(|a| a.id.as_str()));
            let project_id = params.get("project_id").and_then(|v| v.as_str());
            state.coordinator.get_current_context(caller_agent_id, project_id).map(|ctx| serde_json::to_value(ctx).unwrap())
        }

        "project_list" | "project.list" => {
            state.coordinator.list_projects().map(|projects| serde_json::to_value(projects).unwrap())
        }

        "project_context" | "project.context" => {
            let project_id_raw = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let task_id_opt = params.get("task_id").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty());
            let project_id = if project_id_raw.trim().is_empty() {
                let ctx = state.coordinator.get_current_context(None, None)?;
                if let Some(pid) = ctx.active_project_id {
                    pid
                } else {
                    return Err("Missing required parameter 'project_id'. Query 'project_list' to obtain valid project IDs.".to_string());
                }
            } else {
                project_id_raw.to_string()
            };

            if let Some(task_id) = task_id_opt {
                state.coordinator.get_context_pack(&project_id, task_id).map(|cp| serde_json::to_value(cp).unwrap())
            } else {
                state.coordinator.get_project_context(&project_id).map(|pc| serde_json::to_value(pc).unwrap())
            }
        }

        "masterplan_list" | "masterplan.list" => {
            state.coordinator.list_all_masterplans().map(|plans| serde_json::to_value(plans).unwrap())
        }

        "task_list" | "task.list" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'. Query 'project_list' or 'agentxflow_current_context' to obtain valid project IDs.".to_string());
            }
            let include_stale = params.get("include_stale").and_then(|v| v.as_bool()).unwrap_or(false);
            state.coordinator.list_tasks(project_id).map(|tasks| {
                if include_stale {
                    serde_json::to_value(tasks).unwrap()
                } else {
                    let active_tasks: Vec<_> = tasks.into_iter().filter(|t| !t.is_stale).collect();
                    serde_json::to_value(active_tasks).unwrap()
                }
            })
        }

        "task_get" | "task.get" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if task_id.trim().is_empty() {
                return Err("Missing required parameter 'task_id'.".to_string());
            }
            state.coordinator.get_task(task_id).map(|task| serde_json::to_value(task).unwrap())
        }

        "task_details" | "task.details" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if task_id.trim().is_empty() {
                return Err("Missing required parameter 'task_id'.".to_string());
            }
            state.coordinator.get_task_details(task_id).map(|td| serde_json::to_value(td).unwrap())
        }

        "dag_dependencies" | "dependency_list" | "dag.dependencies" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if task_id.trim().is_empty() {
                return Err("Missing required parameter 'task_id'.".to_string());
            }
            state.coordinator.dag.get_dependencies_for_task(task_id).map(|deps| serde_json::to_value(deps).unwrap())
        }

        "merge_queue_status" | "merge.queue_status" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'.".to_string());
            }
            state.coordinator.merge.list_queue(project_id).map(|items| serde_json::to_value(items).unwrap())
        }

        "merge_enqueue" | "merge.enqueue" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() || task_id.trim().is_empty() {
                return Err("Missing required parameters 'project_id' and 'task_id'.".to_string());
            }
            state.coordinator.enqueue_task_by_id(project_id, task_id).map(|item| serde_json::to_value(item).unwrap())
        }

        "merge_process" | "merge.process" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'.".to_string());
            }
            state.coordinator.process_next_merge(project_id).map(|res| serde_json::json!({
                "processed": res.is_some(),
                "attempt": res
            }))
        }

        "task_reconcile" | "task.reconcile" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if task_id.trim().is_empty() {
                return Err("Missing required parameter 'task_id'.".to_string());
            }
            state.coordinator.reconcile_task(task_id)
        }

        // Agent Identity
        "agent_register" | "agent.register" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("Agent");
            let agent_type = params.get("agent_type").and_then(|v| v.as_str()).unwrap_or("Generic");
            state.coordinator.register_agent(name, agent_type).map(|agent| serde_json::to_value(agent).unwrap())
        }

        "agent_heartbeat" | "agent.heartbeat" => {
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.agent_heartbeat(&agent_id).map(|_| serde_json::json!({ "status": "ok" }))
        }

        // Mutation & Task Execution
        "task_claim" | "task.claim" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.claim_task(task_id, &agent_id).map(|task| serde_json::to_value(task).unwrap())
        }

        "task_complete_step" | "task.complete_step" => {
            let step_id = params.get("step_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id_opt = if !raw_agent_id.trim().is_empty() {
                Some(resolve_agent_id(raw_agent_id)?)
            } else {
                None
            };
            let evidence_json = params.get("evidence").map(|v| v.to_string());
            state.coordinator.complete_step(step_id, agent_id_opt.as_deref(), evidence_json.as_deref()).map(|step| serde_json::to_value(step).unwrap())
        }

        "task_submit" | "task.submit" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.submit_task(task_id, &agent_id).map(|res| {
                if res.is_valid {
                    let task = state.coordinator.get_task(task_id).ok();
                    let project_id = task.as_ref().map(|t| t.project_id.as_str()).unwrap_or("");
                    let plan = state.coordinator.get_masterplan(project_id).ok().flatten();
                    let require_approval = plan.as_ref().map(|p| p.require_milestone_approval).unwrap_or(true);
                    let steps = state.coordinator.list_masterplan_steps(project_id).unwrap_or_default();
                    let remaining_pending = steps.iter().filter(|s| s.status == "PENDING").count();
                    let completed_in_plan = steps.iter().filter(|s| s.status == "COMPLETED").count();
                    let total_steps = steps.len();

                    let (next_action, instruction) = if remaining_pending == 0 && (completed_in_plan >= total_steps || total_steps > 0) {
                        (
                            "FINAL_RELEASE_DELIVERY",
                            "🎉 ALL MASTERPLAN STEPS COMPLETED! As the final agent completing the last chunk (Step N/N), you must perform the Final Release Delivery:\n1. Build the production bundle / executable (e.g. npm run build / cargo build --release).\n2. Create or verify the automated launcher script (`run.bat` for Windows / `start.sh` for Unix or project executable).\n3. Test and verify that the application launches successfully.\n4. Create/update a comprehensive user manual (`USER_GUIDE.md` / `HOW_TO_USE.md`) explaining the full app architecture, configuration, features, and step-by-step instructions on how to use the entire application.\n5. Present a full application walkthrough to the user in chat."
                        )
                    } else if require_approval {
                        (
                            "REPORT_TO_USER",
                            "Milestone completed successfully: All chunk steps verified and enqueued for merge. Interactive Milestone Mode is active. Stop calling MCP tools now. Present a comprehensive milestone walkthrough and test summary to the user in this IDE chat, and wait for the user to confirm/prompt before claiming the next chunk."
                        )
                    } else {
                        (
                            "masterplan_claim_chunk",
                            "Chunk verified and enqueued for merge. Continuous Autonomous Swarm Mode is active: proceed immediately to claim the next available chunk using 'masterplan_claim_chunk'."
                        )
                    };

                    serde_json::json!({
                        "is_valid": true,
                        "status": "CHUNK_COMPLETED",
                        "verification": res,
                        "task_id": task_id,
                        "agent_id": agent_id,
                        "require_milestone_approval": require_approval,
                        "masterplan_progress": {
                            "completed_steps": completed_in_plan,
                            "remaining_pending_steps": remaining_pending,
                            "total_steps": total_steps
                        },
                        "next_action": next_action,
                        "instruction": instruction
                    })
                } else {
                    serde_json::json!({
                        "is_valid": false,
                        "status": "VERIFICATION_FAILED",
                        "verification": res,
                        "task_id": task_id,
                        "agent_id": agent_id,
                        "rejection_reasons": res.rejection_reasons,
                        "next_action": "FIX_VIOLATIONS_AND_RESUBMIT",
                        "instruction": "Verification rejected. Inspect rejection_reasons, correct the code inside your assigned worktree, and call task_submit again."
                    })
                }
            })
        }

        "task_cancel" | "task.cancel" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id_opt = if !raw_agent_id.trim().is_empty() {
                Some(resolve_agent_id(raw_agent_id)?)
            } else {
                None
            };
            let reason = params.get("reason").and_then(|v| v.as_str());
            state.coordinator.cancel_task(task_id, agent_id_opt.as_deref(), reason).map(|task| serde_json::json!({
                "success": true,
                "task_id": task.id,
                "state": task.state.as_str(),
                "message": "Task cancelled and scope leases released."
            }))
        }

        "task_requeue" | "task.requeue" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id_opt = if !raw_agent_id.trim().is_empty() {
                Some(resolve_agent_id(raw_agent_id)?)
            } else {
                None
            };
            state.coordinator.requeue_task(task_id, agent_id_opt.as_deref()).map(|_| serde_json::json!({
                "success": true,
                "task_id": task_id,
                "message": "Task chunk requeued to masterplan pending steps and scope leases released."
            }))
        }

        "scope_acquire" | "scope.acquire" | "scope.propose" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            let patterns: Vec<String> = params
                .get("patterns")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            state.coordinator.scope.acquire_scope(task_id, &agent_id, patterns, "EXCLUSIVE_WRITE").map(|leases| serde_json::to_value(leases).unwrap())
        }

        "scope_release" | "scope.release" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if !raw_agent_id.trim().is_empty() {
                let agent_id = resolve_agent_id(raw_agent_id)?;
                state.coordinator.scope.release_scope_by_agent(task_id, &agent_id).map(|_| serde_json::json!({ "status": "released" }))
            } else {
                state.coordinator.scope.release_scope(task_id).map(|_| serde_json::json!({ "status": "released" }))
            }
        }

        "criteria_satisfy" | "criteria.satisfy" => {
            Err("Authorization rejected: Autonomous agents cannot self-satisfy criteria. Criteria satisfaction is derived strictly from automated coordinator machine evaluators.".to_string())
        }

        // Masterplan Hub Tools
        "prepare_masterplan" | "masterplan.prepare" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_text = params.get("raw_text").and_then(|v| v.as_str()).unwrap_or("");
            let target_step_count = params.get("target_step_count").and_then(|v| v.as_i64()).unwrap_or(20) as i32;
            let max_steps_per_agent = params.get("max_steps_per_agent").and_then(|v| v.as_i64()).unwrap_or(5) as i32;

            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'".to_string());
            }
            if raw_text.trim().is_empty() {
                return Err("Missing required parameter 'raw_text'".to_string());
            }

            state.coordinator.prepare_masterplan(project_id, raw_text, target_step_count, max_steps_per_agent).map(|snap| serde_json::to_value(snap).unwrap())
        }

        "masterplan_get" | "masterplan.get" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'. Query 'project_list' or 'agentxflow_current_context' to obtain valid project IDs.".to_string());
            }
            let proj = state.coordinator.list_projects()?.into_iter().find(|p| p.id == project_id)
                .ok_or_else(|| format!("Project '{}' not found", project_id))?;
            match state.coordinator.get_masterplan(project_id) {
                Ok(Some(plan)) => {
                    let steps = state.coordinator.list_masterplan_steps(project_id).unwrap_or_default();
                    let target_steps = if plan.target_step_count > 0 { plan.target_step_count } else { 20 };
                    let p1_end = std::cmp::max(1, (target_steps as f64 * 0.25).round() as i32);
                    let p2_end = std::cmp::max(p1_end + 1, (target_steps as f64 * 0.50).round() as i32);
                    let p3_end = std::cmp::max(p2_end + 1, (target_steps as f64 * 0.75).round() as i32);

                    let (next_action, instruction, architectural_guidelines) = if plan.status == "UNSORTED" {
                        (
                            "masterplan_decompose",
                            format!("The masterplan is UNSORTED. Read raw_text and call masterplan_decompose with the normalized {} steps array (either all at once or in phased batches using append: true).", target_steps),
                            Some(serde_json::json!({
                                "role": "Master Architect / Planner",
                                "target_step_count": target_steps,
                                "objective": format!("Decompose raw master specification into an exhaustive, production-grade 4-phase implementation blueprint with {} steps.", target_steps),
                                "phased_decomposition_strategy": [
                                    format!("Phase 1 (Steps 1–{}): Runnable Baseline Scaffolding & Core Architecture (Step 1 MUST scaffold package.json, index.html, vite.config.ts/framework config, main.tsx/index.js, App.tsx, and router/navigation skeleton; Steps 2–{} implement database schemas, shared types, global state stores, and project utilities).", p1_end, p1_end),
                                    format!("Phase 2 (Steps {}–{}): Domain Business Logic, Service Layers, APIs, IPC Handlers, and State Bindings (connected directly to global app context and stores).", p1_end + 1, p2_end),
                                    format!("Phase 3 (Steps {}–{}): High-Fidelity UI Views & Components (MANDATORY: Every component step must specify exact import & mounting instructions in App.tsx / AppRoutes.tsx / Navigation bar so all features are interactive and visible in the live application).", p2_end + 1, p3_end),
                                    format!("Phase 4 (Steps {}–{}): End-to-End Integration, Error Boundaries, Automated Launcher Build (`run.bat` for Windows / `start.sh` for Unix with dependency check, server start, and browser auto-open), Launch Verification, and Complete `USER_GUIDE.md` / `HOW_TO_USE.md`.", p3_end + 1, target_steps)
                                ],
                                "rules": [
                                    "1. Runnable from Step 1: Step 1 MUST scaffold the complete runnable application baseline (package.json, entry point, build configuration, root App component, and routing) and verify npm run dev / npm run build.",
                                    "2. Mandatory Root Mounting (Zero Isolated Code): Every UI component and view step MUST include explicit instructions to import and mount it into App.tsx, AppRoutes.tsx, or the main navigation menu.",
                                    "3. Deep Step Specifications: Specify Exact Target Files, Concrete Exports & Interfaces, Design Specs (responsive layout, themes, error handling, micro-interactions), and Automated Verification Commands.",
                                    "4. Non-Overlapping Scopes: Assign distinct suggested_scope globs (e.g. 'src/components/Navigation/**', 'src-tauri/src/db/**') so parallel agents never collide.",
                                    format!("5. Final Step Release Deliverable: The final step (Step {}) MUST create a robust, production-grade launcher script (`run.bat` for Windows / `start.sh` for Unix) that checks dependencies, starts the dev/prod server, auto-opens the browser, verifies launch, and writes a comprehensive `USER_GUIDE.md` / `HOW_TO_USE.md`.", target_steps),
                                    format!("6. Phased Decomposition: You can submit in batches using `masterplan_decompose(project_id='...', steps=[...], append=true)` or all {} steps at once.", target_steps)
                                ],
                                "step_schema_example": {
                                    "step_index": 1,
                                    "title": "Module Name: Feature Implementation & State Engine",
                                    "description": "Comprehensive specification including:\n- Target Files: [exact file paths]\n- Exports & Types: [interface/function signatures]\n- App Integration: [exact import & mounting in App.tsx / routes / navigation]\n- Design Specs: [UI layout, theme tokens, error boundaries, micro-interactions]\n- Core Logic: [state hooks, data transformations, edge cases]",
                                    "suggested_scope": "src/components/feature/**",
                                    "acceptance_criteria": "Code compiles cleanly, exports match interfaces, and tests pass via: npm run build / cargo test"
                                }
                            }))
                        )
                    } else {
                        (
                            "masterplan_claim_chunk",
                            "The masterplan is ORGANIZED. Claim chunks using masterplan_claim_chunk.".to_string(),
                            None
                        )
                    };
                    Ok(serde_json::json!({
                        "project_name": proj.name,
                        "project_id": proj.id,
                        "repository_path": proj.path,
                        "masterplan_id": plan.id,
                        "status": plan.status,
                        "next_action": next_action,
                        "plan": plan,
                        "steps": steps,
                        "instruction": instruction,
                        "architectural_guidelines": architectural_guidelines
                    }))
                }
                Ok(None) => Err(format!("No active masterplan is currently published for project '{}'. In Masterplan Hub, toggle ON a masterplan to make it visible and actionable for AI agents.", project_id)),
                Err(e) => Err(e),
            }
        }

        "masterplan_status" | "masterplan.status" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'. Query 'project_list' or 'agentxflow_current_context' to obtain valid project IDs.".to_string());
            }
            match state.coordinator.get_masterplan(project_id) {
                Ok(Some(plan)) => {
                    let steps = state.coordinator.list_masterplan_steps(project_id).unwrap_or_default();
                    let total = steps.len();
                    let pending = steps.iter().filter(|s| s.status == "PENDING").count();
                    let claimed = steps.iter().filter(|s| s.status == "CLAIMED").count();
                    let completed = steps.iter().filter(|s| s.status == "COMPLETED").count();
                    Ok(serde_json::json!({
                        "masterplan_id": plan.id,
                        "status": plan.status,
                        "require_milestone_approval": plan.require_milestone_approval,
                        "stats": {
                            "total_steps": total,
                            "pending_steps": pending,
                            "claimed_steps": claimed,
                            "completed_steps": completed,
                        },
                        "next_action": if total == 0 {
                            "prepare_masterplan"
                        } else if plan.status == "UNSORTED" {
                            "masterplan_decompose"
                        } else if pending > 0 {
                            "masterplan_claim_chunk"
                        } else if claimed > 0 {
                            "AWAIT_CHUNK_COMPLETION"
                        } else {
                            "FINAL_RELEASE_DELIVERY"
                        }
                    }))
                }
                Ok(None) => Err(format!("No active masterplan found for project '{}'", project_id)),
                Err(e) => Err(e),
            }
        }

        "masterplan_reset" | "masterplan.reset" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let masterplan_id = params.get("masterplan_id").and_then(|v| v.as_str());
            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'".to_string());
            }
            state.coordinator.reset_masterplan(project_id, masterplan_id).map(|_| serde_json::json!({
                "status": "RESET",
                "project_id": project_id,
                "masterplan_id": masterplan_id,
                "message": "Masterplan reset successfully. Active tasks cancelled, steps cleared, worktrees wiped, and Git repository reset to HEAD."
            }))
        }

        "masterplan_claim_chunk" | "masterplan.claim_chunk" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let count = params.get("count").and_then(|v| v.as_i64()).map(|n| n as i32);

            if project_id.trim().is_empty() {
                return Err("Missing required parameter 'project_id'".to_string());
            }
            let agent_id = resolve_agent_id(raw_agent_id)?;

            state.coordinator.claim_masterplan_chunk(project_id, &agent_id, count).map(|chunk| {
                let steps = state.coordinator.list_masterplan_steps(project_id).unwrap_or_default();
                let remaining_pending = steps.iter().filter(|s| s.status == "PENDING").count();
                let completed_in_plan = steps.iter().filter(|s| s.status == "COMPLETED").count();
                let total_steps = steps.len();

                serde_json::json!({
                    "id": chunk.id,
                    "task_id": chunk.id,
                    "task_title": chunk.title,
                    "title": chunk.title,
                    "state": chunk.state.as_str(),
                    "worktree_path": chunk.worktree_path,
                    "assigned_agent": agent_id,
                    "branch_name": chunk.branch_name,
                    "masterplan_progress": {
                        "completed_steps": completed_in_plan,
                        "remaining_pending_steps": remaining_pending,
                        "total_steps": total_steps
                    },
                    "base_sha": chunk.base_sha,
                    "message": "Chunk claimed successfully with exclusive write scope leases."
                })
            })
        }

        "masterplan_decompose" | "masterplan.decompose" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let compact = params.get("compact").and_then(|v| v.as_bool()).unwrap_or(true);
            let append = params.get("append").and_then(|v| v.as_bool());
            let steps_val = params.get("steps").cloned().unwrap_or(serde_json::json!([]));
            let steps_res: Result<Vec<crate::models::DecomposedStepInput>, _> = serde_json::from_value(steps_val);
            match steps_res {
                Ok(steps) => {
                    let step_count = steps.len();
                    match state.coordinator.decompose_masterplan(project_id, steps, append) {
                        Ok(decomposed) => {
                            let plan = state.coordinator.get_masterplan(project_id).ok().flatten();
                            let plan_id = plan.as_ref().map(|p| p.id.as_str()).unwrap_or("");
                            let total_steps = decomposed.len();
                            if compact {
                                Ok(serde_json::json!({
                                    "status": "RESORTED",
                                    "masterplan_id": plan_id,
                                    "project_id": project_id,
                                    "step_count": step_count,
                                    "batch_step_count": step_count,
                                    "total_steps": total_steps,
                                    "pending_steps": total_steps,
                                    "is_append": append.unwrap_or(false),
                                    "next_action": "masterplan_claim_chunk",
                                    "instruction": format!("Masterplan steps successfully structured (total {} steps in plan). Call 'masterplan_claim_chunk' to claim your assigned chunk.", total_steps)
                                }))
                            } else {
                                Ok(serde_json::to_value(decomposed).unwrap())
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(format!("Invalid step array format: {}. Expected [{{ 'step_index': 1, 'title': '...', 'description': '...' }}]", e)),
            }
        }

        "unclaim_agent_tasks" | "agent.unclaim_tasks" => {
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.unclaim_agent_tasks(&agent_id).map(|tasks| serde_json::json!({
                "status": "UNCLAIMED",
                "agent_id": agent_id,
                "reclaimed_tasks": tasks
            }))
        }

        "force_agent_idle" | "agent.force_idle" => {
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.force_agent_idle(&agent_id).map(|_| serde_json::json!({
                "status": "IDLE",
                "agent_id": agent_id
            }))
        }

        _ => Err(format!("Unknown MCP tool: '{}'. Call tools/list to see available methods.", tool_name)),
    }
}

fn get_tool_definitions() -> Vec<serde_json::Value> {
    registry::get_all_tool_definitions()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbPool;

    fn setup_test_mcp_state() -> (Arc<McpServerState>, String, String) {
        let temp_dir = std::env::temp_dir().join(format!("agentxflow_mcp_unit_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let readme = temp_dir.join("README.md");
        std::fs::write(&readme, "# Test MCP Project\n").unwrap();

        let run_cmd = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&temp_dir)
                .output()
                .expect("Failed to run git command");
            if !out.status.success() {
                eprintln!("Git cmd {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
            }
        };

        run_cmd(&["init"]);
        run_cmd(&["config", "user.name", "AgentXFlow Unit Test"]);
        run_cmd(&["config", "user.email", "test@agentxflow.local"]);
        run_cmd(&["add", "README.md"]);
        run_cmd(&["commit", "-m", "Initial commit"]);
        run_cmd(&["branch", "-M", "main"]);

        let temp_db = temp_dir.join("test.db");
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite pool");
        let engine = CoordinatorEngine::new(pool);
        let proj = engine.create_project("Test MCP Project", &temp_dir.to_string_lossy(), "Spec", "main").unwrap();
        let task = engine.create_task(&proj.id, "Test Task", "Task desc", "HIGH", vec![], vec![]).unwrap();

        let security = SecurityManager::init_or_load(&temp_dir).unwrap();
        let state = Arc::new(McpServerState {
            coordinator: engine,
            security,
        });

        (state, proj.id, task.id)
    }

    #[test]
    fn test_mcp_project_context_without_task_id() {
        let (state, proj_id, _) = setup_test_mcp_state();
        let params = serde_json::json!({
            "project_id": proj_id
        });
        let res = execute_mcp_tool(&state, None, "project_context", &params);
        assert!(res.is_ok(), "project_context without task_id should succeed: {:?}", res);
        let val = res.unwrap();
        assert_eq!(val["project_id"], proj_id);
        assert_eq!(val["project_name"], "Test MCP Project");
        assert!(val["contract_hash"].is_string());
        assert!(val["project_rules"].is_array());
        assert!(!val["project_rules"].as_array().unwrap().is_empty());
        assert!(val.get("task_id").is_none());
    }

    #[test]
    fn test_mcp_project_context_with_task_id() {
        let (state, proj_id, task_id) = setup_test_mcp_state();
        let params = serde_json::json!({
            "project_id": proj_id,
            "task_id": task_id
        });
        let res = execute_mcp_tool(&state, None, "project_context", &params);
        assert!(res.is_ok(), "project_context with task_id should succeed: {:?}", res);
        let val = res.unwrap();
        assert_eq!(val["project_id"], proj_id);
        assert_eq!(val["task_id"], task_id);
        assert_eq!(val["task_title"], "Test Task");
        assert_eq!(val["task_prompt"], "Task desc");
    }
}


