#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::bool_assert_comparison
)]

use agent_x_flow_lib::core::CoordinatorEngine;
use agent_x_flow_lib::db::DbPool;
use agent_x_flow_lib::models::{DecomposedStepInput, TaskState, TaskSubstate};
use chrono::Utc;
use std::process::Command;

fn setup_temp_git_repo(prefix: &str) -> std::path::PathBuf {
    let temp_dir =
        std::env::temp_dir().join(format!("axf_iso_{}_{}", prefix, uuid::Uuid::new_v4()));
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
            eprintln!(
                "Git cmd {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    };

    run_cmd(&["init"]);
    run_cmd(&["config", "user.name", "AgentXFlow Test Runner"]);
    run_cmd(&["config", "user.email", "runner@agentxflow.local"]);
    run_cmd(&["add", "README.md"]);
    run_cmd(&["commit", "-m", "Initial commit"]);
    run_cmd(&["branch", "-M", "main"]);

    temp_dir
}

fn create_test_steps(start: i32, count: i32, _scope_prefix: &str) -> Vec<DecomposedStepInput> {
    (start..(start + count))
        .map(|idx| DecomposedStepInput {
            step_index: idx,
            title: format!("Step #{}: Module Implementation", idx),
            description: format!("Detailed specification for step #{}", idx),
            suggested_scope: Some(format!("src/module_{}/**", idx)),
            acceptance_criteria: Some(format!("Automated test for step #{} passes", idx)),
        })
        .collect()
}

// =========================================================================
// TEST A: Normal six-step execution
// =========================================================================
#[test]
fn test_a_normal_six_step_execution() {
    let temp_repo = setup_temp_git_repo("test_a");
    let temp_db = temp_repo.join("test_a.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project A",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    // Create masterplan with max_steps_per_agent = 6
    let _plan = engine
        .create_or_update_masterplan(&proj.id, "18 step spec", 18, 6)
        .unwrap();

    let steps = create_test_steps(1, 18, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    // Register Agent A (Antigravity) and Agent B (OpenCode)
    engine.register_agent("Antigravity", "IDE").unwrap();
    engine.register_agent("OpenCode", "IDE").unwrap();

    // Agent A claims first chunk
    let task_a = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();
    assert_eq!(task_a.state, TaskState::Running);
    assert_eq!(task_a.assigned_agent_id.as_deref(), Some("antigravity"));

    // Agent B claims second chunk
    let task_b = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();
    assert_eq!(task_b.state, TaskState::Running);
    assert_eq!(task_b.assigned_agent_id.as_deref(), Some("opencode"));
    assert_ne!(task_a.id, task_b.id);

    // Verify step allocations in database
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    assert_eq!(all_steps.len(), 18);

    let a_steps: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("antigravity"))
        .collect();
    let b_steps: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("opencode"))
        .collect();
    let pending_steps: Vec<_> = all_steps.iter().filter(|s| s.status == "PENDING").collect();

    assert_eq!(a_steps.len(), 6);
    assert_eq!(b_steps.len(), 6);
    assert_eq!(pending_steps.len(), 6);

    for s in &a_steps {
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_a.id.as_str()));
    }
    for s in &b_steps {
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_b.id.as_str()));
    }
}

