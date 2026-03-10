# Echo + Pointer Features Implementation Plan

This document outlines the phased implementation plan for bringing Pointer's core features into Echo. The goal is to transform Echo from a voice transcription tool into a powerful dev-focused AI assistant with task management, Cursor integration, and agentic code execution.

## Architecture Philosophy

### Echo as a Thin Sidecar

Following Pointer's model, **Echo should NOT duplicate agent capabilities**. Claude Code already has MCPs and agentic execution. Echo's role is:

```
┌─────────────────────────────────────────────────────────────────┐
│                         USER                                    │
│                          │                                      │
│                    ┌─────▼─────┐                                │
│                    │   ECHO    │  ◄── Voice input               │
│                    │ (Sidecar) │  ◄── Task management           │
│                    └─────┬─────┘  ◄── Context gathering         │
│                          │                                      │
│              ┌───────────▼───────────┐                          │
│              │     CLAUDE CODE       │  ◄── Agent loop          │
│              │   (The Real Agent)    │  ◄── MCP tools           │
│              │                       │  ◄── File/git/search     │
│              └───────────┬───────────┘                          │
│                          │                                      │
│                    ┌─────▼─────┐                                │
│                    │  CURSOR   │  ◄── Execution surface         │
│                    │   (IDE)   │  ◄── Terminals, files          │
│                    └───────────┘                                │
└─────────────────────────────────────────────────────────────────┘
```

**Key Principle**: Echo does NOT use MCPs for agent work. Claude Code uses MCPs. Echo:
1. Captures voice → transcribes → understands intent
2. Manages tasks (CRUD, timeline, evidence)
3. Gathers context directly (file system, git CLI) to pass to Claude Code
4. Receives results back and stores in task timeline

### Why NOT MCP for Cursor Context?

1. **Duplication** - Claude Code already has file/git MCPs
2. **Window targeting** - MCP can't reliably identify "the right" Cursor window
3. **Direct is better** - Reading workspace files and running `git status` is simpler and more reliable

---

## Current State Assessment

### What Echo Already Has ✅
- **Voice Processing**
  - Voice command recognition
  - Local whisper model support
  - OpenAI transcription fallback
- **MCP Infrastructure** (`src-tauri/src/mcp/`)
  - Can be repurposed for Claude Code communication (not agent duplication)
- **History & Context** - Conversation tracking, recent actions
- **AI Agent Core** - Session management (will be simplified)

### What Needs to Be Built 🔨
1. **Task Management System** - Repo-scoped tasks with full lifecycle
2. **Direct Context Capture** - File system + git CLI (NO MCP)
3. **Claude Code Integration** - Pass context, receive results
4. **Timeline & Evidence** - Store agent outputs in tasks
5. **Lightweight UI** - Task views, approval dialogs

---

## Phase 1: Data Model & Storage Foundation (Week 1-2)

### 1.1 Core Data Structures

Create `src-tauri/src/tasks/` module:

```rust
// src-tauri/src/tasks/mod.rs
pub mod models;
pub mod storage;
pub mod service;

// src-tauri/src/tasks/models.rs
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub path: PathBuf,                    // Local path
    pub remote_url: Option<String>,       // git remote
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked { reason: String },
    InReview { reviewer: Option<String> },
    Done,
}

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
    pub branch: Option<String>,           // Pinned branch or None for HEAD
    pub checklist: Vec<ChecklistItem>,
    pub linked_paths: Vec<FileAnchor>,    // File/code references
    pub linked_prs: Vec<PrLink>,
    pub evidence: Vec<Evidence>,
    pub timeline: Vec<TimelineEvent>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnchor {
    pub path: PathBuf,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub symbol_id: Option<String>,        // For stable references
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub payload: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub actor: Actor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceKind {
    Command { cmd: String, exit_code: i32, stdout: String, stderr: String, duration_ms: u64 },
    TestResult { passed: bool, test_name: String, output: String },
    FileChange { path: PathBuf, diff: String },
    Note { text: String },
    Comment { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrLink {
    pub url: String,
    pub number: u32,
    pub status: PrStatus,
    pub branch: String,
    pub reviewers: Vec<String>,
    pub ci_status: Option<CiStatus>,
}
```

### 1.2 Local Storage (SQLite)

