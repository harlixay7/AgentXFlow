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

use crate::core::CoordinatorEngine;

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
    pub auth_token: String,
}

pub struct McpServer {
    pub port: u16,
    pub auth_token: String,
    pub state: Arc<McpServerState>,
}

impl McpServer {
    pub fn new(coordinator: CoordinatorEngine, port: u16, auth_token: String) -> Self {
        let state = Arc::new(McpServerState {
            coordinator,
            auth_token: auth_token.clone(),
        });
        Self {
            port,
            auth_token,
            state,
        }
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
        info!("Binding Authoritative MCP 2026 Gateway to http://{}", addr);

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
            "service": "Viducia Authoritative MCP Gateway",
            "protocol_version": "2026-07-28",
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

/// Modern Streamable HTTP stateless request handler compliant with MCP 2026-07-28
async fn handle_mcp_streamable_http(
    State(state): State<Arc<McpServerState>>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    // Authenticate Bearer Token with clear instructions on error
    if let Some(auth) = headers.get("authorization") {
        if let Ok(token_str) = auth.to_str() {
            let clean_token = token_str.trim_start_matches("Bearer ").trim();
            if clean_token != state.auth_token {
                return (
                    StatusCode::UNAUTHORIZED,
                    HeaderMap::new(),
                    Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32001,
                            message: "Unauthorized: Invalid Bearer token. Please provide 'Authorization: Bearer axf_sec_v2_live_token_7890' in your HTTP request headers.".to_string(),
                            data: None,
                        }),
                    }),
                );
            }
        }
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert("MCP-Protocol-Version", HeaderValue::from_static("2026-07-28"));

    let params = req.params.clone().unwrap_or(serde_json::json!({}));
    let method = req.method.as_str();

    // Helper to validate registered agent_id on state-modifying actions
    let validate_agent = |agent_id: &str| -> Result<(), String> {
        if agent_id.trim().is_empty() {
            return Err("Agent registration required: The 'agent_id' parameter is missing. Please call the 'agent.register' tool first (e.g. {\"name\": \"Your-Agent-Name\", \"agent_type\": \"Antigravity\"}) to obtain your unique agent_id.".to_string());
        }
        if !state.coordinator.is_agent_registered(agent_id) {
            return Err(format!("Agent registration required: Agent ID '{}' is not registered. Please call 'agent.register' first to register an agent session before claiming tasks or locking scopes.", agent_id));
        }
        Ok(())
    };

    let response_result = match method {
        // --- 1. Read-Only Discovery Tools (Open to any connected agent) ---
        "project.context" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.get_context_pack(project_id, task_id).map(|cp| serde_json::to_value(cp).unwrap())
        }

        "task.list" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.list_tasks(project_id).map(|tasks| serde_json::to_value(tasks).unwrap())
        }

        "task.get" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.get_task(task_id).map(|task| serde_json::to_value(task).unwrap())
        }

        "dag.dependencies" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.dag.get_dependencies_for_task(task_id).map(|deps| serde_json::to_value(deps).unwrap())
        }

        "merge.queue_status" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.merge.list_queue(project_id).map(|items| serde_json::to_value(items).unwrap())
        }

        // --- 2. Agent Identity Registration & Lifecycle ---
        "agent.register" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("Agent");
            let agent_type = params.get("agent_type").and_then(|v| v.as_str()).unwrap_or("Generic");
            state.coordinator.register_agent(name, agent_type).map(|agent| serde_json::to_value(agent).unwrap())
        }

        "agent.heartbeat" => {
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = validate_agent(agent_id) {
                Err(e)
            } else {
                state.coordinator.agent_heartbeat(agent_id).map(|_| serde_json::json!({ "status": "ok" }))
            }
        }

        // --- 3. Mutation & Task Execution Tools (Strictly Require Registered agent_id) ---
        "task.claim" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = validate_agent(agent_id) {
                Err(e)
            } else {
                state.coordinator.claim_task(task_id, agent_id).map(|task| serde_json::to_value(task).unwrap())
            }
        }

        "task.complete_step" => {
            let step_id = params.get("step_id").and_then(|v| v.as_str()).unwrap_or("");
            let evidence_json = params.get("evidence").map(|v| v.to_string());
            state.coordinator.complete_step(step_id, evidence_json.as_deref()).map(|step| serde_json::to_value(step).unwrap())
        }

        "task.submit" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = validate_agent(agent_id) {
                Err(e)
            } else {
                state.coordinator.submit_task(task_id, agent_id).map(|res| serde_json::to_value(res).unwrap())
            }
        }

        "scope.propose" | "scope.acquire" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = validate_agent(agent_id) {
                Err(e)
            } else {
                let patterns: Vec<String> = params
                    .get("patterns")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                state.coordinator.scope.acquire_scope(task_id, agent_id, patterns, "EXCLUSIVE_WRITE").map(|leases| serde_json::to_value(leases).unwrap())
            }
        }

        "scope.release" => {
            let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            state.coordinator.scope.release_scope(task_id).map(|_| serde_json::json!({ "status": "released" }))
        }

        // --- 4. Masterplan Decomposition & Chunked Execution Tools ---
        "masterplan.get" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            match state.coordinator.get_masterplan(project_id) {
                Ok(Some(plan)) => {
                    let steps = state.coordinator.list_masterplan_steps(project_id).unwrap_or_default();
                    let instruction = if plan.status == "UNSORTED" {
                        "The masterplan is UNSORTED. Your mandatory task is to read raw_text and decompose it into structured steps without omitting any requirements. Call masterplan.decompose with the normalized steps array.".to_string()
                    } else {
                        "The masterplan is ORGANIZED. You can claim the next batch of steps using masterplan.claim_chunk.".to_string()
                    };
                    Ok(serde_json::json!({
                        "plan": plan,
                        "steps": steps,
                        "instruction": instruction,
                    }))
                }
                Ok(None) => Err(format!("No masterplan found for project '{}'", project_id)),
                Err(e) => Err(e),
            }
        }

        "masterplan.status" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
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

        "masterplan.decompose" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let steps_val = params.get("steps").cloned().unwrap_or(serde_json::json!([]));
            let steps_res: Result<Vec<crate::models::DecomposedStepInput>, _> = serde_json::from_value(steps_val);
            match steps_res {
                Ok(steps) => state.coordinator.decompose_masterplan(project_id, steps).map(|decomposed| serde_json::to_value(decomposed).unwrap()),
                Err(e) => Err(format!("Invalid step array format for masterplan.decompose: {}. Expected [{{ 'step_index': 1, 'title': '...', 'description': '...' }}]", e)),
            }
        }

        "masterplan.claim_chunk" => {
            let project_id = params.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let agent_id = params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
            let count = params.get("count").and_then(|v| v.as_i64()).map(|n| n as i32);
            if let Err(e) = validate_agent(agent_id) {
                Err(e)
            } else {
                state.coordinator.claim_masterplan_chunk(project_id, agent_id, count).map(|task| serde_json::to_value(task).unwrap())
            }
        }

        _ => Err(format!("Unknown MCP tool method: '{}'. Call tools/list to see available methods.", method)),
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