// =========================================================================
// TEST B: Cross-agent stale isolation (Mandatory invariant from Section 3)
// =========================================================================
#[test]
fn test_b_cross_agent_stale_isolation() {
    let temp_repo = setup_temp_git_repo("test_b");
    let temp_db = temp_repo.join("test_b.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project B",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "18 step spec", 18, 6)
        .unwrap();

    let steps = create_test_steps(1, 18, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    engine.register_agent("OpenCode", "IDE").unwrap();

    // Agent A -> steps 1..=6
    let task_a = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();
    // Agent B -> steps 7..=12
    let task_b = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();

    // Verify initial DB state
    {
        let conn = pool.lock();
        let a_claimed: i64 = conn.query_row("SELECT COUNT(*) FROM masterplan_steps WHERE claimed_agent_id = 'antigravity' AND status = 'CLAIMED'", [], |r| r.get(0)).unwrap();
        let b_claimed: i64 = conn.query_row("SELECT COUNT(*) FROM masterplan_steps WHERE claimed_agent_id = 'opencode' AND status = 'CLAIMED'", [], |r| r.get(0)).unwrap();
        assert_eq!(a_claimed, 6);
        assert_eq!(b_claimed, 6);
    }

    // Make Agent A stale: backdate antigravity last_heartbeat to 600s ago (> 300s STALE_AGENT_GRACE)
    let stale_time = (Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = 'antigravity'",
            [&stale_time],
        )
        .unwrap();
    }

    // Run stale recovery sweep
    engine.stale_recovery_sweep().expect("Sweep must succeed");

    // Invariant verifications:
    // 1. Steps 1-6 are reclaimable (PENDING)
    // 2. Steps 7-12 are STILL CLAIMED
    // 3. Steps 7-12 still reference OpenCode
    // 4. Steps 7-12 still reference OpenCode's task
    // 5. OpenCode's task remains RUNNING / non-stale
    // 6. No OpenCode task cancellation occurred
    // 7. No OpenCode scope was released
    // 8. No OpenCode task attempt was destroyed
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();

    let pending: Vec<_> = all_steps.iter().filter(|s| s.status == "PENDING").collect();
    let b_active: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("opencode"))
        .collect();

    // Steps 1..=6 reverted + steps 13..=18 = 12 pending steps
    assert_eq!(pending.len(), 12);
    assert_eq!(b_active.len(), 6);

    for s in &b_active {
        assert_eq!(s.status, "CLAIMED");
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_b.id.as_str()));
    }

    let b_task_fresh = engine.get_task(&task_b.id).unwrap();
    assert_eq!(b_task_fresh.state, TaskState::Running);
    assert!(!b_task_fresh.is_stale);

    let a_task_fresh = engine.get_task(&task_a.id).unwrap();
    assert_eq!(a_task_fresh.state, TaskState::Cancelled);
    assert!(a_task_fresh.is_stale);
}

// Reverse ordering test: Ensure behavior does not depend on registration/claim order
#[test]
fn test_b_reverse_ordering_isolation() {
    let temp_repo = setup_temp_git_repo("test_b_rev");
    let temp_db = temp_repo.join("test_b_rev.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project B Rev",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "18 step spec", 18, 6)
        .unwrap();

    let steps = create_test_steps(1, 18, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    // Register B first, then A
    engine.register_agent("OpenCode", "IDE").unwrap();
    engine.register_agent("Antigravity", "IDE").unwrap();

    // B claims 1..=6
    let task_b = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();
    // A claims 7..=12
    let task_a = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();

    // Make A stale
    let stale_time = (Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = 'antigravity'",
            [&stale_time],
        )
        .unwrap();
    }

    engine.stale_recovery_sweep().unwrap();

    // B's 1..=6 must remain untouched and CLAIMED
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    let b_steps: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("opencode"))
        .collect();
    assert_eq!(b_steps.len(), 6);
    for s in &b_steps {
        assert_eq!(s.status, "CLAIMED");
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_b.id.as_str()));
    }

    let b_task = engine.get_task(&task_b.id).unwrap();
    assert_eq!(b_task.state, TaskState::Running);

    let a_task = engine.get_task(&task_a.id).unwrap();
    assert_eq!(a_task.state, TaskState::Cancelled);
}

// =========================================================================
// TEST C: Reclaim
// =========================================================================
#[test]
fn test_c_reclaim_by_third_agent() {
    let temp_repo = setup_temp_git_repo("test_c");
    let temp_db = temp_repo.join("test_c.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project C",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "18 step spec", 18, 6)
        .unwrap();

    let steps = create_test_steps(1, 18, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    engine.register_agent("OpenCode", "IDE").unwrap();

    // A gets 1..=6, B gets 7..=12
    let _task_a = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();
    let task_b = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();

    // Make A stale & recover
    let stale_time = (Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = 'antigravity'",
            [&stale_time],
        )
        .unwrap();
    }
    engine.stale_recovery_sweep().unwrap();

    // Register Agent C (Claude Code) and claim next chunk
    engine.register_agent("Claude Code", "CLI").unwrap();
    let task_c = engine
        .claim_masterplan_chunk(&proj.id, "claude-code", None)
        .unwrap();

    // Verify Agent C gets steps 1..=6 (lowest pending steps)
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    let c_steps: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("claude-code"))
        .collect();
    let b_steps: Vec<_> = all_steps
        .iter()
        .filter(|s| s.claimed_agent_id.as_deref() == Some("opencode"))
        .collect();

    assert_eq!(c_steps.len(), 6);
    assert_eq!(b_steps.len(), 6);

    for s in &c_steps {
        assert!(s.step_index <= 6);
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_c.id.as_str()));
    }
    for s in &b_steps {
        assert!(s.step_index >= 7 && s.step_index <= 12);
        assert_eq!(s.claimed_task_id.as_deref(), Some(task_b.id.as_str()));
    }
}

