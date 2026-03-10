//! Prompt builder for Claude Code
//!
//! Builds rich, grounded prompts with task context and workspace state.

use crate::tasks::{Task, TaskStatus};
use crate::workspace::WorkspaceContext;
use serde::{Deserialize, Serialize};

/// Prompt configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    /// Include git status in context
    pub include_git: bool,
    /// Include recent files in context
    pub include_recent_files: bool,
    /// Maximum recent commits to show
    pub max_commits: usize,
    /// Maximum recent files to show
    pub max_files: usize,
    /// Include checklist in task context
    pub include_checklist: bool,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            include_git: true,
            include_recent_files: true,
            max_commits: 5,
            max_files: 10,
            include_checklist: true,
        }
    }
}

/// Prompt builder for Claude Code
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    config: PromptConfig,
    task: Option<Task>,
    workspace: Option<WorkspaceContext>,
    user_request: String,
    additional_context: Vec<String>,
    constraints: Vec<String>,
}

impl PromptBuilder {
    /// Create new prompt builder with user request
    pub fn new(user_request: impl Into<String>) -> Self {
        Self {
            config: PromptConfig::default(),
            task: None,
            workspace: None,
            user_request: user_request.into(),
            additional_context: Vec::new(),
            constraints: Vec::new(),
        }
    }

    /// Set configuration
    pub fn with_config(mut self, config: PromptConfig) -> Self {
        self.config = config;
        self
    }

    /// Add task context
    pub fn with_task(mut self, task: Task) -> Self {
        self.task = Some(task);
        self
    }

    /// Add workspace context
    pub fn with_workspace(mut self, workspace: WorkspaceContext) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// Add additional context (e.g., file contents, error messages)
    pub fn add_context(mut self, context: impl Into<String>) -> Self {
        self.additional_context.push(context.into());
        self
    }

    /// Add constraint (e.g., "don't modify tests", "use existing patterns")
    pub fn add_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.constraints.push(constraint.into());
        self
    }

    /// Build the final prompt
    pub fn build(&self) -> String {
        let mut sections = Vec::new();

        // Task context (if linked to a task)
        if let Some(task) = &self.task {
            sections.push(self.build_task_section(task));
        }

        // Workspace context
        if let Some(workspace) = &self.workspace {
            sections.push(self.build_workspace_section(workspace));
        }

        // User request
        sections.push(self.build_request_section());

        // Additional context
        if !self.additional_context.is_empty() {
            sections.push(self.build_additional_context_section());
        }

        // Constraints
        if !self.constraints.is_empty() {
            sections.push(self.build_constraints_section());
        }

        sections.join("\n\n")
    }

    fn build_task_section(&self, task: &Task) -> String {
        let mut section = String::new();
        
        section.push_str("## Current Task\n\n");
        section.push_str(&format!("**Title**: {}\n", task.title));
        
        if !task.description.is_empty() {
            section.push_str(&format!("**Description**: {}\n", task.description));
        }
        
        section.push_str(&format!("**Status**: {}\n", format_status(&task.status)));
        section.push_str(&format!("**Priority**: {:?}\n", task.priority));

        if let Some(branch) = &task.branch {
            section.push_str(&format!("**Branch**: `{}`\n", branch));
        }

        if !task.labels.is_empty() {
            section.push_str(&format!("**Labels**: {}\n", task.labels.join(", ")));
        }

        // Checklist
        if self.config.include_checklist && !task.checklist.is_empty() {
            section.push_str("\n### Checklist\n");
            for item in &task.checklist {
                let check = if item.done { "x" } else { " " };
                section.push_str(&format!("- [{}] {}\n", check, item.text));
            }
        }

        // Linked files
        if !task.linked_paths.is_empty() {
            section.push_str("\n### Related Files\n");
            for anchor in &task.linked_paths {
                let location = if let (Some(start), Some(end)) = (anchor.start_line, anchor.end_line) {
                    format!("{}:{}:{}", anchor.path.display(), start, end)
                } else {
                    anchor.path.display().to_string()
                };
                section.push_str(&format!("- `{}`\n", location));
            }
        }

        section
    }

    fn build_workspace_section(&self, workspace: &WorkspaceContext) -> String {
        let mut section = String::new();
        
        section.push_str("## Workspace\n\n");
        section.push_str(&format!("**Project**: {} ({:?})\n", workspace.name, workspace.project_type));
        section.push_str(&format!("**Path**: `{}`\n", workspace.path.display()));

        // Git status
        if self.config.include_git {
            if let Some(git) = &workspace.git {
                section.push_str(&format!("\n**Branch**: `{}`\n", git.branch));
                
                // Status summary
                let changed = git.status.staged.len() + git.status.modified.len();
                if changed > 0 {
                    section.push_str(&format!("**Changes**: {} files modified\n", changed));
                }
                if !git.status.untracked.is_empty() {
                    section.push_str(&format!("**Untracked**: {} files\n", git.status.untracked.len()));
                }

                // Recent commits
                if !git.recent_commits.is_empty() {
                    section.push_str("\n### Recent Commits\n");
                    for commit in git.recent_commits.iter().take(self.config.max_commits) {
                        section.push_str(&format!(
                            "- `{}` {} ({})\n",
                            commit.short_hash, commit.message, commit.date
                        ));
                    }
                }

                // Modified files
                if !git.status.modified.is_empty() || !git.status.staged.is_empty() {
                    section.push_str("\n### Changed Files\n");
                    for f in git.status.staged.iter().take(10) {
                        section.push_str(&format!("- `{}` (staged, {})\n", f.path, f.status));
                    }
                    for f in git.status.modified.iter().take(10) {
                        section.push_str(&format!("- `{}` ({})\n", f.path, f.status));
                    }
                }
            }
        }

        // Recent files
        if self.config.include_recent_files && !workspace.recent_files.is_empty() {
            section.push_str("\n### Recently Modified Files\n");
            for f in workspace.recent_files.iter().take(self.config.max_files) {
                section.push_str(&format!("- `{}`\n", f.relative_path));
            }
        }

        section
    }

    fn build_request_section(&self) -> String {
        format!("## Request\n\n{}", self.user_request)
    }

    fn build_additional_context_section(&self) -> String {
        let mut section = String::from("## Additional Context\n\n");
        for (i, ctx) in self.additional_context.iter().enumerate() {
            if self.additional_context.len() > 1 {
                section.push_str(&format!("### Context {}\n", i + 1));
            }
            section.push_str(ctx);
            section.push('\n');
        }
        section
    }

    fn build_constraints_section(&self) -> String {
        let mut section = String::from("## Constraints\n\n");
        for constraint in &self.constraints {
            section.push_str(&format!("- {}\n", constraint));
        }
        section
    }
}

