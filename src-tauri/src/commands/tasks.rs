//! Tauri commands for task management
//!
//! Tasks are stored as MARKDOWN FILES in repositories.
//! Commands read/write to those files directly.

use crate::tasks::{
    CreateTaskInput, Repository, Task, TaskFilters, TaskService, TaskStatus, UpdateTaskInput,
};
use crate::workspace::{WorkspaceContext, WorkspaceDetector};
use crate::claude::{invoke::InvocationResult, prompt::PromptBuilder, api_server};
use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{State, Manager};
use tokio::sync::RwLock;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

// Helper to get app handle (this is a bit hacky, normally we'd pass it in)
// But since this is a command, we can add AppHandle to the arguments
fn try_get_app_handle() -> Option<tauri::AppHandle> {
    // This function is a placeholder. 
    // In reality, we need to update the command signature to accept AppHandle.
    None 
}

/// Shared task service state
pub struct TaskServiceState {
    inner: Arc<RwLock<Option<Arc<TaskService>>>>,
}

impl TaskServiceState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_or_init(&self) -> Result<Arc<TaskService>> {
        // Try read first
        {
            let guard = self.inner.read().await;
            if let Some(service) = guard.as_ref() {
                return Ok(service.clone());
            }
        }

        // Need to initialize
        let mut guard = self.inner.write().await;

        // Double-check after acquiring write lock
        if let Some(service) = guard.as_ref() {
            return Ok(service.clone());
        }

        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("echo")
            .join("repos.db");

        let service = Arc::new(TaskService::new(&data_dir).await?);
        *guard = Some(service.clone());

        Ok(service)
    }
}

// ==================== Orchestration Logs ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationLogEntry {
    pub timestamp_ms: u64,
    pub level: String,   // "info" | "warn" | "error" | "success"
    pub message: String,
}

static ORCHESTRATION_LOGS: Lazy<Mutex<Vec<OrchestrationLogEntry>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn add_orchestration_log(level: impl Into<String>, message: impl Into<String>) {
    let entry = OrchestrationLogEntry {
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        level: level.into(),
        message: message.into(),
    };

    let mut logs = ORCHESTRATION_LOGS.lock();
    logs.push(entry);
    if logs.len() > 200 {
        let drain = logs.len() - 200;
        logs.drain(0..drain);
    }
}

#[tauri::command]
pub async fn get_orchestration_logs() -> Result<Vec<OrchestrationLogEntry>, String> {
    Ok(ORCHESTRATION_LOGS.lock().clone())
}

#[tauri::command]
pub async fn clear_orchestration_logs() -> Result<(), String> {
    ORCHESTRATION_LOGS.lock().clear();
    Ok(())
}

// ==================== API Server State ====================

/// State for the Claude Code API server
pub struct ApiServerState {
    pub port: Arc<RwLock<Option<u16>>>,
    pub shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl ApiServerState {
    pub fn new() -> Self {
        Self {
            port: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }
}

/// Start the Claude Code API server
#[tauri::command]
pub async fn start_claude_api_server(
    task_state: State<'_, TaskServiceState>,
    api_state: State<'_, ApiServerState>,
) -> Result<u16, String> {
    // Check if already running
    {
        let port_guard = api_state.port.read().await;
        if let Some(port) = *port_guard {
            return Ok(port);
        }
    }
    
    // Get task service storage for the API server
    let service = task_state.get_or_init().await.map_err(|e| e.to_string())?;
    let storage = service.storage();
    
    // Create a wrapper that matches what the API server expects
    let task_service_state: Arc<RwLock<Option<Arc<TaskService>>>> = 
        Arc::new(RwLock::new(Some(service)));
    
    // Start the server
    let (port, shutdown_tx) = api_server::start_api_server(task_service_state)
        .await
        .map_err(|e| e.to_string())?;
    
    // Write endpoint info for Claude Code
    api_server::write_api_endpoint_info(port)
        .map_err(|e| format!("Failed to write API endpoint info: {}", e))?;
    
    // Store state
    {
        let mut port_guard = api_state.port.write().await;
        *port_guard = Some(port);
    }
    {
        let mut shutdown_guard = api_state.shutdown_tx.write().await;
        *shutdown_guard = Some(shutdown_tx);
    }
    
    add_orchestration_log("success", format!("API server started on port {}", port));
    
    Ok(port)
}

/// Stop the Claude Code API server
#[tauri::command]
pub async fn stop_claude_api_server(
    api_state: State<'_, ApiServerState>,
) -> Result<(), String> {
    let shutdown_tx = {
        let mut guard = api_state.shutdown_tx.write().await;
        guard.take()
    };
    
    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
        add_orchestration_log("info", "API server stopped");
    }
    
    {
        let mut port_guard = api_state.port.write().await;
        *port_guard = None;
    }
    
    Ok(())
}

/// Get the API server port (if running)
#[tauri::command]
pub async fn get_claude_api_port(
    api_state: State<'_, ApiServerState>,
) -> Result<Option<u16>, String> {
    let port_guard = api_state.port.read().await;
    Ok(*port_guard)
}

/// Get the API endpoint info for Claude Code
#[tauri::command]
pub async fn get_claude_api_info(
    api_state: State<'_, ApiServerState>,
) -> Result<Option<serde_json::Value>, String> {
    let port_guard = api_state.port.read().await;
    
    if let Some(port) = *port_guard {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| "No config dir".to_string())?
            .join("echo");
        let info_path = config_dir.join("api_endpoint.json");
        
        if info_path.exists() {
            let content = tokio::fs::read_to_string(&info_path)
                .await
                .map_err(|e| e.to_string())?;
            let info: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| e.to_string())?;
            Ok(Some(info))
        } else {
            Ok(Some(serde_json::json!({
                "base_url": format!("http://127.0.0.1:{}", port),
                "port": port
            })))
        }
    } else {
        Ok(None)
    }
}

