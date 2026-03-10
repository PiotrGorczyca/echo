<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { marked } from "marked";

  // Configure marked for safe rendering
  marked.setOptions({
    breaks: true,
    gfm: true,
  });

  type OverlayMode = "idle" | "prompt" | "running" | "complete" | "error";
  type StatusType = "idle" | "recording" | "transcribing" | "claude-working" | "claude-waiting" | "success" | "error";
  
  interface Repository {
    id: string;
    name: string;
    path: string;
  }

  // Mode & visibility
  type OverlaySize = "mini" | "full";
  let mode: OverlayMode = $state("idle");
  let overlaySize: OverlaySize = $state("mini");
  let isVisible = $state(false);
  let statusMessage = $state("");
  let statusType: StatusType = $state("idle");
  
  // Prompt input
  let promptText = $state("");
  let repositories: Repository[] = $state([]);
  let focusRepoIds: string[] = $state([]);
  let isAllRepos = $derived(focusRepoIds.length === 0);
  
  // Claude output - store as objects for better formatting
  let claudeMessages = $state<Array<{type: 'tool' | 'text' | 'result', content: string}>>([]);
  let claudeFinalResult = $state<string | null>(null);
  let claudeQuestion = $state<string | null>(null);
  let userInput = $state("");
  let isRecordingResponse = $state(false);
  let isSendingResponse = $state(false);
  
  let currentWindow: any = null;
  let outputScrollContainer: HTMLDivElement | null = null;

  // Render markdown safely
  function renderMarkdown(text: string): string {
    try {
      return marked.parse(text) as string;
    } catch {
      return text;
    }
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      
      currentWindow = getCurrentWindow();
      
      // Load repositories
      try {
        repositories = await invoke<Repository[]>("list_task_repositories");
      } catch (err) {
        console.error("Failed to load repos:", err);
      }

      // Status updates (recording/transcribing/success/error)
      // These show a small mini-indicator near the cursor, NOT the full orchestrator.
      unlisteners.push(await listen<any>("status-update", (event) => {
        const payload = event.payload;
        statusMessage = payload.message || "";
        statusType = payload.type || "idle";
        if (statusType === "idle") {
          hideWindow();
        } else {
          mode = "running";
          overlaySize = "mini";
          showMiniWindow();
        }
      }));

      // Orchestration recording started - uses full overlay
      unlisteners.push(await listen("orchestration-recording-started", () => {
        mode = "running";
        overlaySize = "full";
        statusMessage = "Recording... speak now (double-tap Shift to stop)";
        statusType = "recording";
        claudeMessages = [];
        claudeFinalResult = null;
        claudeQuestion = null;
        showFullWindow();
      }));

      // Transcription completed - show prompt for review (full overlay)
      unlisteners.push(await listen("orchestration-prompt-updated", (event: any) => {
        const prompt = event?.payload?.prompt;
        if (typeof prompt === "string") {
          promptText = prompt;
          mode = "prompt";
          overlaySize = "full";
          statusMessage = "Review your request, then send to Claude Code";
          statusType = "success";
          showFullWindow();
        }
      }));

      // Claude Code output (full overlay)
      unlisteners.push(await listen("claude-output", (event: any) => {
        const line = event?.payload;
        if (typeof line === "string") {
          parseClaudeOutput(line);
        }
        mode = "running";
        overlaySize = "full";
        statusMessage = "Claude Code is working...";
        statusType = "claude-working";
        showFullWindow();
      }));

      // Claude Code completion
      unlisteners.push(await listen("claude-complete", async () => {
        mode = "complete";
        if (claudeQuestion) {
          statusMessage = "Claude Code is waiting for your response";
          statusType = "claude-waiting";
        } else {
          statusMessage = "Claude Code completed";
          statusType = "success";
        }
        
        // Sync tasks to cache after Claude completes
        try {
          await invoke("sync_tasks_to_cache");
          console.log("Tasks synced to cache");
        } catch (err) {
          console.warn("Failed to sync tasks:", err);
        }
      }));

      // Open prompt mode directly (full overlay)
      unlisteners.push(await listen("open-orchestrate-overlay", () => {
        mode = "prompt";
        overlaySize = "full";
        statusMessage = "Describe what Claude Code should do";
        statusType = "idle";
        claudeMessages = [];
        claudeFinalResult = null;
        claudeQuestion = null;
        showFullWindow();
      }));

      // Wake word detection - mini indicator
      unlisteners.push(await listen("wake-word-detected", () => {
        mode = "running";
        overlaySize = "mini";
        statusMessage = "Wake word detected - Starting...";
        statusType = "recording";
        showMiniWindow();
      }));

      // Recording cancelled
      unlisteners.push(await listen("recording-cancelled", () => {
        statusMessage = "Recording cancelled";
        statusType = "error";
        mode = "error";
      }));

    })();

    return () => {
      unlisteners.forEach(fn => fn());
    };
  });

  function parseClaudeOutput(line: string) {
    try {
      const parsed = JSON.parse(line);
      
      if (parsed.type === "assistant" && parsed.message?.content) {
        for (const block of parsed.message.content) {
          if (block.type === "text" && block.text) {
            const text = block.text.trim();
            // Detect questions
            if (text.endsWith("?") || text.includes("Would you like") || text.includes("Do you want")) {
              claudeQuestion = text;
              statusMessage = "Claude Code is waiting for your response";
              statusType = "claude-waiting";
            }
            claudeMessages = [...claudeMessages, { type: 'text', content: text }].slice(-30);
          } else if (block.type === "tool_use") {
            claudeMessages = [...claudeMessages, { type: 'tool', content: block.name }].slice(-30);
          }
        }
      } else if (parsed.type === "result") {
        const result = parsed.result || "Completed";
        // Store the final result for display
        claudeFinalResult = result;
        // Check for questions in result
        if (result.includes("?") || result.includes("Would you like")) {
          claudeQuestion = result;
          statusMessage = "Claude Code is waiting for your response";
          statusType = "claude-waiting";
        }
        claudeMessages = [...claudeMessages, { type: 'result', content: result }].slice(-30);
      } else if (parsed.type === "system" && parsed.subtype === "init") {
        claudeMessages = [{ type: 'text', content: `Started in \`${parsed.cwd}\`` }];
      }
    } catch {
      if (line.trim() && !line.startsWith("{")) {
        claudeMessages = [...claudeMessages, { type: 'text', content: line }].slice(-30);
      }
    }
    
    // Auto-scroll
    setTimeout(() => {
      if (outputScrollContainer) {
        outputScrollContainer.scrollTop = outputScrollContainer.scrollHeight;
      }
    }, 10);
  }

  async function showMiniWindow() {
    isVisible = true;
    overlaySize = "mini";
    try {
      if (!currentWindow) return;
      const { PhysicalSize } = await import("@tauri-apps/api/window");
      // The Rust side positions the window near cursor via position_overlay_near_cursor.
      // We just need to set the size to the small indicator size.
      await currentWindow.setSize(new PhysicalSize(400, 120));
      await currentWindow.show();
      // Do NOT call setFocus - this would steal focus from the user's active window
    } catch (err) {
      console.error("Failed to show mini overlay:", err);
    }
  }

  async function showFullWindow() {
    isVisible = true;
    overlaySize = "full";
    try {
      if (!currentWindow) return;
      const { PhysicalPosition, PhysicalSize, primaryMonitor } = await import("@tauri-apps/api/window");

      const monitor = await primaryMonitor();
      if (monitor) {
        const screenWidth = monitor.size.width;
        const screenHeight = monitor.size.height;
        const windowWidth = 800;
        const windowHeight = 480;
        const x = Math.round((screenWidth - windowWidth) / 2);
        const y = screenHeight - windowHeight - 8;

        await currentWindow.setSize(new PhysicalSize(windowWidth, windowHeight));
        await currentWindow.setPosition(new PhysicalPosition(x, y));
      }
      await currentWindow.show();
      await currentWindow.setFocus();
    } catch (err) {
      console.error("Failed to show full overlay:", err);
    }
  }

  async function hideWindow() {
    isVisible = false;
    mode = "idle";
    claudeMessages = [];
    claudeFinalResult = null;
    claudeQuestion = null;
    userInput = "";
    promptText = "";
    statusMessage = "";
    statusType = "idle";
    
    try {
      if (currentWindow) {
        await currentWindow.hide();
      }
    } catch (err) {
      console.error("Failed to hide overlay:", err);
    }
  }

  async function sendToClaudeCode() {
    if (!promptText.trim()) return;
    
    mode = "running";
    statusMessage = "Sending to Claude Code...";
    statusType = "claude-working";
    claudeMessages = [];
    claudeFinalResult = null;
    claudeQuestion = null;
    
    try {
      await invoke("orchestrate_claude_code_tasks", {
        workspacePath: null,
        userRequest: promptText,
        focusRepoIds: focusRepoIds.length > 0 ? focusRepoIds : null,
      });
    } catch (err) {
      console.error("Failed to orchestrate:", err);
      statusMessage = `Error: ${err}`;
      statusType = "error";
      mode = "error";
    }
  }

  async function sendResponse() {
    if (!userInput.trim() || isSendingResponse) return;
    
    const response = userInput.trim();
    isSendingResponse = true;
    
    // Add user's response to the messages
    claudeMessages = [...claudeMessages, { type: 'text', content: `**You:** ${response}` }];
    
    userInput = "";
    claudeQuestion = null;
    
    try {
      statusMessage = "Sending response...";
      statusType = "claude-working";
      mode = "running";
      
      await invoke("send_claude_response", { response });
      
      statusMessage = "Response sent, Claude Code continuing...";
    } catch (err) {
      console.error("Failed to send response:", err);
      statusMessage = `Failed to send: ${err}`;
      statusType = "error";
      mode = "error";
    } finally {
      isSendingResponse = false;
    }
  }

  async function startVoiceInput() {
    isRecordingResponse = true;
    try {
      await invoke("start_voice_response_recording");
    } catch (err) {
      console.error("Failed to start voice:", err);
      isRecordingResponse = false;
    }
  }

  function toggleRepoFocus(repoId: string) {
    if (focusRepoIds.includes(repoId)) {
      focusRepoIds = focusRepoIds.filter(id => id !== repoId);
    } else {
      focusRepoIds = [...focusRepoIds, repoId];
    }
  }

  function selectAllRepos() {
    focusRepoIds = [];
  }

  function startNewRequest() {
    mode = "prompt";
    statusMessage = "New request";
    statusType = "idle";
    claudeMessages = [];
    claudeFinalResult = null;
    claudeQuestion = null;
    promptText = "";
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      hideWindow();
    } else if (event.key === "Enter" && !event.shiftKey) {
      if (mode === "prompt" && promptText.trim()) {
        event.preventDefault();
        sendToClaudeCode();
      } else if ((mode === "complete" || mode === "running") && claudeQuestion && userInput.trim()) {
        event.preventDefault();
        sendResponse();
      }
    }
  }

  function getStatusColor(type: StatusType): string {
    switch (type) {
      case "recording": return "#ef4444";
      case "transcribing": return "#f59e0b";
      case "claude-working": return "#3b82f6";
      case "claude-waiting": return "#8b5cf6";
      case "success": return "#22c55e";
      case "error": return "#ef4444";
      default: return "#6b7280";
    }
  }