Create `src-tauri/src/tasks/storage.rs` with:
- SQLite database for tasks, repos, evidence
- JSON export/import for portability
- File-based cache for offline operation
- No mandatory cloud sync

### 1.3 Tauri Commands

```rust
// src-tauri/src/commands/tasks.rs
#[tauri::command]
pub async fn create_task(repo_id: String, title: String, ...) -> Result<Task, Error>;

#[tauri::command]
pub async fn update_task_status(task_id: String, status: TaskStatus) -> Result<Task, Error>;

#[tauri::command]
pub async fn add_evidence(task_id: String, evidence: Evidence) -> Result<(), Error>;

#[tauri::command]
pub async fn get_repo_tasks(repo_id: String, filters: TaskFilters) -> Result<Vec<Task>, Error>;
```

---

## Phase 2: Direct Context Capture (Week 2-3)

**No MCPs here** - Echo reads context directly via file system and git CLI.

### 2.1 Workspace Detection

Create `src-tauri/src/workspace/` module:

```rust
// src-tauri/src/workspace/mod.rs
pub mod detection;
pub mod context;
pub mod git;

// src-tauri/src/workspace/detection.rs
use std::path::PathBuf;

/// Detect active workspace - multiple strategies
pub struct WorkspaceDetector;

impl WorkspaceDetector {
    /// Strategy 1: User explicitly sets workspace in Echo settings
    pub fn from_settings() -> Option<PathBuf>;
    
    /// Strategy 2: Watch for recently modified files in known project dirs
    pub fn from_recent_activity(project_dirs: &[PathBuf]) -> Option<PathBuf>;
    
    /// Strategy 3: Parse Cursor's workspace state (if accessible)
    /// Location: ~/.config/Cursor/User/workspaceStorage/
    pub fn from_cursor_state() -> Option<PathBuf>;
    
    /// Strategy 4: Use current working directory of most recent terminal
    pub fn from_terminal_cwd() -> Option<PathBuf>;
}
```

### 2.2 Context Capture (Direct File/Git)

```rust
// src-tauri/src/workspace/context.rs
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    pub path: PathBuf,
    pub git: Option<GitContext>,
    pub recent_files: Vec<RecentFile>,      // From file system mtime
    pub project_type: ProjectType,          // Detected from package.json, Cargo.toml, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitContext {
    pub branch: String,
    pub status: GitStatus,
    pub recent_commits: Vec<CommitSummary>,
    pub remotes: Vec<String>,
}

impl WorkspaceContext {
    /// Gather context using direct commands (no MCP)
    pub async fn capture(workspace_path: &Path) -> Result<Self> {
        let git = Self::capture_git_context(workspace_path).await.ok();
        let recent_files = Self::find_recent_files(workspace_path, 20).await?;
        let project_type = Self::detect_project_type(workspace_path).await;
        
        Ok(Self {
            path: workspace_path.to_path_buf(),
            git,
            recent_files,
            project_type,
        })
    }
    
    async fn capture_git_context(path: &Path) -> Result<GitContext> {
        // Direct git CLI calls - simple and reliable
        let branch = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()?;
            
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()?;
            
        let log = Command::new("git")
            .args(["log", "--oneline", "-10"])
            .current_dir(path)
            .output()?;
        
        // Parse outputs...
        Ok(GitContext { /* ... */ })
    }
}
```

### 2.3 Why Direct Access Works Better

| Approach | Pros | Cons |
|----------|------|------|
| **Direct (chosen)** | Simple, reliable, no window targeting issues | No real-time cursor position |
| MCP | Could get cursor position | Window targeting unreliable, duplicates Claude Code's tools |

**What we CAN capture directly:**
- ✅ Git state (branch, status, diff, log)
- ✅ Recently modified files (mtime)
- ✅ Project type detection
- ✅ File contents
- ✅ Directory structure

**What we CAN'T capture (and that's OK):**
- ❌ Exact cursor position in editor
- ❌ Current selection
- ❌ Open tabs list

**Why it's OK**: Claude Code running in Cursor already has full context. Echo just needs enough to:
1. Know which repo/branch the task is about
2. Provide grounding context in prompts
3. Store evidence in task timeline

---

## Phase 3: Claude Code Integration (Week 3-5)

