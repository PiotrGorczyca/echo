<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from "@tauri-apps/api/core";

  const dispatch = createEventDispatcher();

  function navigateToCore() {
    dispatch('navigateToCore');
  }

  type TranscriptionMode = "OpenAI" | "LocalWhisper" | "CandleWhisper" | "FasterWhisper";

  interface AppSettings {
    transcription_mode: TranscriptionMode;
  }

  let transcriptionMode = $state<string>("Loading...");

  // Map mode values to display names
  const modeDisplayNames: Record<string, string> = {
    "OpenAI": "OpenAI API",
    "LocalWhisper": "Whisper.cpp",
    "CandleWhisper": "HuggingFace Whisper",
    "FasterWhisper": "Faster Whisper"
  };

  onMount(async () => {
    try {
      const settings = await invoke<AppSettings>("get_settings");
      transcriptionMode = modeDisplayNames[settings.transcription_mode] || settings.transcription_mode;
    } catch (err) {
      console.error("Failed to load settings:", err);
      transcriptionMode = "Unknown";
    }
  });
</script>

<div class="welcome-page">
  <div class="page-content">
    <!-- Header Section -->
    <div class="welcome-header">
      <h2 class="welcome-title">Echo</h2>
      <p class="welcome-tagline">Intelligent Voice Assistant</p>
    </div>

    <!-- Status Overview -->
    <div class="status-overview card">
      <div class="card-header">
        <h3>System Status</h3>
      </div>
      <div class="status-grid">
        <div class="status-item">
          <span class="status-label">Mode</span>
          <span class="status-value">{transcriptionMode}</span>
        </div>
      </div>
    </div>

    <!-- Quick Actions -->
    <div class="quick-actions">
      <h3>Quick Actions</h3>
      <div class="action-buttons">
        <button class="btn btn-secondary action-btn" onclick={navigateToCore}>
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.1a2 2 0 0 1-1-1.72v-.51a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
          Settings
        </button>
      </div>
    </div>

    <!-- Getting Started Guide -->
    <div class="getting-started card">
      <h3>Getting Started</h3>
      <div class="steps">
        <div class="step">
          <div class="step-number">1</div>
          <div class="step-content">
            <strong>Set up transcription</strong>
            <p>Choose your preferred transcription method in settings</p>
          </div>
        </div>
        <div class="step">
          <div class="step-number">2</div>
          <div class="step-content">
            <strong>Configure audio</strong>
            <p>Select your microphone and test recording</p>
          </div>
        </div>
        <div class="step">
          <div class="step-number">3</div>
          <div class="step-content">
            <strong>Test your setup</strong>
            <p>Try a quick recording to verify everything works</p>
          </div>
        </div>
        <div class="step">
          <div class="step-number">4</div>
          <div class="step-content">
            <strong>Explore AI features</strong>
            <p>Discover advanced AI agent capabilities</p>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .welcome-page {
    padding: 1.5rem;
    height: 100%;
    overflow-y: auto;
  }

  .page-content {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    max-width: 600px;
    margin: 0 auto;
  }

  .welcome-header {
    text-align: center;
    padding: 1rem 0;
  }

  .welcome-title {
    margin: 0;
    font-size: 2rem;
    font-weight: 700;
    background: linear-gradient(to right, var(--text-primary), var(--text-secondary));
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    letter-spacing: -0.02em;
  }

  .welcome-tagline {
    margin: 0.5rem 0 0;
    color: var(--text-secondary);
    font-size: 1rem;
  }

  .status-overview {
    padding: 1.25rem;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }

  .status-overview h3, .quick-actions h3, .getting-started h3 {
    margin: 0;
    font-size: 0.875rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    font-weight: 600;
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    transition: background-color 0.3s ease;
  }

  .status-indicator.connected {
    background-color: var(--success);
    box-shadow: 0 0 0 2px rgba(34, 197, 94, 0.2);
  }

  .status-indicator.disconnected {
    background-color: var(--error);
    box-shadow: 0 0 0 2px rgba(239, 68, 68, 0.2);
  }

  .pulse {
    animation: pulse 2s infinite;
  }

  @keyframes pulse {
    0% { box-shadow: 0 0 0 0 rgba(34, 197, 94, 0.4); }
    70% { box-shadow: 0 0 0 6px rgba(34, 197, 94, 0); }
    100% { box-shadow: 0 0 0 0 rgba(34, 197, 94, 0); }
  }

  .status-grid {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .status-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border-primary);
  }

  .status-item:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .status-item:first-child {
    padding-top: 0;
  }

  .status-label {
    color: var(--text-secondary);
    font-size: 0.875rem;
  }

  .status-value {
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.875rem;
  }

  .status-success {
    color: var(--success);
  }

  .status-error {
    color: var(--error);
  }

  .status-muted {
    color: var(--text-muted);
  }

  .quick-actions h3 {
    margin-bottom: 1rem;
    text-align: center;
  }

  .action-buttons {
    display: grid;
    grid-template-columns: 1fr;
    gap: 1rem;
  }

  .action-btn {
    height: auto;
    padding: 1rem;
    flex-direction: column;
    gap: 0.75rem;
    transition: transform var(--duration-fast), background-color var(--duration-fast);
  }

  .action-btn:hover {
    transform: translateY(-2px);
    background-color: var(--bg-tertiary);
  }

  .getting-started {
    padding: 1.25rem;
  }

  .getting-started h3 {
    margin-bottom: 1.25rem;
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }

  .step {
    display: flex;
    gap: 1rem;
  }

  .step-number {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background-color: var(--bg-tertiary);
    color: var(--accent-primary);
    border: 1px solid var(--border-highlight);
    border-radius: 50%;
    font-size: 0.75rem;
    font-weight: 600;
    flex-shrink: 0;
    margin-top: 2px;
  }

  .step-content {
    flex: 1;
  }

  .step-content strong {
    display: block;
    color: var(--text-primary);
    font-size: 0.875rem;
    margin-bottom: 0.25rem;
  }

  .step-content p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.8125rem;
    line-height: 1.4;
  }
</style>