</script>

<svelte:head>
  <title>Echo</title>
</svelte:head>

<svelte:window on:keydown={handleKeydown} />

{#if isVisible && overlaySize === "mini"}
  <!-- Mini indicator: small status bar near cursor -->
  <div class="mini-overlay">
    <div class="mini-dot" style="background: {getStatusColor(statusType)}">
      {#if statusType === "recording"}
        <span class="pulse"></span>
      {:else if statusType === "transcribing"}
        <span class="spinner"></span>
      {/if}
    </div>
    <span class="mini-text">{statusMessage || "Echo"}</span>
  </div>
{:else if isVisible}
  <div class="overlay-window">
    <!-- Fixed Header -->
    <div class="header" style="--accent-color: {getStatusColor(statusType)}">
      <div class="status-dot" style="background: {getStatusColor(statusType)}">
        {#if statusType === "recording"}
          <span class="pulse"></span>
        {:else if statusType === "transcribing" || statusType === "claude-working"}
          <span class="spinner"></span>
        {/if}
      </div>
      <span class="status-text">{statusMessage || "Echo"}</span>
      <button class="close-btn" onclick={hideWindow} title="Close (Esc)">×</button>
    </div>

    <!-- Scrollable Content Area -->
    <div class="content-area" bind:this={outputScrollContainer}>
      <!-- Prompt Input Mode -->
      {#if mode === "prompt"}
        <div class="prompt-section">
          <textarea
            class="prompt-input"
            placeholder="What should Claude Code do? Focus on tasks - create, update, check status..."
            bind:value={promptText}
            rows="4"
          ></textarea>
          
          {#if repositories.length > 0}
            <div class="repo-section">
              <span class="repo-label">Repositories:</span>
              <div class="repo-selector">
                <button 
                  class="repo-chip" 
                  class:active={isAllRepos}
                  onclick={selectAllRepos}
                >
                  All
                </button>
                {#each repositories as repo (repo.id)}
                  <button 
                    class="repo-chip"
                    class:active={focusRepoIds.includes(repo.id)}
                    onclick={() => toggleRepoFocus(repo.id)}
                    title={repo.path}
                  >
                    {repo.name}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Output Area with Markdown -->
      {#if (mode === "running" || mode === "complete" || mode === "error")}
        <div class="output-section">
          {#if claudeMessages.length === 0}
            <div class="loading-text">Waiting for Claude Code...</div>
          {:else}
            {#each claudeMessages as msg, i (i)}
              {#if msg.type === 'tool'}
                <div class="message tool">⚡ Using: {msg.content}</div>
              {:else if msg.type === 'result'}
                <div class="message result">
                  <div class="result-header">✓ Result</div>
                  <div class="markdown-content">{@html renderMarkdown(msg.content)}</div>
                </div>
              {:else}
                <div class="message text">
                  <div class="markdown-content">{@html renderMarkdown(msg.content)}</div>
                </div>
              {/if}
            {/each}
          {/if}
        </div>
      {/if}

      <!-- Question/Response Section -->
      {#if claudeQuestion && (mode === "complete" || mode === "running")}
        <div class="question-section">
          <div class="question-label">Claude is asking:</div>
          <div class="question-content">{@html renderMarkdown(claudeQuestion)}</div>
        </div>
      {/if}
    </div>

    <!-- Fixed Footer -->
    <div class="footer">
      {#if mode === "prompt"}
        <button class="voice-btn" onclick={startVoiceInput} title="Voice input">🎤 Voice</button>
        <div class="spacer"></div>
        <button 
          class="action-btn primary" 
          onclick={sendToClaudeCode}
          disabled={!promptText.trim()}
        >
          Send to Claude Code
        </button>
      {:else if claudeQuestion && (mode === "complete" || mode === "running")}
        <input
          type="text"
          class="response-input"
          placeholder="Type your response..."
          bind:value={userInput}
          disabled={isSendingResponse}
        />
        <button 
          class="voice-btn" 
          class:recording={isRecordingResponse}
          onclick={startVoiceInput}
          disabled={isSendingResponse}
        >
          🎤
        </button>
        <button 
          class="action-btn primary" 
          onclick={sendResponse}
          disabled={!userInput.trim() || isSendingResponse}
        >
          {isSendingResponse ? 'Sending...' : 'Send'}
        </button>
      {:else if mode === "running"}
        <span class="footer-hint">Claude Code is working...</span>
        <div class="spacer"></div>
        <button class="action-btn" onclick={hideWindow}>Minimize</button>
      {:else if mode === "complete" || mode === "error"}
        <button class="action-btn" onclick={startNewRequest}>New Request</button>
        <div class="spacer"></div>
        <button class="action-btn primary" onclick={hideWindow}>Done</button>
      {:else}
        <div class="spacer"></div>
        <button class="action-btn" onclick={hideWindow}>Close</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent;
    font-family: 'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    overflow: hidden;
  }

  :global(html) {
    background: transparent;
  }

  .overlay-window {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    height: calc(100vh - 8px);
    background: rgba(10, 10, 14, 0.98);
    backdrop-filter: blur(24px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 16px 16px 0 0;
    margin: 4px 4px 0 4px;
    box-shadow: 0 -8px 50px rgba(0, 0, 0, 0.7);
    overflow: hidden;
  }

  /* Fixed Header */
  .header {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(255, 255, 255, 0.02);
  }

  .status-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex-shrink: 0;
    position: relative;
  }

  .pulse {
    position: absolute;
    inset: -4px;
    border-radius: 50%;
    background: inherit;
    opacity: 0.4;
    animation: pulse 1.5s ease-in-out infinite;
  }

  .spinner {
    position: absolute;
    inset: -2px;
    border: 2px solid transparent;
    border-top-color: white;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .status-text {
    flex: 1;
    color: #e4e4e7;
    font-size: 14px;
    font-weight: 600;
  }

  .close-btn {
    background: none;
    border: none;
    color: #52525b;
    font-size: 22px;
    cursor: pointer;
    padding: 0 6px;
    line-height: 1;
    transition: color 0.15s;
  }

  .close-btn:hover {
    color: #e4e4e7;
  }

  /* Scrollable Content */
  .content-area {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .content-area::-webkit-scrollbar {
    width: 8px;
  }

  .content-area::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 4px;
  }

  /* Prompt Section */
  .prompt-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .prompt-input {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 10px;
    padding: 14px 16px;
    color: #e4e4e7;
    font-size: 14px;
    resize: none;
    outline: none;
    transition: border-color 0.2s, background 0.2s;
    font-family: inherit;
    line-height: 1.5;
  }

  .prompt-input::placeholder {
    color: #52525b;
  }

  .prompt-input:focus {
    border-color: #3b82f6;
    background: rgba(255, 255, 255, 0.08);
  }

  .repo-section {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .repo-label {
    color: #71717a;
    font-size: 12px;
    font-weight: 500;
  }

  .repo-selector {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .repo-chip {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 6px;
    padding: 6px 12px;
    color: #a1a1aa;
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .repo-chip:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #e4e4e7;
  }

  .repo-chip.active {
    background: #3b82f6;
    border-color: #3b82f6;
    color: white;
  }

  /* Output Section */
  .output-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .loading-text {
    color: #52525b;
    font-style: italic;
    padding: 20px;
    text-align: center;
  }

  .message {
    padding: 12px 14px;
    border-radius: 10px;
    font-size: 13px;
    line-height: 1.6;
  }

  .message.tool {
    background: rgba(59, 130, 246, 0.1);
    border: 1px solid rgba(59, 130, 246, 0.2);
    color: #60a5fa;
    font-weight: 500;
  }

  .message.text {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.06);
    color: #d4d4d8;
  }

  .message.result {
    background: rgba(34, 197, 94, 0.08);
    border: 1px solid rgba(34, 197, 94, 0.2);
  }

  .result-header {
    color: #4ade80;
    font-weight: 600;
    margin-bottom: 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  /* Markdown Content Styling */
  .markdown-content {
    color: #d4d4d8;
  }

  .markdown-content :global(h1),
  .markdown-content :global(h2),
  .markdown-content :global(h3) {
    color: #e4e4e7;
    margin: 0.5em 0;
    font-weight: 600;
  }

  .markdown-content :global(h1) { font-size: 1.3em; }
  .markdown-content :global(h2) { font-size: 1.15em; }
  .markdown-content :global(h3) { font-size: 1.05em; }

  .markdown-content :global(p) {
    margin: 0.5em 0;
  }

  .markdown-content :global(ul),
  .markdown-content :global(ol) {
    margin: 0.5em 0;
    padding-left: 1.5em;
  }

  .markdown-content :global(li) {
    margin: 0.25em 0;
  }

  .markdown-content :global(code) {
    background: rgba(0, 0, 0, 0.3);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: 'SF Mono', 'Fira Code', Consolas, monospace;
    font-size: 0.9em;
    color: #f472b6;
  }

  .markdown-content :global(pre) {
    background: rgba(0, 0, 0, 0.4);
    padding: 12px 14px;
    border-radius: 8px;
    overflow-x: auto;
    margin: 0.75em 0;
  }

  .markdown-content :global(pre code) {
    background: none;
    padding: 0;
    color: #d4d4d8;
  }

  .markdown-content :global(strong) {
    color: #e4e4e7;
    font-weight: 600;
  }

  .markdown-content :global(a) {
    color: #60a5fa;
    text-decoration: none;
  }

  .markdown-content :global(a:hover) {
    text-decoration: underline;
  }

  .markdown-content :global(blockquote) {
    border-left: 3px solid #3b82f6;
    margin: 0.5em 0;
    padding-left: 1em;
    color: #a1a1aa;
  }

  /* Question Section */
  .question-section {
    background: rgba(139, 92, 246, 0.1);
    border: 1px solid rgba(139, 92, 246, 0.3);
    border-radius: 10px;
    padding: 14px 16px;
  }

  .question-label {
    color: #a78bfa;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .question-content {
    color: #c4b5fd;
  }

  .question-content :global(p) {
    margin: 0;
  }

  /* Fixed Footer */
  .footer {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 20px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(255, 255, 255, 0.02);
  }

  .spacer {
    flex: 1;
  }

  .footer-hint {
    color: #52525b;
    font-size: 12px;
  }

  .response-input {
    flex: 1;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 8px;
    padding: 10px 14px;
    color: #e4e4e7;
    font-size: 13px;
    outline: none;
    transition: border-color 0.2s;
  }

  .response-input::placeholder {
    color: #52525b;
  }

  .response-input:focus {
    border-color: #8b5cf6;
  }

  .response-input:disabled {
    opacity: 0.6;
  }

  /* Buttons */
  .voice-btn {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    padding: 10px 14px;
    font-size: 14px;
    color: #a1a1aa;
    cursor: pointer;
    transition: all 0.15s;
  }

  .voice-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #e4e4e7;
  }

  .voice-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .voice-btn.recording {
    background: #ef4444;
    border-color: #ef4444;
    color: white;
    animation: pulse-bg 1s infinite;
  }

  .action-btn {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 10px 18px;
    color: #a1a1aa;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .action-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #e4e4e7;
  }

  .action-btn.primary {
    background: #3b82f6;
    border-color: #3b82f6;
    color: white;
  }

  .action-btn.primary:hover:not(:disabled) {
    background: #2563eb;
  }

  .action-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  @keyframes pulse {
    0%, 100% {
      transform: scale(1);
      opacity: 0.4;
    }
    50% {
      transform: scale(1.6);
      opacity: 0;
    }
  }

  @keyframes pulse-bg {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Mini overlay - small indicator near cursor */
  .mini-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 18px;
    background: rgba(10, 10, 14, 0.95);
    backdrop-filter: blur(16px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    margin: 4px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.5);
  }

  .mini-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
    position: relative;
  }

  .mini-dot .pulse {
    position: absolute;
    inset: -3px;
    border-radius: 50%;
    background: inherit;
    opacity: 0.4;
    animation: pulse 1.5s ease-in-out infinite;
  }

  .mini-dot .spinner {
    position: absolute;
    inset: -2px;
    border: 2px solid transparent;
    border-top-color: white;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .mini-text {
    color: #e4e4e7;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