// ==================== Quick Task Queries ====================

/// Sync all tasks from markdown files to the cache
/// Call this after making changes via Claude Code
#[tauri::command]
pub async fn sync_tasks_to_cache(
    state: State<'_, TaskServiceState>,
) -> Result<(), String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.sync_all_tasks_to_cache().await.map_err(|e| e.to_string())
}

/// Get the next task for a repository (highest priority ready/in-progress task)
/// This uses the cached database for fast queries.
/// Call sync_tasks_to_cache first if you need fresh data.
#[tauri::command]
pub async fn get_next_task(
    state: State<'_, TaskServiceState>,
    repo_id: Option<String>,
) -> Result<Option<Task>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    
    // First try cached (fast)
    if let Ok(Some(task)) = service.get_next_task_cached(repo_id.as_deref()).await {
        return Ok(Some(task));
    }
    
    // Fallback to reading from files
    let tasks = if let Some(id) = repo_id {
        service.get_repo_tasks(&id).await.map_err(|e| e.to_string())?
    } else {
        service.get_all_tasks().await.map_err(|e| e.to_string())?
    };
    
    // Priority order for finding "next" task:
    // 1. InProgress tasks (highest priority first)
    // 2. Ready tasks (highest priority first)
    // 3. Backlog tasks (highest priority first)
    
    let priority_value = |p: &crate::tasks::Priority| match p {
        crate::tasks::Priority::Critical => 0,
        crate::tasks::Priority::High => 1,
        crate::tasks::Priority::Medium => 2,
        crate::tasks::Priority::Low => 3,
    };
    
    // Find in-progress tasks first
    let mut in_progress: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::InProgress))
        .collect();
    in_progress.sort_by_key(|t| priority_value(&t.priority));
    
    if let Some(task) = in_progress.first() {
        return Ok(Some((*task).clone()));
    }
    
    // Then ready tasks
    let mut ready: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Ready))
        .collect();
    ready.sort_by_key(|t| priority_value(&t.priority));
    
    if let Some(task) = ready.first() {
        return Ok(Some((*task).clone()));
    }
    
    // Finally backlog tasks
    let mut backlog: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Backlog))
        .collect();
    backlog.sort_by_key(|t| priority_value(&t.priority));
    
    Ok(backlog.first().map(|t| (*t).clone()))
}

/// Get task summary for quick overview
#[tauri::command]
pub async fn get_task_summary(
    state: State<'_, TaskServiceState>,
    repo_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    
    let tasks = if let Some(id) = repo_id {
        service.get_repo_tasks(&id).await.map_err(|e| e.to_string())?
    } else {
        service.get_all_tasks().await.map_err(|e| e.to_string())?
    };
    
    let in_progress = tasks.iter().filter(|t| matches!(t.status, crate::tasks::TaskStatus::InProgress)).count();
    let ready = tasks.iter().filter(|t| matches!(t.status, crate::tasks::TaskStatus::Ready)).count();
    let blocked = tasks.iter().filter(|t| matches!(t.status, crate::tasks::TaskStatus::Blocked { .. })).count();
    let backlog = tasks.iter().filter(|t| matches!(t.status, crate::tasks::TaskStatus::Backlog)).count();
    let done = tasks.iter().filter(|t| matches!(t.status, crate::tasks::TaskStatus::Done)).count();
    
    Ok(serde_json::json!({
        "total": tasks.len(),
        "in_progress": in_progress,
        "ready": ready,
        "blocked": blocked,
        "backlog": backlog,
        "done": done,
        "next_task": get_next_task_internal(&tasks)
    }))
}

