<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  // IDE preference type
  type PreferredIde = "ClaudeCode" | "Cursor";

  interface AppSettings {
    preferred_ide: PreferredIde;
    // ... other settings we don't need here
    [key: string]: unknown;
  }

  let isRunning = $state(false);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);
  
  // IDE preference
  let preferredIde = $state<PreferredIde>("ClaudeCode");
  let isSavingIde = $state(false);
  let ideMessage = $state<string | null>(null);

  onMount(async () => {
    await loadSettings();
  });

  async function loadSettings() {
    try {
      const settings = await invoke<AppSettings>("get_settings");
      preferredIde = settings.preferred_ide || "ClaudeCode";
    } catch (err) {
      console.error("Failed to load settings:", err);
    }
  }

  async function saveIdePreference() {
    isSavingIde = true;
    ideMessage = null;
    
    try {
      // Get current settings, update preferred_ide, save
      const settings = await invoke<AppSettings>("get_settings");
      settings.preferred_ide = preferredIde;
      await invoke("save_settings", { settings });
      ideMessage = "IDE preference saved!";
      setTimeout(() => ideMessage = null, 2000);
    } catch (err) {
      console.error("Failed to save IDE preference:", err);
      ideMessage = `Failed to save: ${err}`;
    } finally {
      isSavingIde = false;
    }
  }

  async function orchestrate() {
    if (isRunning) return;

    isRunning = true;
    message = null;
    error = null;

    try {
      const result = await invoke<{ success: boolean; message: string }>(
        "orchestrate_claude_code_tasks",
        { workspacePath: null }
      );

      if (result.success) {
        message = result.message;
      } else {
        error = result.message;
      }
    } catch (err) {
      error = `Failed to orchestrate tasks: ${err}`;
    } finally {
      isRunning = false;
    }
  }
</script>

<div class="advanced-features-page">
  <div class="page-content">
    <!-- IDE Preference Section -->
    <div class="header">
      <h3>🛠️ IDE Preference</h3>
      <p class="subtitle">
        Choose which IDE to use when working on tasks with the "Work on this" button.
      </p>
    </div>

    <div class="card">
      <div class="card-body">
        <div class="ide-options">
          <label class="ide-option {preferredIde === 'ClaudeCode' ? 'selected' : ''}">
            <input 
              type="radio" 
              name="preferred_ide" 
              value="ClaudeCode" 
              bind:group={preferredIde}
              onchange={saveIdePreference}
            />
            <div class="ide-option-content">
              <span class="ide-icon">🤖</span>
              <div class="ide-info">
                <span class="ide-name">Claude Code</span>
                <span class="ide-desc">Opens task in Claude Code CLI for AI-assisted development</span>
              </div>
            </div>
          </label>
          
          <label class="ide-option {preferredIde === 'Cursor' ? 'selected' : ''}">
            <input 
              type="radio" 
              name="preferred_ide" 
              value="Cursor" 
              bind:group={preferredIde}
              onchange={saveIdePreference}
            />
            <div class="ide-option-content">
              <span class="ide-icon">✨</span>
              <div class="ide-info">
                <span class="ide-name">Cursor IDE</span>
                <span class="ide-desc">Opens repository in Cursor with task context file</span>
              </div>
            </div>
          </label>
        </div>

        {#if ideMessage}
          <div class="message {ideMessage.includes('Failed') ? 'error' : 'success'}">
            {ideMessage}
          </div>
        {/if}
      </div>
    </div>

    <!-- Task Orchestrator Section -->
    <div class="header" style="margin-top: 24px;">
      <h3>🤖 Task Orchestrator</h3>
      <p class="subtitle">
        Double-tap <strong>Shift</strong> to send a multi-repo task orchestration prompt to Claude Code.
      </p>
    </div>

    <div class="card">
      <div class="card-body">
        <p>
          This replaces the old MCP integrations. Echo will gather your tracked repositories + tasks and
          hand them off to Claude Code for cross-repo task planning and updates.
        </p>

        <div class="actions">
          <button class="btn btn-primary" onclick={orchestrate} disabled={isRunning}>
            {isRunning ? "Orchestrating…" : "Orchestrate now"}
          </button>
        </div>

        {#if message}
          <div class="message success">{message}</div>
        {/if}
        {#if error}
          <div class="message error">{error}</div>
        {/if}
      </div>
    </div>

    <div class="hint">
      <strong>Tip:</strong> Add repositories in the <em>Tasks</em> page first so the orchestrator has
      multiple repos to work with.
    </div>
  </div>
</div>

<style>
  .advanced-features-page {
    padding: 24px;
    height: 100%;
  }

  .page-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 900px;
    margin: 0 auto;
  }

  .header h3 {
    margin: 0;
    color: var(--text-primary);
  }

  .subtitle {
    margin: 6px 0 0 0;
    color: var(--text-secondary);
  }

  .card {
    border: 1px solid var(--border-primary);
    border-radius: 12px;
    background: var(--bg-secondary);
  }

  .card-body {
    padding: 16px;
    color: var(--text-primary);
  }

  .actions {
    display: flex;
    gap: 12px;
    margin-top: 12px;
  }

  .btn {
    padding: 10px 14px;
    border-radius: 10px;
    border: 1px solid var(--border-primary);
    cursor: pointer;
    font-weight: 600;
  }

  .btn-primary {
    background: var(--accent-primary);
    color: white;
    border-color: transparent;
  }

  .btn-primary:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .message {
    margin-top: 12px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-primary);
    font-size: 0.9rem;
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .message.success {
    border-color: rgba(34, 197, 94, 0.4);
  }

  .message.error {
    border-color: rgba(239, 68, 68, 0.4);
  }

  .hint {
    color: var(--text-secondary);
    font-size: 0.9rem;
  }

  /* IDE Options */
  .ide-options {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .ide-option {
    display: flex;
    align-items: center;
    padding: 16px;
    border: 2px solid var(--border-primary);
    border-radius: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
    background: var(--bg-tertiary);
  }

  .ide-option:hover {
    border-color: var(--border-highlight, #555);
    background: var(--bg-secondary);
  }

  .ide-option.selected {
    border-color: var(--accent-primary);
    background: rgba(74, 144, 226, 0.1);
  }

  .ide-option input[type="radio"] {
    display: none;
  }

  .ide-option-content {
    display: flex;
    align-items: center;
    gap: 16px;
    width: 100%;
  }

  .ide-icon {
    font-size: 2rem;
    flex-shrink: 0;
  }

  .ide-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ide-name {
    font-weight: 600;
    font-size: 1rem;
    color: var(--text-primary);
  }

  .ide-desc {
    font-size: 0.85rem;
    color: var(--text-secondary);
  }
</style>
