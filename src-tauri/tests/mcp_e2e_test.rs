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
            eprintln!("Git cmd {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
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
    server.start().await.expect("Failed to start test MCP server");

    sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // 3. Health Check (/health)
    let health_res = client.get(format!("{}/health", base_url)).send().await.expect("Health check failed");
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    println!("1. Health Check Response: {:?}", health_json);
    assert_eq!(health_json["status"], "ok");
    assert_eq!(health_json["protocol_version"], "2024-11-05");

    // 4. Legacy SSE Ping (/mcp/sse)
    let sse_res = client.get(format!("{}/mcp/sse", base_url)).send().await.expect("SSE check failed");
    assert_eq!(sse_res.status(), reqwest::StatusCode::OK);
    let sse_text = sse_res.text().await.unwrap();
    println!("2. SSE Response: {:?}", sse_text);
    assert!(sse_text.contains("data: /mcp"));

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
            assert!(!is_error, "RPC returned error: {:?}", json_body.get("error"));
            json_body["result"].clone()
        }
    };

    // 5. Standard MCP 'initialize' with version negotiation
    let init_res_std = send_rpc(&auth_token, "initialize", json!({})).await;
    println!("3. MCP initialize (default) result: {:?}", init_res_std);
    assert_eq!(init_res_std["protocolVersion"], "2024-11-05");
    assert_eq!(init_res_std["serverInfo"]["name"], "AgentXFlow Coordinator");

    let init_res_v2 = send_rpc(&auth_token, "initialize", json!({ "protocolVersion": "2026-07-28" })).await;
    println!("   MCP initialize (negotiated 2026-07-28) result: {:?}", init_res_v2);
    assert_eq!(init_res_v2["protocolVersion"], "2026-07-28");

    // Standard lifecycle notifications and probing
    let _ = send_rpc(&auth_token, "notifications/initialized", json!({})).await;
    let _ = send_rpc(&auth_token, "ping", json!({})).await;
    let prompts_res = send_rpc(&auth_token, "prompts/list", json!({})).await;
    assert!(prompts_res.get("prompts").is_some());
    let resources_res = send_rpc(&auth_token, "resources/list", json!({})).await;
    assert!(resources_res.get("resources").is_some());

    // 6. Standard MCP 'tools/list'
    let list_res = send_rpc(&auth_token, "tools/list", json!({})).await;
    let tools = list_res["tools"].as_array().expect("Tools must be an array");
    println!("4. Discovered {} MCP Tools", tools.len());
    assert!(tools.len() >= 12);
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"agent_register"));
    assert!(tool_names.contains(&"task_claim"));
    assert!(tool_names.contains(&"scope_acquire"));
    assert!(tool_names.contains(&"masterplan_decompose"));

    // 7. Standard MCP 'tools/call' -> agent_register
    let reg_call = send_rpc(&auth_token, "tools/call", json!({
        "name": "agent_register",
        "arguments": {
            "name": "Antigravity Test Agent",
            "agent_type": "Antigravity"
        }
    })).await;
    println!("5. MCP tools/call agent_register: {:?}", reg_call);
    assert_eq!(reg_call["isError"], false);

    // 8. Register agent directly to obtain secure session token
    let reg_res = send_rpc(&auth_token, "agent.register", json!({
        "name": "E2E Automation Agent",
        "agent_type": "Antigravity"
    })).await;
    let agent_id = reg_res["id"].as_str().unwrap().to_string();
    let session_token = reg_res["session_token"].as_str().unwrap().to_string();

    // 9. Heartbeat with Agent Session
    let hb_res = send_rpc(&session_token, "agent.heartbeat", json!({ "agent_id": agent_id })).await;
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
    let proj_ctx_res = send_rpc(&auth_token, "project_context", json!({ "project_id": proj.id })).await;
    assert_eq!(proj_ctx_res["project_id"], proj.id);
    assert_eq!(proj_ctx_res["project_name"], proj.name);
    assert!(proj_ctx_res["contract_hash"].is_string());
    assert!(proj_ctx_res["project_rules"].is_array());
    assert!(!proj_ctx_res["project_rules"].as_array().unwrap().is_empty());
    assert!(proj_ctx_res.get("task_id").is_none());

    // 11. Masterplan Workflow: create raw plan, get it, decompose it, and claim chunk
    coordinator.create_or_update_masterplan(
        &proj.id,
        "Phase 1: Setup authentication.\nPhase 2: Add test suite.",
        2,
        4,
    ).unwrap();

    let plan_get = send_rpc(&auth_token, "masterplan.get", json!({ "project_id": proj.id })).await;
    assert_eq!(plan_get["project_name"], proj.name);
    assert_eq!(plan_get["project_id"], proj.id);
    assert_eq!(plan_get["status"], "UNSORTED");
    assert_eq!(plan_get["next_action"], "masterplan_decompose");
    assert_eq!(plan_get["plan"]["status"], "UNSORTED");

    let mp_list_res = send_rpc(&auth_token, "masterplan_list", json!({})).await;
    assert_eq!(mp_list_res.as_array().unwrap().len(), 1);

    let dec_res = send_rpc(&auth_token, "masterplan.decompose", json!({
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
    })).await;
    assert_eq!(dec_res["status"], "RESORTED");
    assert_eq!(dec_res["step_count"], 2);

    let claim_res = send_rpc(&session_token, "masterplan.claim_chunk", json!({
        "project_id": proj.id,
        "agent_id": agent_id,
        "count": 2
    })).await;
    let task_id = claim_res["id"].as_str().unwrap().to_string();
    assert_eq!(claim_res["state"].as_str().unwrap().to_uppercase(), "RUNNING");

    // 12. Test task_list requires project_id
    let tasks_res = send_rpc(&session_token, "task_list", json!({ "project_id": proj.id })).await;
    assert_eq!(tasks_res.as_array().unwrap().len(), 1);

    // Test project_context WITH task_id (task-specific context pack)
    let task_ctx_res = send_rpc(&session_token, "project_context", json!({ "project_id": proj.id, "task_id": task_id })).await;
    assert_eq!(task_ctx_res["project_id"], proj.id);
    assert_eq!(task_ctx_res["task_id"], task_id);
    assert!(task_ctx_res["required_steps"].is_array());
    assert_eq!(task_ctx_res["required_steps"].as_array().unwrap().len(), 2);

    // 13. Lock Scopes
    let scope_res = send_rpc(&session_token, "scope.acquire", json!({
        "task_id": task_id,
        "agent_id": agent_id,
        "patterns": ["src/auth/**", "tests/**"]
    })).await;
    assert_eq!(scope_res.as_array().unwrap().len(), 2);

    // 14. Complete Task Step
    let steps_list = coordinator.get_task_details(&task_id).unwrap().steps;
    let step_id = &steps_list[0].id;
    let step_res = send_rpc(&session_token, "task.complete_step", json!({
        "step_id": step_id,
        "evidence": "cargo test passed with exit code 0"
    })).await;
    assert_eq!(step_res["status"], "COMPLETED");

    // Cleanup temp dir
    std::fs::remove_dir_all(&temp_repo).ok();
}