fn get_next_task_internal(tasks: &[Task]) -> Option<serde_json::Value> {
    let priority_value = |p: &crate::tasks::Priority| match p {
        crate::tasks::Priority::Critical => 0,
        crate::tasks::Priority::High => 1,
        crate::tasks::Priority::Medium => 2,
        crate::tasks::Priority::Low => 3,
    };
    
    let mut in_progress: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::InProgress))
        .collect();
    in_progress.sort_by_key(|t| priority_value(&t.priority));
    
    if let Some(task) = in_progress.first() {
        return Some(serde_json::json!({
            "id": task.id,
            "title": task.title,
            "status": "in_progress",
            "priority": format!("{:?}", task.priority)
        }));
    }
    
    let mut ready: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Ready))
        .collect();
    ready.sort_by_key(|t| priority_value(&t.priority));
    
    if let Some(task) = ready.first() {
        return Some(serde_json::json!({
            "id": task.id,
            "title": task.title,
            "status": "ready",
            "priority": format!("{:?}", task.priority)
        }));
    }
    
    let mut backlog: Vec<_> = tasks.iter()
        .filter(|t| matches!(t.status, crate::tasks::TaskStatus::Backlog))
        .collect();
    backlog.sort_by_key(|t| priority_value(&t.priority));
    
    backlog.first().map(|task| serde_json::json!({
        "id": task.id,
        "title": task.title,
        "status": "backlog",
        "priority": format!("{:?}", task.priority)
    }))
}

// ==================== Repository Commands ====================

/// List all tracked repositories
#[tauri::command]
pub async fn list_repositories(
    state: State<'_, TaskServiceState>,
) -> Result<Vec<Repository>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.list_repositories().await.map_err(|e| e.to_string())
}

/// Add a repository to track
#[tauri::command]
pub async fn add_repository(
    path: String,
    state: State<'_, TaskServiceState>,
) -> Result<Repository, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service
        .ensure_repository(&PathBuf::from(path))
        .await
        .map_err(|e| e.to_string())
}

/// Remove a repository from tracking (doesn't delete files)
#[tauri::command]
pub async fn remove_repository(
    id: String,
    state: State<'_, TaskServiceState>,
) -> Result<(), String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.remove_repository(&id).await.map_err(|e| e.to_string())
}

/// Register a repository (or get existing) - alias for add_repository
#[tauri::command]
pub async fn ensure_repository(
    path: String,
    state: State<'_, TaskServiceState>,
) -> Result<Repository, String> {
    add_repository(path, state).await
}

/// Open repository in Cursor
#[tauri::command]
pub async fn open_repo_in_cursor(
    repo_id: String,
    state: State<'_, TaskServiceState>,
) -> Result<(), String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.open_repo_in_cursor(&repo_id).await.map_err(|e| e.to_string())
}

/// Open task file in editor
#[tauri::command]
pub async fn open_task_file(
    repo_id: String,
    state: State<'_, TaskServiceState>,
) -> Result<(), String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.open_task_file(&repo_id).await.map_err(|e| e.to_string())
}

/// Get the task file path for a repository
#[tauri::command]
pub async fn get_task_file_path(
    repo_id: String,
    state: State<'_, TaskServiceState>,
) -> Result<Option<String>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    let path = service.get_task_file_path(&repo_id).await.map_err(|e| e.to_string())?;
    Ok(path.map(|p| p.to_string_lossy().to_string()))
}

// ==================== Task Commands ====================

/// Get all tasks from a repository
#[tauri::command]
pub async fn get_repo_tasks(
    repo_id: String,
    state: State<'_, TaskServiceState>,
) -> Result<Vec<Task>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.get_repo_tasks(&repo_id).await.map_err(|e| e.to_string())
}

/// List all tasks from all repositories (with optional filters)
#[tauri::command]
pub async fn list_tasks(
    filters: TaskFilters,
    state: State<'_, TaskServiceState>,
) -> Result<Vec<Task>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    
    let mut tasks = if let Some(repo_id) = &filters.repo_id {
        service.get_repo_tasks(repo_id).await.map_err(|e| e.to_string())?
    } else {
        service.get_all_tasks().await.map_err(|e| e.to_string())?
    };

    // Apply filters
    if let Some(statuses) = &filters.status {
        tasks.retain(|t| statuses.contains(&t.status));
    }

    if let Some(priorities) = &filters.priority {
        tasks.retain(|t| priorities.contains(&t.priority));
    }

    if let Some(labels) = &filters.labels {
        tasks.retain(|t| labels.iter().any(|l| t.labels.contains(l)));
    }

    if let Some(search) = &filters.search {
        let search_lower = search.to_lowercase();
        tasks.retain(|t| {
            t.title.to_lowercase().contains(&search_lower)
                || t.description.to_lowercase().contains(&search_lower)
        });
    }

    Ok(tasks)
}

