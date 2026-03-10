<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";

  // Task types matching Rust backend
  interface Task {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    status: TaskStatus;
    priority: Priority;
    labels: string[];
    assignees: string[];
    due_date: string | null;
    branch: string | null;
    checklist: ChecklistItem[];
    linked_paths: FileAnchor[];
    linked_prs: PrLink[];
    created_at: string;
    updated_at: string;
  }

  interface TaskStatus {
    type: 'Backlog' | 'Ready' | 'InProgress' | 'Blocked' | 'InReview' | 'Done';
    data?: string;
  }

  type Priority = 'Low' | 'Medium' | 'High' | 'Critical';

  interface ChecklistItem {
    id: string;
    text: string;
    done: boolean;
    created_at: string;
  }

  interface FileAnchor {
    path: string;
    start_line: number | null;
    end_line: number | null;
    symbol_id: string | null;
  }

  interface PrLink {
    url: string;
    number: number;
    status: string;
    branch: string;
    reviewers: string[];
    ci_status: string | null;
  }

  interface Repository {
    id: string;
    name: string;
    path: string;
    remote_url: string | null;
    default_branch: string;
  }

  // State
  let tasks: Task[] = $state([]);
  let repositories: Repository[] = $state([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let selectedRepoId = $state<string | null>(null);
  let searchQuery = $state("");
  let statusFilter = $state<string>("all");

  // Add repo modal
  let showAddRepoModal = $state(false);
  let newRepoPath = $state("");
  
  // Repository cards section
  let reposExpanded = $state(true);

  // Task orchestration - now handled by floating overlay window
  // These remain for backwards compatibility with some listeners
  let orchestrationPrompt = $state("");
  type OrchestrationLog = { timestamp_ms: number; level: string; message: string };
  let orchestrationLogs: OrchestrationLog[] = $state([]);

  // New task form
  let showNewTaskForm = $state(false);
  let newTaskTitle = $state("");
  let newTaskDescription = $state("");
  let newTaskPriority: Priority = $state("Medium");

  // Selected task detail
  let selectedTask = $state<Task | null>(null);
  let newChecklistItem = $state("");

  // Current repo's task file path
  let taskFilePath = $state<string | null>(null);
  
  // IDE preference
  type PreferredIde = "ClaudeCode" | "Cursor";
  let preferredIde = $state<PreferredIde>("ClaudeCode");

  // Filtered tasks
  let filteredTasks = $derived.by(() => {
    let result = tasks;

    // Filter by repo
    if (selectedRepoId) {
      result = result.filter(t => t.repo_id === selectedRepoId);
    }

    // Filter by status
    if (statusFilter !== "all") {
      result = result.filter(t => t.status.type.toLowerCase() === statusFilter);
    }

    // Filter by search
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      result = result.filter(t => 
        t.title.toLowerCase().includes(query) ||
        t.description.toLowerCase().includes(query)
      );
    }

    return result;
  });

  onMount(() => {
    const unlisten: Array<() => void> = [];

    (async () => {
      await loadData();
      await loadOrchestrationPrompt();
      await loadOrchestrationLogs();
      await loadIdePreference();

      // Listen for orchestration logs (for the log panel in Tasks page)
      try {
        unlisten.push(await listen("orchestration-log", (event: any) => {
          const payload = event?.payload;
          if (payload?.message) {
            const entry: OrchestrationLog = {
              timestamp_ms: Date.now(),
              level: payload.level || "info",
              message: String(payload.message),
            };
            orchestrationLogs = [...orchestrationLogs, entry].slice(-200);
          }
        }));
        
        // Refresh logs when Claude completes
        unlisten.push(await listen("claude-complete", () => {
          loadOrchestrationLogs();
        }));
      } catch (err) {
        console.warn("Failed to set up orchestration listeners:", err);
      }
    })();

    return () => {
      for (const u of unlisten) u();
    };
  });

  async function loadOrchestrationPrompt() {
    try {
      orchestrationPrompt = await invoke<string>("get_orchestration_prompt");
    } catch (err) {
      // non-fatal
      console.warn("Failed to load orchestration prompt:", err);
      orchestrationPrompt = "";
    }
  }

  async function loadOrchestrationLogs() {
    try {
      orchestrationLogs = await invoke<OrchestrationLog[]>("get_orchestration_logs");
    } catch (err) {
      console.warn("Failed to load orchestration logs:", err);
      orchestrationLogs = [];
    }
  }

  async function loadIdePreference() {
    try {
      const settings = await invoke<{ preferred_ide?: PreferredIde }>("get_settings");
      preferredIde = settings.preferred_ide || "ClaudeCode";
    } catch (err) {
      console.warn("Failed to load IDE preference:", err);
    }
  }

  async function clearOrchestrationLogs() {
    try {
      await invoke("clear_orchestration_logs");
      orchestrationLogs = [];
    } catch (err) {
      console.warn("Failed to clear orchestration logs:", err);
    }
  }

  async function openOrchestrationOverlay() {
    // Open the floating orchestration overlay window
    try {
      await invoke("show_orchestration_overlay");
    } catch (err) {
      console.error("Failed to open orchestration overlay:", err);
    }
  }

  async function loadData() {
    loading = true;
    error = null;
    try {
      // Load repositories and tasks
      const [repos, taskList] = await Promise.all([
        invoke<Repository[]>("list_repositories"),
        invoke<Task[]>("list_tasks", { filters: {} }),
      ]);
      
      repositories = repos;
      tasks = taskList;
    } catch (err) {
      error = `Failed to load: ${err}`;
      console.error(err);
    } finally {
      loading = false;
    }
  }

  async function loadTaskFilePath(repoId: string) {
    try {
      taskFilePath = await invoke<string | null>("get_task_file_path", { repoId });
    } catch (err) {
      console.error("Failed to get task file path:", err);
      taskFilePath = null;
    }
  }

  async function addRepository() {
    if (!newRepoPath.trim()) return;

    try {
      const repo = await invoke<Repository>("add_repository", { path: newRepoPath });
      repositories = [...repositories, repo];
      selectedRepoId = repo.id;
      await loadTaskFilePath(repo.id);
      newRepoPath = "";
      showAddRepoModal = false;
      await loadData(); // Reload tasks
    } catch (err) {
      error = `Failed to add repository: ${err}`;
      console.error(err);
    }
  }

  async function browseForRepository() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select a repository folder",
      });

      if (selected && typeof selected === 'string') {
        // Add the selected folder directly
        const repo = await invoke<Repository>("add_repository", { path: selected });
        repositories = [...repositories, repo];
        selectedRepoId = repo.id;
        await loadTaskFilePath(repo.id);
        showAddRepoModal = false;
        await loadData();
      }
    } catch (err) {
      error = `Failed to browse: ${err}`;
      console.error(err);
    }
  }

  async function removeRepository(repoId: string) {
    if (!confirm("Remove this repository from tracking? (Files will not be deleted)")) return;

    try {
      await invoke("remove_repository", { id: repoId });
      repositories = repositories.filter(r => r.id !== repoId);
      if (selectedRepoId === repoId) {
        selectedRepoId = null;
        taskFilePath = null;
      }
      await loadData();
    } catch (err) {
      error = `Failed to remove repository: ${err}`;
      console.error(err);
    }
  }

  async function openInCursor(repoId: string) {
    try {
      await invoke("open_repo_in_cursor", { repoId });
    } catch (err) {
      error = `Failed to open in Cursor: ${err}`;
      console.error(err);
    }
  }

  async function openTaskFile(repoId: string) {
    try {
      await invoke("open_task_file", { repoId });
    } catch (err) {
      error = `Failed to open task file: ${err}`;
      console.error(err);
    }
  }

  async function onRepoSelect(repoId: string | null) {
    selectedRepoId = repoId;
    if (repoId) {
      await loadTaskFilePath(repoId);
    } else {
      taskFilePath = null;
    }
  }

  async function createTask() {
    if (!newTaskTitle.trim()) return;
    
    if (!selectedRepoId) {
      error = "Please select a repository first";
      return;
    }

    try {
      const task = await invoke<Task>("create_task", {
        input: {
          repo_id: selectedRepoId,
          title: newTaskTitle,
          description: newTaskDescription || null,
          priority: newTaskPriority,
          labels: null,
          branch: null,
        }
      });

      tasks = [task, ...tasks];
      newTaskTitle = "";
      newTaskDescription = "";
      showNewTaskForm = false;
    } catch (err) {
      error = `Failed to create task: ${err}`;
      console.error(err);
    }
  }

  async function quickCreateTask() {
    if (!newTaskTitle.trim()) return;
    
    const selectedRepo = repositories.find(r => r.id === selectedRepoId);
    if (!selectedRepo) {
      error = "Please select a repository first";
      return;
    }

    try {
      const task = await invoke<Task>("quick_create_task", {
        title: newTaskTitle,
        workspacePath: selectedRepo.path,
      });

      tasks = [task, ...tasks];
      newTaskTitle = "";
      showNewTaskForm = false;
    } catch (err) {
      error = `Failed to create task: ${err}`;
      console.error(err);
    }
  }

  async function updateTaskStatus(taskId: string, status: TaskStatus) {
    try {
      const updated = await invoke<Task>("update_task_status", {
        id: taskId,
        status: status,
      });

      tasks = tasks.map(t => t.id === taskId ? updated : t);
      if (selectedTask?.id === taskId) {
        selectedTask = updated;
      }
    } catch (err) {
      console.error("Failed to update status:", err);
    }
  }

  async function deleteTask(taskId: string) {
    if (!confirm("Delete this task?")) return;

    try {
      await invoke("delete_task", { id: taskId });
      tasks = tasks.filter(t => t.id !== taskId);
      if (selectedTask?.id === taskId) {
        selectedTask = null;
      }
    } catch (err) {
      console.error("Failed to delete task:", err);
    }
  }

  async function addChecklistItem(taskId: string) {
    if (!newChecklistItem.trim()) return;

    try {
      const updated = await invoke<Task>("add_checklist_item", {
        taskId: taskId,
        text: newChecklistItem,
      });

      tasks = tasks.map(t => t.id === taskId ? updated : t);
      if (selectedTask?.id === taskId) {
        selectedTask = updated;
      }
      newChecklistItem = "";
    } catch (err) {
      console.error("Failed to add checklist item:", err);
    }
  }

  async function toggleChecklistItem(taskId: string, itemId: string) {
    try {
      const updated = await invoke<Task>("toggle_checklist_item", {
        taskId: taskId,
        itemId: itemId,
      });

      tasks = tasks.map(t => t.id === taskId ? updated : t);
      if (selectedTask?.id === taskId) {
        selectedTask = updated;
      }
    } catch (err) {
      console.error("Failed to toggle checklist item:", err);
    }
  }

  async function workOnTask() {
    if (!selectedTask) return;
    
    // Find the repository for this task
    const repo = repositories.find(r => r.id === selectedTask.repo_id);
    if (!repo) {
      alert("Repository not found for this task");
      return;
    }

    // Build task context
    const taskContext = `## Current Task

**Title**: ${selectedTask.title}
${selectedTask.description ? `**Description**: ${selectedTask.description}` : ''}
**Status**: ${selectedTask.status.type}
**Priority**: ${selectedTask.priority}
${selectedTask.branch ? `**Branch**: \`${selectedTask.branch}\`` : ''}

