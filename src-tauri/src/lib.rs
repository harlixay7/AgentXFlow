pub mod acp;
pub mod core;
pub mod dag;
pub mod db;
pub mod error;
pub mod git;
pub mod mcp;
pub mod merge;
pub mod models;
pub mod policies;
pub mod scheduler;
pub mod scope;
pub mod security;
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
    MasterplanStep, MergeQueueItem, Project, ScopeLease, Task, TaskDependency, TaskDetails,
    TaskStep, VerificationResult,
};
use crate::security::SecurityManager;

pub struct AppState {
    pub coordinator: CoordinatorEngine,
    pub security: SecurityManager,
    pub mcp_port: u16,
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
    let token = state.security.get_token();
    Ok(serde_json::json!({
        "url": format!("http://127.0.0.1:{}/mcp", state.mcp_port),
        "sse_url": format!("http://127.0.0.1:{}/mcp/sse", state.mcp_port),
        "token": token,
        "protocol_version": "2026-07-28",
    }))
}

#[tauri::command]
fn rotate_mcp_token(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    state.security.rotate_token().map_err(|e| e.to_string())
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
fn create_example_project(
    state: State<'_, Arc<AppState>>,
    root_dir: String,
) -> Result<Project, String> {
    state.coordinator.create_example_project(&root_dir)
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
fn get_task_details(state: State<'_, Arc<AppState>>, task_id: String) -> Result<TaskDetails, String> {
    state.coordinator.get_task_details(&task_id)
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
fn enqueue_task_by_id(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    task_id: String,
) -> Result<MergeQueueItem, String> {
    state.coordinator.enqueue_task_by_id(&project_id, &task_id)
}

#[tauri::command]
fn satisfy_acceptance_criterion(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    criterion_id: String,
    evidence: Option<String>,
) -> Result<(), String> {
    state.coordinator.satisfy_acceptance_criterion(&task_id, &criterion_id, evidence.as_deref())
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
fn process_merge_by_id(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    queue_item_id: String,
) -> Result<IntegrationAttempt, String> {
    let proj = state.coordinator.list_projects()?.into_iter().find(|p| p.id == project_id).ok_or("Project not found")?;
    state.coordinator.merge.process_merge_by_id(&queue_item_id, Path::new(&proj.path))
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
    let data_dir = dirs_next::data_dir()
        .map(|p| p.join("AgentXFlow"))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    std::fs::create_dir_all(&data_dir).ok();
    let db_path = data_dir.join("agentxflow_v2.db");

    let db_pool = DbPool::new(&db_path).expect("Failed to initialize SQLite connection pool");
    let coordinator = CoordinatorEngine::new(db_pool);

    let security = SecurityManager::init_or_load(&data_dir).expect("Failed to initialize security manager");
    let mcp_port = 7890;

    let mcp_server = McpServer::new(coordinator.clone(), mcp_port, security.clone());

    tauri::async_runtime::spawn(async move {
        if let Err(e) = mcp_server.start().await {
            eprintln!("Failed to start MCP server: {}", e);
        }
    });

    let app_state = Arc::new(AppState {
        coordinator,
        security,
        mcp_port,
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            inspect_repository,
            get_mcp_info,
            rotate_mcp_token,
            create_project,
            create_example_project,
            list_projects,
            create_task,
            get_task,
            get_task_details,
            list_tasks,
            claim_task,
            complete_step,
            satisfy_acceptance_criterion,
            submit_task,
            request_scope,
            calculate_collision_risk,
            add_task_dependency,
            get_task_dependencies,
            list_merge_queue,
            enqueue_task_for_merge,
            enqueue_task_by_id,
            process_merge_candidate,
            process_merge_by_id,
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