/// Create a new task
#[tauri::command]
pub async fn create_task(
    input: CreateTaskInput,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.create_task(input).await.map_err(|e| e.to_string())
}

/// Quick create task from voice (uses active workspace)
#[tauri::command]
pub async fn quick_create_task(
    title: String,
    workspace_path: Option<String>,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;

    // Determine workspace path
    let path = if let Some(p) = workspace_path {
        PathBuf::from(p)
    } else {
        // Auto-detect workspace
        let detector = WorkspaceDetector::new();
        detector
            .detect()
            .await
            .map(|w| w.path)
            .ok_or_else(|| "Could not detect active workspace".to_string())?
    };

    service
        .quick_create_task(&path, title)
        .await
        .map_err(|e| e.to_string())
}

/// Get task by ID
#[tauri::command]
pub async fn get_task(
    id: String,
    state: State<'_, TaskServiceState>,
) -> Result<Option<Task>, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.get_task(&id).await.map_err(|e| e.to_string())
}

/// Update task
#[tauri::command]
pub async fn update_task(
    id: String,
    input: UpdateTaskInput,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.update_task(&id, input).await.map_err(|e| e.to_string())
}

/// Update task status
#[tauri::command]
pub async fn update_task_status(
    id: String,
    status: TaskStatus,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.update_task_status(&id, status).await.map_err(|e| e.to_string())
}

/// Delete task
#[tauri::command]
pub async fn delete_task(
    id: String,
    state: State<'_, TaskServiceState>,
) -> Result<(), String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service.delete_task(&id).await.map_err(|e| e.to_string())
}

// ==================== Checklist Commands ====================

/// Add checklist item to task
#[tauri::command]
pub async fn add_checklist_item(
    task_id: String,
    text: String,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service
        .add_checklist_item(&task_id, text)
        .await
        .map_err(|e| e.to_string())
}

/// Toggle checklist item
#[tauri::command]
pub async fn toggle_checklist_item(
    task_id: String,
    item_id: String,
    state: State<'_, TaskServiceState>,
) -> Result<Task, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    service
        .toggle_checklist_item(&task_id, &item_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== Workspace Context Commands ====================

/// Get active workspace context
#[tauri::command]
pub async fn get_workspace_context(
    workspace_path: Option<String>,
) -> Result<Option<WorkspaceContext>, String> {
    if let Some(path) = workspace_path {
        let context = WorkspaceContext::capture(&PathBuf::from(path))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(context))
    } else {
        WorkspaceContext::capture_active()
            .await
            .map_err(|e| e.to_string())
    }
}

/// Detect active workspace
#[tauri::command]
pub async fn detect_workspace() -> Result<Option<String>, String> {
    let detector = WorkspaceDetector::new();
    Ok(detector
        .detect()
        .await
        .map(|w| w.path.to_string_lossy().to_string()))
}

// ==================== Claude Code Orchestration ====================

fn orchestration_prompt_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;
    Ok(config_dir.join("echo").join("orchestration_prompt.md"))
}

/// Load the saved orchestration prompt (what you want Claude Code to do).
#[tauri::command]
pub async fn get_orchestration_prompt() -> Result<String, String> {
    let path = orchestration_prompt_path()?;
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read orchestration prompt: {}", e))
}

/// Save the orchestration prompt (what you want Claude Code to do).
#[tauri::command]
pub async fn save_orchestration_prompt(prompt: String) -> Result<(), String> {
    let path = orchestration_prompt_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    tokio::fs::write(&path, prompt)
        .await
        .map_err(|e| format!("Failed to write orchestration prompt: {}", e))
}

/// Build a multi-repo task orchestration prompt and send it to Claude Code.
///
/// This is the new replacement for MCP-based "advanced integrations".
/// Now supports headless mode with API server for callbacks.
#[tauri::command]
pub async fn orchestrate_claude_code_tasks(
    workspace_path: Option<String>,
    user_request: Option<String>,
    focus_repo_ids: Option<Vec<String>>,
    headless: Option<bool>,
    state: State<'_, TaskServiceState>,
    api_state: State<'_, ApiServerState>,
    app_handle: tauri::AppHandle,
) -> Result<InvocationResult, String> {
    let use_headless = headless.unwrap_or(true); // Default to headless mode
    
    // Ensure API server is running for Claude Code callbacks
    let api_port = if use_headless {
        match start_claude_api_server(state.clone(), api_state).await {
            Ok(port) => Some(port),
            Err(e) => {
                add_orchestration_log("warn", format!("API server failed to start: {}. Claude Code won't be able to update tasks directly.", e));
                None
            }
        }
    } else {
        None
    };
    
    process_orchestration_request(
        state, 
        app_handle, 
        user_request.unwrap_or_default(), 
        workspace_path, 
        focus_repo_ids,
        use_headless,
        api_port,
    ).await
}

