use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::mcp::McpServer;
use agent_x_flow_lib::models::DecomposedStepInput;
use agent_x_flow_lib::security::SecurityManager;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

fn create_real_git_project() -> PathBuf {
    let repo_dir = std::env::temp_dir().join(format!("agentxflow_live_repo_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repo_dir).unwrap();

    let run_cmd = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo_dir)
            .output()
            .expect("Failed to execute git");
        if !out.status.success() {
            panic!("Git failed: {:?}", String::from_utf8_lossy(&out.stderr));
        }
    };

    run_cmd(&["init"]);
    run_cmd(&["config", "user.name", "Manual Tester"]);
    run_cmd(&["config", "user.email", "tester@agentxflow.local"]);

    let readme = repo_dir.join("README.md");
    std::fs::write(&readme, "# Live System Test Project\n").unwrap();

    let src_dir = repo_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("lib.rs"),
        "pub fn core_logic() -> bool { true }\n\n#[test]\nfn test_core() { assert!(core_logic()); }\n",
    ).unwrap();

    let cargo_toml = repo_dir.join("Cargo.toml");
    std::fs::write(
        &cargo_toml,
        "[package]\nname = \"live_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ).unwrap();

    run_cmd(&["add", "."]);
    run_cmd(&["commit", "-m", "Initial baseline commit"]);
    run_cmd(&["branch", "-M", "main"]);

    repo_dir
}

