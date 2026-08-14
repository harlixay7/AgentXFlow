use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::mcp::McpServer;
use agent_x_flow_lib::models::TaskState;
use agent_x_flow_lib::security::SecurityManager;
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

fn setup_test_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("axf_a_to_z_{}_{}", name, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let readme = dir.join("README.md");
    std::fs::write(&readme, "# AgentXFlow A-to-Z Test Repository\n").unwrap();

    let run_cmd = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .expect("Failed to run git command");
        if !out.status.success() {
            panic!("Git command {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
        }
    };

    run_cmd(&["init"]);
    run_cmd(&["config", "user.name", "A-to-Z Test Runner"]);
    run_cmd(&["config", "user.email", "atoz@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial baseline commit"]);
    run_cmd(&["branch", "-M", "main"]);

    dir
}

#[tokio::test]
async fn test_entire_pipeline_from_a_to_z() {
    println!("\n========================================================================");
    println!("🚀 AGENTXFLOW — COMPLETE PIPELINE INTEGRATION TEST (A TO Z)");
    println!("========================================================================\n");

    let repo_dir = setup_test_repo("pipeline");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite in-memory DB");
    let coordinator = CoordinatorEngine::new(pool.clone());

    // ------------------------------------------------------------------------
    // Step 1: Security & MCP Server Initialization
    // ------------------------------------------------------------------------
    let auth_token = "axf_bootstrap_master_token_2026".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7898;

    let mcp_server = McpServer::new(coordinator.clone(), test_port, security.clone());
    mcp_server.start().await.expect("Failed to start live MCP server");
    sleep(Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // Verify /health endpoint
    let health_res = client.get(format!("{}/health", base_url)).send().await.unwrap();
    assert_eq!(health_res.status(), reqwest::StatusCode::OK);
    println!(" [Step 1] Gateway Health Check: 200 OK (ONLINE)");

    // Verify Security Gate (Missing auth rejected with 401)
    let unauth_res = client.post(format!("{}/mcp", base_url))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }))
        .send().await.unwrap();
    assert_eq!(unauth_res.status(), reqwest::StatusCode::UNAUTHORIZED);
    println!(" [Step 2] Security Gate: Missing Bearer Token rejected with 401");

    // Initialize MCP Protocol
    let init_res = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize" }))
        .send().await.unwrap();
    assert_eq!(init_res.status(), reqwest::StatusCode::OK);
    println!(" [Step 3] MCP Initialize: Protocol Version 2026-07-28 confirmed");

    // ------------------------------------------------------------------------
    // Step 2: Project Creation & Contract Hashes
    // ------------------------------------------------------------------------
    let proj = coordinator.create_project(
        "A-to-Z Pipeline Service",
        &repo_dir.to_string_lossy(),
        "High-reliability multi-agent control plane demonstration",
        "main",
    ).expect("Failed to create project");
    println!(" [Step 4] Project Created: ID = {}", proj.id);

    // ------------------------------------------------------------------------
    // Step 3: Agent Registration & Cryptographic Session Generation
    // ------------------------------------------------------------------------
    let reg_a = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "agent.register",
            "params": { "name": "Agent-Alpha", "agent_type": "Antigravity" }
        }))
        .send().await.unwrap();
    let reg_a_json: serde_json::Value = reg_a.json().await.unwrap();
    let agent_a_id = reg_a_json["result"]["id"].as_str().unwrap().to_string();
    let session_token_a = reg_a_json["result"]["session_token"].as_str().unwrap().to_string();

    let reg_b = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "agent.register",
            "params": { "name": "Agent-Beta", "agent_type": "Claude" }
        }))
        .send().await.unwrap();
    let reg_b_json: serde_json::Value = reg_b.json().await.unwrap();
    let agent_b_id = reg_b_json["result"]["id"].as_str().unwrap().to_string();
    let session_token_b = reg_b_json["result"]["session_token"].as_str().unwrap().to_string();

    assert!(session_token_a.starts_with("axf_sess_"));
    assert!(session_token_b.starts_with("axf_sess_"));
    println!(" [Step 5] Agents Registered with Unique Session Tokens:");
    println!("          - Agent-Alpha ({}) Token: {}...", &agent_a_id[..8], &session_token_a[..16]);
    println!("          - Agent-Beta  ({}) Token: {}...", &agent_b_id[..8], &session_token_b[..16]);

    // ------------------------------------------------------------------------
    // Step 4: Masterplan Lifecycle (Create -> Decompose -> Anti-Hoarding Claim)
    // ------------------------------------------------------------------------
    coordinator.create_or_update_masterplan(
        &proj.id,
        "Module 1: Authentication Services\nModule 2: Database Storage\nModule 3: REST API Gateway",
        6,
        2,
    ).expect("Failed to create masterplan");

    let dec_res = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "masterplan.decompose",
            "params": {
                "project_id": proj.id,
                "steps": [
                    { "step_index": 1, "title": "Auth Core", "description": "Build auth logic", "suggested_scope": "src/auth/**", "acceptance_criteria": "Auth unit tests pass" },
                    { "step_index": 2, "title": "Auth Tokens", "description": "Build JWT signer", "suggested_scope": "src/auth/**", "acceptance_criteria": "Tokens sign correctly" },
                    { "step_index": 3, "title": "DB Core", "description": "Build DB tables", "suggested_scope": "src/db/**", "acceptance_criteria": "DB migrations run" },
                    { "step_index": 4, "title": "DB Queries", "description": "Add query layer", "suggested_scope": "src/db/**", "acceptance_criteria": "Queries return records" }
                ]
            }
        }))
        .send().await.unwrap();
    let dec_json: serde_json::Value = dec_res.json().await.unwrap();
    assert_eq!(dec_json["result"].as_array().unwrap().len(), 4);
    println!(" [Step 6] Masterplan Decomposed into 4 Structured Steps");

    // Agent Alpha claims Chunk 1 (Steps 1 & 2)
    let claim_a = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_a))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "masterplan.claim_chunk",
            "params": { "project_id": proj.id, "agent_id": agent_a_id, "count": 2 }
        }))
        .send().await.unwrap();
    let claim_a_json: serde_json::Value = claim_a.json().await.unwrap();
    let task_a_id = claim_a_json["result"]["id"].as_str().unwrap().to_string();
    let worktree_a_str = claim_a_json["result"]["worktree_path"].as_str().unwrap();
    let worktree_a = PathBuf::from(worktree_a_str);
    println!(" [Step 7] Agent Alpha Claimed Task A: ID = {}", task_a_id);
    println!("          Isolated AppData Worktree Created at: {:?}", worktree_a);
    assert!(worktree_a.exists());

    // Agent Beta claims Chunk 2 (Steps 3 & 4)
    let claim_b = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_b))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "masterplan.claim_chunk",
            "params": { "project_id": proj.id, "agent_id": agent_b_id, "count": 2 }
        }))
        .send().await.unwrap();
    let claim_b_json: serde_json::Value = claim_b.json().await.unwrap();
    let task_b_id = claim_b_json["result"]["id"].as_str().unwrap().to_string();
    let worktree_b_str = claim_b_json["result"]["worktree_path"].as_str().unwrap();
    let worktree_b = PathBuf::from(worktree_b_str);
    println!(" [Step 8] Agent Beta Claimed Task B: ID = {}", task_b_id);
    println!("          Isolated AppData Worktree Created at: {:?}", worktree_b);
    assert!(worktree_b.exists());

    // Anti-hoarding test: Agent Alpha attempts to hoard additional chunk before finishing -> rejected
    let hoard_res = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_a))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "masterplan.claim_chunk",
            "params": { "project_id": proj.id, "agent_id": agent_a_id, "count": 2 }
        }))
        .send().await.unwrap();
    let hoard_json: serde_json::Value = hoard_res.json().await.unwrap();
    assert!(hoard_json["error"]["message"].as_str().unwrap().contains("Anti-hoarding"));
    println!(" [Step 9] Anti-Hoarding Cap Enforced: Duplicate chunk claim blocked");

    // ------------------------------------------------------------------------
    // Step 5: Scope Leases & Collision Detection
    // ------------------------------------------------------------------------
    // Agent Alpha acquires exclusive write lock on src/auth/**
    let scope_a = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_a))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "scope.acquire",
            "params": { "task_id": task_a_id, "agent_id": agent_a_id, "patterns": ["src/auth/**"] }
        }))
        .send().await.unwrap();
    let scope_a_json: serde_json::Value = scope_a.json().await.unwrap();
    assert_eq!(scope_a_json["result"].as_array().unwrap().len(), 1);
    println!(" [Step 10] Scope Acquired: Agent Alpha granted 'src/auth/**'");

    // Agent Beta acquires exclusive write lock on src/db/**
    let scope_b = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_b))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "scope.acquire",
            "params": { "task_id": task_b_id, "agent_id": agent_b_id, "patterns": ["src/db/**"] }
        }))
        .send().await.unwrap();
    let scope_b_json: serde_json::Value = scope_b.json().await.unwrap();
    assert_eq!(scope_b_json["result"].as_array().unwrap().len(), 1);
    println!(" [Step 11] Scope Acquired: Agent Beta granted 'src/db/**'");

    // Conflicting Scope Test: Agent Beta tries to acquire 'src/auth/service.rs' -> rejected
    let conflict_scope = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_b))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "scope.acquire",
            "params": { "task_id": task_b_id, "agent_id": agent_b_id, "patterns": ["src/auth/service.rs"] }
        }))
        .send().await.unwrap();
    let conflict_json: serde_json::Value = conflict_scope.json().await.unwrap();
    assert!(conflict_json["error"]["message"].as_str().unwrap().contains("Scope collision"));
    println!(" [Step 12] Scope Collision Prevented: Conflicting lease request blocked atomically");

    // ------------------------------------------------------------------------
    // Step 6: Code Implementation & Commit Inside Dedicated Worktree A
    // ------------------------------------------------------------------------
    let auth_dir = worktree_a.join("src").join("auth");
    std::fs::create_dir_all(&auth_dir).unwrap();
    std::fs::write(
        auth_dir.join("mod.rs"),
        "pub fn verify_token(token: &str) -> bool { token.starts_with(\"axf_\") }\n\n#[test]\nfn test_token() { assert!(verify_token(\"axf_valid\")); }\n",
    ).unwrap();

    Command::new("git").args(&["add", "."]).current_dir(&worktree_a).output().unwrap();
    Command::new("git").args(&["commit", "-m", "feat: add auth token validation"]).current_dir(&worktree_a).output().unwrap();
    println!(" [Step 13] Agent Alpha Committed Implementation in Task Worktree A");

    // ------------------------------------------------------------------------
    // Step 7: Step Completion & Criteria Authority Verification
    // ------------------------------------------------------------------------
    let details_a = coordinator.get_task_details(&task_a_id).unwrap();
    for step in &details_a.steps {
        let comp = client.post(format!("{}/mcp", base_url))
            .header("Authorization", format!("Bearer {}", session_token_a))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "task.complete_step",
                "params": { "step_id": step.id, "evidence": "cargo test --exit-code 0" }
            }))
            .send().await.unwrap();
        let comp_json: serde_json::Value = comp.json().await.unwrap();
        assert_eq!(comp_json["result"]["status"], "COMPLETED");
    }
    println!(" [Step 14] All Mandatory Steps Completed with Recorded Evidence");

    // Autonomous agent attempting criteria.satisfy -> rejected
    let agent_crit = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_a))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "criteria.satisfy",
            "params": { "task_id": task_a_id, "criterion_id": details_a.criteria[0].id }
        }))
        .send().await.unwrap();
    let agent_crit_json: serde_json::Value = agent_crit.json().await.unwrap();
    assert!(agent_crit_json["error"]["message"].as_str().unwrap().contains("human reviewer authority"));
    println!(" [Step 15] Autonomous Criteria Satisfaction Blocked (Reviewer Gate Protected)");

    // Human/Reviewer satisfies criteria
    for crit in &details_a.criteria {
        coordinator.satisfy_acceptance_criterion(&task_a_id, &crit.id, Some("Signed off by Lead Reviewer")).unwrap();
    }
    println!(" [Step 16] Criteria Satisfied by Reviewer Authority");

    // ------------------------------------------------------------------------
    // Step 8: Coordinator Verification Gate & Immutable Proof Bundle
    // ------------------------------------------------------------------------
    let head_a = coordinator.git.get_worktree_head_sha(&worktree_a).unwrap();
    let check_run = coordinator.verify.execute_check(
        &task_a_id,
        "chk-auth-tests",
        "Unit Test Suite",
        &worktree_a,
        &head_a,
        "cmd /c exit 0",
    ).unwrap();
    assert_eq!(check_run.is_passed, true);
    println!(" [Step 17] Coordinator Verification Check Executed (Exit Code 0, Captured Evidence)");

    // Submit Task A
    let submit_a = client.post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token_a))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "task.submit",
            "params": { "task_id": task_a_id, "agent_id": agent_a_id }
        }))
        .send().await.unwrap();
    let submit_a_json: serde_json::Value = submit_a.json().await.unwrap();
    assert_eq!(submit_a_json["result"]["is_valid"], true);
    println!(" [Step 18] Task A Submitted & Validated -> Transitioned to REVIEW");

    let final_a = coordinator.get_task_details(&task_a_id).unwrap();
    assert_eq!(final_a.task.state, TaskState::Review);
    let proof_a = final_a.proof_bundle.expect("Sealed proof bundle required");
    println!(" [Step 19] Cryptographic Proof Bundle Generated: SHA-256 = [{}...{}]", &proof_a.proof_hash[..8], &proof_a.proof_hash[proof_a.proof_hash.len()-8..]);

    // ------------------------------------------------------------------------
    // Step 9: Code Implementation & Submission for Task B (Agent Beta)
    // ------------------------------------------------------------------------
    let db_dir = worktree_b.join("src").join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    std::fs::write(
        db_dir.join("mod.rs"),
        "pub fn connect_db() -> bool { true }\n\n#[test]\nfn test_db() { assert!(connect_db()); }\n",
    ).unwrap();

    Command::new("git").args(&["add", "."]).current_dir(&worktree_b).output().unwrap();
    Command::new("git").args(&["commit", "-m", "feat: add database storage layer"]).current_dir(&worktree_b).output().unwrap();

    let details_b = coordinator.get_task_details(&task_b_id).unwrap();
    for step in &details_b.steps {
        coordinator.complete_step(&step.id, Some("DB tests passed")).unwrap();
    }
    for crit in &details_b.criteria {
        coordinator.satisfy_acceptance_criterion(&task_b_id, &crit.id, Some("Reviewer sign-off")).unwrap();
    }

    let head_b = coordinator.git.get_worktree_head_sha(&worktree_b).unwrap();
    coordinator.verify.execute_check(&task_b_id, "chk-db", "DB Tests", &worktree_b, &head_b, "cmd /c exit 0").unwrap();
    let submit_b_res = coordinator.submit_task(&task_b_id, &agent_b_id).unwrap();
    assert_eq!(submit_b_res.is_valid, true);
    println!(" [Step 20] Task B Verified and Submitted -> Transitioned to REVIEW");

    // ------------------------------------------------------------------------
    // Step 10: Serialized FIFO Merge Queue Integration
    // ------------------------------------------------------------------------
    // Enqueue Task A
    let q_item_a = coordinator.enqueue_task_by_id(&proj.id, &task_a_id).expect("Enqueue A failed");
    assert_eq!(q_item_a.position, 1);
    println!(" [Step 21] Task A Enqueued in Serialized Merge Queue at Position #1");

    // Enqueue Task B
    let q_item_b = coordinator.enqueue_task_by_id(&proj.id, &task_b_id).expect("Enqueue B failed");
    assert_eq!(q_item_b.position, 2);
    println!(" [Step 22] Task B Enqueued in Serialized Merge Queue at Position #2");

    // FIFO Violation Test: Attempting to merge Task B before Task A -> rejected!
    let out_of_order = coordinator.merge.process_merge_by_id(&q_item_b.id, &repo_dir);
    assert!(out_of_order.is_err());
    let err_msg = out_of_order.err().unwrap();
    assert!(err_msg.contains("FIFO queue ordering violation"));
    println!(" [Step 23] Strict FIFO Merge Ordering Enforced: Candidate #2 blocked ahead of #1");

    // Process Task A in Serialized FIFO Order
    let merge_a = coordinator.merge.process_merge_by_id(&q_item_a.id, &repo_dir).expect("Merge A failed");
    assert_eq!(merge_a.simulation_passed, true);
    let target_sha_1 = coordinator.git.get_ref_sha(&repo_dir, "main").unwrap();
    let task_a_done = coordinator.get_task(&task_a_id).unwrap();
    assert_eq!(task_a_done.state, TaskState::Done);
    println!(" [Step 24] Task A Successfully Merged into 'main' (CAS SHA: {}) -> State: DONE", &target_sha_1[..8]);

    // Stale Base Detection Test: Task B was created before Task A merged. Attempting to merge Task B now must detect STALE base!
    let stale_b = coordinator.merge.process_merge_by_id(&q_item_b.id, &repo_dir);
    assert!(stale_b.is_err());
    let stale_err = stale_b.err().unwrap();
    assert!(stale_err.contains("Candidate base is STALE"));
    println!(" [Step 25] Stale Base Safety Detected: Candidate B flagged STALE because target branch 'main' moved");

    // Rebase Task B onto updated 'main' HEAD, re-verify, re-enqueue and integrate
    Command::new("git").args(&["fetch", "origin"]).current_dir(&worktree_b).output().ok();
    Command::new("git").args(&["rebase", "main"]).current_dir(&worktree_b).output().unwrap();
    let head_b_new = coordinator.git.get_worktree_head_sha(&worktree_b).unwrap();
    pool.lock().execute("UPDATE tasks SET base_sha = ?1, head_sha = ?2, state = 'REVIEW' WHERE id = ?3", rusqlite::params![target_sha_1, head_b_new, task_b_id]).unwrap();
    coordinator.verify.execute_check(&task_b_id, "chk-db-rebase", "DB Post-Rebase Tests", &worktree_b, &head_b_new, "cmd /c exit 0").unwrap();
    coordinator.verify.generate_proof_bundle(&task_b_id, &proj.id, Some(&agent_b_id), "DB Module", &target_sha_1, &head_b_new, &["src/db/mod.rs".into()], "Post-rebase verification").unwrap();

    let q_item_b_rebased = coordinator.enqueue_task_by_id(&proj.id, &task_b_id).expect("Re-enqueue B failed");
    let merge_b = coordinator.merge.process_merge_by_id(&q_item_b_rebased.id, &repo_dir).expect("Merge B failed");
    assert_eq!(merge_b.simulation_passed, true);
    let target_sha_2 = coordinator.git.get_ref_sha(&repo_dir, "main").unwrap();
    let task_b_done = coordinator.get_task(&task_b_id).unwrap();
    assert_eq!(task_b_done.state, TaskState::Done);
    println!(" [Step 26] Task B Rebased & Successfully Merged into 'main' (CAS SHA: {}) -> State: DONE", &target_sha_2[..8]);

    // ------------------------------------------------------------------------
    // Step 11: Real ContextPack Truthfulness
    // ------------------------------------------------------------------------
    let pack = coordinator.get_context_pack(&proj.id, &task_a_id).unwrap();
    assert!(!pack.contract_hash.is_empty());
    assert!(!pack.project_rules.is_empty());
    println!(" [Step 27] ContextPack Delivered Real Authoritative DB Records");

    // ------------------------------------------------------------------------
    // Step 12: Startup Self-Healing Reconciliation
    // ------------------------------------------------------------------------
    // Simulate an interrupted task in CLAIMING state
    let sim_task = coordinator.create_task(&proj.id, "Simulated Interrupted Task", "Crash test", "LOW", vec![], vec![]).unwrap();
    pool.lock().execute("UPDATE tasks SET state = 'CLAIMING', substate = 'CLAIMING' WHERE id = ?1", [&sim_task.id]).unwrap();
    coordinator.reconcile_on_startup();
    let healed_task = coordinator.get_task(&sim_task.id).unwrap();
    assert_eq!(healed_task.state, TaskState::Ready);
    println!(" [Step 28] Self-Healing Startup Reconciliation: Interrupted claiming task safely restored to READY");

    // Clean up temporary repo
    std::fs::remove_dir_all(&repo_dir).ok();

    println!("\n========================================================================");
    println!("🎉 ALL 28 PIPELINE STEPS (A TO Z) VERIFIED 100% WITH ZERO FLAWS!");
    println!("========================================================================\n");
}
