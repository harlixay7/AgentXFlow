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
            if state.coordinator.is_agent_registered(req_id) {
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
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            if project_id.trim().is_empty() {
                let ctx = state.coordinator.get_current_context(None, None)?;
                if let Some(pid) = ctx.active_project_id {
                    state.coordinator.get_context_pack(&pid, task_id).map(|cp| serde_json::to_value(cp).unwrap())
                } else {
                    Err("Missing required parameter 'project_id'. Query 'project_list' to obtain valid project IDs.".to_string())
                }
            } else {
                state.coordinator.get_context_pack(project_id, task_id).map(|cp| serde_json::to_value(cp).unwrap())
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
            state.coordinator.list_tasks(project_id).map(|tasks| serde_json::to_value(tasks).unwrap())
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
            let agent_id_opt = resolve_agent_id(raw_agent_id).ok();
            let evidence_json = params.get("evidence").map(|v| v.to_string());
            state.coordinator.complete_step(step_id, agent_id_opt.as_deref(), evidence_json.as_deref()).map(|step| serde_json::to_value(step).unwrap())
        }

        "task_submit" | "task.submit" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            state.coordinator.submit_task(task_id, &agent_id).map(|res| serde_json::to_value(res).unwrap())
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
            if let Ok(agent_id) = resolve_agent_id(raw_agent_id) {
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
            let max_steps_per_agent = params.get("max_steps_per_agent").and_then(|v| v.as_i64()).unwrap_or(4) as i32;
            state.coordinator.prepare_masterplan(project_id, raw_text, target_step_count, max_steps_per_agent)
                .map(|snap| serde_json::to_value(snap).unwrap())
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
                    let (next_action, instruction) = if plan.status == "UNSORTED" {
                        (
                            "masterplan_decompose",
                            "The masterplan is UNSORTED. Read raw_text and call masterplan_decompose with the normalized steps array."
                        )
                    } else {
                        (
                            "masterplan_claim_chunk",
                            "The masterplan is ORGANIZED. Claim chunks using masterplan_claim_chunk."
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
                    }))
                }
                Ok(None) => Err(format!("No masterplan found for project '{}'", project_id)),
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
                    let pending = steps.iter().filter(|s| s.status == "PENDING").count();
                    let claimed = steps.iter().filter(|s| s.status == "CLAIMED" || s.status == "IN_PROGRESS").count();
                    let completed = steps.iter().filter(|s| s.status == "COMPLETED").count();
                    Ok(serde_json::json!({
                        "status": plan.status,
                        "total_steps": steps.len(),
                        "pending_steps": pending,
                        "claimed_steps": claimed,
                        "completed_steps": completed,
                        "max_steps_per_agent": plan.max_steps_per_agent,
                        "steps": steps,
                    }))
                }
                Ok(None) => Err(format!("No masterplan found for project '{}'", project_id)),
                Err(e) => Err(e),
            }
        }

        "masterplan_claim_chunk" | "masterplan.claim_chunk" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let raw_agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = resolve_agent_id(raw_agent_id)?;
            let count = params
                .get("chunk_size")
                .or_else(|| params.get("count"))
                .and_then(|v| v.as_i64())
                .map(|n| n as i32);
            state.coordinator.claim_masterplan_chunk(project_id, &agent_id, count).map(|chunk| serde_json::to_value(chunk).unwrap())
        }

        "masterplan_decompose" | "masterplan.decompose" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let steps_val = params.get("steps").cloned().unwrap_or(serde_json::json!([]));
            let steps_res: Result<Vec<crate::models::DecomposedStepInput>, _> = serde_json::from_value(steps_val);
            match steps_res {
                Ok(steps) => state.coordinator.decompose_masterplan(project_id, steps).map(|decomposed| serde_json::to_value(decomposed).unwrap()),
                Err(e) => Err(format!("Invalid step array format: {}. Expected [{{ 'step_index': 1, 'title': '...', 'description': '...' }}]", e)),
            }
        }

        _ => Err(format!("Unknown MCP tool: '{}'. Call tools/list to see available methods.", tool_name)),
    }
}

fn get_tool_definitions() -> Vec<serde_json::Value> {
    registry::get_all_tool_definitions()
}

