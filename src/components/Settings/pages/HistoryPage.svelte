<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  interface TranscriptionHistoryEntry {
    id: string;
    text: string;
    timestamp: string;
    source: 'manual' | 'voicecommand' | 'meeting';
    duration_ms?: number;
    model?: string;
    pinned?: boolean;
  }

  let historyEntries: TranscriptionHistoryEntry[] = $state([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let searchQuery = $state("");
  let filteredEntries = $state<TranscriptionHistoryEntry[]>([]);
  let expandedEntries = $state<Set<string>>(new Set());

  // Load history on mount
  onMount(async () => {
    await loadHistory();
  });

  async function loadHistory() {
    loading = true;
    error = null;
    try {
      historyEntries = await invoke<TranscriptionHistoryEntry[]>("get_transcription_history");
      filterEntries();
    } catch (err) {
      error = `Failed to load history: ${err}`;
      console.error(err);
    } finally {
      loading = false;
    }
  }

  function filterEntries() {
    let entries = historyEntries;

    // Filter by search query
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      entries = entries.filter(e => e.text.toLowerCase().includes(query));
    }

    // Sort happens in backend (Pinned first, then Date), but let's ensure UI respects it if we filter
    // Backend returns them sorted, so filtering preserves order if using stable filter or if we don't re-sort.
    // filter() creates a new array but preserves order.
    
    filteredEntries = entries;
  }

  $effect(() => {
    searchQuery;
    filterEntries();
  });

  async function togglePin(id: string) {
      try {
          await invoke("toggle_history_pin", { id });
          // Optimistic update
          const entry = historyEntries.find(e => e.id === id);
          if (entry) {
              entry.pinned = !entry.pinned;
              // Re-sort locally or reload
              // Let's reload to be safe and get correct server sorting if logic is complex
              await loadHistory(); 
          }
      } catch (err) {
          console.error("Failed to toggle pin:", err);
      }
  }

  async function copyToClipboard(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      console.log("Copied to clipboard");
    } catch (err) {
      console.error("Failed to copy:", err);
      alert(`Failed to copy: ${err}`);
    }
  }

  async function deleteEntry(id: string) {
    if (!confirm("Are you sure you want to delete this entry?")) {
      return;
    }

    try {
      await invoke("delete_history_entry", { id });
      await loadHistory();
    } catch (err) {
      console.error("Failed to delete:", err);
      alert(`Failed to delete: ${err}`);
    }
  }

  async function clearHistory() {
    if (!confirm("Are you sure you want to clear all history? This cannot be undone.")) {
      return;
    }

    try {
      await invoke("clear_transcription_history");
      await loadHistory();
    } catch (err) {
      console.error("Failed to clear history:", err);
      alert(`Failed to clear history: ${err}`);
    }
  }

  function toggleExpanded(id: string) {
    if (expandedEntries.has(id)) {
      expandedEntries.delete(id);
    } else {
      expandedEntries.add(id);
    }
    expandedEntries = expandedEntries; // Trigger reactivity
  }

  function formatDate(timestamp: string): string {
    const date = new Date(timestamp);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const seconds = Math.floor(diff / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) {
      return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    } else if (hours > 0) {
      return `${hours}h ago`;
    } else if (minutes > 0) {
      return `${minutes}m ago`;
    } else {
      return 'Just now';
    }
  }

  function getSourceLabel(source: string): string {
    switch (source) {
      case 'manual': return 'Dictation'; // Renamed for clarity
      case 'voicecommand': return 'Command';
      case 'meeting': return 'Meeting';
      default: return source;
    }
  }
</script>

