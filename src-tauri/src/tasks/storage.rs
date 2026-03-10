//! SQLite storage for repository registry and task cache
//!
//! Tasks are stored in MARKDOWN files within repos (source of truth).
//! SQLite tracks repositories AND caches tasks for quick queries.

use super::models::{Repository, Task, TaskStatus, Priority};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::path::Path;

/// Repository registry using SQLite
pub struct TaskStorage {
    pool: Pool<Sqlite>,
}

impl TaskStorage {
    /// Create a new storage, initializing the database
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await?;

        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Initialize database schema
    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS repositories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                remote_url TEXT,
                default_branch TEXT NOT NULL DEFAULT 'main',
                created_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_repos_path ON repositories(path);
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Task cache table (synced from markdown files)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS task_cache (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'backlog',
                priority TEXT NOT NULL DEFAULT 'medium',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (repo_id) REFERENCES repositories(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_task_cache_repo ON task_cache(repo_id);
            CREATE INDEX IF NOT EXISTS idx_task_cache_status ON task_cache(status);
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
    
    // ==================== Task Cache Operations ====================
    
    /// Sync tasks from a list to the cache (replaces all tasks for given repo)
    pub async fn sync_tasks_to_cache(&self, repo_id: &str, tasks: &[Task]) -> Result<()> {
        // Delete existing cached tasks for this repo
        sqlx::query("DELETE FROM task_cache WHERE repo_id = ?")
            .bind(repo_id)
            .execute(&self.pool)
            .await?;
        
        // Insert new tasks
        for task in tasks {
            sqlx::query(
                r#"
                INSERT INTO task_cache (id, repo_id, title, description, status, priority, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&task.id)
            .bind(repo_id)
            .bind(&task.title)
            .bind(&task.description)
            .bind(task.status.as_str())
            .bind(format!("{:?}", task.priority).to_lowercase())
            .bind(task.created_at.to_rfc3339())
            .bind(task.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }
        
        Ok(())
    }
    
    /// Get cached tasks for a repository
    pub async fn get_cached_tasks(&self, repo_id: &str) -> Result<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT * FROM task_cache WHERE repo_id = ? ORDER BY created_at DESC"
        )
        .bind(repo_id)
        .fetch_all(&self.pool)
        .await?;
        
        let mut tasks = Vec::new();
        for row in rows {
            if let Ok(task) = Self::row_to_task(&row) {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }
    
    /// Get all cached tasks
    pub async fn get_all_cached_tasks(&self) -> Result<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT * FROM task_cache ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        
        let mut tasks = Vec::new();
        for row in rows {
            if let Ok(task) = Self::row_to_task(&row) {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }
    
    /// Get next task (highest priority non-done task)
    pub async fn get_next_cached_task(&self, repo_id: Option<&str>) -> Result<Option<Task>> {
        let query = if let Some(rid) = repo_id {
            sqlx::query(
                r#"
                SELECT * FROM task_cache 
                WHERE repo_id = ? AND status NOT IN ('done')
                ORDER BY 
                    CASE status 
                        WHEN 'in_progress' THEN 1 
                        WHEN 'ready' THEN 2 
                        WHEN 'backlog' THEN 3 
                        ELSE 4 
                    END,
                    CASE priority 
                        WHEN 'critical' THEN 1 
                        WHEN 'high' THEN 2 
                        WHEN 'medium' THEN 3 
                        WHEN 'low' THEN 4 
                        ELSE 5 
                    END,
                    created_at DESC
                LIMIT 1
                "#
            )
            .bind(rid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT * FROM task_cache 
                WHERE status NOT IN ('done')
                ORDER BY 
                    CASE status 
                        WHEN 'in_progress' THEN 1 
                        WHEN 'ready' THEN 2 
                        WHEN 'backlog' THEN 3 
                        ELSE 4 
                    END,
                    CASE priority 
                        WHEN 'critical' THEN 1 
                        WHEN 'high' THEN 2 
                        WHEN 'medium' THEN 3 
                        WHEN 'low' THEN 4 
                        ELSE 5 
                    END,
                    created_at DESC
                LIMIT 1
                "#
            )
            .fetch_optional(&self.pool)
            .await?
        };
        
        query.map(|row| Self::row_to_task(&row)).transpose()
    }
    
    fn row_to_task(row: &sqlx::sqlite::SqliteRow) -> Result<Task> {
        let status_str: String = row.get("status");
        let priority_str: String = row.get("priority");
        
        let status = match status_str.as_str() {
            "backlog" => TaskStatus::Backlog,
            "ready" => TaskStatus::Ready,
            "in_progress" => TaskStatus::InProgress,
            "in_review" => TaskStatus::InReview { reviewer: None },
            "done" => TaskStatus::Done,
            s if s.starts_with("blocked") => TaskStatus::Blocked { reason: String::new() },
            _ => TaskStatus::Backlog,
        };
        
        let priority = match priority_str.as_str() {
            "critical" => Priority::Critical,
            "high" => Priority::High,
            "medium" => Priority::Medium,
            "low" => Priority::Low,
            _ => Priority::Medium,
        };
        
        Ok(Task {
            id: row.get("id"),
            repo_id: row.get("repo_id"),
            title: row.get("title"),
            description: row.get("description"),
            status,
            priority,
            labels: Vec::new(),
            assignees: Vec::new(),
            due_date: None,
            branch: None,
            checklist: Vec::new(),
            linked_paths: Vec::new(),
            linked_prs: Vec::new(),
            created_at: DateTime::parse_from_rfc3339(row.get("created_at"))
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(row.get("updated_at"))
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }

    // ==================== Repository Operations ====================

    /// Create a new repository entry
    pub async fn create_repository(&self, repo: &Repository) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO repositories (id, name, path, remote_url, default_branch, created_at, last_accessed)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                remote_url = excluded.remote_url,
                default_branch = excluded.default_branch,
                last_accessed = excluded.last_accessed
            "#,
        )
        .bind(&repo.id)
        .bind(&repo.name)
        .bind(repo.path.to_string_lossy().to_string())
        .bind(&repo.remote_url)
        .bind(&repo.default_branch)
        .bind(repo.created_at.to_rfc3339())
        .bind(repo.last_accessed.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get repository by ID
    pub async fn get_repository(&self, id: &str) -> Result<Option<Repository>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, path, remote_url, default_branch, created_at, last_accessed
            FROM repositories WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Self::row_to_repository(&row)?)),
            None => Ok(None),
        }
    }

    /// Get repository by path
    pub async fn get_repository_by_path(&self, path: &Path) -> Result<Option<Repository>> {
        let path_str = path.to_string_lossy().to_string();
        let row = sqlx::query(
            r#"
            SELECT id, name, path, remote_url, default_branch, created_at, last_accessed
            FROM repositories WHERE path = ?
            "#,
        )
        .bind(&path_str)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Self::row_to_repository(&row)?)),
            None => Ok(None),
        }
    }