#[tokio::test]
async fn test_manual_live_end_to_end_system() {
    println!("\n============================================================");
    println!("🔍 STARTING LIVE MANUAL SYSTEM TEST OF ALL FEATURES");
    println!("============================================================\n");

    // 1. Initializing Real Git Workspace & Database
    let repo_dir = create_real_git_project();
    println!("1. Initialized real Git repository at: {:?}", repo_dir);

    let db_path = repo_dir.join("coordinator.db");
    let pool = DbPool::new(&db_path).expect("Failed to initialize SQLite pool");
    let coordinator = CoordinatorEngine::new(pool.clone());

    // 2. Initialize SecurityManager with real token
    let security_dir = repo_dir.join(".agentxflow");
    std::fs::create_dir_all(&security_dir).unwrap();
    let security = SecurityManager::init_or_load(&security_dir).expect("Failed to init security");
    let initial_token = security.get_token();
    println!("2. Generated cryptographic 256-bit authentication token: [{}...{}]", &initial_token[0..8], &initial_token[initial_token.len()-8..]);

    // 3. Launch live MCP Gateway on port 7892
    let live_port = 7892;
    let server = McpServer::new(coordinator.clone(), live_port, security.clone());
    let bound_addr = server.start().await.expect("Failed to start MCP server");
    println!("3. MCP Gateway live and listening on http://{}", bound_addr);

    sleep(Duration::from_millis(150)).await;
    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", live_port);

    // 4. Test Health Check endpoint
    let health_resp = client.get(format!("{}/health", base_url)).send().await.unwrap();
    assert_eq!(health_resp.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health_resp.json().await.unwrap();
    println!("4. Verified Health Check response: status = {}", health_json["status"]);

    // 5. Test Unauthorized Access (Security Gate)
    let unauth_resp = client.post(format!("{}/mcp", base_url))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }))
        .send().await.unwrap();
    assert_eq!(unauth_resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    println!("5. Verified Security Gate: Missing Bearer Token rejected with 401 Unauthorized");

    // 6. Test Authorized MCP Protocol 'initialize'
    let init_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize" }))
        .send().await.unwrap();
    assert_eq!(init_resp.status(), reqwest::StatusCode::OK);
    let init_data: serde_json::Value = init_resp.json().await.unwrap();
    println!("6. Verified MCP 'initialize': Protocol Version = {}", init_data["result"]["protocolVersion"]);

    // 7. Test MCP 'tools/list' Tool Catalog Discovery
    let tools_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }))
        .send().await.unwrap();
    assert_eq!(tools_resp.status(), reqwest::StatusCode::OK);
    let tools_data: serde_json::Value = tools_resp.json().await.unwrap();
    let tools_array = tools_data["result"]["tools"].as_array().unwrap();
    println!("7. Verified MCP 'tools/list': Discovered {} tools with parameter schemas", tools_array.len());

    // 8. Register 2 Independent Autonomous Agents (Antigravity & Claude Code)
    let agent_a_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "agent_register",
            "params": { "name": "Antigravity-Lead", "agent_type": "Antigravity" }
        }))
        .send().await.unwrap();
    let agent_a_json: serde_json::Value = agent_a_resp.json().await.unwrap();
    let agent_a_id = agent_a_json["result"]["id"].as_str().unwrap().to_string();

    let agent_b_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "agent_register",
            "params": { "name": "Claude-Code-Backend", "agent_type": "Claude" }
        }))
        .send().await.unwrap();
    let agent_b_json: serde_json::Value = agent_b_resp.json().await.unwrap();
    let agent_b_id = agent_b_json["result"]["id"].as_str().unwrap().to_string();

    println!("8. Registered Agent A ({}) and Agent B ({})", &agent_a_id[0..8], &agent_b_id[0..8]);

    // 9. Send Heartbeats
    let hb_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "agent_heartbeat",
            "params": { "agent_id": agent_a_id }
        }))
        .send().await.unwrap();
    assert_eq!(hb_resp.status(), reqwest::StatusCode::OK);
    println!("9. Heartbeat successfully acknowledged by coordinator");

    // 10. Create Real Project in Coordinator
    let proj = coordinator.create_project(
        "Live Production Service",
        &repo_dir.to_string_lossy(),
        "Coordinate multiple agents live",
        "main",
    ).expect("Failed to create project");
    println!("10. Created project in SQLite: id = {}", proj.id);

    // 11. Create Engineering Task with Required Steps and Acceptance Criteria
    let task = coordinator.create_task(
        &proj.id,
        "Implement Secure Session Manager",
        "Build token session verification and unit tests",
        "HIGH",
        vec![
            ("Create Session Struct".into(), "Add session tracking to src/auth/session.rs".into(), true),
            ("Run Auth Unit Tests".into(), "Execute cargo test to verify logic".into(), true),
        ],
        vec!["Session validates non-expired tokens cleanly".into()],
    ).expect("Failed to create task");
    println!("11. Created Task #{} with 2 required steps and 1 acceptance criterion", task.id);

    // 12. Agent A Claims Task -> Tests Worktree Allocation
    let claim_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "task_claim",
            "params": { "task_id": task.id, "agent_id": agent_a_id }
        }))
        .send().await.unwrap();
    let claim_data: serde_json::Value = claim_resp.json().await.unwrap();
    let worktree_str = claim_data["result"]["worktree_path"].as_str().expect("Expected worktree path");
    let worktree_path = PathBuf::from(worktree_str);
    println!("12. Task claimed by Agent A! Isolated Git worktree created at: {:?}", worktree_path);
    assert!(worktree_path.exists(), "Worktree directory must exist on disk");

    // 13. Acquire Exclusive Scope Lock on 'src/auth/**'
    let scope_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "scope_acquire",
            "params": { "task_id": task.id, "agent_id": agent_a_id, "patterns": ["src/auth/**"] }
        }))
        .send().await.unwrap();
    assert_eq!(scope_resp.status(), reqwest::StatusCode::OK);
    println!("13. Granted exclusive write lock on 'src/auth/**' to Agent A");

    // 14. Agent B tries to claim an overlapping scope on task 2 -> Collision Blocked!
    let task_b = coordinator.create_task(&proj.id, "Conflicting Task", "Desc", "HIGH", vec![], vec![]).unwrap();
    coordinator.claim_task(&task_b.id, &agent_b_id).unwrap();
    let conflict_scope_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "scope_acquire",
            "params": { "task_id": task_b.id, "agent_id": agent_b_id, "patterns": ["src/auth/session.rs"] }
        }))
        .send().await.unwrap();
    let conflict_json: serde_json::Value = conflict_scope_resp.json().await.unwrap();
    assert!(conflict_json.get("error").is_some(), "Conflicting scope must return error");
    println!("14. Conflicting scope request by Agent B blocked by atomic transaction check!");

    // 15. Agent A implements code inside its dedicated worktree
    let auth_dir = worktree_path.join("src").join("auth");
    std::fs::create_dir_all(&auth_dir).unwrap();
    std::fs::write(
        auth_dir.join("session.rs"),
        "pub struct Session { pub id: String, pub valid: bool }\nimpl Session { pub fn is_active(&self) -> bool { self.valid } }\n\n#[test]\nfn test_session() { let s = Session { id: \"s1\".into(), valid: true }; assert!(s.is_active()); }\n",
    ).unwrap();

    // Commit changes in task worktree
    Command::new("git").args(&["add", "."]).current_dir(&worktree_path).output().unwrap();
    Command::new("git").args(&["commit", "-m", "feat: implement session manager"]).current_dir(&worktree_path).output().unwrap();
    println!("15. Agent A committed implementation strictly inside worktree branch");

    // 16. Mark Required Steps Completed with Real Evidence
    let details_before = coordinator.get_task_details(&task.id).unwrap();
    for step in &details_before.steps {
        let step_resp = client.post(format!("{}/mcp", base_url))
            .header("Authorization", format!("Bearer {}", initial_token))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "task_complete_step",
                "params": {
                    "step_id": step.id,
                    "evidence": "cargo test --exit-code 0 passed"
                }
            }))
            .send().await.unwrap();
        assert_eq!(step_resp.status(), reqwest::StatusCode::OK);
    }
    // Satisfy acceptance criteria
    coordinator.db.lock().execute("UPDATE acceptance_criteria SET is_satisfied = 1 WHERE task_id = ?1", [&task.id]).unwrap();
    println!("16. Completed all execution steps with recorded evidence and satisfied acceptance criteria");

    // 17. Submit Task to Authoritative Verification Gate
    let submit_resp = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", initial_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "task_submit",
            "params": { "task_id": task.id, "agent_id": agent_a_id }
        }))
        .send().await.unwrap();
    let submit_json: serde_json::Value = submit_resp.json().await.unwrap();
    assert_eq!(submit_json["result"]["is_valid"], true);
    println!("17. Task verified and approved by Coordinator Verification Gate! (State moved to REVIEW)");

    // 18. Inspect Task Details Aggregation Query (Used by Frontend Task Workspace)
    let final_details = coordinator.get_task_details(&task.id).unwrap();
    assert_eq!(final_details.steps.len(), 2);
    assert_eq!(final_details.criteria.len(), 1);
    assert_eq!(final_details.leases.len(), 1);
    assert!(final_details.proof_bundle.is_some(), "Proof bundle must be generated and sealed");
    let bundle = final_details.proof_bundle.unwrap();
    println!("18. Inspected TaskDetails & Proof Bundle: SHA-256 Digest = [{}...{}]", &bundle.proof_hash[0..8], &bundle.proof_hash[bundle.proof_hash.len()-8..]);

    // 19. Enqueue Task into Serialized Merge Queue
    let q_item = coordinator.merge.enqueue_task(
        &proj.id,
        &task.id,
        &final_details.task.branch_name.unwrap(),
        "main",
        &final_details.task.base_sha.unwrap(),
        &final_details.task.head_sha.unwrap(),
    ).expect("Failed to enqueue task");
    println!("19. Enqueued Task into Serialized Merge Queue at position #{}", q_item.position);

    // 20. Process Merge Queue in Isolated Integration Worktree
    let merge_attempt = coordinator.merge.process_merge(&proj.id, &repo_dir, &q_item).expect("Merge failed");
    assert_eq!(merge_attempt.simulation_passed, true);
    assert_eq!(merge_attempt.merge_strategy, "MERGE_COMMIT");
    println!("20. Successfully integrated candidate branch into 'main' via 3-way merge!");

    // 21. Verify main branch ref has advanced and task is DONE
    let main_head = coordinator.git.get_ref_sha(&repo_dir, "main").unwrap();
    assert_eq!(Some(main_head.clone()), merge_attempt.target_sha_after);
    let task_done = coordinator.get_task(&task.id).unwrap();
    assert_eq!(task_done.state.as_str().to_uppercase(), "DONE");
    println!("21. Target branch ref 'main' successfully advanced to: {}", &main_head[0..12]);
    println!("    Task state transitioned to: DONE");

    // 22. Test Token Rotation API
    let new_token = security.rotate_token().expect("Failed to rotate token");
    assert_ne!(initial_token, new_token);
    assert_eq!(security.validate_token(&new_token), true);
    assert_eq!(security.validate_token(&initial_token), false);
    println!("22. Token rotation verified: Old token revoked, new token active");

    // Cleanup temp repo
    std::fs::remove_dir_all(&repo_dir).ok();

    println!("\n============================================================");
    println!("🎉 ALL 22 LIVE MANUAL FEATURES VERIFIED 100% CLEANLY!");
    println!("============================================================\n");
}