/// Process an orchestration request by building context and invoking Claude Code
/// 
/// SIMPLIFIED: Always starts Claude in the target repo directory.
/// - If user selected specific repo(s) → use first selected
/// - If "All repos" → use first repo alphabetically but include all in context
pub async fn process_orchestration_request(
    state: State<'_, TaskServiceState>,
    app_handle: tauri::AppHandle,
    user_request: String,
    workspace_path: Option<String>,
    focus_repo_ids: Option<Vec<String>>,
    use_headless: bool,
    api_port: Option<u16>,
) -> Result<InvocationResult, String> {
    let service = state.get_or_init().await.map_err(|e| e.to_string())?;
    let all_repos = service.list_repositories().await.map_err(|e| e.to_string())?;
    
    if all_repos.is_empty() {
        return Err("No repositories tracked. Add repositories in Tasks settings.".to_string());
    }

    // SIMPLIFIED: Always determine a concrete working directory
    // Priority: explicit path > first focused repo > first repo alphabetically
    let (working_path, repos_in_scope): (PathBuf, Vec<Repository>) = {
        if let Some(p) = workspace_path {
            // Explicit path provided
            let path = PathBuf::from(&p);
            let matching_repo = all_repos.iter().find(|r| r.path == path);
            let repos = matching_repo.map(|r| vec![r.clone()]).unwrap_or_else(|| all_repos.clone());
            (path, repos)
        } else if let Some(ref focus_ids) = focus_repo_ids {
            if !focus_ids.is_empty() {
                // Specific repos selected - use first one as working dir
                let focused: Vec<Repository> = all_repos.iter()
                    .filter(|r| focus_ids.contains(&r.id))
                    .cloned()
                    .collect();
                if let Some(first) = focused.first() {
                    (first.path.clone(), focused)
                } else {
                    // Fallback if no matching repos found
                    (all_repos[0].path.clone(), all_repos.clone())
                }
            } else {
                // Empty focus = all repos, use first as working dir
                (all_repos[0].path.clone(), all_repos.clone())
            }
        } else {
            // No focus = all repos, use first as working dir
            (all_repos[0].path.clone(), all_repos.clone())
        }
    };
    
    add_orchestration_log("info", format!("Working directory: {}", working_path.display()));
    add_orchestration_log("info", format!("Repos in scope: {} ({})", 
        repos_in_scope.len(),
        repos_in_scope.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")
    ));

    // Capture workspace context from the target directory
    let workspace = WorkspaceContext::capture(&working_path)
        .await
        .map_err(|e| e.to_string())?;
    
    let tasks = service.get_all_tasks().await.map_err(|e| e.to_string())?;

    // Resolve user request (persist it so Shift hotkey can reuse it)
    let user_request_str = user_request.trim().to_string();
    
    if !user_request_str.is_empty() {
        let _ = save_orchestration_prompt(user_request_str.clone()).await;
    }
    
    let final_request = if !user_request_str.is_empty() {
        user_request_str
    } else {
        get_orchestration_prompt()
            .await
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                "What's the current task status? Provide a brief update.".to_string()
            })
    };

    // Build a compact overview, prioritizing non-done tasks.
    let mut tasks_by_repo: std::collections::HashMap<String, Vec<Task>> = std::collections::HashMap::new();
    for t in tasks {
        tasks_by_repo.entry(t.repo_id.clone()).or_default().push(t);
    }

    for list in tasks_by_repo.values_mut() {
        // newest first
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    }

    let mut overview = String::new();
    overview.push_str("## Repository Index\n\n");
    overview.push_str("Echo tracks local repositories and stores tasks as markdown inside each repo (usually `.echo/tasks.md` or `TODO.md`).\n\n");

    for repo in &all_repos {
        overview.push_str(&format!(
            "- **{}** (`{}`)\n  - id: `{}`\n  - remote: {}\n  - default branch: `{}`\n",
            repo.name,
            repo.path.display(),
            repo.id,
            repo.remote_url.clone().unwrap_or_else(|| "(none)".to_string()),
            repo.default_branch
        ));
    }
    overview.push_str("\n");

    overview.push_str("## Tracked Repositories & Open Tasks\n\n");
    if repos_in_scope.len() < all_repos.len() {
        overview.push_str(&format!("Scope: **{} selected repositories**.\n\n", repos_in_scope.len()));
    } else {
        overview.push_str("Scope: **all tracked repositories**.\n\n");
    }

    for repo in &repos_in_scope {
        overview.push_str(&format!(
            "### {}\n- **id**: `{}`\n- **Path**: `{}`\n- **Remote**: {}\n- **Default branch**: `{}`\n",
            repo.name,
            repo.id,
            repo.path.display(),
            repo.remote_url.clone().unwrap_or_else(|| "(none)".to_string()),
            repo.default_branch
        ));

        let repo_tasks = tasks_by_repo.get(&repo.id).cloned().unwrap_or_default();
        let mut non_done: Vec<Task> = repo_tasks
            .into_iter()
            .filter(|t| t.status != TaskStatus::Done)
            .collect();
        non_done.truncate(20);

        if non_done.is_empty() {
            overview.push_str("- **Open tasks**: (none)\n\n");
            continue;
        }

        overview.push_str("- **Open tasks**:\n");
        for t in non_done {
            overview.push_str(&format!(
                "  - [{}] ({:?}) {}\n",
                t.status.as_str(),
                t.priority,
                t.title
            ));
        }
        overview.push('\n');
    }

    // Read tasks.md from each tracked repo and include in context
    let mut tasks_content = String::new();
    tasks_content.push_str("## Task Files from Repositories\n\n");
    tasks_content.push_str("Each repository stores tasks in `.echo/tasks.md`. Here are the current contents:\n\n");
    
    for repo in &repos_in_scope {
        let tasks_file = repo.path.join(".echo").join("tasks.md");
        if tasks_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&tasks_file) {
                tasks_content.push_str(&format!("### {} ({})\n", repo.name, tasks_file.display()));
                tasks_content.push_str("```markdown\n");
                // Limit content to first 1500 chars to avoid huge prompts
                if content.len() > 1500 {
                    tasks_content.push_str(&content[..1500]);
                    tasks_content.push_str("\n... (truncated)\n");
                } else {
                    tasks_content.push_str(&content);
                }
                tasks_content.push_str("\n```\n\n");
            }
        } else {
            tasks_content.push_str(&format!("### {} \n*No `.echo/tasks.md` file yet. Create one.*\n\n", repo.name));
        }
    }

    // Build prompt with task-focused constraints
    let is_multi_repo_mode = repos_in_scope.len() > 1;
    
    let mut builder = PromptBuilder::new(final_request.clone())
        .with_workspace(workspace.clone())
        .add_context(tasks_content)
        .add_context(format!("Repository index and task summary is in 'echo_context.md' in the same directory."))
        // ACTION-ORIENTED: Don't ask, just do
        .add_constraint("ACT WITHOUT ASKING: NEVER ask for permission. If a task file doesn't exist, CREATE IT. If a task needs updating, UPDATE IT. Complete actions immediately.")
        .add_constraint("CREATE .echo/tasks.md: If a repository doesn't have a .echo/tasks.md file, create one immediately with proper markdown structure.")
        // Task-focused constraints
        .add_constraint("FOCUS ON TASKS: Your primary job is to manage tasks - create, update, check status, mark complete. Use the Echo Task API or edit .echo/tasks.md files directly.")
        .add_constraint("DO NOT read entire codebases or large files unless specifically asked. Use targeted grep/search to find specific information.")
        .add_constraint("KEEP RESPONSES BRIEF: Provide concise status updates. Don't dump file contents unless asked.")
        .add_constraint("USE TOOLS EFFICIENTLY: Prefer API calls over file reads. Use grep over reading entire files.")
        .add_constraint("Do not use MCP servers; assume Claude Code has native tool access.");
    
    // In multi-repo mode, tell Claude it can switch repos
    if is_multi_repo_mode {
        let repo_list: Vec<String> = repos_in_scope.iter()
            .map(|r| format!("- {}: `{}`", r.name, r.path.display()))
            .collect();
        builder = builder.add_context(format!(
            "**MULTI-REPO MODE**: {} repositories in scope. You're starting in the first one. Switch with `cd /path` if needed.\n{}",
            repos_in_scope.len(),
            repo_list.join("\n")
        ));
    }
    
    // Add hint for Cursor handoff
    builder = builder.add_context(
        "If code implementation is needed, write instructions to `.echo/CURSOR_INSTRUCTIONS.md` in the repo for the user to open in Cursor IDE."
    );
    
    // Add API server info for task updates if running
    if let Some(port) = api_port {
        builder = builder.add_context(format!(
            r#"## Echo Task API

You can update tasks in Echo directly using the local API at http://127.0.0.1:{port}.

**Available endpoints:**
- `GET /health` - Check API status
- `GET /tasks` - List all tasks (add `?repo_path=/path` to filter)
- `GET /tasks/<task_id>` - Get specific task
- `POST /tasks/create` - Create new task (body: `{{"repo_path": "/path", "title": "...", "status": "in_progress"}}`)
- `POST /tasks/update` - Update task (body: `{{"task_id": "...", "status": "done"}}`)
- `POST /log` - Send log message (body: `{{"level": "info", "message": "..."}}`)

**Status values:** backlog, ready, in_progress, blocked, in_review, done
**Priority values:** low, medium, high, critical

**Example - Mark task as done:**
```bash
curl -X POST http://127.0.0.1:{port}/tasks/update \
  -H 'Content-Type: application/json' \
  -d '{{"task_id": "<id>", "status": "done"}}'
```

**Example - Create a new task:**
```bash
curl -X POST http://127.0.0.1:{port}/tasks/create \
  -H 'Content-Type: application/json' \
  -d '{{"repo_path": "/path/to/repo", "title": "New Task", "priority": "high"}}'
```

Use these APIs to keep Echo's task list in sync with your progress.
"#
        ));
    }
    
    let prompt = builder.build();

    // Write context to separate file
    if let Some(config_dir) = dirs::config_dir() {
        let echo_dir = config_dir.join("echo");
        let context_path = echo_dir.join("echo_context.md");
        let _ = std::fs::write(context_path, overview);
    }

    // Start file watcher for Cursor handoff in the working directory
    {
        let state_arc = app_handle.state::<Arc<std::sync::Mutex<crate::state::AppState>>>();
        if let Ok(mut app_state) = state_arc.inner().lock() {
            if app_state.handoff_watcher.is_none() {
                app_state.handoff_watcher = Some(Arc::new(std::sync::Mutex::new(
                    crate::workspace::CursorHandoffWatcher::new(app_handle.clone())
                )));
            }
            
            if let Some(watcher) = &app_state.handoff_watcher {
                if let Ok(mut w) = watcher.lock() {
                    let _ = w.start_watching(&working_path);
                }
            }
        }
    }

    // Choose invocation method - ALWAYS pass concrete working directory
    let cwd = Some(working_path.clone());
    let method = if use_headless {
        crate::claude::invoke::InvocationMethod::HeadlessStreaming { cwd }
    } else {
        let default_method = crate::claude::invoke::InvocationMethod::default();
        match default_method {
            crate::claude::invoke::InvocationMethod::Terminal { .. } => {
                crate::claude::invoke::InvocationMethod::Terminal { cwd }
            },
            crate::claude::invoke::InvocationMethod::Headless { .. } => {
                crate::claude::invoke::InvocationMethod::Headless { cwd }
            },
            m => m,
        }
    };

    add_orchestration_log("info", format!("Invoking Claude Code with method: {:?}", method));

    crate::claude::invoke::invoke_claude_code_with_handle(&prompt, &method, Some(app_handle))
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Claude Code Conversation Support
// ============================================================================

