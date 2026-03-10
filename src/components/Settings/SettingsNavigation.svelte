<script lang="ts">
  import { createEventDispatcher } from 'svelte';

  export let currentPage: 'welcome' | 'core' | 'commands' | 'meetings' | 'history' | 'tasks' = 'welcome';
  export let hasUnsavedChanges = false;

  const dispatch = createEventDispatcher();

  function handlePageChange(page: 'welcome' | 'core' | 'commands' | 'meetings' | 'history' | 'tasks') {
    if (page !== currentPage) {
      dispatch('pageChange', page);
    }
  }

  function handleClose() {
    dispatch('close');
  }
</script>

<nav class="settings-nav">
  <div class="nav-content">
    <div class="nav-items">
      <button 
        class="nav-item {currentPage === 'welcome' ? 'active' : ''}"
        onclick={() => handlePageChange('welcome')}
        aria-label="Home"
        title="Home"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>
        <span class="nav-label">Home</span>
      </button>
      
      <button 
        class="nav-item {currentPage === 'core' ? 'active' : ''}"
        onclick={() => handlePageChange('core')}
        aria-label="Settings"
        title="Settings"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.1a2 2 0 0 1-1-1.72v-.51a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
        <span class="nav-label">Settings</span>
        {#if hasUnsavedChanges && currentPage === 'core'}
          <span class="status-dot"></span>
        {/if}
      </button>
      
      <button
        class="nav-item {currentPage === 'history' ? 'active' : ''}"
        onclick={() => handlePageChange('history')}
        aria-label="History"
        title="History"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
        <span class="nav-label">History</span>
      </button>

      <button
        class="nav-item {currentPage === 'tasks' ? 'active' : ''}"
        onclick={() => handlePageChange('tasks')}
        aria-label="Tasks"
        title="Tasks"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
        <span class="nav-label">Tasks</span>
      </button>
    </div>
    
    <button class="close-btn" onclick={handleClose} aria-label="Close">
      <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
    </button>
  </div>
</nav>

<style>
  .settings-nav {
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
    padding: 0.5rem 1rem;
    position: sticky;
    top: 0;
    z-index: 50;
  }

  .nav-content {
    display: flex;
    align-items: center;
    justify-content: space-between;
    max-width: 100%;
    margin: 0 auto;
  }

  .nav-items {
    display: flex;
    gap: 0.5rem;
    overflow-x: auto;
    /* Hide scrollbar but allow scrolling if needed */
    scrollbar-width: none;
    -ms-overflow-style: none;
  }

  .nav-items::-webkit-scrollbar {
    display: none;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
    white-space: nowrap;
    position: relative;
  }

  .nav-item:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .nav-item.active {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
    border-color: var(--border-primary);
  }

  .nav-item svg {
    opacity: 0.7;
    transition: opacity var(--duration-fast);
  }

  .nav-item:hover svg,
  .nav-item.active svg {
    opacity: 1;
    color: var(--accent-primary);
  }

  .nav-label {
    display: none;
  }

  /* Show labels on larger screens or if space permits */
  @media (min-width: 400px) {
    .nav-label {
      display: inline;
    }
  }

  .status-dot {
    width: 6px;
    height: 6px;
    background-color: var(--warning);
    border-radius: 50%;
    position: absolute;
    top: 6px;
    right: 6px;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.5rem;
    background: transparent;
    border: none;
    border-radius: 0.5rem;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--duration-fast);
    margin-left: 0.5rem;
  }

  .close-btn:hover {
    background-color: var(--bg-tertiary);
    color: var(--error);
  }
</style>
