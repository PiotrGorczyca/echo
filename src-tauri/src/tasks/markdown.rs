//! Markdown task parser and writer
//!
//! Tasks are stored as markdown files in repositories.
//! This module handles parsing and writing task markdown files.
//!
//! ## Format
//!
//! ```markdown
//! # Project Tasks
//!
//! ## In Progress
//! - [ ] Task title @branch:feature-x #high
//!   Description goes here on indented lines
//!   - [ ] Subtask 1
//!   - [x] Subtask 2 (done)
//!
//! ## Backlog  
//! - [ ] Another task #medium
//! ```

use super::models::*;
use anyhow::{anyhow, Result};
use chrono::Utc;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Default task file locations to check (in order of preference)
pub const DEFAULT_TASK_FILES: &[&str] = &[
    ".echo/tasks.md",
    "TODO.md",
    "TASKS.md",
    "docs/tasks.md",
    ".tasks.md",
];

/// Parse a markdown task file
pub fn parse_task_file(content: &str, repo_id: &str, file_path: &Path) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    let mut current_section: Option<TaskStatus> = None;
    let mut current_task: Option<TaskBuilder> = None;
    let mut line_number = 0;

    for line in content.lines() {
        line_number += 1;

        // Check for section headers (## Status)
        if line.starts_with("## ") {
            // Save previous task if exists
            if let Some(builder) = current_task.take() {
                tasks.push(builder.build(repo_id, file_path, line_number - 1));
            }

            let section_name = line.trim_start_matches("## ").trim();
            current_section = parse_section_to_status(section_name);
            continue;
        }

        // Check for task items (- [ ] or - [x])
        if let Some(task_match) = parse_task_line(line) {
            // Save previous task if exists
            if let Some(builder) = current_task.take() {
                tasks.push(builder.build(repo_id, file_path, line_number - 1));
            }

            let status = current_section.clone().unwrap_or(TaskStatus::Backlog);
            current_task = Some(TaskBuilder::new(task_match, status, line_number));
            continue;
        }

        // Check for subtask/checklist items (indented - [ ] or - [x])
        if let Some(checklist_match) = parse_checklist_line(line) {
            if let Some(ref mut builder) = current_task {
                builder.add_checklist_item(checklist_match);
            }
            continue;
        }

        // Check for description lines (indented text)
        if line.starts_with("  ") && !line.trim().is_empty() {
            if let Some(ref mut builder) = current_task {
                builder.append_description(line.trim());
            }
        }
    }

    // Save last task
    if let Some(builder) = current_task {
        tasks.push(builder.build(repo_id, file_path, line_number));
    }

    Ok(tasks)
}

/// Parse section header to TaskStatus
fn parse_section_to_status(section: &str) -> Option<TaskStatus> {
    let lower = section.to_lowercase();
    match lower.as_str() {
        "backlog" | "todo" | "to do" => Some(TaskStatus::Backlog),
        "ready" | "up next" | "next" => Some(TaskStatus::Ready),
        "in progress" | "doing" | "wip" | "current" => Some(TaskStatus::InProgress),
        "blocked" | "waiting" => Some(TaskStatus::Blocked {
            reason: String::new(),
        }),
        "in review" | "review" | "pr" => Some(TaskStatus::InReview { reviewer: None }),
        "done" | "completed" | "finished" => Some(TaskStatus::Done),
        _ => None,
    }
}

/// Parsed task line data
#[derive(Debug)]
struct TaskLineMatch {
    title: String,
    done: bool,
    priority: Priority,
    branch: Option<String>,
    labels: Vec<String>,
}

/// Parse a task line (- [ ] or - [x])
fn parse_task_line(line: &str) -> Option<TaskLineMatch> {
    let trimmed = line.trim();

    // Match: - [ ] or - [x] followed by text
    let (done, rest) = if trimmed.starts_with("- [ ] ") {
        (false, trimmed.strip_prefix("- [ ] ")?)
    } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
        (true, &trimmed[6..])
    } else {
        return None;
    };

    // Don't match indented items (those are checklists)
    if line.starts_with("  ") || line.starts_with("\t") {
        return None;
    }

    let (title, priority, branch, labels) = parse_task_metadata(rest);

    Some(TaskLineMatch {
        title,
        done,
        priority,
        branch,
        labels,
    })
}

/// Parse checklist line (indented - [ ] or - [x])
fn parse_checklist_line(line: &str) -> Option<(String, bool)> {
    // Must be indented
    if !line.starts_with("  ") && !line.starts_with("\t") {
        return None;
    }

    let trimmed = line.trim();

    if trimmed.starts_with("- [ ] ") {
        Some((trimmed[6..].to_string(), false))
    } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
        Some((trimmed[6..].to_string(), true))
    } else {
        None
    }
}