/// State for tracking Claude Code conversation context
static CLAUDE_CONVERSATION_CONTEXT: Lazy<Mutex<ClaudeConversationContext>> = 
    Lazy::new(|| Mutex::new(ClaudeConversationContext::default()));

#[derive(Debug, Default)]
struct ClaudeConversationContext {
    /// Last Claude Code session ID (if available)
    session_id: Option<String>,
    /// Working directory for the conversation
    working_dir: Option<PathBuf>,
    /// Last question asked by Claude
    last_question: Option<String>,
    /// Conversation history (for context)
    history: Vec<String>,
}

/// Update the Claude conversation context (called when parsing output)
pub fn update_claude_context(session_id: Option<String>, working_dir: Option<PathBuf>) {
    let mut ctx = CLAUDE_CONVERSATION_CONTEXT.lock();
    if let Some(sid) = session_id {
        ctx.session_id = Some(sid);
    }
    if let Some(wd) = working_dir {
        ctx.working_dir = Some(wd);
    }
}

/// Store the last question from Claude
pub fn store_claude_question(question: String) {
    let mut ctx = CLAUDE_CONVERSATION_CONTEXT.lock();
    ctx.last_question = Some(question.clone());
    ctx.history.push(format!("[Claude] {}", question));
}

/// Send a response to Claude Code
/// 
/// This uses the --resume flag to continue the conversation in the same session.
#[tauri::command]
pub async fn send_claude_response(
    response: String,
    app_handle: tauri::AppHandle,
) -> Result<InvocationResult, String> {
    use tauri::Emitter;
    
    let (session_id, working_dir) = {
        let ctx = CLAUDE_CONVERSATION_CONTEXT.lock();
        (ctx.session_id.clone(), ctx.working_dir.clone())
    };
    
    add_orchestration_log("info", format!("Sending user response: {}", response));
    add_orchestration_log("info", format!("Session ID: {:?}, Working dir: {:?}", session_id, working_dir));
    
    // Use Claude Code's --resume flag to continue the session
    if let Some(ref sid) = session_id {
        add_orchestration_log("info", format!("Resuming session: {}", sid));
        
        let mut cmd = tokio::process::Command::new("claude");
        cmd.arg("--resume").arg(sid);
        cmd.arg("-p").arg(&response);
        cmd.arg("--dangerously-skip-permissions");
        cmd.arg("--output-format").arg("stream-json");
        
        if let Some(ref wd) = working_dir {
            cmd.current_dir(wd);
        }
        
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        
        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id().unwrap_or(0);
                add_orchestration_log("success", format!("Resumed Claude Code session (PID: {})", pid));
                
                // Emit that we're starting
                let _ = app_handle.emit("claude-output", r#"{"type":"system","message":"Resuming conversation..."}"#);
                
                // Stream output in background
                stream_claude_output(child, app_handle).await;
                
                Ok(InvocationResult {
                    success: true,
                    method: crate::claude::invoke::InvocationMethod::HeadlessStreaming { cwd: working_dir },
                    message: format!("Resumed conversation (PID: {})", pid),
                })
            }
            Err(e) => {
                let err_msg = format!("Failed to spawn Claude Code: {}", e);
                add_orchestration_log("error", &err_msg);
                Err(err_msg)
            }
        }
    } else {
        // No session to resume - start fresh with the response as context
        add_orchestration_log("warn", "No session to resume, starting fresh conversation with response");
        
        let prompt = format!(
            "The user has provided this response to your previous question:\n\n\"{}\"\n\nPlease acknowledge and continue based on their answer.",
            response
        );
        
        let method = crate::claude::invoke::InvocationMethod::HeadlessStreaming { cwd: working_dir };
        crate::claude::invoke::invoke_claude_code_with_handle(&prompt, &method, Some(app_handle))
            .await
            .map_err(|e| e.to_string())
    }
}

