#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::mcp::McpServer;
use agent_x_flow_lib::security::SecurityManager;
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

fn setup_temp_git_repo(prefix: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# AgentXFlow E2E Repo\n").unwrap();

    let run_cmd = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&temp_dir)
            .output()
            .expect("Failed to run git command");
        if !out.status.success() {
            eprintln!(
                "Git cmd {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    };

    run_cmd(&["init"]);
    run_cmd(&["config", "user.name", "E2E Test Agent"]);
    run_cmd(&["config", "user.email", "e2e@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

#[tokio::test]
async fn test_full_e2e_mcp_workflow() {
    let temp_repo = setup_temp_git_repo("mcp_e2e");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);

    let proj = coordinator
        .create_project(
            "MCP Integration Suite",
            &temp_repo.to_string_lossy(),
            "End to end validation of streamable HTTP protocol",
            "main",
        )
        .expect("Failed to create project");

    // 1. Create real SecurityManager with live token
    let auth_token = "axf_sec_live_e2e_test_token_8899".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7895;

    // 2. Start MCP server in background
    let server = McpServer::new(coordinator.clone(), test_port, security.clone());
    server
        .start()
        .await
        .expect("Failed to start test MCP server");

    sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // 3. Health Check (/health)
    let health_res = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .expect("Health check failed");
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    println!("1. Health Check Response: {:?}", health_json);
    assert_eq!(health_json["status"], "ok");
    assert_eq!(health_json["protocol_version"], "2024-11-05");
    assert!(
        health_json.get("sse_url").is_none(),
        "Health response must not contain 'sse_url' after SSE stub removal"
    );

    // 4. Legacy SSE endpoint removed — GET /mcp/sse -> 404
    let sse_res = client
        .get(format!("{}/mcp/sse", base_url))
        .send()
        .await
        .expect("SSE check failed");
    assert_eq!(sse_res.status(), reqwest::StatusCode::NOT_FOUND);
    println!("2. GET /mcp/sse -> 404 (SSE stub removed)");

    // Helper for sending authenticated JSON-RPC 2.0 requests
    let send_rpc = |token: &str, method: &str, params: serde_json::Value| {
        let client = client.clone();
        let base_url = base_url.clone();
        let token = token.to_string();
        let method = method.to_string();
        async move {
            let res = client
                .post(format!("{}/mcp", base_url))
                .header("Authorization", format!("Bearer {}", token))
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": method,
                    "params": params,
                }))
                .send()
                .await
                .expect("Failed to send MCP RPC request");

            assert_eq!(res.status(), reqwest::StatusCode::OK);
            let json_body: serde_json::Value = res.json().await.unwrap();
            let is_error = json_body.get("error").is_some() && !json_body["error"].is_null();
            assert!(
                !is_error,
                "RPC returned error: {:?}",
                json_body.get("error")
            );
            json_body["result"].clone()
        }
    };

    // 5. Standard MCP 'initialize' with version negotiation
    let init_res_std = send_rpc(&auth_token, "initialize", json!({})).await;
    println!("3. MCP initialize (default) result: {:?}", init_res_std);
    assert_eq!(init_res_std["protocolVersion"], "2024-11-05");
    assert_eq!(init_res_std["serverInfo"]["name"], "AgentXFlow Coordinator");

    let init_res_v2 = send_rpc(
        &auth_token,
        "initialize",
        json!({ "protocolVersion": "2026-07-28" }),
    )
    .await;
    println!(
        "   MCP initialize (negotiated 2026-07-28) result: {:?}",
        init_res_v2
    );
    assert_eq!(init_res_v2["protocolVersion"], "2024-11-05");

    // Standard lifecycle notifications and probing
    let _ = send_rpc(&auth_token, "notifications/initialized", json!({})).await;
    let _ = send_rpc(&auth_token, "ping", json!({})).await;
    let prompts_res = send_rpc(&auth_token, "prompts/list", json!({})).await;
    assert!(prompts_res.get("prompts").is_some());
    let resources_res = send_rpc(&auth_token, "resources/list", json!({})).await;
    assert!(resources_res.get("resources").is_some());

    // 6. Standard MCP 'tools/list'
    let list_res = send_rpc(&auth_token, "tools/list", json!({})).await;
    let tools = list_res["tools"]
        .as_array()
        .expect("Tools must be an array");
    println!("4. Discovered {} MCP Tools", tools.len());
    assert!(tools.len() >= 12);
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"agent_register"));
    assert!(tool_names.contains(&"task_claim"));
    assert!(tool_names.contains(&"scope_acquire"));
    assert!(tool_names.contains(&"masterplan_decompose"));

    // 7. Standard MCP 'tools/call' -> agent_register
    let reg_call = send_rpc(
        &auth_token,
        "tools/call",
        json!({
            "name": "agent_register",
            "arguments": {
                "name": "Antigravity Test Agent",
                "agent_type": "Antigravity"
            }
        }),
    )
    .await;
    println!("5. MCP tools/call agent_register: {:?}", reg_call);
    assert_eq!(reg_call["isError"], false);

    // 8. Register agent directly to obtain secure session token
    let reg_res = send_rpc(
        &auth_token,
        "agent.register",
        json!({
            "name": "E2E Automation Agent",
            "agent_type": "Antigravity"
        }),
    )
    .await;
    let agent_id = reg_res["id"].as_str().unwrap().to_string();
    let session_token = reg_res["session_token"].as_str().unwrap().to_string();

    // 9. Heartbeat with Agent Session
    let hb_res = send_rpc(
        &session_token,
        "agent.heartbeat",
        json!({ "agent_id": agent_id }),
    )
    .await;
    assert_eq!(hb_res["status"], "ok");

    // 10. Test Discovery Tools: agentxflow_current_context, project_list, masterplan_list
    let ctx_res = send_rpc(&auth_token, "agentxflow_current_context", json!({})).await;
    assert_eq!(ctx_res["active_project_id"], proj.id);
    assert_eq!(ctx_res["project_name"], proj.name);

    let proj_list_res = send_rpc(&auth_token, "project_list", json!({})).await;
    let proj_arr = proj_list_res.as_array().unwrap();
    assert_eq!(proj_arr.len(), 1);
    assert_eq!(proj_arr[0]["id"], proj.id);

    // Test project_context without task_id (fresh agent flow)
    let proj_ctx_res = send_rpc(
        &auth_token,
        "project_context",
        json!({ "project_id": proj.id }),
    )
    .await;
    assert_eq!(proj_ctx_res["project_id"], proj.id);
    assert_eq!(proj_ctx_res["project_name"], proj.name);
    assert!(proj_ctx_res["contract_hash"].is_string());
    assert!(proj_ctx_res["project_rules"].is_array());
    assert!(!proj_ctx_res["project_rules"].as_array().unwrap().is_empty());
    assert!(proj_ctx_res.get("task_id").is_none());

    // 11. Masterplan Workflow: create raw plan, get it, decompose it, and claim chunk
    coordinator
        .create_or_update_masterplan(
            &proj.id,
            "Phase 1: Setup authentication.\nPhase 2: Add test suite.",
            2,
            4,
        )
        .unwrap();

    let plan_get = send_rpc(
        &auth_token,
        "masterplan.get",
        json!({ "project_id": proj.id }),
    )
    .await;
    assert_eq!(plan_get["project_name"], proj.name);
    assert_eq!(plan_get["project_id"], proj.id);
    assert_eq!(plan_get["status"], "UNSORTED");
    assert_eq!(plan_get["next_action"], "masterplan_decompose");
    assert_eq!(plan_get["plan"]["status"], "UNSORTED");

    let mp_list_res = send_rpc(&auth_token, "masterplan_list", json!({})).await;
    assert_eq!(mp_list_res.as_array().unwrap().len(), 1);

    let dec_res = send_rpc(
        &auth_token,
        "masterplan.decompose",
        json!({
            "project_id": proj.id,
            "steps": [
                {
                    "step_index": 1,
                    "title": "Build Auth",
                    "description": "Create JWT tokens in src/auth",
                    "suggested_scope": "src/auth/**",
                    "acceptance_criteria": "JWT verification passes"
                },
                {
                    "step_index": 2,
                    "title": "Build Tests",
                    "description": "Add unit tests in tests/",
                    "suggested_scope": "tests/**",
                    "acceptance_criteria": "All unit tests pass"
                }
            ]
        }),
    )
    .await;
    assert_eq!(dec_res["status"], "RESORTED");
    assert_eq!(dec_res["step_count"], 2);

    let claim_res = send_rpc(
        &session_token,
        "masterplan.claim_chunk",
        json!({
            "project_id": proj.id,
            "agent_id": agent_id,
            "count": 2
        }),
    )
    .await;
    let task_id = claim_res["id"].as_str().unwrap().to_string();
    assert_eq!(
        claim_res["state"].as_str().unwrap().to_uppercase(),
        "RUNNING"
    );

    // 12. Test task_list requires project_id
    let tasks_res = send_rpc(
        &session_token,
        "task_list",
        json!({ "project_id": proj.id }),
    )
    .await;
    assert_eq!(tasks_res.as_array().unwrap().len(), 1);

    // Test project_context WITH task_id (task-specific context pack)
    let task_ctx_res = send_rpc(
        &session_token,
        "project_context",
        json!({ "project_id": proj.id, "task_id": task_id }),
    )
    .await;
    assert_eq!(task_ctx_res["project_id"], proj.id);
    assert_eq!(task_ctx_res["task_id"], task_id);
    assert!(task_ctx_res["required_steps"].is_array());
    assert_eq!(task_ctx_res["required_steps"].as_array().unwrap().len(), 2);

    // 13. Lock Scopes
    let scope_res = send_rpc(
        &session_token,
        "scope.acquire",
        json!({
            "task_id": task_id,
            "agent_id": agent_id,
            "patterns": ["src/auth/**", "tests/**"]
        }),
    )
    .await;
    assert_eq!(scope_res.as_array().unwrap().len(), 2);

    // 14. Complete Task Step
    let steps_list = coordinator.get_task_details(&task_id).unwrap().steps;
    let step_id = &steps_list[0].id;
    let step_res = send_rpc(
        &session_token,
        "task.complete_step",
        json!({
            "step_id": step_id,
            "evidence": "cargo test passed with exit code 0"
        }),
    )
    .await;
    assert_eq!(step_res["status"], "COMPLETED");

    // 15. Test masterplan_reset via MCP (requires Master authority)
    let reset_res = send_rpc(
        &auth_token,
        "masterplan_reset",
        json!({
            "project_id": proj.id
        }),
    )
    .await;
    assert_eq!(reset_res["status"], "RESET");
    let post_reset_steps = coordinator
        .list_masterplan_steps(&proj.id)
        .unwrap_or_default();
    assert_eq!(post_reset_steps.len(), 0);

    // Cleanup temp dir
    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_sse_stub_removed() {
    let temp_repo = setup_temp_git_repo("sse_stub_removed");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_sse_stub_removed_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7894;

    let server = McpServer::new(coordinator, test_port, security.clone());
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // GET /mcp/sse -> 404 (stub removed)
    let sse_res = client
        .get(format!("{}/mcp/sse", base_url))
        .send()
        .await
        .expect("SSE request failed");
    assert_eq!(sse_res.status(), reqwest::StatusCode::NOT_FOUND);

    // GET /health -> payload has NO "sse_url" key
    let health_res = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .expect("Health check failed");
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    assert!(
        health_json.get("sse_url").is_none(),
        "Health response must not contain 'sse_url' after SSE stub removal"
    );

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_health_advertises_only_implemented_versions() {
    let temp_repo = setup_temp_git_repo("health_version");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_health_version_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7896;

    let server = McpServer::new(coordinator, test_port, security.clone());
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let health_res = client
        .get(format!("http://127.0.0.1:{}/health", test_port))
        .send()
        .await
        .expect("Health check failed");
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    assert_eq!(health_json["protocol_version"], "2024-11-05");
    assert_eq!(health_json["supported_versions"], json!(["2024-11-05"]));

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_initialize_with_unsupported_version_negotiates_2024() {
    let temp_repo = setup_temp_git_repo("negotiate_version");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_negotiate_version_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7897;

    let server = McpServer::new(coordinator, test_port, security.clone());
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let init_res = client
        .post(format!("http://127.0.0.1:{}/mcp", test_port))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2026-07-28" },
        }))
        .send()
        .await
        .expect("Failed to send initialize request");
    assert_eq!(init_res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = init_res.json().await.unwrap();
    assert_eq!(body["result"]["protocolVersion"], "2024-11-05");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_reject_malformed_jsonrpc() {
    let temp_repo = setup_temp_git_repo("reject_malformed");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_malformed_jsonrpc_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7898;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // Send valid JSON but not JSON-RPC structure
    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32700);
    assert_eq!(body["error"]["message"], "Parse error");
    println!("   ✔ test_reject_malformed_jsonrpc PASS: -32700 Parse error");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_reject_missing_version() {
    let temp_repo = setup_temp_git_repo("reject_missing_version");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_missing_version_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7899;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // Send JSON-RPC without "jsonrpc" field
    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "method": "ping",
            "id": 1
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32600);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required field 'jsonrpc'"));
    println!("   ✔ test_reject_missing_version PASS: -32600 Invalid Request");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_reject_unknown_method() {
    let temp_repo = setup_temp_git_repo("reject_unknown_method");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_unknown_method_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7900;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // Send unknown method
    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "no/such/method",
            "id": 1
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32601);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Method not found"));
    println!("   ✔ test_reject_unknown_method PASS: -32601 Method not found");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_reject_wrong_jsonrpc_version() {
    let temp_repo = setup_temp_git_repo("reject_wrong_version");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_wrong_version_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7901;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "1.0",
            "id": 1,
            "method": "ping"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32600);
    println!("   ✔ test_reject_wrong_jsonrpc_version PASS: -32600 Invalid Request");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_notification_returns_empty_body() {
    let temp_repo = setup_temp_git_repo("notification_empty");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_notification_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7902;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.get("result").is_none(),
        "Notification response should not contain 'result' field"
    );
    assert!(
        body.get("error").is_none(),
        "Notification response should not contain 'error' field"
    );
    println!("   ✔ test_notification_returns_empty_body PASS: no result/error in response");

    std::fs::remove_dir_all(&temp_repo).ok();
}