fn format_status(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Backlog => "Backlog".to_string(),
        TaskStatus::Ready => "Ready".to_string(),
        TaskStatus::InProgress => "In Progress".to_string(),
        TaskStatus::Blocked { reason } => format!("Blocked ({})", reason),
        TaskStatus::InReview { reviewer } => {
            if let Some(r) = reviewer {
                format!("In Review ({})", r)
            } else {
                "In Review".to_string()
            }
        }
        TaskStatus::Done => "Done".to_string(),
    }
}

/// Quick prompt builders for common scenarios
impl PromptBuilder {
    /// Create a "fix this" prompt
    pub fn fix(issue: impl Into<String>) -> Self {
        Self::new(format!("Fix the following issue:\n\n{}", issue.into()))
            .add_constraint("Make minimal changes to fix the issue")
            .add_constraint("Don't refactor unrelated code")
    }

    /// Create a "explain this" prompt
    pub fn explain(code_or_concept: impl Into<String>) -> Self {
        Self::new(format!("Explain the following:\n\n{}", code_or_concept.into()))
    }

    /// Create a "refactor" prompt
    pub fn refactor(what: impl Into<String>, how: impl Into<String>) -> Self {
        Self::new(format!(
            "Refactor the following:\n\n{}\n\nApproach: {}",
            what.into(),
            how.into()
        ))
    }

    /// Create a "add feature" prompt
    pub fn add_feature(description: impl Into<String>) -> Self {
        Self::new(format!("Implement the following feature:\n\n{}", description.into()))
            .add_constraint("Follow existing code patterns and style")
            .add_constraint("Add appropriate tests if applicable")
    }

    /// Create a "debug" prompt
    pub fn debug(error: impl Into<String>) -> Self {
        Self::new(format!(
            "Debug and fix the following error:\n\n```\n{}\n```",
            error.into()
        ))
        .add_constraint("First explain what's causing the error")
        .add_constraint("Then provide a fix")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_prompt() {
        let prompt = PromptBuilder::new("Fix the auth bug")
            .add_constraint("Don't break existing tests")
            .build();

        assert!(prompt.contains("Fix the auth bug"));
        assert!(prompt.contains("Don't break existing tests"));
    }

    #[test]
    fn test_fix_prompt() {
        let prompt = PromptBuilder::fix("Users can't log in after password reset").build();

        assert!(prompt.contains("Fix the following issue"));
        assert!(prompt.contains("password reset"));
        assert!(prompt.contains("minimal changes"));
    }
}






