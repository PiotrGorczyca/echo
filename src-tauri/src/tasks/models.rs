//! Task data models
//! 
//! Core types for repo-scoped task management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Repository - a local or remote code repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

/// Task lifecycle states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum TaskStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked { reason: String },
    InReview { reviewer: Option<String> },
    Done,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Backlog
    }
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Ready => "ready",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked { .. } => "blocked",
            TaskStatus::InReview { .. } => "in_review",
            TaskStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str, data: Option<String>) -> Self {
        match s {
            "backlog" => TaskStatus::Backlog,
            "ready" => TaskStatus::Ready,
            "in_progress" => TaskStatus::InProgress,
            "blocked" => TaskStatus::Blocked {
                reason: data.unwrap_or_default(),
            },
            "in_review" => TaskStatus::InReview { reviewer: data },
            "done" => TaskStatus::Done,
            _ => TaskStatus::Backlog,
        }
    }
}

/// Task priority levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl Priority {
    pub fn as_i32(&self) -> i32 {
        match self {
            Priority::Low => 0,
            Priority::Medium => 1,
            Priority::High => 2,
            Priority::Critical => 3,
        }
    }

    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Priority::Low,
            1 => Priority::Medium,
            2 => Priority::High,
            3 => Priority::Critical,
            _ => Priority::Medium,
        }
    }
}

/// A repo-scoped task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub branch: Option<String>,
    pub checklist: Vec<ChecklistItem>,
    pub linked_paths: Vec<FileAnchor>,
    pub linked_prs: Vec<PrLink>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    pub fn new(repo_id: String, title: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id,
            title,
            description: String::new(),
            status: TaskStatus::default(),
            priority: Priority::default(),
            labels: Vec::new(),
            assignees: Vec::new(),
            due_date: None,
            branch: None,
            checklist: Vec::new(),
            linked_paths: Vec::new(),
            linked_prs: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Checklist item within a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}

impl ChecklistItem {
    pub fn new(text: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            done: false,
            created_at: Utc::now(),
        }
    }
}

/// File anchor - stable reference to code location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnchor {
    pub path: PathBuf,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub symbol_id: Option<String>,
}

impl FileAnchor {
    pub fn file(path: PathBuf) -> Self {
        Self {
            path,
            start_line: None,
            end_line: None,
            symbol_id: None,
        }
    }

    pub fn lines(path: PathBuf, start: u32, end: u32) -> Self {
        Self {
            path,
            start_line: Some(start),
            end_line: Some(end),
            symbol_id: None,
        }
    }
}

/// PR link with status tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrLink {
    pub url: String,
    pub number: u32,
    pub status: PrStatus,
    pub branch: String,
    pub reviewers: Vec<String>,
    pub ci_status: Option<CiStatus>,
}

/// PR status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PrStatus {
    Draft,
    Open,
    Merged,
    Closed,
}

/// CI status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CiStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

/// Evidence attached to task timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub task_id: String,
    pub kind: EvidenceKind,
    pub timestamp: DateTime<Utc>,
    pub actor: Actor,
}

impl Evidence {
    pub fn new(task_id: String, kind: EvidenceKind, actor: Actor) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_id,
            kind,
            timestamp: Utc::now(),
            actor,
        }
    }
}

/// Evidence types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EvidenceKind {
    Command {
        cmd: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
        duration_ms: u64,
        working_dir: Option<String>,
    },
    TestResult {
        passed: bool,
        test_name: String,
        output: String,
    },
    FileChange {
        path: String,
        diff: String,
    },
    Note {
        text: String,
    },
    Comment {
        text: String,
    },
    StatusChange {
        from: String,
        to: String,
    },
}

/// Who created the evidence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Actor {
    User,
    Agent,
    System,
}

/// Timeline event - wrapper for evidence with context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub evidence: Evidence,
    pub related_checklist_item: Option<String>,
}

/// Input for creating a new task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskInput {
    pub repo_id: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub labels: Option<Vec<String>>,
    pub branch: Option<String>,
}

/// Input for updating a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<Priority>,
    pub labels: Option<Vec<String>>,
    pub due_date: Option<DateTime<Utc>>,
    pub branch: Option<String>,
}

/// Filters for querying tasks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskFilters {
    pub repo_id: Option<String>,
    pub status: Option<Vec<TaskStatus>>,
    pub priority: Option<Vec<Priority>>,
    pub labels: Option<Vec<String>>,
    pub search: Option<String>,
}






