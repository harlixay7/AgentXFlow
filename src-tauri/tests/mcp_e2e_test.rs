use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::mcp::McpServer;
use serde_json::json;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

fn setup_temp_git_repo() -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("viducia_mcp_git_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# MCP E2E Test Repo\n").unwrap();

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
    run_cmd(&["config", "user.name", "Viducia MCP Test"]);
    run_cmd(&["config", "user.email", "test@viducia.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

#[tokio::test]
async fn test_full_e2e_mcp_workflow() {
    // 1. Setup temp Git repo and in-memory Coordinator Engine
    let temp_repo = setup_temp_git_repo();
    let pool = DbPool::new_in_memory().expect("Failed to initialize test SQLite pool");
    let coordinator = CoordinatorEngine::new(pool);

    coordinator.db.lock().execute(
        "UPDATE projects SET path = ?1 WHERE id = 'proj-agentxflow-v2'",
        [&temp_repo.to_string_lossy().to_string()],
    ).unwrap();

    let test_port = 7895;
    let auth_token = "test_bearer_token_7895".to_string();

    // 2. Start MCP Server in background
    let server = McpServer::new(coordinator.clone(), test_port, auth_token.clone());
    let bound_addr = server.start().await.expect("Failed to start test MCP server");
    println!(">>> Test MCP Server running on http://{}", bound_addr);

    sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // 3. Health Check (/health)
    let health_res = client.get(format!("{}/health", base_url)).send().await.expect("Health check failed");
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_res.json().await.unwrap();
    println!("1. Health Check Response: {:?}", health_json);
    assert_eq!(health_json["status"], "ok");
    assert_eq!(health_json["protocol_version"], "2026-07-28");

    // 4. Legacy SSE Ping (/mcp/sse)
    let sse_res = client.get(format!("{}/mcp/sse", base_url)).send().await.expect("SSE check failed");
    assert_eq!(sse_res.status(), reqwest::StatusCode::OK);
    let sse_text = sse_res.text().await.unwrap();
    println!("2. SSE Response: {:?}", sse_text);
    assert!(sse_text.contains("data: /mcp"));

    // Helper for sending authenticated JSON-RPC 2.0 requests
    let send_rpc = |method: &str, params: serde_json::Value| {
        let client = client.clone();
        let base_url = base_url.clone();
        let auth_token = auth_token.clone();
        let method = method.to_string();
        async move {
            let res = client
                .post(format!("{}/mcp", base_url))
                .header("Authorization", format!("Bearer {}", auth_token))
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

    // 5. Agent Registration (agent.register)
    let reg_result = send_rpc("agent.register", json!({ "name": "Codex-E2E-Agent", "agent_type": "Codex" })).await;
    println!("3. Agent Register Result: {:?}", reg_result);
    let agent_id = reg_result["id"].as_str().unwrap().to_string();
    assert_eq!(reg_result["name"], "Codex-E2E-Agent");

    // 6. Agent Heartbeat (agent.heartbeat)
    let heartbeat_result = send_rpc("agent.heartbeat", json!({ "agent_id": agent_id })).await;
    println!("4. Agent Heartbeat Result: {:?}", heartbeat_result);
    assert_eq!(heartbeat_result["status"], "ok");

    // 7. List Tasks (task.list)
    let tasks_result = send_rpc("task.list", json!({})).await;
    println!("5. Tasks List Result: Found {} tasks", tasks_result.as_array().unwrap().len());
    let task_id = tasks_result[0]["id"].as_str().unwrap().to_string();

    // 8. Fetch Project Context Pack (project.context)
    let ctx_result = send_rpc("project.context", json!({ "project_id": "proj-agentxflow-v2", "task_id": task_id })).await;
    println!("6. Context Pack Result: Contract Hash={:?}", ctx_result["contract_hash"]);
    assert!(!ctx_result["contract_hash"].as_str().unwrap().is_empty());
    assert!(!ctx_result["project_rules"].as_array().unwrap().is_empty());

    // 9. Acquire Exclusive Write Scope (scope.acquire)
    let scope_result = send_rpc("scope.acquire", json!({
        "task_id": task_id,
        "agent_id": agent_id,
        "patterns": ["src-tauri/src/models/**"]
    })).await;
    println!("7. Scope Acquire Result: {:?}", scope_result);
    assert_eq!(scope_result.as_array().unwrap().len(), 1);

    // 10. Complete All Mandatory Steps with Evidence (task.complete_step)
    let step1_id = format!("{}-s1", task_id);
    let step1_result = send_rpc("task.complete_step", json!({
        "step_id": step1_id,
        "evidence": { "stdout": "Core implementation committed", "exit_code": 0 }
    })).await;
    println!("8a. Complete Step 1 Result: Title={:?}, Status={:?}", step1_result["title"], step1_result["status"]);
    assert_eq!(step1_result["status"], "COMPLETED");

    let step2_id = format!("{}-s2", task_id);
    let step2_result = send_rpc("task.complete_step", json!({
        "step_id": step2_id,
        "evidence": { "stdout": "test result: ok. 5 passed; 0 failed", "exit_code": 0 }
    })).await;
    println!("8b. Complete Step 2 Result: Title={:?}, Status={:?}", step2_result["title"], step2_result["status"]);
    assert_eq!(step2_result["status"], "COMPLETED");

    // 11. Check Task DAG & Dependencies (dag.dependencies)
    let dag_result = send_rpc("dag.dependencies", json!({ "task_id": task_id })).await;
    println!("9. DAG Dependencies Result: {:?}", dag_result);

    // 12. Submit Task for Authoritative Verification (task.submit)
    let submit_result = send_rpc("task.submit", json!({ "task_id": task_id, "agent_id": agent_id })).await;
    println!("10. Submit Task Result: is_valid={:?}, rejections={:?}", submit_result["is_valid"], submit_result["rejection_reasons"]);
    assert_eq!(submit_result["is_valid"], true);

    // 13. Enqueue for Serialized Merge & Check Queue (merge.queue_status)
    coordinator.merge.enqueue_task(
        "proj-agentxflow-v2",
        &task_id,
        "agentxflow/task-AUTH-01",
        "main",
        "sha-base-01",
        "sha-head-01",
    ).expect("Failed to enqueue task for merge");

    let queue_result = send_rpc("merge.queue_status", json!({ "project_id": "proj-agentxflow-v2" })).await;
    println!("11. Merge Queue Result: Total Candidates={}", queue_result.as_array().unwrap().len());
    assert_eq!(queue_result.as_array().unwrap().len(), 1);
    assert_eq!(queue_result[0]["task_id"], task_id);

    // 14. Release Scope (scope.release)
    let release_result = send_rpc("scope.release", json!({ "task_id": task_id })).await;
    println!("12. Scope Release Result: {:?}", release_result);
    assert_eq!(release_result["status"], "released");

    // 15. Masterplan Execution Hub MCP Endpoints
    // a. Create masterplan in coordinator
    let mp_raw = "1. Setup DB Schema\n2. Build Handlers\n3. Write Tests\n4. Deploy UI";
    coordinator.create_or_update_masterplan("proj-agentxflow-v2", mp_raw, 4, 2).unwrap();

    // b. masterplan.get
    let mp_get = send_rpc("masterplan.get", json!({ "project_id": "proj-agentxflow-v2" })).await;
    println!("13. masterplan.get Result: Status={:?}", mp_get["plan"]["status"]);
    assert_eq!(mp_get["plan"]["status"], "UNSORTED");
    assert!(mp_get["instruction"].as_str().unwrap().contains("UNSORTED"));

    // c. masterplan.decompose
    let decomposed_steps = vec![
        json!({ "step_index": 1, "title": "Setup DB Schema", "description": "Write migrations", "suggested_scope": "src/db/**" }),
        json!({ "step_index": 2, "title": "Build Handlers", "description": "Write HTTP routes", "suggested_scope": "src/api/**" }),
        json!({ "step_index": 3, "title": "Write Tests", "description": "Execute cargo test", "suggested_scope": "tests/**" }),
        json!({ "step_index": 4, "title": "Deploy UI", "description": "Build React app", "suggested_scope": "src/ui/**" }),
    ];
    let mp_decomp = send_rpc("masterplan.decompose", json!({ "project_id": "proj-agentxflow-v2", "steps": decomposed_steps })).await;
    println!("14. masterplan.decompose Result: Decomposed {} steps", mp_decomp.as_array().unwrap().len());
    assert_eq!(mp_decomp.as_array().unwrap().len(), 4);

    // d. masterplan.status
    let mp_status = send_rpc("masterplan.status", json!({ "project_id": "proj-agentxflow-v2" })).await;
    println!("15. masterplan.status Result: Status={:?}, Total={}", mp_status["status"], mp_status["total_steps"]);
    assert_eq!(mp_status["status"], "RESORTED");
    assert_eq!(mp_status["total_steps"], 4);
    assert_eq!(mp_status["pending_steps"], 4);

    // e. masterplan.claim_chunk (requests 4, but capped to max_steps_per_agent = 2)
    let mp_claim = send_rpc("masterplan.claim_chunk", json!({
        "project_id": "proj-agentxflow-v2",
        "agent_id": agent_id,
        "count": 4
    })).await;
    println!("16. masterplan.claim_chunk Result: Task Title={:?}", mp_claim["title"]);
    assert!(mp_claim["title"].as_str().unwrap().contains("Steps 1-2"));

    println!(">>> FULL END-TO-END MCP WORKFLOW TEST COMPLETED WITH 100% SUCCESS ACROSS ALL TOOLS!");
}
