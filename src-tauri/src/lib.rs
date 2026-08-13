pub mod acp;
pub mod core;
pub mod dag;
pub mod db;
pub mod git;
pub mod mcp;
pub mod merge;
pub mod models;
pub mod policies;
pub mod scheduler;
pub mod scope;
pub mod verification;

use std::path::Path;
use std::sync::Arc;
use tauri::State;

use crate::core::CoordinatorEngine;
use crate::db::DbPool;
use crate::git::RepoInspectionResult;
use crate::mcp::McpServer;
use crate::models::{
    Agent, CollisionRisk, ContextPack, EventItem, IntegrationAttempt, Masterplan,
    MasterplanStep, MergeQueueItem, Project, ScopeLease, Task, TaskDependency, TaskStep,
    VerificationResult,
};

pub struct AppState {
    pub coordinator: CoordinatorEngine,
    pub mcp_port: u16,
    pub mcp_token: String,
}

#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select Project / Git Repository Directory")
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn inspect_repository(state: State<'_, Arc<AppState>>, path: String) -> RepoInspectionResult {
    state.coordinator.git.inspect_repository(Path::new(&path))
}

#[tauri::command]
fn get_mcp_info(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "url": format!("http://127.0.0.1:{}/mcp", state.mcp_port),
        "sse_url": format!("http://127.0.0.1:{}/mcp/sse", state.mcp_port),
        "token": state.mcp_token,
        "protocol_version": "2026-07-28",
    }))
}

#[tauri::command]
fn create_project(
    state: State<'_, Arc<AppState>>,
    name: String,
    path: String,
    master_spec: String,
    target_branch: String,
) -> Result<Project, String> {
    state.coordinator.create_project(&name, &path, &master_spec, &target_branch)
}

#[tauri::command]
fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>, String> {
    state.coordinator.list_projects()
}

#[tauri::command]
fn create_task(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    title: String,
    description: String,
    priority: String,
    steps: Vec<(String, String, bool)>,
    criteria: Vec<String>,
) -> Result<Task, String> {
    state.coordinator.create_task(&project_id, &title, &description, &priority, steps, criteria)
}

#[tauri::command]
fn get_task(state: State<'_, Arc<AppState>>, task_id: String) -> Result<Task, String> {
    state.coordinator.get_task(&task_id)
}

#[tauri::command]
fn list_tasks(state: State<'_, Arc<AppState>>, project_id: String) -> Result<Vec<Task>, String> {
    state.coordinator.list_tasks(&project_id)
}

#[tauri::command]
fn claim_task(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    agent_id: String,
) -> Result<Task, String> {
    state.coordinator.claim_task(&task_id, &agent_id)
}

#[tauri::command]
fn complete_step(
    state: State<'_, Arc<AppState>>,
    step_id: String,
    evidence_json: Option<String>,
) -> Result<TaskStep, String> {
    state.coordinator.complete_step(&step_id, evidence_json.as_deref())
}

#[tauri::command]
fn submit_task(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    agent_id: String,
) -> Result<VerificationResult, String> {
    state.coordinator.submit_task(&task_id, &agent_id)
}

#[tauri::command]
fn request_scope(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    agent_id: String,
    patterns: Vec<String>,
) -> Result<Vec<ScopeLease>, String> {
    state.coordinator.scope.acquire_scope(&task_id, &agent_id, patterns, "EXCLUSIVE_WRITE")
}

#[tauri::command]
fn calculate_collision_risk(
    state: State<'_, Arc<AppState>>,
    task_a_id: String,
    task_b_id: String,
) -> Result<CollisionRisk, String> {
    state.coordinator.scope.calculate_collision_risk(&task_a_id, &task_b_id)
}

#[tauri::command]
fn add_task_dependency(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    depends_on_task_id: String,
    dependency_type: String,
) -> Result<TaskDependency, String> {
    state.coordinator.dag.add_dependency(&task_id, &depends_on_task_id, &dependency_type)
}

