//! Workspace detection strategies
//!
//! Multiple methods to detect the active workspace without relying on MCPs.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Detected workspace
#[derive(Debug, Clone)]
pub struct DetectedWorkspace {
    pub path: PathBuf,
    pub source: DetectionSource,
    pub confidence: f32,
}

/// How the workspace was detected
#[derive(Debug, Clone)]
pub enum DetectionSource {
    /// User explicitly configured this workspace
    UserSetting,
    /// Most recently modified files in known project dirs
    RecentActivity,
    /// Cursor's workspace storage
    CursorState,
    /// Current working directory
    CurrentDir,
}

/// Workspace detector with multiple strategies
pub struct WorkspaceDetector {
    /// Known project directories to scan
    project_dirs: Vec<PathBuf>,
    /// User-configured default workspace
    default_workspace: Option<PathBuf>,
}

impl WorkspaceDetector {
    pub fn new() -> Self {
        Self {
            project_dirs: Self::default_project_dirs(),
            default_workspace: None,
        }
    }

    pub fn with_default_workspace(mut self, path: PathBuf) -> Self {
        self.default_workspace = Some(path);
        self
    }

    pub fn with_project_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.project_dirs = dirs;
        self
    }

    /// Detect the most likely active workspace
    pub async fn detect(&self) -> Option<DetectedWorkspace> {
        // Strategy 1: User-configured default (highest priority)
        if let Some(path) = &self.default_workspace {
            if path.exists() && path.is_dir() {
                return Some(DetectedWorkspace {
                    path: path.clone(),
                    source: DetectionSource::UserSetting,
                    confidence: 1.0,
                });
            }
        }

        // Strategy 2: Check Cursor's workspace state
        if let Some(workspace) = self.from_cursor_state().await {
            return Some(workspace);
        }

        // Strategy 3: Most recently modified project
        if let Some(workspace) = self.from_recent_activity().await {
            return Some(workspace);
        }

        // Strategy 4: Current working directory
        if let Ok(cwd) = std::env::current_dir() {
            if self.is_project_root(&cwd) {
                // If the CWD has an .echo directory, we are very confident this is the intended workspace
                let confidence = if cwd.join(".echo").exists() { 1.0 } else { 0.5 };
                
                return Some(DetectedWorkspace {
                    path: cwd,
                    source: DetectionSource::CurrentDir,
                    confidence,
                });
            }
        }

        None
    }

    /// Strategy: Read Cursor's workspace storage
    async fn from_cursor_state(&self) -> Option<DetectedWorkspace> {
        // Cursor stores workspace info in:
        // ~/.config/Cursor/User/workspaceStorage/
        // or on macOS: ~/Library/Application Support/Cursor/User/workspaceStorage/
        
        let cursor_storage = self.get_cursor_storage_path()?;
        
        if !cursor_storage.exists() {
            return None;
        }

        // Find most recently modified workspace folder
        let mut recent_workspace: Option<(PathBuf, SystemTime)> = None;
        
        if let Ok(entries) = std::fs::read_dir(&cursor_storage) {
            for entry in entries.flatten() {
                let workspace_json = entry.path().join("workspace.json");
                if workspace_json.exists() {
                    if let Ok(metadata) = workspace_json.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if recent_workspace.as_ref().map_or(true, |(_, t)| modified > *t) {
                                // Parse workspace.json to get the actual path
                                if let Ok(content) = std::fs::read_to_string(&workspace_json) {
                                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                                        if let Some(folder) = json.get("folder").and_then(|f| f.as_str()) {
                                            // Handle file:// URLs
                                            let path = if folder.starts_with("file://") {
                                                PathBuf::from(folder.strip_prefix("file://").unwrap_or(folder))
                                            } else {
                                                PathBuf::from(folder)
                                            };
                                            
                                            if path.exists() {
                                                recent_workspace = Some((path, modified));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        recent_workspace.map(|(path, _)| DetectedWorkspace {
            path,
            source: DetectionSource::CursorState,
            confidence: 0.9,
        })
    }

    /// Strategy: Find most recently active project
    async fn from_recent_activity(&self) -> Option<DetectedWorkspace> {
        let mut most_recent: Option<(PathBuf, SystemTime)> = None;

        for project_dir in &self.project_dirs {
            if !project_dir.exists() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(project_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && self.is_project_root(&path) {
                        if let Some(modified) = self.get_recent_file_time(&path) {
                            if most_recent.as_ref().map_or(true, |(_, t)| modified > *t) {
                                most_recent = Some((path, modified));
                            }
                        }
                    }
                }
            }
        }

        most_recent.map(|(path, _)| DetectedWorkspace {
            path,
            source: DetectionSource::RecentActivity,
            confidence: 0.7,
        })
    }

    /// Get Cursor's workspace storage path
    fn get_cursor_storage_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        
        #[cfg(target_os = "linux")]
        let path = home.join(".config/Cursor/User/workspaceStorage");
        
        #[cfg(target_os = "macos")]
        let path = home.join("Library/Application Support/Cursor/User/workspaceStorage");
        
        #[cfg(target_os = "windows")]
        let path = home.join("AppData/Roaming/Cursor/User/workspaceStorage");

        Some(path)
    }

    /// Default project directories
    fn default_project_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Projects"));
            dirs.push(home.join("projects"));
            dirs.push(home.join("Code"));
            dirs.push(home.join("code"));
            dirs.push(home.join("dev"));
            dirs.push(home.join("Developer"));
            dirs.push(home.join("workspace"));
            dirs.push(home.join("repos"));
            dirs.push(home.join("src"));
        }
        
        dirs
    }

    /// Check if a directory is a project root
    fn is_project_root(&self, path: &Path) -> bool {
        // Look for common project markers
        let markers = [
            ".git",
            "package.json",
            "Cargo.toml",
            "pyproject.toml",
            "go.mod",
            "pom.xml",
            "build.gradle",
            "Makefile",
            ".project",
            "Gemfile",
            "composer.json",
            "mix.exs",
            "deno.json",
            "bun.lockb",
        ];

        markers.iter().any(|marker| path.join(marker).exists())
    }

    /// Get the most recent modification time of files in a directory
    fn get_recent_file_time(&self, path: &Path) -> Option<SystemTime> {
        let mut most_recent: Option<SystemTime> = None;
        
        // Check common frequently-modified files/dirs
        let check_paths = [
            ".git/index",
            ".git/COMMIT_EDITMSG",
            "src",
            "lib",
            "app",
        ];

        for check in check_paths {
            let full_path = path.join(check);
            if let Ok(metadata) = std::fs::metadata(&full_path) {
                if let Ok(modified) = metadata.modified() {
                    if most_recent.as_ref().map_or(true, |t| modified > *t) {
                        most_recent = Some(modified);
                    }
                }
            }
        }

        most_recent
    }
}

impl Default for WorkspaceDetector {
    fn default() -> Self {
        Self::new()
    }
}