**Key insight**: Echo does NOT run the agent loop. Claude Code does. Echo:
1. Gathers context
2. Formats a prompt with task + context
3. Invokes Claude Code (in Cursor)
4. Captures results back for timeline

### 3.1 Integration Architecture

```
┌────────────────────────────────────────────────────────────────┐
│  Echo (Sidecar)                                                │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│  │ Voice Input  │───►│ Task Manager │───►│   Context    │     │
│  │ "fix auth"   │    │ (get/create) │    │   Capture    │     │
│  └──────────────┘    └──────────────┘    └──────┬───────┘     │
│                                                  │             │
│                                          ┌──────▼───────┐     │
│                                          │    Prompt    │     │
│                                          │   Builder    │     │
│                                          └──────┬───────┘     │
└─────────────────────────────────────────────────┼─────────────┘
                                                  │
                    ┌─────────────────────────────▼─────────────┐
                    │  Claude Code (in Cursor)                  │
                    │  - Observe/Plan/Act/Reflect              │
                    │  - Uses MCPs (file, git, search, etc.)   │
                    │  - Executes in terminals                  │
                    └─────────────────────────────┬─────────────┘
                                                  │
┌─────────────────────────────────────────────────▼─────────────┐
│  Echo (Sidecar) - Results Capture                             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    │
│  │   Parse      │───►│   Evidence   │───►│   Timeline   │    │
│  │   Output     │    │   Creation   │    │   Update     │    │
│  └──────────────┘    └──────────────┘    └──────────────┘    │
└───────────────────────────────────────────────────────────────┘
```

### 3.2 Prompt Builder

Echo builds rich prompts to pass to Claude Code:

```rust
// src-tauri/src/claude/mod.rs
pub mod prompt;
pub mod invoke;
pub mod results;

// src-tauri/src/claude/prompt.rs
#[derive(Debug, Clone)]
pub struct ClaudePrompt {
    pub task: Option<Task>,
    pub user_request: String,
    pub workspace_context: WorkspaceContext,
    pub constraints: Vec<String>,
}

impl ClaudePrompt {
    /// Build a grounded prompt for Claude Code
    pub fn build(&self) -> String {
        let mut prompt = String::new();
        
        // Task context (if linked to a task)
        if let Some(task) = &self.task {
            prompt.push_str(&format!("## Current Task\n"));
            prompt.push_str(&format!("**{}**: {}\n", task.title, task.description));
            prompt.push_str(&format!("Status: {:?}\n", task.status));
            if !task.checklist.is_empty() {
                prompt.push_str("Checklist:\n");
                for item in &task.checklist {
                    let check = if item.done { "x" } else { " " };
                    prompt.push_str(&format!("- [{}] {}\n", check, item.text));
                }
            }
            prompt.push_str("\n");
        }
        
        // Workspace context
        prompt.push_str(&format!("## Workspace\n"));
        prompt.push_str(&format!("Path: {}\n", self.workspace_context.path.display()));
        if let Some(git) = &self.workspace_context.git {
            prompt.push_str(&format!("Branch: {}\n", git.branch));
            prompt.push_str(&format!("Status: {} modified, {} staged\n", 
                git.status.modified.len(), git.status.staged.len()));
        }
        prompt.push_str("\n");
        
        // User request
        prompt.push_str(&format!("## Request\n{}\n", self.user_request));
        
        prompt
    }
}
```

### 3.3 Invoking Claude Code

**Option A: Clipboard + Keyboard Simulation** (Simplest)
```rust
// Copy prompt to clipboard, simulate Cmd+K or Cmd+L in Cursor
pub async fn invoke_via_clipboard(prompt: &str) -> Result<()> {
    clipboard::set_text(prompt)?;
    // Simulate keyboard shortcut to open Claude Code
    keyboard::send_keys("cmd+l")?; // Or cmd+k for inline
    keyboard::send_keys("cmd+v")?; // Paste
    keyboard::send_keys("enter")?;
    Ok(())
}
```

**Option B: Cursor Extension API** (If available)
```rust
// Direct API call if Cursor exposes one
pub async fn invoke_via_api(prompt: &str) -> Result<()> {
    // TBD - depends on Cursor's extension API
}
```

