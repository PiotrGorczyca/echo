<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import SettingsNavigation from "../components/Settings/SettingsNavigation.svelte";
  import WelcomePage from "../components/Settings/pages/WelcomePage.svelte";
  import CoreSettingsPage from "../components/Settings/pages/CoreSettingsPage.svelte";
  import AdvancedFeaturesPage from "../components/Settings/pages/AdvancedFeaturesPage.svelte";
  import VoiceCommandsPage from "../components/Settings/pages/VoiceCommandsPage.svelte";

  // Current page state
  let currentPage: 'welcome' | 'core' | 'advanced' | 'commands' = 'welcome';
  let isAnimating = false;
  let hasUnsavedChanges = false;

  // Handle page navigation
  function handlePageChange(event: CustomEvent<string>) {
    if (isAnimating) return;
    
    isAnimating = true;
    currentPage = event.detail as 'welcome' | 'core' | 'advanced' | 'commands';
    
    setTimeout(() => {
      isAnimating = false;
    }, 200); // Match page transition duration
  }

  // Handle minimize window (previously close)
  async function minimizeWindow() {
    try {
      await invoke("hide_main_window");
    } catch (err) {
      console.error("Failed to minimize window:", err);
    }
  }

  // Handle escape key to minimize
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      minimizeWindow();
    }
  }

  onMount(() => {
    // Position the window properly when the app loads
    (async () => {
      try {
        await invoke("position_main_window");
      } catch (err) {
        console.error("Failed to position window:", err);
      }
    })();
    
    // Listen for keyboard events
    document.addEventListener('keydown', handleKeydown);
    
    // Listen for voice command navigation from double Shift tap
    let unlistenNavigation: (() => void) | null = null;
    
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlistenNavigation = await listen("navigate-to-voice-commands", () => {
          console.log("Double Shift detected - navigating to voice commands");
          currentPage = 'commands';
        });
      } catch (err) {
        console.error("Failed to set up navigation listener:", err);
      }
    })();

    return () => {
      document.removeEventListener('keydown', handleKeydown);
      if (unlistenNavigation) {
        unlistenNavigation();
      }
    };
  });
</script>

<svelte:head>
  <title>Echo Settings</title>
</svelte:head>

<!-- Main Settings App -->
<div class="settings-app">
  <!-- Settings Navigation -->
  <SettingsNavigation 
    {currentPage} 
    {hasUnsavedChanges}
    on:pageChange={handlePageChange}
    on:close={minimizeWindow}
  />

  <!-- Page Content -->
  <div class="settings-content">
    {#if currentPage === 'welcome'}
      <WelcomePage 
        on:navigateToCore={() => handlePageChange(new CustomEvent('pageChange', { detail: 'core' }))}
        on:navigateToAdvanced={() => handlePageChange(new CustomEvent('pageChange', { detail: 'advanced' }))}
        on:navigateToCommands={() => handlePageChange(new CustomEvent('pageChange', { detail: 'commands' }))}
      />
    {:else if currentPage === 'core'}
      <CoreSettingsPage 
        bind:hasUnsavedChanges={hasUnsavedChanges}
      />
    {:else if currentPage === 'advanced'}
      <AdvancedFeaturesPage />
    {:else if currentPage === 'commands'}
      <VoiceCommandsPage />
    {/if}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background-color: var(--bg-primary, #1a1a1a);
    color: var(--text-primary, #ffffff);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
    overflow: hidden;
  }

  :global(html) {
    background-color: var(--bg-primary, #1a1a1a);
  }

  .settings-app {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-primary, #1a1a1a);
  }

  .settings-content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Custom scrollbar */
  .settings-content::-webkit-scrollbar {
    width: 8px;
  }

  .settings-content::-webkit-scrollbar-track {
    background: var(--bg-secondary, #2d2d2d);
  }

  .settings-content::-webkit-scrollbar-thumb {
    background: var(--border-primary, #404040);
    border-radius: 4px;
  }

  .settings-content::-webkit-scrollbar-thumb:hover {
    background: var(--accent-primary, #4A90E2);
  }

  /* Ensure all global button and input styles are available */
  :global(.btn) {
    padding: 8px 16px;
    border: 1px solid var(--border-primary, #404040);
    border-radius: 6px;
    background-color: var(--bg-secondary, #2d2d2d);
    color: var(--text-primary, #ffffff);
    font-size: 0.9rem;
    transition: all var(--duration-fast, 150ms) var(--ease-out, ease-out);
    cursor: pointer;
  }

  :global(.btn:hover) {
    background-color: var(--hover-bg, #404040);
    border-color: var(--border-accent, #4A90E2);
  }

  :global(.btn-primary) {
    background-color: var(--accent-primary, #4A90E2);
    border-color: var(--accent-primary, #4A90E2);
    color: white;
  }

  :global(.btn-primary:hover) {
    background-color: var(--accent-tertiary, #3A7BD5);
    border-color: var(--accent-tertiary, #3A7BD5);
  }

  :global(.btn-secondary) {
    background-color: transparent;
    border-color: var(--accent-primary, #4A90E2);
    color: var(--accent-primary, #4A90E2);
  }

  :global(.btn-secondary:hover) {
    background-color: var(--accent-primary, #4A90E2);
    color: white;
  }

  :global(.card) {
    background-color: var(--bg-secondary, #2d2d2d);
    border: 1px solid var(--border-primary, #404040);
    border-radius: 8px;
    box-shadow: var(--shadow-sm, 0 2px 4px rgba(0, 0, 0, 0.3));
  }

  :global(.status-success) {
    color: var(--success, #4CAF50);
  }

  :global(.status-warning) {
    color: var(--warning, #FF9800);
  }

  :global(.status-error) {
    color: var(--error, #F44336);
  }

  :global(.status-info) {
    color: var(--info, #2196F3);
  }
</style>