#[tauri::command]
fn get_task_dependencies(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<Vec<TaskDependency>, String> {
    state.coordinator.dag.get_dependencies_for_task(&task_id)
}

#[tauri::command]
fn list_merge_queue(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Vec<MergeQueueItem>, String> {
    state.coordinator.merge.list_queue(&project_id)
}

#[tauri::command]
fn enqueue_task_for_merge(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    task_id: String,
    branch_name: String,
    target_branch: String,
    base_sha: String,
    head_sha: String,
) -> Result<MergeQueueItem, String> {
    state.coordinator.merge.enqueue_task(&project_id, &task_id, &branch_name, &target_branch, &base_sha, &head_sha)
}

#[tauri::command]
fn process_merge_candidate(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    item: MergeQueueItem,
) -> Result<IntegrationAttempt, String> {
    let proj = state.coordinator.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;
    state.coordinator.merge.process_merge(&project_id, Path::new(&proj.path), &item)
}

#[tauri::command]
fn get_events_after(
    state: State<'_, Arc<AppState>>,
    last_sequence: i64,
) -> Result<Vec<EventItem>, String> {
    state.coordinator.get_events_after(last_sequence)
}

#[tauri::command]
fn get_context_pack(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    task_id: String,
) -> Result<ContextPack, String> {
    state.coordinator.get_context_pack(&project_id, &task_id)
}

#[tauri::command]
fn register_agent(
    state: State<'_, Arc<AppState>>,
    name: String,
    agent_type: String,
) -> Result<Agent, String> {
    state.coordinator.register_agent(&name, &agent_type)
}

#[tauri::command]
fn unregister_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<(), String> {
    state.coordinator.unregister_agent(&agent_id)
}

#[tauri::command]
fn list_agents(state: State<'_, Arc<AppState>>) -> Result<Vec<Agent>, String> {
    state.coordinator.list_agents()
}

#[tauri::command]
fn create_or_update_masterplan(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    raw_text: String,
    target_step_count: i32,
    max_steps_per_agent: i32,
) -> Result<Masterplan, String> {
    state.coordinator.create_or_update_masterplan(&project_id, &raw_text, target_step_count, max_steps_per_agent)
}

#[tauri::command]
fn get_masterplan(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Option<Masterplan>, String> {
    state.coordinator.get_masterplan(&project_id)
}

#[tauri::command]
fn list_masterplan_steps(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Vec<MasterplanStep>, String> {
    state.coordinator.list_masterplan_steps(&project_id)
}

#[tauri::command]
fn decompose_masterplan(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    steps: Vec<crate::models::DecomposedStepInput>,
) -> Result<Vec<MasterplanStep>, String> {
    state.coordinator.decompose_masterplan(&project_id, steps)
}

#[tauri::command]
fn claim_masterplan_chunk(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    agent_id: String,
    count: Option<i32>,
) -> Result<Task, String> {
    state.coordinator.claim_masterplan_chunk(&project_id, &agent_id, count)
}

#[tauri::command]
fn reset_masterplan(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<(), String> {
    state.coordinator.reset_masterplan(&project_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_path = dirs_next::data_dir()
        .map(|p| p.join("AgentXFlow").join("agentxflow_v2.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("agentxflow_v2.db"));

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let db_pool = DbPool::new(&db_path).expect("Failed to initialize SQLite connection pool");
    let coordinator = CoordinatorEngine::new(db_pool);

    let mcp_port = 7890;
    let mcp_token = "axf_sec_v2_live_token_7890".to_string();

    let mcp_server = McpServer::new(coordinator.clone(), mcp_port, mcp_token.clone());

    tauri::async_runtime::spawn(async move {
        if let Err(e) = mcp_server.start().await {
            eprintln!("Failed to start MCP server: {}", e);
        }
    });

    let app_state = Arc::new(AppState {
        coordinator,
        mcp_port,
        mcp_token,
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            inspect_repository,
            get_mcp_info,
            create_project,
            list_projects,
            create_task,
            get_task,
            list_tasks,
            claim_task,
            complete_step,
            submit_task,
            request_scope,
            calculate_collision_risk,
            add_task_dependency,
            get_task_dependencies,
            list_merge_queue,
            enqueue_task_for_merge,
            process_merge_candidate,
            get_events_after,
            get_context_pack,
            register_agent,
            unregister_agent,
            list_agents,
            create_or_update_masterplan,
            get_masterplan,
            list_masterplan_steps,
            decompose_masterplan,
            claim_masterplan_chunk,
            reset_masterplan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
