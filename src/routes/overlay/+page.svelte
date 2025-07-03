<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";

  interface StatusUpdate {
    message: string;
    type: "recording" | "transcribing" | "success" | "error" | "idle";
    timestamp: number;
  }

  let currentStatus: StatusUpdate = $state({
    message: "Ready",
    type: "idle",
    timestamp: Date.now()
  });

  let isVisible: boolean = $state(false);
  let debugVisible: boolean = $state(false); // Debug mode off
  let fadeTimeout: ReturnType<typeof setTimeout> | null = null;
  let cursorTrackingInterval: ReturnType<typeof setInterval> | null = null;
  let currentWindow: any = null;

  onMount(() => {
    let unlisten: any;
    
    // Setup event listener and get window reference
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      currentWindow = getCurrentWindow();
      
      unlisten = await listen<StatusUpdate>("status-update", (event) => {
        currentStatus = event.payload;
        showStatus();
      });
    })();

    // Cleanup function
    return () => {
      if (unlisten) unlisten();
      stopCursorTracking();
    };
  });

  async function showStatus() {
    isVisible = true;
    
    // Clear any existing timeout
    if (fadeTimeout) {
      clearTimeout(fadeTimeout);
    }
    
    try {
      // Show the window and bring it to top
      if (currentWindow) {
        await currentWindow.show();
        await currentWindow.setAlwaysOnTop(true);
      }
    } catch (error) {
      console.error("Failed to show overlay window:", error);
    }
    
    // Start cursor tracking when status is shown
    startCursorTracking();
    
    // Hide after 3 seconds for non-persistent states
    if (currentStatus.type !== "recording" && currentStatus.type !== "transcribing") {
      fadeTimeout = setTimeout(async () => {
        await hideStatus();
        fadeTimeout = null;
      }, 3000);
    }
  }

  async function hideStatus() {
    isVisible = false;
    stopCursorTracking();
    
    try {
      // Properly hide the window and remove always-on-top
      if (currentWindow) {
        await currentWindow.setAlwaysOnTop(false);
        await currentWindow.hide();
      }
      
      // Also call the backend command as a backup
      await invoke("hide_overlay_window");
    } catch (error) {
      console.error("Failed to hide overlay window:", error);
    }
  }

  async function startCursorTracking() {
    if (cursorTrackingInterval) return; // Already tracking
    
    const updatePosition = async () => {
      try {
        // Get cursor position from backend
        const [x, y] = await invoke<[number, number]>("get_cursor_position_command");
        
        // Position window near cursor (offset so it doesn't interfere)
        const windowX = x + 20;
        const windowY = y - 100; // Above cursor
        
        const { PhysicalPosition } = await import("@tauri-apps/api/window");
        if (currentWindow) {
          await currentWindow.setPosition(new PhysicalPosition(windowX, windowY));
        }
      } catch (error) {
        console.error("Failed to update cursor position:", error);
      }
    };
    
    // Update position immediately
    updatePosition();
    
    // Update position every 50ms while visible (smoother tracking)
    cursorTrackingInterval = setInterval(updatePosition, 50);
  }

  function stopCursorTracking() {
    if (cursorTrackingInterval) {
      clearInterval(cursorTrackingInterval);
      cursorTrackingInterval = null;
    }
  }

  function getStatusIcon(type: string): string {
    switch (type) {
      case "recording": return "🎤";
      case "transcribing": return "⏳";
      case "success": return "✅";
      case "error": return "❌";
      default: return "💬";
    }
  }

  function getStatusColor(type: string): string {
    switch (type) {
      case "recording": return "#f44336";
      case "transcribing": return "#ff9800";
      case "success": return "#4caf50";
      case "error": return "#f44336";
      default: return "#2196f3";
    }
  }

  onDestroy(() => {
    if (fadeTimeout) {
      clearTimeout(fadeTimeout);
    }
    stopCursorTracking();
  });

  // Hide overlay when recording stops (for success/error states)
  $effect(() => {
    if (currentStatus.type === "success" || currentStatus.type === "error") {
      // Auto-hide after showing success/error
      if (fadeTimeout) clearTimeout(fadeTimeout);
      fadeTimeout = setTimeout(async () => {
        await hideStatus();
        fadeTimeout = null;
      }, 3000);
    } else if (currentStatus.type === "idle") {
      // Hide immediately for idle state
      hideStatus();
    }
  });
</script>

<svelte:head>
  <title>EchoType Status</title>
</svelte:head>

<!-- Debug indicator - always visible -->
<!-- {#if debugVisible}
  <div class="debug-indicator">
    DEBUG: Overlay window is loaded and visible!<br/>
    Current status: {currentStatus.type}<br/>
    isVisible: {isVisible}
  </div>
{/if} -->

{#if isVisible}
  <div 
    class="overlay-container"
    style="border-left-color: {getStatusColor(currentStatus.type)}"
  >
    <div class="status-content">
      <span class="status-icon">{getStatusIcon(currentStatus.type)}</span>
      <span class="status-message">{currentStatus.message}</span>
    </div>
    {#if currentStatus.type === "recording"}
      <div class="recording-pulse"></div>
    {/if}
  </div>
{/if}

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif;
    overflow: hidden;
  }

  :global(html) {
    background: transparent;
  }

  .overlay-container {
    background: rgba(26, 26, 26, 0.95);
    backdrop-filter: blur(10px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-left: 4px solid #2196f3;
    border-radius: 8px;
    padding: 16px 20px;
    margin: 8px;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
    animation: slideIn 0.3s ease-out;
    position: relative;
    max-width: 380px;
    min-height: 60px;
  }

  .status-content {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .status-icon {
    font-size: 18px;
    flex-shrink: 0;
    margin-top: 2px;
  }

  .status-message {
    color: white;
    font-size: 15px;
    font-weight: 500;
    line-height: 1.4;
    flex: 1;
    word-wrap: break-word;
    overflow-wrap: break-word;
    max-height: 60px;
    overflow: hidden;
  }

  .recording-pulse {
    position: absolute;
    top: 50%;
    right: 12px;
    transform: translateY(-50%);
    width: 8px;
    height: 8px;
    background: #f44336;
    border-radius: 50%;
    animation: pulse 1s infinite;
  }

  @keyframes slideIn {
    from {
      opacity: 0;
      transform: translateY(-10px) scale(0.95);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
      transform: translateY(-50%) scale(1);
    }
    50% {
      opacity: 0.5;
      transform: translateY(-50%) scale(1.1);
    }
  }

  .debug-indicator {
    position: fixed;
    top: 10px;
    left: 10px;
    background: red;
    color: white;
    padding: 10px;
    border-radius: 4px;
    font-size: 12px;
    z-index: 9999;
    border: 2px solid yellow;
  }
</style> 