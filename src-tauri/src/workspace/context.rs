//! Full workspace context - combines all context sources
//!
//! This is what gets passed to Claude Code for grounding.

use super::detection::WorkspaceDetector;
use super::git::{capture_git_context, GitContext};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Project type detection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectType {
    Rust,
    JavaScript,
    TypeScript,
    Python,
    Go,
    Java,
    Ruby,
    Elixir,
    Unknown,
}

/// Recently modified file info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub modified: String, // ISO 8601
    pub size: u64,
}

/// Full workspace context for Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    pub path: PathBuf,
    pub name: String,
    pub project_type: ProjectType,
    pub git: Option<GitContext>,
    pub recent_files: Vec<RecentFile>,
    pub key_files: Vec<String>, // package.json, Cargo.toml, etc.
}

impl WorkspaceContext {
    /// Capture full context for a workspace
    pub async fn capture(workspace_path: &Path) -> Result<Self> {
        let name = workspace_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let project_type = detect_project_type(workspace_path);
        let git = capture_git_context(workspace_path).await?;
        let recent_files = find_recent_files(workspace_path, 20).await?;
        let key_files = find_key_files(workspace_path);

        Ok(Self {
            path: workspace_path.to_path_buf(),
            name,
            project_type,
            git,
            recent_files,
            key_files,
        })
    }

    /// Detect and capture context for active workspace
    pub async fn capture_active() -> Result<Option<Self>> {
        let detector = WorkspaceDetector::new();
        
        if let Some(detected) = detector.detect().await {
            let context = Self::capture(&detected.path).await?;
            Ok(Some(context))
        } else {
            Ok(None)
        }
    }

    /// Format as markdown for prompt building
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        
        md.push_str(&format!("## Workspace: {}\n", self.name));
        md.push_str(&format!("**Path**: `{}`\n", self.path.display()));
        md.push_str(&format!("**Type**: {:?}\n\n", self.project_type));

        if let Some(git) = &self.git {
            md.push_str("### Git Status\n");
            md.push_str(&format!("**Branch**: `{}`\n", git.branch));
            
            if git.has_uncommitted_changes {
                md.push_str("⚠️ Has uncommitted changes\n");
            }
            if git.has_untracked_files {
                md.push_str(&format!("📄 {} untracked files\n", git.status.untracked.len()));
            }
            
            if !git.status.staged.is_empty() {
                md.push_str("\n**Staged**:\n");
                for f in &git.status.staged {
                    md.push_str(&format!("- `{}` ({})\n", f.path, f.status));
                }
            }
            
            if !git.status.modified.is_empty() {
                md.push_str("\n**Modified**:\n");
                for f in &git.status.modified {
                    md.push_str(&format!("- `{}` ({})\n", f.path, f.status));
                }
            }

            if !git.recent_commits.is_empty() {
                md.push_str("\n**Recent Commits**:\n");
                for c in git.recent_commits.iter().take(5) {
                    md.push_str(&format!("- `{}` {} ({})\n", c.short_hash, c.message, c.date));
                }
            }
            md.push_str("\n");
        }

        if !self.key_files.is_empty() {
            md.push_str("### Key Files\n");
            for f in &self.key_files {
                md.push_str(&format!("- `{}`\n", f));
            }
            md.push_str("\n");
        }

        if !self.recent_files.is_empty() {
            md.push_str("### Recently Modified\n");
            for f in self.recent_files.iter().take(10) {
                md.push_str(&format!("- `{}`\n", f.relative_path));
            }
        }

        md
    }

    /// Get a summary (shorter than full markdown)
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("Workspace: {} ({:?})", self.name, self.project_type),
        ];

        if let Some(git) = &self.git {
            parts.push(format!("Branch: {}", git.branch));
            if git.has_uncommitted_changes {
                let count = git.status.staged.len() + git.status.modified.len();
                parts.push(format!("{} changed files", count));
            }
        }

        parts.join(" | ")
    }
}

/// Detect project type from files
fn detect_project_type(path: &Path) -> ProjectType {
    if path.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if path.join("tsconfig.json").exists() {
        ProjectType::TypeScript
    } else if path.join("package.json").exists() {
        ProjectType::JavaScript
    } else if path.join("pyproject.toml").exists() || path.join("setup.py").exists() {
        ProjectType::Python
    } else if path.join("go.mod").exists() {
        ProjectType::Go
    } else if path.join("pom.xml").exists() || path.join("build.gradle").exists() {
        ProjectType::Java
    } else if path.join("Gemfile").exists() {
        ProjectType::Ruby
    } else if path.join("mix.exs").exists() {
        ProjectType::Elixir
    } else {
        ProjectType::Unknown
    }
}

/// Find recently modified files
async fn find_recent_files(path: &Path, limit: usize) -> Result<Vec<RecentFile>> {
    let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    
    collect_recent_files(path, path, &mut files, 3)?; // Max 3 levels deep
    
    // Sort by modification time (newest first)
    files.sort_by(|a, b| b.1.cmp(&a.1));
    
    Ok(files
        .into_iter()
        .take(limit)
        .map(|(full_path, modified, size)| {
            let relative = full_path
                .strip_prefix(path)
                .unwrap_or(&full_path)
                .to_string_lossy()
                .to_string();
            
            let modified_str = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            
            RecentFile {
                path: full_path,
                relative_path: relative,
                modified: modified_str,
                size,
            }
        })
        .collect())
}

fn collect_recent_files(
    root: &Path,
    path: &Path,
    files: &mut Vec<(PathBuf, SystemTime, u64)>,
    depth: usize,
) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        
        // Skip hidden, node_modules, target, etc.
        if name.starts_with('.') 
            || name == "node_modules" 
            || name == "target" 
            || name == "dist"
            || name == "build"
            || name == "__pycache__"
            || name == ".git"
        {
            continue;
        }

        if entry_path.is_dir() {
            collect_recent_files(root, &entry_path, files, depth - 1)?;
        } else if entry_path.is_file() {
            // Only include source files
            if is_source_file(&entry_path) {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        files.push((entry_path, modified, metadata.len()));
                    }
                }
            }
        }
    }

    Ok(())
}

fn is_source_file(path: &Path) -> bool {
    let extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "rb", "ex", "exs",
        "svelte", "vue", "html", "css", "scss", "json", "yaml", "yml", "toml",
        "md", "sql", "sh", "bash", "zsh",
    ];
    
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| extensions.contains(&e))
        .unwrap_or(false)
}

/// Find key project files
fn find_key_files(path: &Path) -> Vec<String> {
    let key_files = [
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "Gemfile",
        "mix.exs",
        "tsconfig.json",
        "README.md",
        ".env.example",
        "docker-compose.yml",
        "Dockerfile",
    ];

    key_files
        .iter()
        .filter(|f| path.join(f).exists())
        .map(|f| f.to_string())
        .collect()
}






