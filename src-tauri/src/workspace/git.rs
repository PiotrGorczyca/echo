//! Git context capture via direct CLI calls
//!
//! Simple and reliable - no MCP middleware.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Git repository state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitContext {
    pub branch: String,
    pub status: GitStatus,
    pub recent_commits: Vec<CommitSummary>,
    pub remotes: Vec<String>,
    pub has_uncommitted_changes: bool,
    pub has_untracked_files: bool,
}

/// Git status (staged, modified, untracked)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitStatus {
    pub staged: Vec<FileStatus>,
    pub modified: Vec<FileStatus>,
    pub untracked: Vec<String>,
}

/// File status entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub status: String, // M, A, D, R, C, U
}

/// Commit summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSummary {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

/// Capture git context for a workspace
pub async fn capture_git_context(workspace_path: &Path) -> Result<Option<GitContext>> {
    // Check if this is a git repo
    if !is_git_repo(workspace_path).await {
        return Ok(None);
    }

    let branch = get_current_branch(workspace_path).await?;
    let status = get_git_status(workspace_path).await?;
    let recent_commits = get_recent_commits(workspace_path, 10).await?;
    let remotes = get_remotes(workspace_path).await?;

    let has_uncommitted_changes = !status.staged.is_empty() || !status.modified.is_empty();
    let has_untracked_files = !status.untracked.is_empty();

    Ok(Some(GitContext {
        branch,
        status,
        recent_commits,
        remotes,
        has_uncommitted_changes,
        has_untracked_files,
    }))
}

/// Check if path is a git repository
pub async fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get current branch name
pub async fn get_current_branch(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(path)
        .output()
        .await?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            // Detached HEAD - get commit hash
            let hash_output = Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(path)
                .output()
                .await?;
            Ok(format!(
                "detached@{}",
                String::from_utf8_lossy(&hash_output.stdout).trim()
            ))
        } else {
            Ok(branch)
        }
    } else {
        Ok("unknown".to_string())
    }
}

/// Get git status (staged, modified, untracked)
pub async fn get_git_status(path: &Path) -> Result<GitStatus> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-uall"])
        .current_dir(path)
        .output()
        .await?;

    let mut status = GitStatus::default();

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.len() < 4 {
                continue;
            }

            let index_status = line.chars().next().unwrap_or(' ');
            let worktree_status = line.chars().nth(1).unwrap_or(' ');
            let file_path = line[3..].to_string();

            // Staged changes (index)
            if index_status != ' ' && index_status != '?' {
                status.staged.push(FileStatus {
                    path: file_path.clone(),
                    status: index_status.to_string(),
                });
            }

            // Working tree changes
            if worktree_status != ' ' && worktree_status != '?' {
                status.modified.push(FileStatus {
                    path: file_path.clone(),
                    status: worktree_status.to_string(),
                });
            }

            // Untracked files
            if index_status == '?' && worktree_status == '?' {
                status.untracked.push(file_path);
            }
        }
    }

    Ok(status)
}

/// Get recent commits
pub async fn get_recent_commits(path: &Path, count: usize) -> Result<Vec<CommitSummary>> {
    let output = Command::new("git")
        .args([
            "log",
            &format!("-{}", count),
            "--pretty=format:%H|%h|%s|%an|%ar",
        ])
        .current_dir(path)
        .output()
        .await?;

    let mut commits = Vec::new();

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() == 5 {
                commits.push(CommitSummary {
                    hash: parts[0].to_string(),
                    short_hash: parts[1].to_string(),
                    message: parts[2].to_string(),
                    author: parts[3].to_string(),
                    date: parts[4].to_string(),
                });
            }
        }
    }

    Ok(commits)
}

/// Get remote URLs
pub async fn get_remotes(path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["remote", "-v"])
        .current_dir(path)
        .output()
        .await?;

    let mut remotes = Vec::new();

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && !remotes.contains(&parts[1].to_string()) {
                remotes.push(parts[1].to_string());
            }
        }
    }

    Ok(remotes)
}

/// Get diff for staged changes
pub async fn get_staged_diff(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(path)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get diff for unstaged changes
pub async fn get_unstaged_diff(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff"])
        .current_dir(path)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get diff for a specific file
pub async fn get_file_diff(path: &Path, file_path: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "--", file_path])
        .current_dir(path)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get file content at a specific commit
pub async fn get_file_at_commit(path: &Path, commit: &str, file_path: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["show", &format!("{}:{}", commit, file_path)])
        .current_dir(path)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}