**Option C: File-based Handoff**
```rust
// Write prompt to a known location, watch for response
pub async fn invoke_via_file(prompt: &str, workspace: &Path) -> Result<()> {
    let prompt_file = workspace.join(".echo/prompt.md");
    fs::write(&prompt_file, prompt)?;
    // Claude Code can be configured to watch this file
    // Or user manually opens it
    Ok(())
}
```

### 3.4 Results Capture

Watch for changes and parse Claude Code output:

```rust
// src-tauri/src/claude/results.rs
pub struct ResultsWatcher {
    workspace: PathBuf,
    task_id: Option<String>,
}

impl ResultsWatcher {
    /// Watch for file changes made by Claude Code
    pub async fn watch_for_changes(&self) -> Result<Vec<FileChange>> {
        // Use notify crate to watch workspace
        // Capture diffs for files modified after prompt was sent
    }
    
    /// Parse terminal output (if accessible)
    pub async fn capture_terminal_output(&self) -> Result<Vec<CommandResult>> {
        // Read from Cursor's terminal files if accessible
        // Or parse from git diff if commands modified files
    }
    
    /// Create evidence from captured results
    pub fn create_evidence(&self, changes: &[FileChange], commands: &[CommandResult]) -> Vec<Evidence> {
        let mut evidence = Vec::new();
        
        for change in changes {
            evidence.push(Evidence {
                id: Uuid::new_v4().to_string(),
                kind: EvidenceKind::FileChange {
                    path: change.path.clone(),
                    diff: change.diff.clone(),
                },
                timestamp: Utc::now(),
                actor: Actor::Agent,
            });
        }
        
        for cmd in commands {
            evidence.push(Evidence {
                id: Uuid::new_v4().to_string(),
                kind: EvidenceKind::Command {
                    cmd: cmd.command.clone(),
                    exit_code: cmd.exit_code,
                    stdout: cmd.stdout.clone(),
                    stderr: cmd.stderr.clone(),
                    duration_ms: cmd.duration_ms,
                },
                timestamp: cmd.timestamp,
                actor: Actor::Agent,
            });
        }
        
        evidence
    }
}
```

### 3.5 What Echo Does NOT Do

❌ Run its own agent loop (Claude Code does this)
❌ Have its own file/git/search MCPs (Claude Code has these)
❌ Execute commands itself (Claude Code executes in Cursor terminals)
❌ Make code edits (Claude Code writes files)

✅ Voice transcription → intent
✅ Task CRUD and storage
✅ Context gathering (direct file/git)
✅ Prompt building with task context
✅ Results capture → evidence → timeline

---

## Phase 4: Frontend UI (Week 5-6)

### 4.1 New Svelte Components

```
src/components/
├── Tasks/
│   ├── TaskList.svelte           # List view with filters
│   ├── TaskCard.svelte           # Card with status, labels
│   ├── TaskDetail.svelte         # Full task view with timeline
│   ├── TaskChecklist.svelte      # Interactive checklist
│   ├── EvidenceList.svelte       # Command/test results
│   └── PrLinks.svelte            # Linked PRs with status
├── Agent/
│   ├── AgentChat.svelte          # Conversation interface
│   ├── PlanPreview.svelte        # Show plan before execution
│   ├── ExecutionLog.svelte       # Step-by-step execution
│   ├── DiffPreview.svelte        # File change preview
│   └── ApprovalDialog.svelte     # Approve/reject actions
├── Context/
│   ├── ContextPanel.svelte       # Current cursor context
│   ├── FileAnchors.svelte        # Linked code references
│   └── GitStatus.svelte          # Branch, diff summary
└── Settings/pages/
    └── DevToolsPage.svelte       # Agent & task settings
```

### 4.2 UX Flows

**Quick Start Flow:**
1. Voice: "Create task for fixing the auth bug"
2. Echo captures current file/branch as context
3. Creates task with file anchors
4. Opens in sidebar or overlay

**Agent Execution Flow:**
1. Voice: "Fix the failing tests in auth module"
2. Agent observes: git status, failing tests, file context
3. Proposes plan with file edits
4. User approves (or voice: "do it")
5. Executes with live progress
6. Attaches evidence to task timeline

---

## Phase 5: Integration & Polish (Week 6-7)

### 5.1 Voice Commands Extension

