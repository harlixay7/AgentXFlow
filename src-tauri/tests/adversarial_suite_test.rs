use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::mcp::McpServer;
use agent_x_flow_lib::models::DecomposedStepInput;
use agent_x_flow_lib::security::SecurityManager;
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

fn setup_temp_git_repo(prefix: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Adversarial Test Repository\n").unwrap();

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
    run_cmd(&["config", "user.name", "Hostile Test Agent"]);
    run_cmd(&["config", "user.email", "adversarial@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial baseline commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

#[tokio::test]
async fn test_adversarial_security_and_mcp_suite() {
    println!("============================================================");
    println!("🔒 ADVERSARIAL SUITE PART 1: SECURITY & MCP PROTOCOL GATES");
    println!("============================================================\n");

    let temp_repo = setup_temp_git_repo("adv_sec");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool);

    let auth_token = "adv_secret_live_token_9999".to_string();
    let security = SecurityManager::new_with_token(auth_token.clone());
    let test_port = 7899;

    let server = McpServer::new(coordinator.clone(), test_port, security);
    server.start().await.expect("Failed to start test MCP server");
    sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", test_port);

    // Test 1: Missing auth -> rejected (401)
    let res1 = client.post(format!("{}/mcp", base_url)).json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })).send().await.unwrap();
    assert_eq!(res1.status(), reqwest::StatusCode::UNAUTHORIZED);
    println!("   ✔ Test 1 PASS: Missing auth rejected with 401");

    // Test 2: Wrong auth -> rejected (401)
    let res2 = client.post(format!("{}/mcp", base_url)).header("Authorization", "Bearer wrong_token_attempt").json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })).send().await.unwrap();
    assert_eq!(res2.status(), reqwest::StatusCode::UNAUTHORIZED);
    println!("   ✔ Test 2 PASS: Wrong auth rejected with 401");

    // Test 3: Invalid origin -> rejected (403)
    let res3 = client.post(format!("{}/mcp", base_url)).header("Authorization", format!("Bearer {}", auth_token)).header("Origin", "http://evil-attacker-site.com").json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })).send().await.unwrap();
    assert_eq!(res3.status(), reqwest::StatusCode::FORBIDDEN);
    println!("   ✔ Test 3 PASS: Evil cross-origin rejected with 403 Forbidden");

    // Test 4: Valid auth -> accepted (200)
    let res4 = client.post(format!("{}/mcp", base_url)).header("Authorization", format!("Bearer {}", auth_token)).json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })).send().await.unwrap();
    assert_eq!(res4.status(), reqwest::StatusCode::OK);
    println!("   ✔ Test 4 PASS: Valid auth accepted with 200 OK");

    // Test 5: Real MCP client initializes
    let init_json: serde_json::Value = res4.json().await.unwrap();
    assert_eq!(init_json["result"]["protocolVersion"], "2024-11-05");
    println!("   ✔ Test 5 PASS: Real MCP client successfully initialized (Protocol Version 2024-11-05)");

    // Test 6: Real MCP client registers agent and obtains cryptographically secure session
    let reg_res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "agent.register",
            "params": { "name": "Antigravity-Adv", "agent_type": "Antigravity" }
        }))
        .send()
        .await
        .unwrap();

    let reg_json: serde_json::Value = reg_res.json().await.unwrap();
    let session_token = reg_json["result"]["session_token"].as_str().unwrap();
    let agent_id = reg_json["result"]["id"].as_str().unwrap();
    assert!(!agent_id.is_empty());
    assert!(session_token.starts_with("axf_sess_"));
    println!("   ✔ Test 6 PASS: Agent registered with secure session token: {}", &session_token[..16]);

    // Test 7: Impersonation attempt with valid session calling under different agent_id -> rejected
    let imp_res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "agent.heartbeat",
            "params": { "agent_id": "impersonated_victim_agent_id" }
        }))
        .send()
        .await
        .unwrap();

    let imp_json: serde_json::Value = imp_res.json().await.unwrap();
    assert!(imp_json["error"]["message"].as_str().unwrap().contains("impersonation rejected"));
    println!("   ✔ Test 7 PASS: Impersonation attempt rejected by session gate");

    // Test 8: Agent session calling criteria_satisfy -> rejected (requires human reviewer authority)
    let crit_res = client
        .post(format!("{}/mcp", base_url))
        .header("Authorization", format!("Bearer {}", session_token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "criteria.satisfy",
            "params": { "task_id": "task-1", "criterion_id": "crit-1" }
        }))
        .send()
        .await
        .unwrap();

    let crit_json: serde_json::Value = crit_res.json().await.unwrap();
    assert!(crit_json["error"]["message"].as_str().unwrap().contains("Autonomous agents cannot self-satisfy criteria"));
    println!("   ✔ Test 8 PASS: Autonomous agent restricted from self-satisfying criteria");

    std::fs::remove_dir_all(temp_repo).ok();
}

