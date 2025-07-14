<script lang="ts">
  import { createEventDispatcher } from 'svelte';

  export let currentPage: 'welcome' | 'core' | 'advanced' | 'commands' = 'welcome';
  export let hasUnsavedChanges = false;

  const dispatch = createEventDispatcher();

  function handlePageChange(page: 'welcome' | 'core' | 'advanced' | 'commands') {
    if (page !== currentPage) {
      dispatch('pageChange', page);
    }
  }

  function handleClose() {
    dispatch('close');
  }

  // Page titles for display
  const pageTitle = {
    welcome: 'Welcome',
    core: 'Core Settings', 
    advanced: 'Advanced Features',
    commands: 'Voice Commands'
  };
</script>

<nav class="settings-navigation">
  <!-- Header with title and close button -->
  <div class="nav-header">
    <div class="nav-title">
      <button class="back-btn" on:click={() => handlePageChange('welcome')} aria-label="Back to welcome">
        ←
      </button>
      <h1 id="settings-title">{pageTitle[currentPage]}</h1>
    </div>
    <button class="close-btn" on:click={handleClose} aria-label="Close settings">
      ×
    </button>
  </div>

  <!-- Page navigation tabs -->
  <div class="nav-tabs">
    <button 
      class="nav-tab {currentPage === 'welcome' ? 'active' : ''}"
      on:click={() => handlePageChange('welcome')}
      aria-pressed={currentPage === 'welcome'}
    >
      <span class="tab-indicator"></span>
      Welcome
    </button>
    
    <button 
      class="nav-tab {currentPage === 'core' ? 'active' : ''}"
      on:click={() => handlePageChange('core')}
      aria-pressed={currentPage === 'core'}
    >
      <span class="tab-indicator"></span>
      Core Settings
      {#if hasUnsavedChanges && currentPage === 'core'}
        <span class="unsaved-dot" aria-label="Unsaved changes"></span>
      {/if}
    </button>
    
    <button 
      class="nav-tab {currentPage === 'advanced' ? 'active' : ''}"
      on:click={() => handlePageChange('advanced')}
      aria-pressed={currentPage === 'advanced'}
    >
      <span class="tab-indicator"></span>
      Advanced Features
      {#if hasUnsavedChanges && currentPage === 'advanced'}
        <span class="unsaved-dot" aria-label="Unsaved changes"></span>
      {/if}
    </button>

    <button 
      class="nav-tab {currentPage === 'commands' ? 'active' : ''}"
      on:click={() => handlePageChange('commands')}
      aria-pressed={currentPage === 'commands'}
    >
      <span class="tab-indicator"></span>
      Voice Commands
    </button>
  </div>
</nav>

<style>
  .settings-navigation {
    background-color: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
    padding: 0;
  }

  .nav-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-primary);
  }

  .nav-title {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .back-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 1.2rem;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .back-btn:hover {
    background-color: var(--hover-bg);
    color: var(--accent-primary);
  }

  #settings-title {
    margin: 0;
    font-size: 1.2rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 1.5rem;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
    transition: all var(--duration-fast) var(--ease-out);
    line-height: 1;
  }

  .close-btn:hover {
    background-color: var(--hover-bg);
    color: var(--error);
  }

  .nav-tabs {
    display: flex;
    padding: 0 20px;
  }

  .nav-tab {
    position: relative;
    flex: 1;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 500;
    padding: 12px 8px;
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
    text-align: center;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .nav-tab:hover {
    color: var(--accent-secondary);
  }

  .nav-tab.active {
    color: var(--accent-primary);
  }

  .nav-tab.active .tab-indicator {
    background-color: var(--accent-primary);
  }

  .tab-indicator {
    position: absolute;
    bottom: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 24px;
    height: 2px;
    background-color: transparent;
    border-radius: 1px;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .unsaved-dot {
    width: 6px;
    height: 6px;
    background-color: var(--warning);
    border-radius: 50%;
    margin-left: 4px;
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    .nav-header {
      padding: 12px 16px;
    }

    .nav-tabs {
      padding: 0 16px;
    }

    .nav-tab {
      font-size: 0.8rem;
      padding: 10px 4px;
    }

    #settings-title {
      font-size: 1.1rem;
    }
  }

  /* Focus states for accessibility */
  .back-btn:focus,
  .close-btn:focus,
  .nav-tab:focus {
    outline: none;
    box-shadow: var(--shadow-glow);
  }
</style> 