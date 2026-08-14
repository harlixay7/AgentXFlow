use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use std::process::Command;

fn setup_temp_git_repo() -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("axf_git_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let readme = temp_dir.join("README.md");
    std::fs::write(&readme, "# Test Repo\n").unwrap();

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
    run_cmd(&["config", "user.name", "AgentXFlow Test"]);
    run_cmd(&["config", "user.email", "test@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

#[tokio::test]
async fn test_multi_agent_concurrent_collaboration_and_merge() {
    println!("============================================================");
    println!("🚀 STARTING MULTI-AGENT CONCURRENT COLLABORATION TEST");
    println!("============================================================\n");

    // 1. Setup Temp Git Repo and Coordinator SQLite DB
    let temp_repo = setup_temp_git_repo();
    let temp_db = temp_repo.join("test_db.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test SQLite DB");
    let coordinator = CoordinatorEngine::new(pool.clone());

    let proj = coordinator.create_project(
        "Multi-Agent Concurrent Project",
        &temp_repo.to_string_lossy(),
        "Specification for 3 concurrent agents",
        "main",
    ).expect("Failed to create project");
    let project_id = &proj.id;

    // 2. Register 3 Concurrent AI Agents / IDEs
    println!("▶ Step 1: Registering 3 Autonomous Agents (Antigravity, Claude, Cursor)...");
    let agent_antigravity = coordinator
        .register_agent("Antigravity-Lead", "Antigravity")
        .expect("Failed to register Antigravity");
    let agent_claude = coordinator
        .register_agent("Claude-Code-Backend", "Claude")
        .expect("Failed to register Claude");
    let agent_cursor = coordinator
        .register_agent("Cursor-Frontend", "Cursor")
        .expect("Failed to register Cursor");

    println!("   ✔ Registered Agent 1: {} [{}]", agent_antigravity.name, agent_antigravity.id);
    println!("   ✔ Registered Agent 2: {} [{}]", agent_claude.name, agent_claude.id);
    println!("   ✔ Registered Agent 3: {} [{}]", agent_cursor.name, agent_cursor.id);

    // 3. Create 3 Distinct Engineering Tasks with Required Steps
    println!("\n▶ Step 2: Creating 3 Concurrent Engineering Tasks in Backlog...");
    let t1 = coordinator
        .create_task(
            project_id,
            "Implement Auth Tokens",
            "Build JWT authentication and token verification",
            "HIGH",
            vec![
                ("Implement JWT".into(), "Token verification logic".into(), true),
                ("Run Unit Tests".into(), "Test execution".into(), true),
            ],
            vec!["JWT signed and verified".into()],
        )
        .expect("Failed to create Task 1");

    let t2 = coordinator
        .create_task(
            project_id,
            "Implement Database Schema",
            "Create database connection pooling and migrations",
            "HIGH",
            vec![
                ("Migrate Schema".into(), "Run migration runner".into(), true),
                ("Run DB Tests".into(), "Pool verification".into(), true),
            ],
            vec!["Migrations execute cleanly".into()],
        )
        .expect("Failed to create Task 2");

    let t3 = coordinator
        .create_task(
            project_id,
            "Build Web UI Components",
            "Create responsive task board and theme controls",
            "MEDIUM",
            vec![
                ("Build Components".into(), "Layout assembly".into(), true),
                ("Run UI Tests".into(), "Component rendering tests".into(), true),
            ],
            vec!["UI renders with 0 errors".into()],
        )
        .expect("Failed to create Task 3");

    println!("   ✔ Created Task 1: {} (Scope: src/auth/**)", t1.id);
    println!("   ✔ Created Task 2: {} (Scope: src/db/**)", t2.id);
    println!("   ✔ Created Task 3: {} (Scope: src/ui/**)", t3.id);

    // 4. Concurrently Claim Tasks & Allocate Isolated Git Worktrees
    println!("\n▶ Step 3: Agents Concurrently Claiming Tasks and Allocating Worktrees...");
    let claimed1 = coordinator.claim_task(&t1.id, &agent_antigravity.id).expect("Antigravity claim failed");
    let claimed2 = coordinator.claim_task(&t2.id, &agent_claude.id).expect("Claude claim failed");
    let claimed3 = coordinator.claim_task(&t3.id, &agent_cursor.id).expect("Cursor claim failed");

    assert_eq!(claimed1.assigned_agent_id, Some(agent_antigravity.id.clone()));
    assert_eq!(claimed2.assigned_agent_id, Some(agent_claude.id.clone()));
    assert_eq!(claimed3.assigned_agent_id, Some(agent_cursor.id.clone()));
    println!("   ✔ Task 1 claimed by Antigravity (Branch: {:?})", claimed1.branch_name);
    println!("   ✔ Task 2 claimed by Claude Code (Branch: {:?})", claimed2.branch_name);
    println!("   ✔ Task 3 claimed by Cursor IDE (Branch: {:?})", claimed3.branch_name);

    // 5. Exclusive Write Scope Allocation & Conflict Detection
    println!("\n▶ Step 4: Allocating Exclusive File Scopes & Testing Collision Risk Engine...");
    let lease1 = coordinator
        .scope
        .acquire_scope(&t1.id, &agent_antigravity.id, vec!["src/auth/**".into()], "EXCLUSIVE_WRITE")
        .expect("Failed to acquire scope 1");
    let lease2 = coordinator
        .scope
        .acquire_scope(&t2.id, &agent_claude.id, vec!["src/db/**".into()], "EXCLUSIVE_WRITE")
        .expect("Failed to acquire scope 2");
    let lease3 = coordinator
        .scope
        .acquire_scope(&t3.id, &agent_cursor.id, vec!["src/ui/**".into()], "EXCLUSIVE_WRITE")
        .expect("Failed to acquire scope 3");

    assert_eq!(lease1.len(), 1);
    assert_eq!(lease2.len(), 1);
    assert_eq!(lease3.len(), 1);
    println!("   ✔ 3 Non-overlapping write scope leases granted concurrently.");

    // Collision Risk Calculation
    let collision_risk = coordinator.scope.calculate_collision_risk(&t1.id, &t2.id).unwrap();
    println!("   ✔ Calculated Collision Risk between Task 1 & Task 2: Score = {} (Clean: 0.0)", collision_risk.risk_score);
    assert_eq!(collision_risk.risk_score, 0.0);

    // 6. Complete Steps & Satisfy Criteria for All 3 Tasks
    println!("\n▶ Step 5: Agents Executing Steps & Attaching Verification Evidence...");
    let get_task_step_ids = |task_id: &str| -> Vec<String> {
        let conn = pool.lock();
        let mut stmt = conn.prepare("SELECT id FROM task_steps WHERE task_id = ?1 ORDER BY order_index ASC").unwrap();
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0)).unwrap();
        rows.into_iter().map(|r| r.unwrap()).collect()
    };

    for tid in [&t1.id, &t2.id, &t3.id] {
        let step_ids = get_task_step_ids(tid);
        for step_id in step_ids {
            let s = coordinator.complete_step(&step_id, Some(r#"{"stdout": "check ok", "exit_code": 0}"#)).unwrap();
            assert_eq!(s.status, "COMPLETED");
        }
        let details = coordinator.get_task_details(tid).unwrap();
        for crit in &details.criteria {
            coordinator.satisfy_acceptance_criterion(tid, &crit.id, Some("Automated test pass")).unwrap();
        }
        let wt_path = std::path::PathBuf::from(details.task.worktree_path.unwrap());
        let head = coordinator.git.get_worktree_head_sha(&wt_path).unwrap();
        coordinator.verify.execute_check(
            tid,
            "chk-1",
            "Unit Tests",
            &wt_path,
            &head,
            "cmd /c exit 0",
        ).unwrap();
    }
    println!("   ✔ All mandatory steps marked COMPLETED, criteria satisfied, and coordinator checks passed.");

    // 7. Authoritative Task Submissions
    println!("\n▶ Step 6: Submitting Tasks to Authoritative Verification Engine...");
    let verify1 = coordinator.submit_task(&t1.id, &agent_antigravity.id).expect("Verification 1 failed");
    let verify2 = coordinator.submit_task(&t2.id, &agent_claude.id).expect("Verification 2 failed");
    let verify3 = coordinator.submit_task(&t3.id, &agent_cursor.id).expect("Verification 3 failed");

    assert!(verify1.is_valid, "Task 1 verification failed: {:?}", verify1.rejection_reasons);
    assert!(verify2.is_valid, "Task 2 verification failed: {:?}", verify2.rejection_reasons);
    assert!(verify3.is_valid, "Task 3 verification failed: {:?}", verify3.rejection_reasons);
    println!("   ✔ Task 1 Verified & Sealed with SHA-256 Proof Bundle");
    println!("   ✔ Task 2 Verified & Sealed with SHA-256 Proof Bundle");
    println!("   ✔ Task 3 Verified & Sealed with SHA-256 Proof Bundle");

    // 8. Serialized Merge Queue Processing (FIFO Order)
    println!("\n▶ Step 7: Enqueueing Tasks into Serialized Merge Queue...");
    let q1 = coordinator.merge.enqueue_task(project_id, &t1.id, "agentxflow/task-1", "main", "sha-base", "sha-head-1").unwrap();
    let q2 = coordinator.merge.enqueue_task(project_id, &t2.id, "agentxflow/task-2", "main", "sha-base", "sha-head-2").unwrap();
    let q3 = coordinator.merge.enqueue_task(project_id, &t3.id, "agentxflow/task-3", "main", "sha-base", "sha-head-3").unwrap();

    assert_eq!(q1.position, 1);
    assert_eq!(q2.position, 2);
    assert_eq!(q3.position, 3);
    println!("   ✔ Merge Queue Position #1: Task {}", t1.id);
    println!("   ✔ Merge Queue Position #2: Task {}", t2.id);
    println!("   ✔ Merge Queue Position #3: Task {}", t3.id);

    // 9. Inspect Merge Queue
    let queue_items = coordinator.merge.list_queue(project_id).unwrap();
    assert_eq!(queue_items.len(), 3);
    println!("   ✔ Verified 3 tasks ordered in Serialized Merge Queue.");

    // Clean up
    let _ = std::fs::remove_dir_all(temp_repo);

    println!("\n============================================================");
    println!("🎉 ALL 3 CONCURRENT AGENTS COMPLETED & VERIFIED SUCCESSFULLY!");
    println!("============================================================");
}
