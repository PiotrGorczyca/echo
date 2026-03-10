//! Local HTTP API server for Claude Code integration
//!
//! This server runs locally and allows Claude Code (via bash/curl) to
//! update tasks, add logs, and communicate back to Echo.
//!
//! The server listens on a local port and provides REST-like endpoints
//! for task management operations.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

/// API server state
pub struct ApiServerState {
    port: u16,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ApiServerState {
    pub fn new() -> Self {
        Self {
            port: 0,
            shutdown_tx: None,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_running(&self) -> bool {
        self.shutdown_tx.is_some()
    }
}

/// Task update request from Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdateRequest {
    /// Task ID to update
    pub task_id: Option<String>,
    /// Repository path (for creating new tasks or finding by repo)
    pub repo_path: Option<String>,
    /// Task title (for new tasks or search)
    pub title: Option<String>,
    /// New status
    pub status: Option<String>,
    /// Description update
    pub description: Option<String>,
    /// Priority (low, medium, high, critical)
    pub priority: Option<String>,
    /// Labels to set
    pub labels: Option<Vec<String>>,
    /// Checklist items to add
    pub add_checklist: Option<Vec<String>>,
    /// Checklist item IDs to toggle
    pub toggle_checklist: Option<Vec<String>>,
}

/// Log entry from Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRequest {
    pub level: String,
    pub message: String,
    pub context: Option<HashMap<String, String>>,
}

/// API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl ApiResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    pub fn ok_with_data(message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}

/// Start the local API server for Claude Code integration
/// Returns the port number the server is listening on
pub async fn start_api_server(
    task_service_state: Arc<RwLock<Option<Arc<crate::tasks::TaskService>>>>,
) -> Result<(u16, tokio::sync::oneshot::Sender<()>)> {
    use crate::commands::tasks::add_orchestration_log;
    
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    // Clone state for handlers
    let task_state = task_service_state.clone();
    let task_state_2 = task_service_state.clone();
    let task_state_3 = task_service_state.clone();
    let task_state_4 = task_service_state.clone();
    
    // Health check endpoint
    let health = warp::path("health")
        .and(warp::get())
        .map(|| {
            warp::reply::json(&ApiResponse::ok("Echo API is running"))
        });
    
    // List tasks endpoint
    let list_tasks = warp::path!("tasks")
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and_then(move |query: HashMap<String, String>| {
            let state = task_state.clone();
            async move {
                handle_list_tasks(state, query).await
            }
        });
    
    // Get task by ID
    let get_task = warp::path!("tasks" / String)
        .and(warp::get())
        .and_then(move |task_id: String| {
            let state = task_state_2.clone();
            async move {
                handle_get_task(state, task_id).await
            }
        });
    
    // Update task
    let update_task = warp::path!("tasks" / "update")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |body: TaskUpdateRequest| {
            let state = task_state_3.clone();
            async move {
                handle_update_task(state, body).await
            }
        });
    
    // Create task
    let create_task = warp::path!("tasks" / "create")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |body: TaskUpdateRequest| {
            let state = task_state_4.clone();
            async move {
                handle_create_task(state, body).await
            }
        });
    
    // Log endpoint for Claude Code to send logs
    let log_endpoint = warp::path!("log")
        .and(warp::post())
        .and(warp::body::json())
        .map(|body: LogRequest| {
            add_orchestration_log(&body.level, &body.message);
            warp::reply::json(&ApiResponse::ok("Log recorded"))
        });
    
    // Notify endpoint for Claude Code to signal completion/events
    let notify = warp::path!("notify")
        .and(warp::post())
        .and(warp::body::json())
        .map(|body: HashMap<String, String>| {
            let event = body.get("event").cloned().unwrap_or_else(|| "unknown".to_string());
            let message = body.get("message").cloned().unwrap_or_default();
            add_orchestration_log("info", format!("[Claude] Event: {} - {}", event, message));
            warp::reply::json(&ApiResponse::ok("Notification received"))
        });
    
    // Combine all routes
    let routes = health
        .or(list_tasks)
        .or(get_task)
        .or(update_task)
        .or(create_task)
        .or(log_endpoint)
        .or(notify)
        .with(warp::cors().allow_any_origin());
    
    // Find an available port (try 17832 first, then fallback to random)
    let preferred_port = 17832u16;
    let addr: SocketAddr = format!("127.0.0.1:{}", preferred_port)
        .parse()
        .map_err(|e| anyhow!("Invalid address: {}", e))?;
    
    // Try to bind to preferred port using bind_with_graceful_shutdown
    let (addr, server) = match warp::serve(routes.clone())
        .try_bind_with_graceful_shutdown(addr, async move {
            shutdown_rx.await.ok();
        }) {
        Ok(result) => result,
        Err(_) => {
            // Need a new shutdown channel for the fallback
            let (new_shutdown_tx, new_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            
            // Fallback to any available port
            let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let (addr, server) = warp::serve(routes)
                .try_bind_with_graceful_shutdown(addr, async move {
                    new_shutdown_rx.await.ok();
                })?;
            
            // Return with the new shutdown_tx
            let port = addr.port();
            add_orchestration_log("info", format!("Echo API server started on port {}", port));
            
            tokio::spawn(server);
            
            return Ok((port, new_shutdown_tx));
        }
    };
    
    let port = addr.port();
    add_orchestration_log("info", format!("Echo API server started on port {}", port));
    
    // Spawn the server
    tokio::spawn(server);
    
    Ok((port, shutdown_tx))
}