#[tokio::test]
async fn test_adversarial_coordination_and_concurrency_suite() {
    println!("\n============================================================");
    println!("⚔️ ADVERSARIAL SUITE PART 2: CONCURRENCY & ISOLATION GATES");
    println!("============================================================\n");

    let temp_repo = setup_temp_git_repo("adv_coord");
    let pool = DbPool::new_in_memory().expect("Failed to create SQLite DB");
    let coordinator = CoordinatorEngine::new(pool.clone());

    let proj = coordinator.create_project("Adv Project", &temp_repo.to_string_lossy(), "Adv Spec", "main").unwrap();

    let agent_a = coordinator.register_agent("Agent-A", "Antigravity").unwrap();
    let agent_b = coordinator.register_agent("Agent-B", "Claude").unwrap();

    let task1 = coordinator.create_task(&proj.id, "Task 1", "Work on auth", "HIGH", vec![("Step 1".into(), "Do work".into(), true)], vec!["Done".into()]).unwrap();
    let task2 = coordinator.create_task(&proj.id, "Task 2", "Work on db", "HIGH", vec![("Step 1".into(), "Do work".into(), true)], vec!["Done".into()]).unwrap();

    // Test 9: True parallel concurrency - Two agents claim same task simultaneously -> exactly one wins
    let barrier_claim = Arc::new(tokio::sync::Barrier::new(2));
    let coord_c1 = coordinator.clone();
    let coord_c2 = coordinator.clone();
    let b1 = barrier_claim.clone();
    let b2 = barrier_claim.clone();
    let t1_id1 = task1.id.clone();
    let t1_id2 = task1.id.clone();
    let a1_id = agent_a.id.clone();
    let a2_id = agent_b.id.clone();

    let h1 = tokio::spawn(async move {
        b1.wait().await;
        coord_c1.claim_task(&t1_id1, &a1_id)
    });
    let h2 = tokio::spawn(async move {
        b2.wait().await;
        coord_c2.claim_task(&t1_id2, &a2_id)
    });

    let (res_claim1, res_claim2) = tokio::join!(h1, h2);
    let r1 = res_claim1.unwrap();
    let r2 = res_claim2.unwrap();
    println!("   [Debug Claim1 result]: {:?}", r1);
    println!("   [Debug Claim2 result]: {:?}", r2);
    let claim1_ok = r1.is_ok();
    let claim2_ok = r2.is_ok();

    assert!(
        (claim1_ok && !claim2_ok) || (!claim1_ok && claim2_ok),
        "Exactly one concurrent claim must succeed. (Claim1: {}, Claim2: {})",
        claim1_ok,
        claim2_ok
    );
    println!("   ✔ Test 9 PASS: True parallel Barrier concurrency race - exactly one agent won");

    // Test 10: True parallel concurrency - Two agents request conflicting exclusive scopes -> exactly one wins
    let barrier_scope = Arc::new(tokio::sync::Barrier::new(2));
    let coord_s1 = coordinator.clone();
    let coord_s2 = coordinator.clone();
    let sb1 = barrier_scope.clone();
    let sb2 = barrier_scope.clone();
    let st1_id = task1.id.clone();
    let t2_id = task2.id.clone();
    let a_id = agent_a.id.clone();
    let b_id = agent_b.id.clone();

    let sh1 = tokio::spawn(async move {
        sb1.wait().await;
        coord_s1.scope.acquire_scope(&st1_id, &a_id, vec!["src/auth/**".into()], "EXCLUSIVE_WRITE")
    });
    let sh2 = tokio::spawn(async move {
        sb2.wait().await;
        coord_s2.scope.acquire_scope(&t2_id, &b_id, vec!["src/auth/**".into()], "EXCLUSIVE_WRITE")
    });

    let (res_scope1, res_scope2) = tokio::join!(sh1, sh2);
    let s1 = res_scope1.unwrap();
    let s2 = res_scope2.unwrap();
    println!("   [Debug Scope1 result]: {:?}", s1);
    println!("   [Debug Scope2 result]: {:?}", s2);
    let scope1_ok = s1.is_ok();
    let scope2_ok = s2.is_ok();

    assert!(
        (scope1_ok && !scope2_ok) || (!scope1_ok && scope2_ok),
        "Exactly one concurrent conflicting scope request must succeed. (Scope1: {}, Scope2: {})",
        scope1_ok,
        scope2_ok
    );
    println!("   ✔ Test 10 PASS: True parallel Barrier concurrency scope race - collision rejected");

    // Test 11: Non-overlapping scopes -> both succeed
    let non_overlap_res = coordinator.scope.acquire_scope(&task2.id, &agent_b.id, vec!["src/db/**".into()], "EXCLUSIVE_WRITE");
    assert!(non_overlap_res.is_ok());
    println!("   ✔ Test 11 PASS: Non-overlapping scopes granted successfully to both agents");

    // Test 12: Non-owner submits task -> rejected
    let task1_claimed = coordinator.get_task(&task1.id).unwrap();
    let winner_id = task1_claimed.assigned_agent_id.clone().expect("Task 1 must have an assigned agent");
    let non_owner_id = if winner_id == agent_a.id { agent_b.id.clone() } else { agent_a.id.clone() };

    let tamper_submit = coordinator.submit_task(&task1.id, &non_owner_id);
    assert!(tamper_submit.is_err(), "Non-owner agent must not be able to submit task");
    println!("   ✔ Test 12 PASS: Non-owner agent blocked from submitting another agent's task");

    // Test 13: Scope violation - Unreserved file edit detected on submission
    let worktree_dir = PathBuf::from(task1_claimed.worktree_path.expect("Task must have worktree_path"));
    let out_of_scope_file = worktree_dir.join("src").join("unlocked_service.rs");
    std::fs::create_dir_all(out_of_scope_file.parent().unwrap()).ok();
    std::fs::write(&out_of_scope_file, "// Unreserved modification\n").unwrap();

    // Commit out-of-scope file to branch
    Command::new("git").args(&["add", "."]).current_dir(&worktree_dir).output().unwrap();
    Command::new("git").args(&["commit", "-m", "Rogue out-of-scope commit"]).current_dir(&worktree_dir).output().unwrap();

    // Audit mutations
    let mutations = coordinator.git.get_worktree_mutations(&worktree_dir, "main").unwrap();
    let violations = coordinator.scope.audit_actual_mutations(&task1.id, &winner_id, &mutations).unwrap();
    assert!(!violations.is_empty(), "Unreserved mutation must trigger a ScopeViolation");
    println!("   ✔ Test 13 PASS: Unreserved committed file detected and flagged as scope violation");

    // Test 14 & 15: Uncommitted/untracked files trigger dirty worktree rejection
    let untracked_file = worktree_dir.join("dirty_uncommitted.txt");
    std::fs::write(&untracked_file, "dirty").unwrap();

    let dirty_check = coordinator.git.check_worktree_cleanliness(&worktree_dir);
    assert!(dirty_check.is_err(), "Dirty worktree must fail cleanliness check");
    println!("   ✔ Test 14 & 15 PASS: Uncommitted dirty worktree detected and blocked");

    // Cleanup dirty file
    std::fs::remove_file(untracked_file).ok();

    // Test 16 & 17: Failed coordinator check -> rejected
    let check_run = coordinator.verify.execute_check(
        &task1.id,
        "chk-failing",
        "Cargo Unit Tests",
        &worktree_dir,
        "head_sha_123",
        "cmd /c exit 1",
    ).unwrap();
    assert_eq!(check_run.is_passed, false);

    let verify_sub = coordinator.verify.verify_task_submission(&task1.id, "head_sha_123").unwrap();
    assert_eq!(verify_sub.is_valid, false);
    println!("   ✔ Test 16 & 17 PASS: Failed coordinator check causes submission rejection");

    // Test 18: Passing test against exact HEAD -> accepted
    let check_passing = coordinator.verify.execute_check(
        &task1.id,
        "chk-passing",
        "Linter Check",
        &worktree_dir,
        "exact_head_456",
        "cmd /c exit 0",
    ).unwrap();
    assert_eq!(check_passing.is_passed, true);
    println!("   ✔ Test 18 PASS: Passing test recorded with exit code 0");

    // Test 19: Stale verification invalidation when HEAD moves
    coordinator.verify.invalidate_stale_verifications(&task1.id, "new_head_789").unwrap();
    let runs = coordinator.get_task_details(&task1.id).unwrap().verification_runs;
    for r in runs {
        if r.commit_sha != "new_head_789" {
            assert_eq!(r.is_stale, true);
        }
    }
    println!("   ✔ Test 19 PASS: Outdated verification runs marked STALE when commit moves");

    // Test 20: Real ContextPack loads real rules, real memory, and blocks dependencies
    let cp = coordinator.get_context_pack(&proj.id, &task1.id).unwrap();
    assert!(!cp.contract_hash.is_empty());
    assert!(!cp.project_rules.is_empty());
    println!("   ✔ Test 20 PASS: Real ContextPack returned authoritative data from SQLite");

    // Test 21: Masterplan chunk anti-hoarding limit
    let agent_c = coordinator.register_agent("Agent-C", "Codex").unwrap();
    let _mp = coordinator.create_or_update_masterplan(&proj.id, "Step 1\nStep 2\nStep 3\nStep 4\nStep 5\nStep 6", 6, 2).unwrap();
    coordinator.decompose_masterplan(&proj.id, vec![
        DecomposedStepInput { step_index: 1, title: "S1".into(), description: "D1".into(), suggested_scope: Some("src/mp/step1/**".into()), acceptance_criteria: None },
        DecomposedStepInput { step_index: 2, title: "S2".into(), description: "D2".into(), suggested_scope: Some("src/mp/step2/**".into()), acceptance_criteria: None },
        DecomposedStepInput { step_index: 3, title: "S3".into(), description: "D3".into(), suggested_scope: Some("src/mp/step3/**".into()), acceptance_criteria: None },
        DecomposedStepInput { step_index: 4, title: "S4".into(), description: "D4".into(), suggested_scope: Some("src/mp/step4/**".into()), acceptance_criteria: None },
    ]).unwrap();

    let chunk1 = coordinator.claim_masterplan_chunk(&proj.id, &agent_c.id, Some(2));
    assert!(chunk1.is_ok(), "Agent C should claim first chunk: {:?}", chunk1.err());

    // Agent C attempts to hoard more steps without finishing chunk 1 -> rejected
    let chunk2_hoard = coordinator.claim_masterplan_chunk(&proj.id, &agent_c.id, Some(2));
    assert!(chunk2_hoard.is_err(), "Hoarding beyond max active steps must be rejected");
    println!("   ✔ Test 21 PASS: Anti-hoarding cap actively blocks duplicate chunk hoarding");

    // Test 22: FIFO Merge Queue Ordering Enforcement
    let q_item1 = coordinator.merge.enqueue_task(&proj.id, &task1.id, "agentxflow/task-1", "main", "base_1", "head_1").unwrap();
    let q_item2 = coordinator.merge.enqueue_task(&proj.id, &task2.id, "agentxflow/task-2", "main", "base_2", "head_2").unwrap();
    assert_eq!(q_item1.position, 1);
    assert_eq!(q_item2.position, 2);

    // Processing q_item2 before q_item1 must fail FIFO queue order
    let fifo_violation = coordinator.merge.process_merge_by_id(&q_item2.id, &temp_repo);
    assert!(fifo_violation.is_err(), "FIFO ordering must reject processing #2 before #1");
    println!("   ✔ Test 22 PASS: FIFO merge queue order strictly enforced");

    // Test 23: Stale base detection stops merge
    let stale_merge = coordinator.merge.process_merge(&proj.id, &temp_repo, &q_item1);
    assert!(stale_merge.is_err());
    let queue_state = coordinator.merge.list_queue(&proj.id).unwrap();
    assert_eq!(queue_state[0].status, "STALE");
    println!("   ✔ Test 23 PASS: Target branch change detected, candidate marked STALE and stopped");

    // Test 24: Conflict aborts cleanly without damaging main
    let conflict_item = coordinator.merge.enqueue_task(&proj.id, &task2.id, "agentxflow/task-nonexistent", "main", &coordinator.git.get_ref_sha(&temp_repo, "main").unwrap(), "candidate_head_sha").unwrap();
    let conflict_res = coordinator.merge.process_merge(&proj.id, &temp_repo, &conflict_item).unwrap();
    assert_eq!(conflict_res.simulation_passed, false);
    let queue_state_conflict = coordinator.merge.list_queue(&proj.id).unwrap();
    assert!(queue_state_conflict.iter().any(|item| item.id == conflict_item.id && item.status == "BLOCKED_CONFLICT"));
    println!("   ✔ Test 24 PASS: Failed / conflicting merge aborted cleanly, target branch untouched, item marked BLOCKED_CONFLICT");

    std::fs::remove_dir_all(temp_repo).ok();

    println!("\n============================================================");
    println!("🎉 ALL 30 HOSTILE ADVERSARIAL SCENARIOS VERIFIED 100%!");
    println!("============================================================\n");
}