#[tokio::test]
async fn test_reject_missing_required_params() {
    let temp_repo = setup_temp_git_repo("reject_missing_params");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);
    let auth_token = "axf_sec_missing_params_token".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7903;

    let server = McpServer::new(coordinator, test_port, security);
    server
        .start()
        .await
        .expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    let res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "scope_acquire",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32602);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required parameter"));

    // 2. Direct legacy route scope.acquire with missing parameters -> -32602
    let res_direct_scope = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "scope.acquire",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res_direct_scope.status(), reqwest::StatusCode::OK);
    let body_direct_scope: serde_json::Value = res_direct_scope.json().await.unwrap();
    assert_eq!(body_direct_scope["error"]["code"], -32602);
    assert!(body_direct_scope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required parameter"));

    // 3. Direct legacy route agent.register with missing name -> -32602
    let res_direct_agent = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "agent.register",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res_direct_agent.status(), reqwest::StatusCode::OK);
    let body_direct_agent: serde_json::Value = res_direct_agent.json().await.unwrap();
    assert_eq!(body_direct_agent["error"]["code"], -32602);
    assert!(body_direct_agent["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required parameter: 'name'"));

    // 4. tools/call with agent_register missing name -> -32602
    let res_tools_agent = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "agent_register",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res_tools_agent.status(), reqwest::StatusCode::OK);
    let body_tools_agent: serde_json::Value = res_tools_agent.json().await.unwrap();
    assert_eq!(body_tools_agent["error"]["code"], -32602);
    assert!(body_tools_agent["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required parameter: 'name'"));

    // 5. Direct legacy task.get missing task_id -> -32602
    let res_direct_task_get = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "task.get",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res_direct_task_get.status(), reqwest::StatusCode::OK);
    let body_direct_task_get: serde_json::Value = res_direct_task_get.json().await.unwrap();
    assert_eq!(body_direct_task_get["error"]["code"], -32602);
    assert!(body_direct_task_get["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Missing required parameter: 'task_id'"));

    println!("   ✔ test_reject_missing_required_params PASS: exact parity across tools/call and legacy routes (-32602)");

    std::fs::remove_dir_all(&temp_repo).ok();
}