```rust
// Add to src-tauri/src/voice_command.rs
pub fn dev_voice_commands() -> Vec<VoiceCommand> {
    vec![
        VoiceCommand::new("create task *", create_task_handler),
        VoiceCommand::new("show tasks", show_tasks_handler),
        VoiceCommand::new("start task *", start_task_handler),
        VoiceCommand::new("run tests", run_tests_handler),
        VoiceCommand::new("commit changes", commit_handler),
        VoiceCommand::new("explain this code", explain_code_handler),
        VoiceCommand::new("fix *", agent_fix_handler),
        VoiceCommand::new("refactor *", agent_refactor_handler),
    ]
}
```

### 5.2 Settings & Configuration

```typescript
// Settings structure
interface DevToolsSettings {
    enabled: boolean;
    defaultRepo: string | null;
    agentSettings: {
        autoApproveSimple: boolean;      // Auto-approve trivial edits
        requireApprovalFor: string[];    // ["commit", "push", "delete"]
        maxFileEditsPerAction: number;
        shellCommandTimeout: number;
    };
    safetySettings: {
        blockedCommands: string[];
        secretPatterns: string[];
        redactLogsEnabled: boolean;
    };
    cursorIntegration: {
        captureTerminals: boolean;
        captureGitState: boolean;
        watchFileChanges: boolean;
    };
}
```

---

## Implementation Priority

| Priority | Feature | Value | Effort |
|----------|---------|-------|--------|
| P0 | Task data model & storage | Foundation | Medium |
| P0 | Basic task CRUD UI | User-visible | Medium |
| P1 | Direct context capture (git/files) | Agent grounding | Low |
| P1 | Prompt builder | Claude Code integration | Low |
| P1 | Claude Code invocation (clipboard) | Core feature | Low |
| P2 | Results capture & evidence | Audit trail | Medium |
| P2 | Timeline UI | Task tracking | Medium |
| P2 | Voice commands for tasks | UX | Low |
| P3 | PR linkage (via git remote) | Integration | Medium |
| P3 | File watching for live updates | Polish | Medium |

**Note**: Effort is MUCH lower now because Claude Code does the heavy lifting. Echo just needs to:
- Store tasks (SQLite)
- Capture context (git CLI + file system)  
- Build prompts (string formatting)
- Pass to Claude Code (clipboard)
- Watch for results (file system watcher)

---

## Technical Decisions

### Storage
- **SQLite** for tasks (via `rusqlite` or `sqlx`)
- **JSON files** for settings and quick export
- **No mandatory cloud** - sync optional

### Agent Integration
- **Claude Code IN Cursor** is the agent (not Echo)
- Echo builds prompts and passes context
- Echo captures results for task timeline
- No duplicate MCP tools in Echo

### Context Capture
- **Direct file system** - read files, check mtimes
- **Direct git CLI** - `git status`, `git log`, `git diff`
- **No MCP** for context (avoids duplication, more reliable)

### Claude Code Invocation
- **Clipboard + keyboard** for MVP (simplest)
- **File-based handoff** as alternative
- Future: Cursor extension API if available

---

## Success Metrics

1. ✅ Users can create/manage tasks from Echo UI or voice
2. ✅ Echo can invoke Claude Code with rich context (task + git state)
3. ✅ File changes by Claude Code are captured as task evidence
4. ✅ Works fully offline for task management (no cloud required)
5. ✅ < 500ms to prepare and invoke Claude Code
6. ✅ No MCP duplication - Claude Code owns agent execution

---

## Next Steps

1. **Review this plan** - identify any missing requirements
2. **Start Phase 1** - Create task data model and SQLite storage
3. **Phase 2 in parallel** - Direct context capture is simple (git CLI)
4. **Test Claude Code invocation** - Clipboard approach should work immediately
5. **Build minimal UI** - Task list + create task form

### Simplified MVP Scope

Since Claude Code does the heavy lifting, the MVP is much simpler:

```
Voice: "Create task to fix auth bug" 
  → Echo creates task in SQLite
  → Echo captures git context (branch, status)
  → User says "work on this task"
  → Echo builds prompt with task + context
  → Echo copies to clipboard + opens Claude Code
  → Claude Code does the work
  → Echo watches for file changes
  → Echo adds evidence to task timeline
```

**MVP = ~2-3 weeks** (down from 6-7 weeks)

Would you like to start with Phase 1 (task storage) or test the Claude Code clipboard integration first?