async fn handle_list_tasks(
    state: Arc<RwLock<Option<Arc<crate::tasks::TaskService>>>>,
    query: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let guard = state.read().await;
    let service = match guard.as_ref() {
        Some(s) => s.clone(),
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("Task service not initialized")));
        }
    };
    drop(guard);
    
    let repo_path = query.get("repo_path");
    
    let tasks = if let Some(path) = repo_path {
        // First ensure repo is registered
        match service.ensure_repository(&std::path::PathBuf::from(path)).await {
            Ok(repo) => {
                service.get_repo_tasks(&repo.id).await.unwrap_or_default()
            }
            Err(e) => {
                return Ok(warp::reply::json(&ApiResponse::error(format!("Failed to access repo: {}", e))));
            }
        }
    } else {
        service.get_all_tasks().await.unwrap_or_default()
    };
    
    Ok(warp::reply::json(&ApiResponse::ok_with_data(
        format!("Found {} tasks", tasks.len()),
        serde_json::to_value(&tasks).unwrap_or_default(),
    )))
}

async fn handle_get_task(
    state: Arc<RwLock<Option<Arc<crate::tasks::TaskService>>>>,
    task_id: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    let guard = state.read().await;
    let service = match guard.as_ref() {
        Some(s) => s.clone(),
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("Task service not initialized")));
        }
    };
    drop(guard);
    
    match service.get_task(&task_id).await {
        Ok(Some(task)) => {
            Ok(warp::reply::json(&ApiResponse::ok_with_data(
                "Task found",
                serde_json::to_value(&task).unwrap_or_default(),
            )))
        }
        Ok(None) => {
            Ok(warp::reply::json(&ApiResponse::error("Task not found")))
        }
        Err(e) => {
            Ok(warp::reply::json(&ApiResponse::error(format!("Error: {}", e))))
        }
    }
}

async fn handle_update_task(
    state: Arc<RwLock<Option<Arc<crate::tasks::TaskService>>>>,
    body: TaskUpdateRequest,
) -> Result<impl warp::Reply, warp::Rejection> {
    use crate::commands::tasks::add_orchestration_log;
    use crate::tasks::{TaskStatus, Priority, UpdateTaskInput};
    
    let guard = state.read().await;
    let service = match guard.as_ref() {
        Some(s) => s.clone(),
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("Task service not initialized")));
        }
    };
    drop(guard);
    
    let task_id = match body.task_id {
        Some(id) => id,
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("task_id is required")));
        }
    };
    
    add_orchestration_log("info", format!("[Claude] Updating task: {}", task_id));
    
    // Parse status if provided
    let status = body.status.map(|s| match s.to_lowercase().as_str() {
        "backlog" => TaskStatus::Backlog,
        "ready" => TaskStatus::Ready,
        "in_progress" | "inprogress" => TaskStatus::InProgress,
        "blocked" => TaskStatus::Blocked { reason: "".to_string() },
        "in_review" | "inreview" => TaskStatus::InReview { reviewer: None },
        "done" => TaskStatus::Done,
        _ => TaskStatus::Backlog,
    });
    
    // Parse priority if provided
    let priority = body.priority.map(|p| match p.to_lowercase().as_str() {
        "low" => Priority::Low,
        "medium" => Priority::Medium,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => Priority::Medium,
    });
    
    let input = UpdateTaskInput {
        title: body.title,
        description: body.description,
        status,
        priority,
        labels: body.labels,
        due_date: None,
        branch: None,
    };
    
    match service.update_task(&task_id, input).await {
        Ok(task) => {
            add_orchestration_log("success", format!("[Claude] Task updated: {}", task.title));
            Ok(warp::reply::json(&ApiResponse::ok_with_data(
                "Task updated",
                serde_json::to_value(&task).unwrap_or_default(),
            )))
        }
        Err(e) => {
            add_orchestration_log("error", format!("[Claude] Task update failed: {}", e));
            Ok(warp::reply::json(&ApiResponse::error(format!("Update failed: {}", e))))
        }
    }
}

