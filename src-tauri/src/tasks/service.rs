//! Task service - business logic layer
//!
//! Tasks are stored as MARKDOWN FILES in repositories.
//! This service coordinates reading/writing markdown + repo registry.

use super::markdown::{
    add_task_to_file, find_task_file, get_or_create_task_file, parse_task_file,
    toggle_checklist_item_in_file, update_task_in_file, write_task_file, TaskUpdate,
};
use super::models::*;
use super::storage::TaskStorage;
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Task service for business logic
pub struct TaskService {
    storage: Arc<RwLock<TaskStorage>>,
}

impl TaskService {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let storage = TaskStorage::new(db_path).await?;
        Ok(Self {
            storage: Arc::new(RwLock::new(storage)),
        })
    }

    /// Get storage reference
    pub fn storage(&self) -> Arc<RwLock<TaskStorage>> {
        self.storage.clone()
    }

    // ==================== Repository Operations ====================

    /// Register or get existing repository for a path
    pub async fn ensure_repository(&self, path: &Path) -> Result<Repository> {
        let storage = self.storage.read().await;

        // Check if repo already exists
        if let Some(repo) = storage.get_repository_by_path(path).await? {
            storage.touch_repository(&repo.id).await?;
            return Ok(repo);
        }

        drop(storage);

        // Create new repo
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let remote_url = self.detect_git_remote(path).await;
        let default_branch = self
            .detect_default_branch(path)
            .await
            .unwrap_or_else(|| "main".to_string());

        let repo = Repository {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path: path.to_path_buf(),
            remote_url,
            default_branch,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };

        let storage = self.storage.write().await;
        storage.create_repository(&repo).await?;
        
        // Automatically create .echo/tasks.md if it doesn't exist
        if let Err(e) = get_or_create_task_file(path) {
            log::warn!("Failed to create task file for {}: {}", path.display(), e);
        }
        
        Ok(repo)
    }

    /// Detect git remote URL
    async fn detect_git_remote(&self, path: &Path) -> Option<String> {
        let output = tokio::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Detect default branch
    async fn detect_default_branch(&self, path: &Path) -> Option<String> {
        let output = tokio::process::Command::new("git")
            .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
            .current_dir(path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Some(branch.strip_prefix("origin/").unwrap_or(&branch).to_string());
        }

        // Fallback: check if main or master exists
        for branch in &["main", "master"] {
            let check = tokio::process::Command::new("git")
                .args(["rev-parse", "--verify", branch])
                .current_dir(path)
                .output()
                .await
                .ok()?;

            if check.status.success() {
                return Some(branch.to_string());
            }
        }

        None
    }

    /// List all repositories
    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        let storage = self.storage.read().await;
        storage.list_repositories().await
    }

    /// Get repository by ID
    pub async fn get_repository(&self, id: &str) -> Result<Option<Repository>> {
        let storage = self.storage.read().await;
        storage.get_repository(id).await
    }

    /// Remove a repository from tracking (doesn't delete files)
    pub async fn remove_repository(&self, id: &str) -> Result<()> {
        let storage = self.storage.write().await;
        storage.delete_repository(id).await
    }

    // ==================== Task Cache Operations ====================
    
    /// Sync tasks from markdown files to the cache for all repositories
    pub async fn sync_all_tasks_to_cache(&self) -> Result<()> {
        let repos = self.list_repositories().await?;
        for repo in repos {
            if let Err(e) = self.sync_repo_tasks_to_cache(&repo.id).await {
                log::warn!("Failed to sync tasks for {}: {}", repo.name, e);
            }
        }
        Ok(())
    }
    
    /// Sync tasks from a single repo's markdown file to the cache
    pub async fn sync_repo_tasks_to_cache(&self, repo_id: &str) -> Result<()> {
        let tasks = self.get_repo_tasks(repo_id).await?;
        let storage = self.storage.write().await;
        storage.sync_tasks_to_cache(repo_id, &tasks).await
    }
    
    /// Get next task from cache (fast query)
    pub async fn get_next_task_cached(&self, repo_id: Option<&str>) -> Result<Option<Task>> {
        let storage = self.storage.read().await;
        storage.get_next_cached_task(repo_id).await
    }

    // ==================== Task Operations (Markdown-based) ====================

    /// Get all tasks from a repository's markdown file
    pub async fn get_repo_tasks(&self, repo_id: &str) -> Result<Vec<Task>> {
        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", repo_id))?;

        let task_file = find_task_file(&repo.path);

        match task_file {
            Some(file_path) => {
                let content = tokio::fs::read_to_string(&file_path).await?;
                parse_task_file(&content, repo_id, &file_path)
            }
            None => Ok(Vec::new()), // No task file yet
        }
    }

    /// Get all tasks from all tracked repositories
    pub async fn get_all_tasks(&self) -> Result<Vec<Task>> {
        let repos = self.list_repositories().await?;
        let mut all_tasks = Vec::new();

        for repo in repos {
            match self.get_repo_tasks(&repo.id).await {
                Ok(tasks) => all_tasks.extend(tasks),
                Err(e) => {
                    log::warn!("Failed to load tasks from {}: {}", repo.name, e);
                }
            }
        }

        Ok(all_tasks)
    }

    /// Get a single task by ID (searches all repos)
    pub async fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        let all_tasks = self.get_all_tasks().await?;
        Ok(all_tasks.into_iter().find(|t| t.id == task_id))
    }

    /// Create a new task in a repository
    pub async fn create_task(&self, input: CreateTaskInput) -> Result<Task> {
        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(&input.repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", input.repo_id))?;
        drop(storage);

        // Get or create task file
        let task_file = get_or_create_task_file(&repo.path)?;

        // Create the task
        let mut task = Task::new(input.repo_id.clone(), input.title);

        if let Some(desc) = input.description {
            task.description = desc;
        }
        if let Some(priority) = input.priority {
            task.priority = priority;
        }
        if let Some(labels) = input.labels {
            task.labels = labels;
        }
        if let Some(branch) = input.branch {
            task.branch = Some(branch);
        }

        // Add to file
        add_task_to_file(&task_file, &task)?;

        Ok(task)
    }

    /// Update a task
    pub async fn update_task(&self, task_id: &str, input: UpdateTaskInput) -> Result<Task> {
        // Find the task and its file
        let task = self
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(&task.repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", task.repo_id))?;
        drop(storage);

        let task_file =
            find_task_file(&repo.path).ok_or_else(|| anyhow!("Task file not found"))?;

        // Build update
        let update = TaskUpdate {
            status: input.status,
            title: input.title,
            description: input.description,
            priority: input.priority,
            checklist: None,
        };

        update_task_in_file(&task_file, task_id, update)?;

        // Return updated task
        self.get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task disappeared after update"))
    }

    /// Update task status
    pub async fn update_task_status(&self, task_id: &str, status: TaskStatus) -> Result<Task> {
        self.update_task(
            task_id,
            UpdateTaskInput {
                status: Some(status),
                ..Default::default()
            },
        )
        .await
    }

    /// Delete a task (removes from markdown file)
    pub async fn delete_task(&self, task_id: &str) -> Result<()> {
        let task = self
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(&task.repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", task.repo_id))?;
        drop(storage);

        let task_file =
            find_task_file(&repo.path).ok_or_else(|| anyhow!("Task file not found"))?;

        // Read, filter, write
        let content = std::fs::read_to_string(&task_file)?;
        let mut tasks = parse_task_file(&content, &task.repo_id, &task_file)?;
        tasks.retain(|t| t.id != task_id);

        let new_content = write_task_file(&tasks);
        std::fs::write(&task_file, new_content)?;

        Ok(())
    }

    /// Quick task creation from voice (uses active workspace)
    pub async fn quick_create_task(&self, workspace_path: &Path, title: String) -> Result<Task> {
        let repo = self.ensure_repository(workspace_path).await?;

        // Detect current branch
        let branch = self.get_current_branch(workspace_path).await;

        let input = CreateTaskInput {
            repo_id: repo.id,
            title,
            description: None,
            priority: None,
            labels: None,
            branch,
        };

        self.create_task(input).await
    }

    async fn get_current_branch(&self, path: &Path) -> Option<String> {
        let output = tokio::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    // ==================== Checklist Operations ====================

    /// Add checklist item to task
    pub async fn add_checklist_item(&self, task_id: &str, text: String) -> Result<Task> {
        let task = self
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(&task.repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", task.repo_id))?;
        drop(storage);

        let task_file =
            find_task_file(&repo.path).ok_or_else(|| anyhow!("Task file not found"))?;

        // Read, update, write
        let content = std::fs::read_to_string(&task_file)?;
        let mut tasks = parse_task_file(&content, &task.repo_id, &task_file)?;

        let task_mut = tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| anyhow!("Task not found in file"))?;

        task_mut.checklist.push(ChecklistItem::new(text));
        task_mut.updated_at = Utc::now();

        let new_content = write_task_file(&tasks);
        std::fs::write(&task_file, new_content)?;

        self.get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task disappeared"))
    }

    /// Toggle checklist item
    pub async fn toggle_checklist_item(&self, task_id: &str, item_id: &str) -> Result<Task> {
        let task = self
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(&task.repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", task.repo_id))?;
        drop(storage);

        let task_file =
            find_task_file(&repo.path).ok_or_else(|| anyhow!("Task file not found"))?;

        toggle_checklist_item_in_file(&task_file, task_id, item_id)?;

        self.get_task(task_id)
            .await?
            .ok_or_else(|| anyhow!("Task disappeared"))
    }

    // ==================== File Operations ====================

    /// Get the task file path for a repository
    pub async fn get_task_file_path(&self, repo_id: &str) -> Result<Option<PathBuf>> {
        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", repo_id))?;

        Ok(find_task_file(&repo.path))
    }

    /// Open the task file in Cursor (or default editor)
    pub async fn open_task_file(&self, repo_id: &str) -> Result<()> {
        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", repo_id))?;
        drop(storage);

        let task_file = get_or_create_task_file(&repo.path)?;

        // Try to open with cursor, fall back to xdg-open
        let cursor_result = tokio::process::Command::new("cursor")
            .arg(&task_file)
            .spawn();

        if cursor_result.is_err() {
            // Fallback to xdg-open on Linux
            #[cfg(target_os = "linux")]
            {
                tokio::process::Command::new("xdg-open")
                    .arg(&task_file)
                    .spawn()?;
            }

            #[cfg(target_os = "macos")]
            {
                tokio::process::Command::new("open")
                    .arg(&task_file)
                    .spawn()?;
            }
        }

        Ok(())
    }

    /// Open the repository in Cursor
    pub async fn open_repo_in_cursor(&self, repo_id: &str) -> Result<()> {
        let storage = self.storage.read().await;
        let repo = storage
            .get_repository(repo_id)
            .await?
            .ok_or_else(|| anyhow!("Repository not found: {}", repo_id))?;

        tokio::process::Command::new("cursor")
            .arg(&repo.path)
            .spawn()?;

        Ok(())
    }
}

impl Default for UpdateTaskInput {
    fn default() -> Self {
        Self {
            title: None,
            description: None,
            status: None,
            priority: None,
            labels: None,
            due_date: None,
            branch: None,
        }
    }
}




