<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import AnimatedLogo from "../../ui/AnimatedLogo.svelte";

  // Voice command state types
  interface VoiceCommandMessage {
    id: string;
    timestamp: number;
    type: 'user' | 'system' | 'result' | 'error';
    content: string;
    metadata?: {
      transcription?: string;
      intent?: string;
      tool?: string;
      server?: string;
      confidence?: number;
      processing_time?: number;
    };
  }

  interface VoiceCommandState {
    is_recording: boolean;
    is_processing: boolean;
    current_state: string; // 'idle' | 'recording' | 'transcribing' | 'processing' | 'executing'
    recording_start_time?: number;
  }

  // Component state
  let messages: VoiceCommandMessage[] = [];
  let commandState: VoiceCommandState = {
    is_recording: false,
    is_processing: false,
    current_state: 'idle'
  };
  let chatContainer: HTMLElement;
  let messageInput = '';
  let isVoiceCommandEnabled = true;

  // Event listeners
  let unlistenVoiceCommand: (() => void) | null = null;
  let unlistenVoiceState: (() => void) | null = null;
  let unlistenRecordingEvents: (() => void) | null = null;

  // Auto-scroll to bottom when new messages arrive
  function scrollToBottom() {
    if (chatContainer) {
      setTimeout(() => {
        chatContainer.scrollTop = chatContainer.scrollHeight;
      }, 100);
    }
  }

  // Update messages array when new messages arrive
  $: if (messages.length > 0) {
    scrollToBottom();
  }

  // Format timestamp for display
  function formatTime(timestamp: number): string {
    const date = new Date(timestamp);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  // Get status indicator for current state
  function getStatusIndicator(state: string): string {
    switch (state) {
      case 'recording': return '🎤';
      case 'transcribing': return '📝';
      case 'processing': return '🧠';
      case 'executing': return '⚡';
      case 'error': return '❌';
      default: return '💬';
    }
  }

  // Load initial messages and state
  async function loadInitialData() {
    try {
      messages = await invoke<VoiceCommandMessage[]>('get_voice_command_messages');
      commandState = await invoke<VoiceCommandState>('get_voice_command_state');
    } catch (error) {
      console.error('Failed to load initial voice command data:', error);
      // Fallback to default state
      commandState = {
        is_recording: false,
        is_processing: false,
        current_state: 'idle'
      };
      messages = [];
    }
  }

  // Send manual text command (for testing)
  async function sendTextCommand() {
    if (!messageInput.trim()) return;

    const userMessage = messageInput.trim();
    messageInput = '';

    try {
      await invoke('process_text_command', { commandText: userMessage });
      // Messages will be updated via event listeners
    } catch (error) {
      console.error('Failed to process text command:', error);
    }
  }

  // Handle Enter key in input
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      sendTextCommand();
    }
  }

  // Clear chat history
  async function clearHistory() {
    if (confirm('Clear all voice command history?')) {
      try {
        await invoke('clear_voice_command_messages');
        messages = [];
      } catch (error) {
        console.error('Failed to clear messages:', error);
      }
    }
  }

  // Test voice command system
  async function testVoiceCommand() {
    try {
      await invoke('start_voice_recording');
      // State and messages will be updated via event listeners
    } catch (error) {
      console.error('Voice command test failed:', error);
    }
  }

  // Start actual voice recording
  async function startVoiceRecording() {
    try {
      // Call the backend command directly instead of emitting event
      await invoke('start_voice_recording');
      
      // The state and messages will be updated via event listeners from the backend
      
    } catch (error) {
      console.error('Failed to start voice recording:', error);
    }
  }

  // Stop voice recording
  async function stopVoiceRecording() {
    try {
      // For now, we don't have a specific stop command since the test runs automatically
      // In the future, this would call a stop_voice_command_recording command
      console.log('Stop recording requested');
      
    } catch (error) {
      console.error('Failed to stop voice recording:', error);
    }
  }

  onMount(async () => {
    // Load initial data
    await loadInitialData();

    // Add welcome message if no messages exist
    if (messages.length === 0) {
      const welcomeMessage = {
        id: `welcome-${Date.now()}`,
        timestamp: Date.now(),
        type: 'system' as const,
        content: 'Voice Commands interface ready. Use the buttons below to test, or type commands for development.'
      };
      messages = [welcomeMessage];
    }

    try {
      // Set up voice command event listeners
      unlistenVoiceCommand = await listen('voice-command-event', (event: any) => {
        const newMessage = event.payload as VoiceCommandMessage;
        messages = [...messages, newMessage];
      });

      unlistenVoiceState = await listen('voice-command-state', (event: any) => {
        commandState = { ...commandState, ...event.payload };
      });

      console.log('Voice command listeners set up successfully');

    } catch (error) {
      console.error('Failed to set up voice command listeners:', error);
    }
  });

  onDestroy(() => {
    if (unlistenVoiceCommand) unlistenVoiceCommand();
    if (unlistenVoiceState) unlistenVoiceState();
  });