/// Parse metadata from task title (priority, branch, labels)
fn parse_task_metadata(text: &str) -> (String, Priority, Option<String>, Vec<String>) {
    let mut title = text.to_string();
    let mut priority = Priority::Medium;
    let mut branch = None;
    let mut labels = Vec::new();

    // Extract @branch:xxx
    let branch_re = Regex::new(r"@branch:(\S+)").unwrap();
    if let Some(caps) = branch_re.captures(&title) {
        branch = Some(caps[1].to_string());
        title = branch_re.replace(&title, "").to_string();
    }

    // Extract #priority or #label
    let tag_re = Regex::new(r"#(\S+)").unwrap();
    let mut new_title = title.clone();
    for caps in tag_re.captures_iter(&title) {
        let tag = &caps[1].to_lowercase();
        match tag.as_str() {
            "critical" | "p0" => priority = Priority::Critical,
            "high" | "p1" => priority = Priority::High,
            "medium" | "p2" => priority = Priority::Medium,
            "low" | "p3" => priority = Priority::Low,
            _ => labels.push(caps[1].to_string()),
        }
        new_title = new_title.replace(&caps[0], "");
    }

    (new_title.trim().to_string(), priority, branch, labels)
}

/// Builder for constructing tasks from parsed lines
struct TaskBuilder {
    match_data: TaskLineMatch,
    status: TaskStatus,
    start_line: usize,
    description: Vec<String>,
    checklist: Vec<ChecklistItem>,
}

impl TaskBuilder {
    fn new(match_data: TaskLineMatch, status: TaskStatus, start_line: usize) -> Self {
        // Override status if task is marked done
        let status = if match_data.done {
            TaskStatus::Done
        } else {
            status
        };

        Self {
            match_data,
            status,
            start_line,
            description: Vec::new(),
            checklist: Vec::new(),
        }
    }

    fn append_description(&mut self, line: &str) {
        self.description.push(line.to_string());
    }

    fn add_checklist_item(&mut self, (text, done): (String, bool)) {
        self.checklist.push(ChecklistItem {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            done,
            created_at: Utc::now(),
        });
    }