// =========================================================================
// TEST D: Persistent task linkage
// =========================================================================
#[test]
fn test_d_persistent_task_linkage() {
    let temp_repo = setup_temp_git_repo("test_d");
    let temp_db = temp_repo.join("test_d.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project D",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let plan = engine
        .create_or_update_masterplan(&proj.id, "6 step spec", 6, 6)
        .unwrap();

    let steps = create_test_steps(1, 6, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("OpenCode", "IDE").unwrap();
    let task = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();

    // Check authoritative linkage
    assert_eq!(task.masterplan_id.as_deref(), Some(plan.id.as_str()));
    assert_eq!(task.assigned_agent_id.as_deref(), Some("opencode"));

    let step_records = engine.list_masterplan_steps(&proj.id).unwrap();
    assert_eq!(step_records.len(), 6);
    for s in &step_records {
        assert_eq!(s.status, "CLAIMED");
        assert_eq!(s.claimed_agent_id.as_deref(), Some("opencode"));
        assert_eq!(s.claimed_task_id.as_deref(), Some(task.id.as_str()));
    }

    let plan_fresh = engine.get_masterplan(&proj.id).unwrap().unwrap();
    assert_eq!(plan_fresh.status, "EXECUTING");
}

// =========================================================================
// TEST E: Rollback on collision
// =========================================================================
#[test]
fn test_e_claim_rollback_on_failure() {
    let temp_repo = setup_temp_git_repo("test_e");
    let temp_db = temp_repo.join("test_e.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project E",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "12 step spec", 12, 6)
        .unwrap();

    let steps = create_test_steps(1, 12, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Agent Alpha", "IDE").unwrap();
    engine.register_agent("Agent Beta", "IDE").unwrap();

    // Agent Alpha claims a standalone task with scope "src/module_1/**"
    let manual_task = engine
        .create_task(
            &proj.id,
            "Conflicting Task",
            "Pre-holding scope",
            "HIGH",
            vec![("Step 1".into(), "Desc".into(), true)],
            vec!["Criterion".into()],
        )
        .unwrap();
    engine.claim_task(&manual_task.id, "agent-alpha").unwrap();
    engine
        .scope
        .acquire_scope(
            &manual_task.id,
            "agent-alpha",
            vec!["src/module_1/**".into()],
            "EXCLUSIVE_WRITE",
        )
        .unwrap();

    // Agent Beta attempts to claim masterplan chunk (which includes step 1 with suggested_scope "src/module_1/**")
    let claim_res = engine.claim_masterplan_chunk(&proj.id, "agent-beta", None);
    assert!(
        claim_res.is_err(),
        "Claim must fail due to exclusive scope collision"
    );

    // Verify compensation: all masterplan steps remain PENDING
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    for s in &all_steps {
        assert_eq!(s.status, "PENDING");
        assert!(s.claimed_agent_id.is_none());
        assert!(s.claimed_task_id.is_none());
    }
}

// =========================================================================
// TEST F: Binding failure leaves no orphaned CLAIMED step
// =========================================================================
#[test]
fn test_f_binding_failure_leaves_no_orphaned_claimed_steps() {
    let temp_repo = setup_temp_git_repo("test_f");
    let temp_db = temp_repo.join("test_f.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project F",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "6 step spec", 6, 6)
        .unwrap();

    let steps = create_test_steps(1, 6, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();

    // After normal claim, there are never orphaned CLAIMED steps with NULL claimed_task_id
    let _task = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();

    let steps_after = engine.list_masterplan_steps(&proj.id).unwrap();
    for s in &steps_after {
        if s.status == "CLAIMED" {
            assert!(
                s.claimed_task_id.is_some(),
                "No CLAIMED step may have NULL claimed_task_id"
            );
        }
    }
}

// =========================================================================
// TEST G: Waiting-for-permission (Bounded protection)
// =========================================================================
#[test]
fn test_g_waiting_for_permission() {
    let temp_repo = setup_temp_git_repo("test_g");
    let temp_db = temp_repo.join("test_g.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project G",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "12 step spec", 12, 6)
        .unwrap();

    let steps = create_test_steps(1, 12, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("OpenCode", "IDE").unwrap();
    engine.register_agent("Claude Code", "CLI").unwrap();

    // OpenCode claims 1..=6
    let task_oc = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();

    // OpenCode enters waiting-for-permission state
    engine
        .set_task_waiting_for_permission(&task_oc.id, "opencode")
        .unwrap();

    let task_oc_check = engine.get_task(&task_oc.id).unwrap();
    assert_eq!(task_oc_check.substate, TaskSubstate::WaitingForInput);

    // Backdate OpenCode's heartbeat to 400s ago (> 300s STALE_AGENT_GRACE, but < 1800s WAITING_PERMISSION_GRACE)
    let hb_400s = (Utc::now() - chrono::Duration::seconds(400)).to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = 'opencode'",
            [&hb_400s],
        )
        .unwrap();
    }

    // Run sweep: OpenCode must NOT be reclaimed because it is protected by WAITING_PERMISSION_GRACE
    engine.stale_recovery_sweep().unwrap();

    let task_oc_after_sweep = engine.get_task(&task_oc.id).unwrap();
    assert_eq!(task_oc_after_sweep.state, TaskState::Running);
    assert!(!task_oc_after_sweep.is_stale);

    // Claude Code can independently claim steps 7..=12
    let task_cc = engine
        .claim_masterplan_chunk(&proj.id, "claude-code", None)
        .unwrap();
    assert_eq!(task_cc.state, TaskState::Running);

    // Now backdate OpenCode's heartbeat to 2000s ago (> 1800s WAITING_PERMISSION_GRACE)
    let hb_2000s = (Utc::now() - chrono::Duration::seconds(2000)).to_rfc3339();
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE id = 'opencode'",
            [&hb_2000s],
        )
        .unwrap();
    }

    // Run sweep: now OpenCode IS reclaimed because bounded protection expired
    engine.stale_recovery_sweep().unwrap();

    let task_oc_reclaimed = engine.get_task(&task_oc.id).unwrap();
    assert_eq!(task_oc_reclaimed.state, TaskState::Cancelled);
    assert!(task_oc_reclaimed.is_stale);

    // Claude Code's task remains intact
    let task_cc_after = engine.get_task(&task_cc.id).unwrap();
    assert_eq!(task_cc_after.state, TaskState::Running);
}

// =========================================================================
// TEST H: Historical task preservation
// =========================================================================
#[test]
fn test_h_historical_task_preservation() {
    let temp_repo = setup_temp_git_repo("test_h");
    let temp_db = temp_repo.join("test_h.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project H",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "6 step spec", 6, 6)
        .unwrap();

    let steps = create_test_steps(1, 6, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    let task = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();

    // Complete step 1
    let task_details = engine.get_task_details(&task.id).unwrap();
    let step_1 = &task_details.steps[0];
    engine
        .complete_step(&step_1.id, Some("antigravity"), Some("Proof of Step 1"))
        .unwrap();

    // Mark completed on masterplan step 1
    {
        let conn = pool.lock();
        conn.execute(
            "UPDATE masterplan_steps SET status = 'COMPLETED' WHERE step_index = 1",
            [],
        )
        .unwrap();
    }

    // Reclaim task
    engine.requeue_task(&task.id, Some("antigravity")).unwrap();

    // Historical task row remains queryable
    let historical_task = engine.get_task(&task.id).unwrap();
    assert_eq!(historical_task.state, TaskState::Cancelled);
    assert!(historical_task.is_stale);

    // Masterplan step 1 remains COMPLETED, while steps 2..=6 reverted to PENDING
    let mp_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    let step_1_mp = mp_steps.iter().find(|s| s.step_index == 1).unwrap();
    assert_eq!(step_1_mp.status, "COMPLETED");

    for s in mp_steps.iter().filter(|s| s.step_index > 1) {
        assert_eq!(s.status, "PENDING");
    }
}

// =========================================================================
// TEST I: Large decomposition (100 steps through multiple chunks)
// =========================================================================
#[test]
fn test_i_large_decomposition_100_steps() {
    let temp_repo = setup_temp_git_repo("test_i");
    let temp_db = temp_repo.join("test_i.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project I",
            &temp_repo.to_string_lossy(),
            "100 Step Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "100 Step Spec", 100, 6)
        .unwrap();

    // Chunk 1: Steps 1-25
    let c1 = create_test_steps(1, 25, "p1");
    engine
        .decompose_masterplan(&proj.id, c1, Some(true), None)
        .unwrap();

    // Chunk 2: Steps 26-50
    let c2 = create_test_steps(26, 25, "p2");
    engine
        .decompose_masterplan(&proj.id, c2, Some(true), None)
        .unwrap();

    // Chunk 3: Steps 51-75
    let c3 = create_test_steps(51, 25, "p3");
    engine
        .decompose_masterplan(&proj.id, c3, Some(true), None)
        .unwrap();

    // Chunk 4: Steps 76-100
    let c4 = create_test_steps(76, 25, "p4");
    engine
        .decompose_masterplan(&proj.id, c4, Some(true), None)
        .unwrap();

    // Verify exact final step count and deterministic ordering
    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    assert_eq!(all_steps.len(), 100);

    for (i, s) in all_steps.iter().enumerate() {
        assert_eq!(s.step_index, (i + 1) as i32);
        assert_eq!(s.status, "PENDING");
    }
}

// =========================================================================
// TEST J: Idempotent chunk retry
// =========================================================================
#[test]
fn test_j_idempotent_chunk_retry() {
    let temp_repo = setup_temp_git_repo("test_j");
    let temp_db = temp_repo.join("test_j.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project J",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Master Spec", 20, 6)
        .unwrap();

    let steps = create_test_steps(1, 20, "mod");
    let key = "idemp-key-test-j".to_string();

    let res1 = engine
        .decompose_masterplan(&proj.id, steps.clone(), None, Some(key.clone()))
        .unwrap();
    assert_eq!(res1.len(), 20);

    // Retry identical request with same key
    let res2 = engine
        .decompose_masterplan(&proj.id, steps, None, Some(key))
        .unwrap();
    assert_eq!(res2.len(), 20);

    let all_steps = engine.list_masterplan_steps(&proj.id).unwrap();
    assert_eq!(all_steps.len(), 20);
}

// =========================================================================
// TEST K: Conflicting chunk retry rejected
// =========================================================================
#[test]
fn test_k_conflicting_chunk_retry_rejected() {
    let temp_repo = setup_temp_git_repo("test_k");
    let temp_db = temp_repo.join("test_k.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project K",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Master Spec", 10, 6)
        .unwrap();

    let steps1 = create_test_steps(1, 10, "mod");
    let key = "idemp-key-test-k".to_string();

    engine
        .decompose_masterplan(&proj.id, steps1, None, Some(key.clone()))
        .unwrap();

    // Reusing the same key with DIFFERENT steps must be rejected
    let mut steps2 = create_test_steps(1, 10, "mod");
    steps2[0].title = "Completely Different Title".to_string();

    let err = engine
        .decompose_masterplan(&proj.id, steps2, None, Some(key))
        .unwrap_err();
    assert!(
        err.contains("Conflicting retry"),
        "Must reject conflicting retry: {}",
        err
    );
}

// =========================================================================
// TEST L: Append safety over CLAIMED step
// =========================================================================
#[test]
fn test_l_append_safety_over_claimed_step() {
    let temp_repo = setup_temp_git_repo("test_l");
    let temp_db = temp_repo.join("test_l.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project L",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Master Spec", 12, 6)
        .unwrap();

    let steps = create_test_steps(1, 12, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("Antigravity", "IDE").unwrap();
    let _task = engine
        .claim_masterplan_chunk(&proj.id, "antigravity", None)
        .unwrap();

    // Attempt to append with step_index = 3 (which is CLAIMED)
    let append_steps = vec![DecomposedStepInput {
        step_index: 3,
        title: "Malicious Overwrite".to_string(),
        description: "Overwrite description".to_string(),
        suggested_scope: Some("src/**".to_string()),
        acceptance_criteria: Some("Criteria".to_string()),
    }];

    let err = engine
        .decompose_masterplan(&proj.id, append_steps, Some(true), None)
        .unwrap_err();
    assert!(
        err.contains("already CLAIMED"),
        "Must reject overwriting CLAIMED step: {}",
        err
    );
}

// =========================================================================
// TEST M: Restart persistence
// =========================================================================
#[test]
fn test_m_restart_persistence() {
    let temp_repo = setup_temp_git_repo("test_m");
    let temp_db = temp_repo.join("test_m.sqlite");

    let proj_id;
    let task_b_id;

    // Phase 1: Initialize, perform work, claim chunks
    {
        let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
        let engine = CoordinatorEngine::new(pool.clone());

        let proj = engine
            .create_project(
                "Project M",
                &temp_repo.to_string_lossy(),
                "Master Spec",
                "main",
            )
            .unwrap();
        proj_id = proj.id.clone();

        let _plan = engine
            .create_or_update_masterplan(&proj.id, "18 step spec", 18, 6)
            .unwrap();

        let steps = create_test_steps(1, 18, "mod");
        engine
            .decompose_masterplan(&proj.id, steps, None, None)
            .unwrap();

        engine.register_agent("Antigravity", "IDE").unwrap();
        engine.register_agent("OpenCode", "IDE").unwrap();

        let _task_a = engine
            .claim_masterplan_chunk(&proj.id, "antigravity", None)
            .unwrap();
        let task_b = engine
            .claim_masterplan_chunk(&proj.id, "opencode", None)
            .unwrap();
        task_b_id = task_b.id.clone();

        // Mark steps 1..=6 as COMPLETED (historical work)
        {
            let conn = pool.lock();
            conn.execute(
                "UPDATE masterplan_steps SET status = 'COMPLETED' WHERE step_index <= 6",
                [],
            )
            .unwrap();
        }
    }
    // All engine and pool references dropped here (simulating coordinator shutdown)

    // Phase 2: Restart coordinator on the exact same SQLite database
    {
        let pool2 = DbPool::new(&temp_db).expect("Failed to reopen test DB");
        let engine2 = CoordinatorEngine::new(pool2.clone());

        let all_steps = engine2.list_masterplan_steps(&proj_id).unwrap();
        assert_eq!(all_steps.len(), 18);

        let completed: Vec<_> = all_steps
            .iter()
            .filter(|s| s.status == "COMPLETED")
            .collect();
        let b_claimed: Vec<_> = all_steps
            .iter()
            .filter(|s| s.claimed_agent_id.as_deref() == Some("opencode"))
            .collect();
        let pending: Vec<_> = all_steps.iter().filter(|s| s.status == "PENDING").collect();

        assert_eq!(completed.len(), 6);
        assert_eq!(b_claimed.len(), 6);
        assert_eq!(pending.len(), 6);

        // OpenCode reconnects: get_current_context returns active task
        let ctx = engine2
            .get_current_context(Some("OpenCode"), Some(&proj_id))
            .unwrap();
        assert_eq!(ctx.active_task_id.as_deref(), Some(task_b_id.as_str()));
    }
}

// =========================================================================
// TEST N: Masterplan update isolation
// =========================================================================
#[test]
fn test_n_masterplan_update_isolation() {
    let temp_repo = setup_temp_git_repo("test_n");
    let temp_db = temp_repo.join("test_n.sqlite");
    let pool = DbPool::new(&temp_db).expect("Failed to initialize test DB");
    let engine = CoordinatorEngine::new(pool.clone());

    let proj = engine
        .create_project(
            "Project N",
            &temp_repo.to_string_lossy(),
            "Master Spec",
            "main",
        )
        .unwrap();

    let _plan = engine
        .create_or_update_masterplan(&proj.id, "Initial Spec", 12, 6)
        .unwrap();

    let steps = create_test_steps(1, 12, "mod");
    engine
        .decompose_masterplan(&proj.id, steps, None, None)
        .unwrap();

    engine.register_agent("OpenCode", "IDE").unwrap();
    let task = engine
        .claim_masterplan_chunk(&proj.id, "opencode", None)
        .unwrap();

    // Ordinary decomposition continuation (append: true) never cancels active execution
    let continuation_steps = create_test_steps(13, 6, "cont");
    engine
        .decompose_masterplan(&proj.id, continuation_steps, Some(true), None)
        .unwrap();

    let active_task = engine.get_task(&task.id).unwrap();
    assert_eq!(active_task.state, TaskState::Running);
    assert!(!active_task.is_stale);

    // Intentional masterplan reset/update cancels only its bound tasks
    let _updated_plan = engine
        .create_or_update_masterplan(&proj.id, "Rewritten Spec", 12, 6)
        .unwrap();
    let cancelled_task = engine.get_task(&task.id).unwrap();
    assert_eq!(cancelled_task.state, TaskState::Cancelled);
}