${selectedTask.checklist.length > 0 ? `### Checklist
${selectedTask.checklist.map(item => `- [${item.done ? 'x' : ' '}] ${item.text}`).join('\n')}` : ''}

## Repository

**Project**: ${repo.name}
**Path**: \`${repo.path}\`
**Default Branch**: \`${repo.default_branch}\`

## Request

Work on this task. Please:
1. Understand what needs to be done
2. Make the necessary changes
3. Run tests if applicable`;

    try {
      if (preferredIde === "Cursor") {
        // Open in Cursor
        const result = await invoke<{success: boolean, message: string}>("open_task_in_cursor", {
          repoPath: repo.path,
          taskContext: taskContext,
        });

        if (result.success) {
          alert(result.message);
        }
      } else {
        // Send to Claude Code (default)
        const result = await invoke<{success: boolean, message: string}>("send_to_claude_code", {
          prompt: taskContext,
          method: null, // Use default
        });

        if (result.success) {
          alert(result.message);
        }
      }
    } catch (err) {
      console.error(`Failed to open in ${preferredIde}:`, err);
      alert(`Failed: ${err}`);
    }
  }

  function getStatusColor(status: TaskStatus): string {
    switch (status.type) {
      case 'Backlog': return '#666';
      case 'Ready': return '#4A90E2';
      case 'InProgress': return '#F5A623';
      case 'Blocked': return '#D0021B';
      case 'InReview': return '#9013FE';
      case 'Done': return '#7ED321';
      default: return '#666';
    }
  }

  function getPriorityIcon(priority: Priority): string {
    switch (priority) {
      case 'Critical': return '🔴';
      case 'High': return '🟠';
      case 'Medium': return '🟡';
      case 'Low': return '🟢';
      default: return '';
    }
  }

  function formatDate(timestamp: string): string {
    const date = new Date(timestamp);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const hours = Math.floor(diff / (1000 * 60 * 60));
    const days = Math.floor(hours / 24);

    if (days > 0) {
      return `${days}d ago`;
    } else if (hours > 0) {
      return `${hours}h ago`;
    } else {
      return 'Just now';
    }
  }

  function getTaskCountForRepo(repoId: string): { total: number; active: number } {
    const repoTasks = tasks.filter(t => t.repo_id === repoId);
    const active = repoTasks.filter(t => t.status.type !== 'Done').length;
    return { total: repoTasks.length, active };
  }

  function truncatePath(path: string, maxLength: number = 45): string {
    if (path.length <= maxLength) return path;
    const parts = path.split('/');
    if (parts.length <= 3) return path;
    return `${parts[0]}/${parts[1]}/.../${parts.slice(-2).join('/')}`;
  }
