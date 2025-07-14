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
  let isVoiceActivationListening: boolean = $state(false);

  onMount(() => {
    let unlisten: any;
    let unlistenWakeWord: any;
    let voiceStatusInterval: ReturnType<typeof setInterval> | null = null;
    
    // Setup event listener and get window reference
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const { invoke } = await import("@tauri-apps/api/core");
      currentWindow = getCurrentWindow();
      
      unlisten = await listen<StatusUpdate>("status-update", (event) => {
        currentStatus = event.payload;
        showStatus();
      });
      
      // Listen for wake word detection events
      unlistenWakeWord = await listen("wake-word-detected", (event) => {
        console.log("Wake word detected in overlay:", event.payload);
        isVoiceActivationListening = true;
        // Show brief indication that wake word was detected
        currentStatus = {
          message: "🎤 Wake word detected - Starting recording...",
          type: "recording",
          timestamp: Date.now()
        };
        showStatus();
      });
      
      // Poll voice activation status every 5 seconds
      voiceStatusInterval = setInterval(async () => {
        try {
          isVoiceActivationListening = await invoke("get_voice_activation_status");
        } catch (err) {
          console.error("Failed to check voice activation status:", err);
        }
      }, 5000);
      
      // Check initial status
      try {
        isVoiceActivationListening = await invoke("get_voice_activation_status");
      } catch (err) {
        console.error("Failed to check initial voice activation status:", err);
      }
    })();

    // Add keyboard event listener
    document.addEventListener('keydown', handleKeydown);

    // Cleanup function
    return () => {
      if (unlisten) unlisten();
      if (unlistenWakeWord) unlistenWakeWord();
      if (voiceStatusInterval) clearInterval(voiceStatusInterval);
      stopCursorTracking();
      document.removeEventListener('keydown', handleKeydown);
    };
  });

  async function showStatus() {
    isVisible = true;
    
    // Clear any existing timeout
    if (fadeTimeout) {
      clearTimeout(fadeTimeout);
    }
    
    try {
      // Show the window (already configured to not steal focus)
      if (currentWindow) {
        await currentWindow.show();
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
      // Hide the window
      if (currentWindow) {
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

  // Handle keyboard events for dismissing error overlays
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && currentStatus.type === "error") {
      hideStatus();
    }
  }

  // Hide overlay when recording stops (for success/error states)
  $effect(() => {
    if (currentStatus.type === "success") {
      // Auto-hide success messages after 3 seconds
      if (fadeTimeout) clearTimeout(fadeTimeout);
      fadeTimeout = setTimeout(async () => {
        await hideStatus();
        fadeTimeout = null;
      }, 3000);
    } else if (currentStatus.type === "error") {
      // Error messages stay visible until manually dismissed with ESC
      if (fadeTimeout) clearTimeout(fadeTimeout);
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
      {#if isVoiceActivationListening && currentStatus.type === "idle"}
        <span class="voice-indicator">🎙️</span>
      {/if}
      {#if currentStatus.type === "error"}
        <span class="dismiss-hint">Press ESC to dismiss</span>
      {/if}
    </div>
    {#if currentStatus.type === "recording"}
      <div class="recording-pulse"></div>
    {:else if currentStatus.type === "transcribing"}
      <div class="transcribing-spinner"></div>
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
    border-radius: 6px;
    padding: 6px 8px;
    margin: 4px;
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.4);
    animation: slideIn 0.3s ease-out;
    position: relative;
    max-width: 200px;
    min-height: 24px;
  }

  .status-content {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-icon {
    font-size: 14px;
    flex-shrink: 0;
  }

  .status-message {
    color: white;
    font-size: 13px;
    font-weight: 500;
    line-height: 1.3;
    flex: 1;
    word-wrap: break-word;
    overflow-wrap: break-word;
    max-height: 40px;
    overflow: hidden;
  }

  .voice-indicator {
    opacity: 0.7;
    font-size: 10px;
    margin-left: 4px;
    animation: pulse-gentle 2s infinite;
  }

  .dismiss-hint {
    opacity: 0.6;
    font-size: 10px;
    color: #ccc;
    margin-left: 4px;
    font-style: italic;
    align-self: center;
  }

  .recording-pulse {
    position: absolute;
    top: 50%;
    right: 8px;
    transform: translateY(-50%);
    width: 6px;
    height: 6px;
    background: #f44336;
    border-radius: 50%;
    animation: pulse 1s infinite;
  }

  .transcribing-spinner {
    position: absolute;
    top: 50%;
    right: 8px;
    transform: translateY(-50%);
    width: 10px;
    height: 10px;
    border: 2px solid #ff9800;
    border-top: 2px solid transparent;
    border-radius: 50%;
    animation: spin 1s linear infinite;
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

  @keyframes pulse-gentle {
    0%, 100% {
      opacity: 1;
      transform: translateY(-50%) scale(1);
    }
    50% {
      opacity: 0.5;
      transform: translateY(-50%) scale(1.1);
    }
  }

  @keyframes spin {
    0% {
      transform: translateY(-50%) rotate(0deg);
    }
    100% {
      transform: translateY(-50%) rotate(360deg);
    }
  }


</style> 