async fn handle_create_task(
    state: Arc<RwLock<Option<Arc<crate::tasks::TaskService>>>>,
    body: TaskUpdateRequest,
) -> Result<impl warp::Reply, warp::Rejection> {
    use crate::commands::tasks::add_orchestration_log;
    use crate::tasks::{CreateTaskInput, Priority};
    
    let guard = state.read().await;
    let service = match guard.as_ref() {
        Some(s) => s.clone(),
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("Task service not initialized")));
        }
    };
    drop(guard);
    
    let repo_path = match body.repo_path {
        Some(p) => p,
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("repo_path is required")));
        }
    };
    
    let title = match body.title {
        Some(t) => t,
        None => {
            return Ok(warp::reply::json(&ApiResponse::error("title is required")));
        }
    };
    
    add_orchestration_log("info", format!("[Claude] Creating task: {}", title));
    
    // Ensure repo is registered
    let repo = match service.ensure_repository(&std::path::PathBuf::from(&repo_path)).await {
        Ok(r) => r,
        Err(e) => {
            return Ok(warp::reply::json(&ApiResponse::error(format!("Failed to access repo: {}", e))));
        }
    };
    
    // Parse priority if provided
    let priority = body.priority.map(|p| match p.to_lowercase().as_str() {
        "low" => Priority::Low,
        "medium" => Priority::Medium,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => Priority::Medium,
    });
    
    let input = CreateTaskInput {
        repo_id: repo.id,
        title,
        description: body.description,
        priority,
        labels: body.labels,
        branch: None,
    };
    
    match service.create_task(input).await {
        Ok(task) => {
            add_orchestration_log("success", format!("[Claude] Task created: {}", task.title));
            Ok(warp::reply::json(&ApiResponse::ok_with_data(
                "Task created",
                serde_json::to_value(&task).unwrap_or_default(),
            )))
        }
        Err(e) => {
            add_orchestration_log("error", format!("[Claude] Task creation failed: {}", e));
            Ok(warp::reply::json(&ApiResponse::error(format!("Creation failed: {}", e))))
        }
    }
}

/// Write the API endpoint info to a well-known location for Claude Code to discover
pub fn write_api_endpoint_info(port: u16) -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("No config dir"))?
        .join("echo");
    std::fs::create_dir_all(&config_dir)?;
    
    let info_path = config_dir.join("api_endpoint.json");
    let info = serde_json::json!({
        "base_url": format!("http://127.0.0.1:{}", port),
        "port": port,
        "endpoints": {
            "health": "GET /health",
            "list_tasks": "GET /tasks?repo_path=<path>",
            "get_task": "GET /tasks/<task_id>",
            "create_task": "POST /tasks/create",
            "update_task": "POST /tasks/update",
            "log": "POST /log",
            "notify": "POST /notify"
        },
        "examples": {
            "list_tasks": format!("curl http://127.0.0.1:{}/tasks", port),
            "create_task": format!(
                "curl -X POST http://127.0.0.1:{}/tasks/create -H 'Content-Type: application/json' -d '{{\"repo_path\": \"/path/to/repo\", \"title\": \"My Task\", \"status\": \"in_progress\"}}'",
                port
            ),
            "update_task": format!(
                "curl -X POST http://127.0.0.1:{}/tasks/update -H 'Content-Type: application/json' -d '{{\"task_id\": \"<id>\", \"status\": \"done\"}}'",
                port
            ),
            "log": format!(
                "curl -X POST http://127.0.0.1:{}/log -H 'Content-Type: application/json' -d '{{\"level\": \"info\", \"message\": \"Hello from Claude\"}}'",
                port
            ),
        }
    });
    
    std::fs::write(&info_path, serde_json::to_string_pretty(&info)?)?;
    
    Ok(())
}