</script>

<div class="tasks-page">
  <div class="page-header">
    <div class="header-content">
      <h1>Tasks</h1>
      <p class="subtitle">Tasks stored as markdown in your repositories</p>
    </div>
    <div class="header-actions">
      <button class="btn btn-secondary" onclick={openOrchestrationOverlay}>
        🧩 Orchestrate
      </button>
      <button class="btn btn-primary" onclick={() => showNewTaskForm = !showNewTaskForm}>
        {showNewTaskForm ? 'Cancel' : '+ New Task'}
      </button>
    </div>
  </div>

  <!-- Add Repository Modal -->
  {#if showAddRepoModal}
    <div class="modal-overlay">
      <div class="modal">
        <h3>Add Repository</h3>
        <p class="modal-description">Add a local repository to track tasks. Tasks will be stored in a markdown file within the repo.</p>
        
        <!-- Browse button - primary action -->
        <button class="btn btn-primary full-width browse-btn" onclick={browseForRepository}>
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z"/></svg>
          Browse for folder...
        </button>
        
        <div class="modal-divider">or enter path manually</div>
        
        <div class="path-input-row">
          <input
            type="text"
            placeholder="/path/to/your/repo"
            bind:value={newRepoPath}
            class="modal-input"
            onkeydown={(e) => e.key === 'Enter' && addRepository()}
          />
          <button class="btn btn-secondary" onclick={addRepository} disabled={!newRepoPath.trim()}>Add</button>
        </div>
        
        <div class="modal-actions">
          <button class="btn btn-secondary" onclick={() => showAddRepoModal = false}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Repository Cards Section -->
  <div class="repos-section">
    <div class="repos-header">
      <button class="repos-header-toggle" onclick={() => reposExpanded = !reposExpanded}>
        <svg class="chevron {reposExpanded ? 'expanded' : ''}" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
        <span class="repos-title">Tracked Repositories</span>
        <span class="repos-count">{repositories.length}</span>
      </button>
      <button class="btn btn-secondary btn-small add-repo-btn" onclick={() => showAddRepoModal = true}>
        + Add Repository
      </button>
    </div>
    
    {#if reposExpanded}
      <div class="repos-grid">
        {#if repositories.length === 0}
          <div class="repos-empty">
            <p>No repositories tracked yet</p>
            <button class="btn btn-primary" onclick={() => showAddRepoModal = true}>
              Add your first repository
            </button>
          </div>
        {:else}
          {#each repositories as repo (repo.id)}
            {@const taskCounts = getTaskCountForRepo(repo.id)}
            <div class="repo-card {selectedRepoId === repo.id ? 'selected' : ''}">
              <button class="repo-card-main" onclick={() => onRepoSelect(selectedRepoId === repo.id ? null : repo.id)}>
                <div class="repo-card-icon">📁</div>
                <div class="repo-card-info">
                  <span class="repo-card-name">{repo.name}</span>
                  <span class="repo-card-path" title={repo.path}>{truncatePath(repo.path)}</span>
                  <div class="repo-card-meta">
                    <span class="repo-branch">
                      <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 3v12"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/></svg>
                      {repo.default_branch}
                    </span>
                    <span class="repo-tasks-count">
                      {#if taskCounts.active > 0}
                        <span class="active-count">{taskCounts.active} active</span>
                      {:else}
                        <span class="no-tasks">no tasks</span>
                      {/if}
                    </span>
                  </div>
                </div>
              </button>
              <div class="repo-card-actions">
                <button class="repo-action-btn" title="Open in Cursor" onclick={() => openInCursor(repo.id)}>
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </button>
                <button class="repo-action-btn" title="Edit task file" onclick={() => openTaskFile(repo.id)}>
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
                </button>
                <button class="repo-action-btn danger" title="Remove from tracking" onclick={() => removeRepository(repo.id)}>
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                </button>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <!-- Filter Bar -->
  <div class="filter-bar">
    <select 
      value={selectedRepoId ?? ""} 
      onchange={(e) => onRepoSelect((e.target as HTMLSelectElement).value || null)}
      class="repo-filter-select"
    >
      <option value="">All Repositories</option>
      {#each repositories as repo}
        <option value={repo.id}>{repo.name}</option>
      {/each}
    </select>
    
    {#if taskFilePath && selectedRepoId}
      <div class="task-file-badge">
        <span class="file-icon">📄</span>
        <code>{taskFilePath.split('/').slice(-2).join('/')}</code>
      </div>
    {/if}
  </div>

  <!-- New Task Form -->
  {#if showNewTaskForm}
    <div class="new-task-form">
      <input
        type="text"
        placeholder="Task title..."
        bind:value={newTaskTitle}
        class="task-title-input"
        onkeydown={(e) => e.key === 'Enter' && quickCreateTask()}
      />
      <textarea
        placeholder="Description (optional)"
        bind:value={newTaskDescription}
        class="task-desc-input"
        rows="2"
      ></textarea>
      <div class="form-row">
        <select bind:value={newTaskPriority} class="priority-select">
          <option value="Low">🟢 Low</option>
          <option value="Medium">🟡 Medium</option>
          <option value="High">🟠 High</option>
          <option value="Critical">🔴 Critical</option>
        </select>
        <button class="btn btn-primary" onclick={quickCreateTask}>Create Task</button>
      </div>
    </div>
  {/if}

  <!-- Filters -->
  <div class="filters">
    <div class="search-bar">
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      <input
        type="text"
        placeholder="Search tasks..."
        bind:value={searchQuery}
        class="search-input"
      />
    </div>

    <select bind:value={statusFilter} class="status-filter">
      <option value="all">All Status</option>
      <option value="backlog">Backlog</option>
      <option value="ready">Ready</option>
      <option value="inprogress">In Progress</option>
      <option value="blocked">Blocked</option>
      <option value="inreview">In Review</option>
      <option value="done">Done</option>
    </select>

    <button class="icon-btn" title="Refresh" onclick={loadData}>
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></svg>
    </button>
  </div>

  <!-- Task List -->
  <div class="task-list-container">
    <div class="task-list">
      {#if loading}
        <div class="loading">
          <div class="spinner"></div>
          <span>Loading tasks...</span>
        </div>
      {:else if error}
        <div class="error-message">{error}</div>
      {:else if filteredTasks.length === 0}
        <div class="empty-state">
          <p>No tasks found</p>
          {#if searchQuery || statusFilter !== "all"}
            <button class="btn btn-secondary" onclick={() => { searchQuery = ""; statusFilter = "all"; }}>
              Clear Filters
            </button>
          {/if}
        </div>
      {:else}
        {#each filteredTasks as task (task.id)}
          <button 
            class="task-card {selectedTask?.id === task.id ? 'selected' : ''}"
            onclick={() => selectedTask = selectedTask?.id === task.id ? null : task}
          >
            <div class="task-header">
              <span class="task-priority">{getPriorityIcon(task.priority)}</span>
              <span class="task-title">{task.title}</span>
              <span 
                class="task-status" 
                style="background-color: {getStatusColor(task.status)}20; color: {getStatusColor(task.status)}"
              >
                {task.status.type}
              </span>
            </div>
            {#if task.description}
              <p class="task-description">{task.description.slice(0, 100)}{task.description.length > 100 ? '...' : ''}</p>
            {/if}
            <div class="task-meta">
              {#if task.branch}
                <span class="task-branch">
                  <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 3v12"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/></svg>
                  {task.branch}
                </span>
              {/if}
              {#if task.checklist.length > 0}
                <span class="task-checklist-count">
                  ☑ {task.checklist.filter(i => i.done).length}/{task.checklist.length}
                </span>
              {/if}
              <span class="task-updated">{formatDate(task.updated_at)}</span>
            </div>
          </button>
        {/each}
      {/if}
    </div>

    <!-- Task Detail Panel -->
    {#if selectedTask}
      {@const task = selectedTask}
      <div class="task-detail">
        <div class="detail-header">
          <h2>{task.title}</h2>
          <div class="detail-actions">
            <button class="btn btn-primary btn-small" onclick={workOnTask} title="Open in {preferredIde === 'Cursor' ? 'Cursor' : 'Claude Code'}">
              🚀 Work on this
            </button>
            <button class="btn btn-danger btn-small" onclick={() => deleteTask(task.id)}>
              Delete
            </button>
          </div>
        </div>

        <div class="detail-section">
          <span class="section-label">Status</span>
          <div class="status-buttons">
            {#each ['Backlog', 'Ready', 'InProgress', 'InReview', 'Done'] as status}
              <button
                class="status-btn {task.status.type === status ? 'active' : ''}"
                style="--status-color: {getStatusColor({type: status as TaskStatus['type']})}"
                onclick={() => updateTaskStatus(task.id, {type: status as TaskStatus['type']})}
              >
                {status}
              </button>
            {/each}
          </div>
        </div>

        {#if task.description}
          <div class="detail-section">
            <span class="section-label">Description</span>
            <p class="detail-description">{task.description}</p>
          </div>
        {/if}

        <div class="detail-section">
          <span class="section-label">Checklist</span>
          <div class="checklist">
            {#each task.checklist as item (item.id)}
              <div class="checklist-item">
                <input
                  type="checkbox"
                  id="checklist-{item.id}"
                  checked={item.done}
                  onchange={() => toggleChecklistItem(task.id, item.id)}
                />
                <label for="checklist-{item.id}" class={item.done ? 'done' : ''}>{item.text}</label>
              </div>
            {/each}
            <div class="checklist-add">
              <input
                type="text"
                placeholder="Add item..."
                bind:value={newChecklistItem}
                onkeydown={(e) => e.key === 'Enter' && addChecklistItem(task.id)}
              />
              <button onclick={() => addChecklistItem(task.id)}>+</button>
            </div>
          </div>
        </div>

        {#if task.linked_paths.length > 0}
          <div class="detail-section">
            <span class="section-label">Linked Files</span>
            <div class="linked-files">
              {#each task.linked_paths as anchor}
                <code>{anchor.path}{anchor.start_line ? `:${anchor.start_line}` : ''}</code>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .tasks-page {
    padding: 1.5rem;
    max-width: 1400px;
    margin: 0 auto;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 1rem;
  }

  .header-actions {
    display: flex;
    gap: 0.5rem;
  }

  .header-content h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--text-primary, #ffffff);
  }

  .subtitle {
    margin: 0.25rem 0 0 0;
    color: var(--text-secondary, #888888);
    font-size: 0.9rem;
  }

  /* Modal styles */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 12px;
    padding: 1.5rem;
    width: 90%;
    max-width: 480px;
  }

  .form-group {
    margin-top: 1rem;
  }

  .form-label {
    display: block;
    margin-bottom: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .muted {
    color: var(--text-secondary);
    font-size: 0.85em;
  }

  .hint {
    margin-top: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .inline-message {
    margin-top: 1rem;
    padding: 0.75rem;
    border-radius: 10px;
    border: 1px solid var(--border-primary);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .inline-message.success {
    border-color: rgba(34, 197, 94, 0.4);
  }

  .inline-message.error {
    border-color: rgba(239, 68, 68, 0.4);
  }

  .answer-body {
    margin-top: 0.5rem;
    white-space: pre-wrap;
    color: var(--text-primary);
  }

  .logs-header {
    margin-top: 1rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .logs-title {
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .logs {
    margin-top: 0.5rem;
    border: 1px solid var(--border-primary);
    border-radius: 10px;
    background: var(--bg-tertiary);
    padding: 0.75rem;
    max-height: 160px;
    overflow: auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
    font-size: 0.8rem;
    line-height: 1.4;
  }

  .logs-empty {
    color: var(--text-secondary);
  }

  .log-line {
    display: flex;
    gap: 8px;
    padding: 2px 0;
  }

  .log-level {
    opacity: 0.8;
    min-width: 70px;
  }

  .log-line.error .log-level { color: rgba(239, 68, 68, 0.9); }
  .log-line.warn .log-level { color: rgba(245, 158, 11, 0.95); }
  .log-line.success .log-level { color: rgba(34, 197, 94, 0.95); }

  .btn-small {
    padding: 6px 10px;
    font-size: 0.85rem;
  }

  .modal h3 {
    margin: 0 0 0.5rem 0;
    color: var(--text-primary);
  }

  .modal-description {
    margin: 0 0 1rem 0;
    color: var(--text-secondary);
    font-size: 0.9rem;
  }

  .modal-input {
    width: 100%;
    padding: 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }

  .modal-divider {
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.8rem;
    margin: 1rem 0;
    position: relative;
  }

  .modal-divider::before,
  .modal-divider::after {
    content: '';
    position: absolute;
    top: 50%;
    width: 40%;
    height: 1px;
    background: var(--border-primary);
  }

  .modal-divider::before { left: 0; }
  .modal-divider::after { right: 0; }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .full-width {
    width: 100%;
  }

  .browse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 0.875rem;
    font-size: 1rem;
  }

  .path-input-row {
    display: flex;
    gap: 0.5rem;
  }

  .path-input-row .modal-input {
    flex: 1;
    margin-bottom: 0;
  }

  /* Repository Cards Section */
  .repos-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 10px;
    margin-bottom: 1rem;
    overflow: hidden;
  }

  .repos-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 1rem 0.5rem 0;
  }

  .repos-header-toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    background: transparent;
    border: none;
    cursor: pointer;
    color: var(--text-primary);
    transition: background 0.15s;
    border-radius: 6px;
  }

  .repos-header-toggle:hover {
    background: var(--bg-tertiary);
  }

  .chevron {
    transition: transform 0.2s;
    color: var(--text-secondary);
  }

  .chevron.expanded {
    transform: rotate(90deg);
  }

  .repos-title {
    font-weight: 600;
    font-size: 0.95rem;
  }

  .repos-count {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    padding: 0.125rem 0.5rem;
    border-radius: 10px;
    font-size: 0.75rem;
    font-weight: 500;
  }

  .add-repo-btn {
    font-size: 0.8rem;
    padding: 0.375rem 0.75rem;
  }

  .repos-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 0.75rem;
    padding: 0 1rem 1rem 1rem;
  }

  .repos-empty {
    grid-column: 1 / -1;
    text-align: center;
    padding: 2rem;
    color: var(--text-secondary);
  }

  .repos-empty p {
    margin: 0 0 1rem 0;
  }

  .repo-card {
    display: flex;
    align-items: stretch;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    overflow: hidden;
    transition: all 0.15s;
  }

  .repo-card:hover {
    border-color: var(--border-highlight, #555);
  }

  .repo-card.selected {
    border-color: var(--accent-primary);
    background: rgba(74, 144, 226, 0.08);
  }

  .repo-card-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem;
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    color: inherit;
    min-width: 0;
  }

  .repo-card-icon {
    font-size: 1.5rem;
    flex-shrink: 0;
  }

  .repo-card-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .repo-card-name {
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-card-path {
    font-size: 0.75rem;
    color: var(--text-secondary);
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-card-meta {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.25rem;
  }

  .repo-branch {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.7rem;
    color: var(--accent-primary);
  }

  .repo-tasks-count {
    font-size: 0.7rem;
  }

  .repo-tasks-count .active-count {
    color: var(--accent-primary);
    font-weight: 500;
  }

  .repo-tasks-count .no-tasks {
    color: var(--text-secondary);
  }

  .repo-card-actions {
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border-primary);
    background: rgba(0, 0, 0, 0.1);
  }

  .repo-action-btn {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 0.625rem;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s;
  }

  .repo-action-btn:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .repo-action-btn.danger:hover {
    color: var(--error, #D0021B);
    background: rgba(208, 2, 27, 0.1);
  }

  .repo-action-btn + .repo-action-btn {
    border-top: 1px solid var(--border-primary);
  }

  /* Filter Bar */
  .filter-bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  .repo-filter-select {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 0.85rem;
    min-width: 160px;
  }

  .task-file-badge {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.625rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .task-file-badge code {
    font-size: 0.7rem;
    color: var(--text-primary);
  }

  .file-icon {
    font-size: 0.875rem;
  }

  .icon-btn {
    padding: 0.375rem;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .icon-btn.danger:hover {
    color: var(--error, #D0021B);
  }

  .new-task-form {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .task-title-input, .task-desc-input {
    width: 100%;
    padding: 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .task-title-input:focus, .task-desc-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-row {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }

  .priority-select, .status-filter, .repo-filter {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 0.85rem;
  }

  .filters {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 1rem;
    align-items: center;
  }

  .search-bar {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-bar svg {
    position: absolute;
    left: 0.75rem;
    color: var(--text-secondary);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    padding: 0.5rem 0.75rem 0.5rem 2.25rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .task-list-container {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr;
    gap: 1rem;
    min-height: 0;
    overflow: hidden;
  }
  
  /* When a task is selected, show fixed-width list + expanding detail */
  .task-list-container:has(.task-detail) {
    grid-template-columns: 380px 1fr;
  }

  .task-list {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .task-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: 1rem;
    cursor: pointer;
    transition: all 0.2s;
  }

  .task-card:hover {
    border-color: var(--border-highlight);
    background: var(--bg-tertiary);
  }

  .task-card.selected {
    border-color: var(--accent-primary);
    background: rgba(74, 144, 226, 0.1);
  }

  .task-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .task-priority {
    font-size: 0.8rem;
  }

  .task-title {
    flex: 1;
    font-weight: 500;
    color: var(--text-primary);
  }

  .task-status {
    padding: 0.125rem 0.5rem;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 500;
  }

  .task-description {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  .task-meta {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.5rem;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .task-branch {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    color: var(--accent-primary);
  }

  .task-detail {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    padding: 1.5rem;
    overflow-y: auto;
  }

  .detail-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 1.5rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border-primary);
  }

  .detail-header h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .detail-actions {
    display: flex;
    gap: 0.5rem;
  }

  .detail-section {
    margin-bottom: 1.5rem;
  }

  .detail-section .section-label {
    display: block;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.5rem;
  }

  .detail-description {
    margin: 0;
    color: var(--text-primary);
    line-height: 1.6;
  }

  .status-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .status-btn {
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 0.2s;
  }

  .status-btn:hover {
    border-color: var(--status-color);
    color: var(--status-color);
  }

  .status-btn.active {
    background: var(--status-color);
    border-color: var(--status-color);
    color: white;
  }

  .checklist {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .checklist-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem;
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .checklist-item input[type="checkbox"] {
    width: 18px;
    height: 18px;
    cursor: pointer;
  }

  .checklist-item label {
    flex: 1;
    cursor: pointer;
  }

  .checklist-item label.done {
    text-decoration: line-through;
    color: var(--text-secondary);
  }

  .checklist-add {
    display: flex;
    gap: 0.5rem;
  }

  .checklist-add input {
    flex: 1;
    padding: 0.5rem;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .checklist-add button {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .checklist-add button:hover {
    background: var(--accent-primary);
    color: white;
  }

  .linked-files {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .linked-files code {
    padding: 0.25rem 0.5rem;
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  /* Buttons */
  .btn {
    padding: 0.625rem 1rem;
    border: none;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn-primary {
    background: var(--accent-primary, #4A90E2);
    color: white;
  }

  .btn-primary:hover {
    background: var(--accent-hover, #3A7BD5);
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border-primary);
  }

  .btn-danger {
    background: var(--error, #D0021B);
    color: white;
  }

  .btn-small {
    padding: 0.375rem 0.75rem;
    font-size: 0.8rem;
  }

  /* Loading & Empty States */
  .loading, .empty-state, .error-message {
    text-align: center;
    padding: 3rem 1rem;
    color: var(--text-secondary);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--bg-tertiary);
    border-top-color: var(--text-secondary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin: 0 auto 1rem;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .error-message {
    color: var(--error);
  }
</style>