<div class="history-page">
  <div class="page-header">
    <h1>Transcription History</h1>
    <p class="subtitle">View and manage your past transcriptions</p>
  </div>

  <!-- Controls -->
  <div class="controls">
    <div class="search-bar">
      <div class="search-icon">
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      </div>
      <input
        type="text"
        placeholder="Search transcriptions..."
        bind:value={searchQuery}
        class="search-input"
      />
    </div>

    <div class="action-buttons">
      <button class="icon-btn-large" onclick={loadHistory} title="Refresh">
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16l5 5"/><path d="M21 21v-5h-5"/></svg>
      </button>
      <button class="icon-btn-large danger" onclick={clearHistory} title="Clear All">
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
      </button>
    </div>
  </div>

  <!-- History List -->
  <div class="history-list">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
        <span>Loading history...</span>
      </div>
    {:else if error}
      <div class="error-message">{error}</div>
    {:else if filteredEntries.length === 0}
      <div class="empty-state">
        <p>No transcriptions found</p>
        {#if searchQuery}
          <button class="btn btn-secondary" onclick={() => { searchQuery = ""; }}>
            Clear Search
          </button>
        {/if}
      </div>
    {:else}
      {#each filteredEntries as entry (entry.id)}
        <div class="history-entry {entry.pinned ? 'pinned' : ''}">
          <div class="entry-header">
            <div class="entry-meta">
              {#if entry.pinned}
                <span class="pin-indicator">
                   <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M21.41 11.58l-9-9C12.05 2.22 11.55 2 11 2H4c-1.1 0-2 .9-2 2v7c0 .55.22 1.05.59 1.42l9 9c.36.36.86.58 1.41.58.55 0 1.05-.22 1.41-.59l7-7c.37-.36.59-.86.59-1.41 0-.55-.23-1.06-.59-1.42zM5.5 7C4.67 7 4 6.33 4 5.5S4.67 4 5.5 4 7 4.67 7 5.5 6.33 7 5.5 7z"/></svg>
                   Pinned
                </span>
                <span class="meta-separator">•</span>
              {/if}
              <span class="timestamp">{formatDate(entry.timestamp)}</span>
              {#if entry.model}
                <span class="meta-separator">•</span>
                <span class="model-info">{entry.model}</span>
              {/if}
              <span class="meta-separator">•</span>
              <span class="source-info">{getSourceLabel(entry.source)}</span>
            </div>
            <div class="entry-actions">
              <button
                class="icon-btn {entry.pinned ? 'active' : ''}"
                title={entry.pinned ? "Unpin" : "Pin to top"}
                onclick={() => togglePin(entry.id)}
              >
                 <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill={entry.pinned ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12h10"/><path d="M9 4v16"/><path d="M3 9l3 3-3 3"/><path d="M14 8l2-2 2 2 2-2 2 2"/><path d="M14 16l2 2 2-2 2 2 2-2"/></svg>
                 <!-- Simple Pin Icon -->
                 <svg style="display:none;" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill={entry.pinned ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="17" x2="12" y2="22"></line><path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z"></path></svg>
                 <!-- Thumbtack icon -->
                 <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill={entry.pinned ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"></path></svg>
              </button>
              <button
                class="icon-btn"
                title="Copy to clipboard"
                onclick={() => copyToClipboard(entry.text)}
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>
              </button>
              <button
                class="icon-btn danger"
                title="Delete"
                onclick={() => deleteEntry(entry.id)}
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
              </button>
            </div>
          </div>

          <div class="entry-content">
            {#if expandedEntries.has(entry.id) || entry.text.length < 200}
              <p class="text-full">{entry.text}</p>
            {:else}
              <p class="text-preview">{entry.text.slice(0, 200)}...</p>
            {/if}

            {#if entry.text.length > 200}
              <button
                class="expand-btn"
                onclick={() => toggleExpanded(entry.id)}
              >
                {expandedEntries.has(entry.id) ? 'Show less' : 'Show more'}
              </button>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .history-page {
    padding: 1.5rem;
    max-width: 1000px;
    margin: 0 auto;
  }

  .page-header {
    margin-bottom: 2rem;
  }

  .page-header h1 {
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

  .controls {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1.5rem;
    background: var(--bg-secondary, #2d2d2d);
    padding: 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--border-primary, #404040);
  }

  .search-bar {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 0.75rem;
    color: var(--text-secondary);
    pointer-events: none;
    display: flex;
  }

  .search-input {
    width: 100%;
    padding: 0.5rem 0.75rem 0.5rem 2.25rem;
    border: 1px solid transparent;
    border-radius: 6px;
    background-color: var(--bg-tertiary, #1a1a1a);
    color: var(--text-primary, #ffffff);
    font-size: 0.9rem;
    transition: all 0.2s;
  }

  .search-input:focus {
    outline: none;
    background-color: var(--bg-primary);
    border-color: var(--border-highlight);
  }

  .action-buttons {
    display: flex;
    gap: 0.5rem;
    border-left: 1px solid var(--border-primary);
    padding-left: 1rem;
  }

  .icon-btn-large {
    padding: 0.5rem;
    border: 1px solid transparent;
    border-radius: 6px;
    background-color: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-btn-large:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .icon-btn-large.danger:hover {
    background-color: rgba(244, 67, 54, 0.1);
    color: var(--error);
  }

  .history-list {
    display: flex;
    flex-direction: column;
    gap: 1px; /* Minimal gap */
    background: var(--border-primary); /* For border effect between items */
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    overflow: hidden;
  }

  .history-entry {
    padding: 1rem 1.25rem;
    background-color: var(--bg-secondary);
    transition: background-color 0.2s;
  }

  .history-entry:hover {
    background-color: var(--bg-tertiary);
  }
  
  .history-entry.pinned {
      background-color: rgba(74, 144, 226, 0.05);
  }
  
  .history-entry.pinned:hover {
      background-color: rgba(74, 144, 226, 0.08);
  }

  .entry-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 0.5rem;
    gap: 1rem;
  }

  .entry-meta {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  .meta-separator {
    opacity: 0.3;
  }
  
  .pin-indicator {
      color: var(--accent-primary, #4A90E2);
      display: flex;
      align-items: center;
      gap: 4px;
      font-weight: 500;
  }

  .entry-actions {
    display: flex;
    gap: 0.25rem;
    opacity: 0;
    transition: opacity 0.2s;
  }

  .history-entry:hover .entry-actions {
    opacity: 1;
  }

  .icon-btn {
    padding: 0.35rem;
    border: none;
    border-radius: 4px;
    background-color: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-btn:hover {
    background-color: var(--bg-primary);
    color: var(--text-primary);
  }
  
  .icon-btn.active {
      color: var(--accent-primary, #4A90E2);
  }

  .icon-btn.danger:hover {
    background-color: rgba(244, 67, 54, 0.1);
    color: var(--error);
  }

  .entry-content {
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .text-full, .text-preview {
    margin: 0;
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .expand-btn {
    margin-top: 0.25rem;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.8rem;
    font-weight: 500;
  }

  .expand-btn:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }

  .loading, .empty-state, .error-message {
    text-align: center;
    padding: 3rem 1rem;
    color: var(--text-secondary);
    background: var(--bg-secondary);
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