</script>

<div class="voice-commands-page">
  <!-- Header with status -->
  <div class="page-header">
    <div class="header-content">
      <h2>Voice Commands</h2>
      <div class="status-indicator">
        <span class="status-icon">{getStatusIndicator(commandState.current_state)}</span>
        <span class="status-text">
          {#if commandState.is_recording}
            Recording...
          {:else if commandState.is_processing}
            Processing...
          {:else if commandState.current_state === 'transcribing'}
            Transcribing...
          {:else if commandState.current_state === 'executing'}
            Executing...
          {:else}
            Ready
          {/if}
        </span>
      </div>
    </div>
    
    <div class="header-actions">
      <button class="test-btn" onclick={testVoiceCommand} disabled={commandState.is_recording || commandState.is_processing}>
        Test Command
      </button>
      <button class="clear-btn" onclick={clearHistory} disabled={messages.length === 0}>
        Clear History
      </button>
    </div>
  </div>

  <!-- Voice Command Visualization -->
  <div class="voice-visualization">
    <AnimatedLogo 
      isRecording={commandState.is_recording}
      state={commandState.current_state as 'idle' | 'recording' | 'transcribing' | 'processing' | 'executing'}
      size="160px"
    />
    
    <!-- Recording Controls -->
    <div class="recording-controls">
      {#if commandState.current_state === 'idle'}
        <button class="record-btn" onclick={startVoiceRecording}>
          🎤 Start Voice Recording
        </button>
      {:else if commandState.is_recording}
        <button class="stop-btn" onclick={stopVoiceRecording}>
          ⏹️ Stop Recording
        </button>
      {:else}
        <div class="processing-indicator">
          {getStatusIndicator(commandState.current_state)} Processing...
        </div>
      {/if}
    </div>
  </div>

  <!-- Instructions -->
  <div class="instructions">
    <div class="instruction-item">
      <span class="instruction-icon">⌨️</span>
      <span>Double-tap <strong>Shift</strong> triggers the Cloud Code task orchestrator (not voice commands)</span>
    </div>
    <div class="instruction-item">
      <span class="instruction-icon">💬</span>
      <span>Type commands below for testing and development</span>
    </div>
    {#if !isVoiceCommandEnabled}
      <div class="instruction-item warning">
        <span class="instruction-icon">⚠️</span>
        <span>Voice commands are disabled. Enable in Core Settings.</span>
      </div>
    {/if}
  </div>

  <!-- Chat Interface -->
  <div class="chat-container" bind:this={chatContainer}>
    <div class="chat-messages">
      {#each messages as message (message.id)}
        <div class="message message-{message.type}">
          <div class="message-header">
            <span class="message-type">
              {#if message.type === 'user'}
                👤 You
              {:else if message.type === 'system'}
                🤖 System
              {:else if message.type === 'result'}
                ✅ Result
              {:else if message.type === 'error'}
                ❌ Error
              {/if}
            </span>
            <span class="message-time">{formatTime(message.timestamp)}</span>
          </div>
          
          <div class="message-content">
            {message.content}
          </div>

          {#if message.metadata}
            <div class="message-metadata">
              {#if message.metadata.transcription}
                <div class="metadata-item">
                  <strong>Transcription:</strong> "{message.metadata.transcription}"
                </div>
              {/if}
              {#if message.metadata.intent}
                <div class="metadata-item">
                  <strong>Intent:</strong> {message.metadata.intent}
                </div>
              {/if}
              {#if message.metadata.tool && message.metadata.server}
                <div class="metadata-item">
                  <strong>Tool:</strong> {message.metadata.server}/{message.metadata.tool}
                </div>
              {/if}
              {#if message.metadata.confidence}
                <div class="metadata-item">
                  <strong>Confidence:</strong> {(message.metadata.confidence * 100).toFixed(1)}%
                </div>
              {/if}
              {#if message.metadata.processing_time}
                <div class="metadata-item">
                  <strong>Processing Time:</strong> {message.metadata.processing_time}ms
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <!-- Command Input -->
  <div class="command-input">
    <div class="input-container">
      <input
        type="text"
        placeholder="Type a command to test..."
        bind:value={messageInput}
        on:keydown={handleKeydown}
        disabled={commandState.is_recording || commandState.is_processing}
      />
      <button 
        class="send-btn" 
        onclick={sendTextCommand}
        disabled={!messageInput.trim() || commandState.is_recording || commandState.is_processing}
      >
        Send
      </button>
    </div>
    <div class="input-help">
      Press Enter to send • Shift+Enter for new line
    </div>
  </div>
</div>

<style>
  .voice-commands-page {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .page-header {
    padding: 20px;
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .header-content {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .header-content h2 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-primary);
    border-radius: 8px;
    font-size: 0.9rem;
  }

  .status-icon {
    font-size: 1.1rem;
  }

  .header-actions {
    display: flex;
    gap: 12px;
  }

  .test-btn, .clear-btn {
    padding: 8px 16px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 0.9rem;
    transition: all 0.2s ease;
  }

  .test-btn:hover, .clear-btn:hover {
    background: var(--bg-hover);
    border-color: var(--border-hover);
  }

  .test-btn:disabled, .clear-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .instructions {
    padding: 16px 20px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border-primary);
  }

  .instruction-item {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
    font-size: 0.9rem;
  }

  .instruction-item:last-child {
    margin-bottom: 0;
  }

  .instruction-item.warning {
    color: var(--text-warning);
  }

  .instruction-icon {
    font-size: 1.1rem;
    width: 20px;
    text-align: center;
  }

  .chat-container {
    flex: 1;
    overflow-y: auto;
    padding: 0;
  }

  .chat-messages {
    padding: 20px;
    min-height: 100%;
  }

  .message {
    margin-bottom: 16px;
    padding: 12px;
    border-radius: 8px;
    border-left: 3px solid;
  }

  .message-user {
    background: var(--bg-secondary);
    border-left-color: #4A90E2;
    margin-left: 40px;
  }

  .message-system {
    background: var(--bg-tertiary);
    border-left-color: #7B68EE;
  }

  .message-result {
    background: var(--bg-success);
    border-left-color: #50C878;
  }

  .message-error {
    background: var(--bg-error);
    border-left-color: #FF6B6B;
  }

  .message-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
    font-size: 0.85rem;
    opacity: 0.8;
  }

  .message-content {
    font-size: 0.95rem;
    line-height: 1.4;
    margin-bottom: 8px;
  }

  .message-metadata {
    font-size: 0.8rem;
    opacity: 0.7;
    padding-top: 8px;
    border-top: 1px solid var(--border-primary);
  }

  .metadata-item {
    margin-bottom: 4px;
  }

  .metadata-item:last-child {
    margin-bottom: 0;
  }

  .command-input {
    padding: 20px;
    border-top: 1px solid var(--border-primary);
    background: var(--bg-secondary);
  }

  .input-container {
    display: flex;
    gap: 12px;
    margin-bottom: 8px;
  }

  .input-container input {
    flex: 1;
    padding: 12px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .input-container input:focus {
    outline: none;
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 2px rgba(74, 144, 226, 0.2);
  }

  .send-btn {
    padding: 12px 20px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.9rem;
    transition: background-color 0.2s ease;
  }

  .send-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .send-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .input-help {
    font-size: 0.8rem;
    opacity: 0.6;
    text-align: center;
  }

  /* Voice Visualization Styles */
  .voice-visualization {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 24px;
    padding: 32px 20px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border-primary);
  }

  .recording-controls {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .record-btn, .stop-btn {
    padding: 12px 24px;
    border: none;
    border-radius: 8px;
    font-size: 1rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s ease;
    min-width: 180px;
  }

  .record-btn {
    background: linear-gradient(135deg, #4CAF50, #45a049);
    color: white;
    box-shadow: 0 2px 8px rgba(76, 175, 80, 0.3);
  }

  .record-btn:hover {
    background: linear-gradient(135deg, #45a049, #3d8b40);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(76, 175, 80, 0.4);
  }

  .stop-btn {
    background: linear-gradient(135deg, #FF4444, #e53e3e);
    color: white;
    box-shadow: 0 2px 8px rgba(255, 68, 68, 0.3);
  }

  .stop-btn:hover {
    background: linear-gradient(135deg, #e53e3e, #c53030);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(255, 68, 68, 0.4);
  }

  .processing-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    font-size: 1rem;
    font-weight: 500;
    color: var(--text-primary);
    min-width: 180px;
    justify-content: center;
  }

  /* Scrollbar styling */
  .chat-container::-webkit-scrollbar {
    width: 6px;
  }

  .chat-container::-webkit-scrollbar-track {
    background: var(--bg-secondary);
  }

  .chat-container::-webkit-scrollbar-thumb {
    background: var(--border-primary);
    border-radius: 3px;
  }

  .chat-container::-webkit-scrollbar-thumb:hover {
    background: var(--border-hover);
  }
</style> 