    /// List all repositories
    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, path, remote_url, default_branch, created_at, last_accessed
            FROM repositories ORDER BY last_accessed DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_repository).collect()
    }

    /// Update repository last accessed time
    pub async fn touch_repository(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE repositories SET last_accessed = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a repository entry
    pub async fn delete_repository(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM repositories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update repository details
    pub async fn update_repository(&self, repo: &Repository) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE repositories SET
                name = ?, remote_url = ?, default_branch = ?, last_accessed = ?
            WHERE id = ?
            "#,
        )
        .bind(&repo.name)
        .bind(&repo.remote_url)
        .bind(&repo.default_branch)
        .bind(Utc::now().to_rfc3339())
        .bind(&repo.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn row_to_repository(row: &sqlx::sqlite::SqliteRow) -> Result<Repository> {
        Ok(Repository {
            id: row.get("id"),
            name: row.get("name"),
            path: std::path::PathBuf::from(row.get::<String, _>("path")),
            remote_url: row.get("remote_url"),
            default_branch: row.get("default_branch"),
            created_at: DateTime::parse_from_rfc3339(row.get("created_at"))?
                .with_timezone(&Utc),
            last_accessed: DateTime::parse_from_rfc3339(row.get("last_accessed"))?
                .with_timezone(&Utc),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_and_get_repository() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = TaskStorage::new(&db_path).await.unwrap();

        let repo = Repository {
            id: "repo1".to_string(),
            name: "test-repo".to_string(),
            path: std::path::PathBuf::from("/tmp/test"),
            remote_url: Some("https://github.com/test/repo".to_string()),
            default_branch: "main".to_string(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };
        storage.create_repository(&repo).await.unwrap();

        let fetched = storage.get_repository("repo1").await.unwrap().unwrap();
        assert_eq!(fetched.name, "test-repo");
        assert_eq!(fetched.default_branch, "main");
    }

    #[tokio::test]
    async fn test_get_repository_by_path() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = TaskStorage::new(&db_path).await.unwrap();

        let repo = Repository {
            id: "repo1".to_string(),
            name: "test-repo".to_string(),
            path: std::path::PathBuf::from("/tmp/my-project"),
            remote_url: None,
            default_branch: "main".to_string(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };
        storage.create_repository(&repo).await.unwrap();

        let fetched = storage
            .get_repository_by_path(&std::path::PathBuf::from("/tmp/my-project"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, "repo1");
    }
}