    fn build(self, repo_id: &str, file_path: &Path, _end_line: usize) -> Task {
        let now = Utc::now();

        // Create a deterministic ID based on file path and title
        let id = format!(
            "{}-{}",
            file_path.display(),
            self.match_data.title.to_lowercase().replace(' ', "-")
        );
        let id = format!("{:x}", md5_hash(&id));

        Task {
            id,
            repo_id: repo_id.to_string(),
            title: self.match_data.title,
            description: self.description.join("\n"),
            status: self.status,
            priority: self.match_data.priority,
            labels: self.match_data.labels,
            assignees: Vec::new(),
            due_date: None,
            branch: self.match_data.branch,
            checklist: self.checklist,
            linked_paths: vec![FileAnchor {
                path: file_path.to_path_buf(),
                start_line: Some(self.start_line as u32),
                end_line: None,
                symbol_id: None,
            }],
            linked_prs: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Simple MD5-like hash for deterministic IDs
fn md5_hash(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

// ==================== Writing Tasks ====================

/// Write tasks back to markdown format
pub fn write_task_file(tasks: &[Task]) -> String {
    let mut output = String::new();
    output.push_str("# Tasks\n\n");

    // Group by status
    let mut by_status: HashMap<String, Vec<&Task>> = HashMap::new();
    for task in tasks {
        let status_name = status_to_section_name(&task.status);
        by_status.entry(status_name).or_default().push(task);
    }

    // Write sections in order
    let section_order = ["In Progress", "Ready", "Backlog", "Blocked", "In Review", "Done"];

    for section in section_order {
        if let Some(section_tasks) = by_status.get(section) {
            if !section_tasks.is_empty() {
                output.push_str(&format!("## {}\n\n", section));

                for task in section_tasks {
                    output.push_str(&format_task(task));
                    output.push('\n');
                }

                output.push('\n');
            }
        }
    }

    output
}

/// Convert status to section name
fn status_to_section_name(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Backlog => "Backlog".to_string(),
        TaskStatus::Ready => "Ready".to_string(),
        TaskStatus::InProgress => "In Progress".to_string(),
        TaskStatus::Blocked { .. } => "Blocked".to_string(),
        TaskStatus::InReview { .. } => "In Review".to_string(),
        TaskStatus::Done => "Done".to_string(),
    }
}

/// Format a single task as markdown
fn format_task(task: &Task) -> String {
    let mut line = String::new();

    // Checkbox
    let checkbox = if task.status == TaskStatus::Done {
        "- [x] "
    } else {
        "- [ ] "
    };
    line.push_str(checkbox);

    // Title
    line.push_str(&task.title);

    // Branch
    if let Some(branch) = &task.branch {
        line.push_str(&format!(" @branch:{}", branch));
    }

    // Priority (if not medium)
    match task.priority {
        Priority::Critical => line.push_str(" #critical"),
        Priority::High => line.push_str(" #high"),
        Priority::Low => line.push_str(" #low"),
        Priority::Medium => {} // Don't add medium, it's default
    }

    // Labels
    for label in &task.labels {
        line.push_str(&format!(" #{}", label));
    }

    line.push('\n');

    // Description (indented)
    if !task.description.is_empty() {
        for desc_line in task.description.lines() {
            line.push_str(&format!("  {}\n", desc_line));
        }
    }

    // Checklist items (indented)
    for item in &task.checklist {
        let check = if item.done { "[x]" } else { "[ ]" };
        line.push_str(&format!("  - {} {}\n", check, item.text));
    }

    line
}

/// Find the task file in a repository
pub fn find_task_file(repo_path: &Path) -> Option<PathBuf> {
    for file in DEFAULT_TASK_FILES {
        let path = repo_path.join(file);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Get or create the task file path for a repository
pub fn get_or_create_task_file(repo_path: &Path) -> Result<PathBuf> {
    // Check if any existing file exists
    if let Some(existing) = find_task_file(repo_path) {
        return Ok(existing);
    }

    // Create default: .echo/tasks.md
    let echo_dir = repo_path.join(".echo");
    std::fs::create_dir_all(&echo_dir)?;

    let task_file = echo_dir.join("tasks.md");

    // Create initial content
    let initial_content = r#"# Tasks

## In Progress

## Backlog

## Done

"#;

    std::fs::write(&task_file, initial_content)?;
    Ok(task_file)
}

/// Update a single task in the markdown file
pub fn update_task_in_file(file_path: &Path, task_id: &str, update: TaskUpdate) -> Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let repo_id = "temp"; // Not used for comparison

    // Parse existing tasks
    let mut tasks = parse_task_file(&content, repo_id, file_path)?;

    // Find and update the task
    let task = tasks
        .iter_mut()
        .find(|t| t.id == task_id)
        .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

    // Apply updates
    if let Some(status) = update.status {
        task.status = status;
    }
    if let Some(title) = update.title {
        task.title = title;
    }
    if let Some(description) = update.description {
        task.description = description;
    }
    if let Some(priority) = update.priority {
        task.priority = priority;
    }
    if let Some(checklist) = update.checklist {
        task.checklist = checklist;
    }

    task.updated_at = Utc::now();

    // Write back
    let new_content = write_task_file(&tasks);
    std::fs::write(file_path, new_content)?;

    Ok(())
}

/// Task update struct
#[derive(Debug, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub checklist: Option<Vec<ChecklistItem>>,
}

/// Add a new task to the file
pub fn add_task_to_file(file_path: &Path, task: &Task) -> Result<()> {
    let content = if file_path.exists() {
        std::fs::read_to_string(file_path)?
    } else {
        String::new()
    };

    let repo_id = "temp";
    let mut tasks = if content.is_empty() {
        Vec::new()
    } else {
        parse_task_file(&content, repo_id, file_path)?
    };

    tasks.push(task.clone());

    let new_content = write_task_file(&tasks);
    std::fs::write(file_path, new_content)?;

    Ok(())
}

/// Toggle a checklist item in a task
pub fn toggle_checklist_item_in_file(
    file_path: &Path,
    task_id: &str,
    item_id: &str,
) -> Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let repo_id = "temp";

    let mut tasks = parse_task_file(&content, repo_id, file_path)?;

    let task = tasks
        .iter_mut()
        .find(|t| t.id == task_id)
        .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;

    let item = task
        .checklist
        .iter_mut()
        .find(|i| i.id == item_id)
        .ok_or_else(|| anyhow!("Checklist item not found: {}", item_id))?;

    item.done = !item.done;
    task.updated_at = Utc::now();

    let new_content = write_task_file(&tasks);
    std::fs::write(file_path, new_content)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_task() {
        let content = r#"# Tasks

## In Progress
- [ ] Fix the auth bug #high @branch:fix-auth

## Backlog
- [ ] Add user settings
- [x] Setup project
"#;

        let tasks = parse_task_file(content, "repo1", Path::new("tasks.md")).unwrap();
        assert_eq!(tasks.len(), 3);

        assert_eq!(tasks[0].title, "Fix the auth bug");
        assert_eq!(tasks[0].priority, Priority::High);
        assert_eq!(tasks[0].branch, Some("fix-auth".to_string()));
        assert_eq!(tasks[0].status, TaskStatus::InProgress);

        assert_eq!(tasks[1].title, "Add user settings");
        assert_eq!(tasks[1].status, TaskStatus::Backlog);

        assert_eq!(tasks[2].title, "Setup project");
        assert_eq!(tasks[2].status, TaskStatus::Done); // Marked [x] overrides section
    }

    #[test]
    fn test_parse_task_with_checklist() {
        let content = r#"## In Progress
- [ ] Implement feature
  This is the description
  - [ ] Step 1
  - [x] Step 2
"#;

        let tasks = parse_task_file(content, "repo1", Path::new("tasks.md")).unwrap();
        assert_eq!(tasks.len(), 1);

        let task = &tasks[0];
        assert_eq!(task.title, "Implement feature");
        assert_eq!(task.description, "This is the description");
        assert_eq!(task.checklist.len(), 2);
        assert!(!task.checklist[0].done);
        assert!(task.checklist[1].done);
    }

    #[test]
    fn test_write_task_file() {
        let tasks = vec![Task::new("repo1".to_string(), "Test task".to_string())];

        let output = write_task_file(&tasks);
        assert!(output.contains("## Backlog"));
        assert!(output.contains("- [ ] Test task"));
    }
}






