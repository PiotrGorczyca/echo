<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import SettingsNavigation from "../components/Settings/SettingsNavigation.svelte";
  import WelcomePage from "../components/Settings/pages/WelcomePage.svelte";
  import CoreSettingsPage from "../components/Settings/pages/CoreSettingsPage.svelte";
  import HistoryPage from "../components/Settings/pages/HistoryPage.svelte";
  import TasksPage from "../components/Settings/pages/TasksPage.svelte";
  import "../styles/dark-theme.css";

  // Current page state
  let currentPage: 'welcome' | 'core' | 'history' | 'tasks' = $state('welcome');
  let isAnimating = false;
  let hasUnsavedChanges = $state(false);

  // Handle page navigation
  function handlePageChange(event: CustomEvent<string>) {
    if (isAnimating) return;

    const newPage = event.detail as 'welcome' | 'core' | 'history' | 'tasks';
    // Prevent navigation to hidden pages if triggered programmatically
    if (['meetings', 'commands'].includes(newPage)) return;

    isAnimating = true;
    currentPage = newPage;

    setTimeout(() => {
      isAnimating = false;
    }, 200);
  }

  // Handle hide window
  async function hideWindow() {
    try {
      await invoke("hide_main_window");
    } catch (err) {
      console.error("Failed to hide window:", err);
    }
  }

  onMount(() => {
    // Allow backend hotkeys to navigate the Settings window.
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        await listen("navigate-to-tasks", () => {
          currentPage = 'tasks';
        });
      } catch (err) {
        console.error("Failed to set up navigation listener:", err);
      }
    })();

    return () => {
      // cleanup
    };
  });
</script>

<svelte:head>
  <title>Echo</title>
</svelte:head>

<!-- Main Settings App -->
<div class="app-layout">
  <!-- Settings Navigation -->
  <SettingsNavigation
    {currentPage}
    {hasUnsavedChanges}
    on:pageChange={handlePageChange}
    on:close={hideWindow}
  />

  <!-- Page Content -->
  <div class="page-container">
    {#if currentPage === 'welcome'}
      <WelcomePage
        on:navigateToCore={() => handlePageChange(new CustomEvent('pageChange', { detail: 'core' }))}
      />
    {:else if currentPage === 'core'}
      <CoreSettingsPage
        bind:hasUnsavedChanges={hasUnsavedChanges}
      />
    {:else if currentPage === 'history'}
      <HistoryPage />
    {:else if currentPage === 'tasks'}
      <TasksPage />
    {/if}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background-color: var(--bg-primary);
    color: var(--text-primary);
    overflow: hidden; /* Prevent scrollbars on body */
    width: 100%;
  }

  :global(html) {
    background-color: var(--bg-primary);
    overflow: hidden; /* Ensure html also clips overflow */
    width: 100%;
  }

  .app-layout {
    width: 100%;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-primary);
    overflow: hidden; /* Ensure app layout doesn't overflow */
  }

  .page-container {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
    width: 100%;
  }

  /* Custom scrollbar */
  .page-container::-webkit-scrollbar {
    width: 6px;
  }

  .page-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .page-container::-webkit-scrollbar-thumb {
    background: var(--border-primary);
    border-radius: 3px;
  }

  .page-container::-webkit-scrollbar-thumb:hover {
    background: var(--border-highlight);
  }
</style>