async fn stream_claude_output(mut child: tokio::process::Child, app_handle: tauri::AppHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tauri::Emitter;
    
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    
    tokio::spawn(async move {
        // Read stdout
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            
            while let Ok(Some(line)) = lines.next_line().await {
                // Parse for session ID and other metadata
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(sid) = parsed.get("session_id").and_then(|v| v.as_str()) {
                        add_orchestration_log("info", format!("Captured session_id: {}", sid));
                        update_claude_context(Some(sid.to_string()), None);
                    }
                    if let Some(cwd) = parsed.get("cwd").and_then(|v| v.as_str()) {
                        update_claude_context(None, Some(PathBuf::from(cwd)));
                    }
                }
                
                // Emit to frontend
                let _ = app_handle.emit("claude-output", &line);
            }
        }
        
        // Also read stderr for any errors
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    add_orchestration_log("warn", format!("[stderr] {}", &line));
                }
            }
        }
        
        let status = child.wait().await;
        match status {
            Ok(exit) if exit.success() => {
                add_orchestration_log("success", "Claude Code completed");
            }
            Ok(exit) => {
                add_orchestration_log("warn", format!("Claude Code exited with code: {:?}", exit.code()));
            }
            Err(e) => {
                add_orchestration_log("error", format!("Claude Code process error: {}", e));
            }
        }
        
        let _ = app_handle.emit("claude-complete", "done");
    });
}

/// Start voice recording for response
#[tauri::command]
pub async fn start_voice_response_recording(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Emitter;
    
    // Emit event to start recording (reuse existing recording infrastructure)
    app_handle.emit("start-response-recording", ()).map_err(|e| e.to_string())?;
    
    // The recording will be handled by the existing voice infrastructure
    // When transcription completes, it should call send_claude_response
    
    add_orchestration_log("info", "Voice response recording started");
    Ok(())
}

/// Show the orchestration overlay window
#[tauri::command]
pub async fn show_orchestration_overlay(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    
    // Show the overlay window
    if let Some(overlay) = app_handle.get_webview_window("overlay") {
        overlay.show().map_err(|e| e.to_string())?;
        overlay.set_focus().map_err(|e| e.to_string())?;
    }
    
    // Emit event to switch overlay to prompt mode
    app_handle.emit("open-orchestrate-overlay", ()).map_err(|e| e.to_string())?;
    
    Ok(())
}
