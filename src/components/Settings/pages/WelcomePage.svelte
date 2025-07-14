<script lang="ts">
  import { createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher();

  function navigateToCore() {
    dispatch('navigateToCore');
  }

  function navigateToAdvanced() {
    dispatch('navigateToAdvanced');
  }

  function navigateToCommands() {
    dispatch('navigateToCommands');
  }
</script>

<div class="welcome-page">
  <div class="page-content">
    <!-- Header Section -->
    <div class="welcome-header">
      <div class="app-icon">
        <img src="/favicon.png" alt="EchoType" class="brand-icon" />
      </div>
      <h2 class="welcome-title">Welcome to EchoType AI Agent</h2>
      <p class="welcome-tagline">Your intelligent voice-to-text assistant</p>
    </div>

    <!-- Status Overview -->
    <div class="status-overview card">
      <h3>System Status</h3>
      <div class="status-grid">
        <div class="status-item">
          <span class="status-label">Current Mode:</span>
          <span class="status-value">Candle Whisper</span>
        </div>
        <div class="status-item">
          <span class="status-label">Connection:</span>
          <span class="status-value status-success">✅ Ready</span>
        </div>
        <div class="status-item">
          <span class="status-label">Voice Activation:</span>
          <span class="status-value">Disabled</span>
        </div>
        <div class="status-item">
          <span class="status-label">Voice Commands:</span>
          <span class="status-value">Available</span>
        </div>
      </div>
    </div>

    <!-- Quick Actions -->
    <div class="quick-actions">
      <h3>Quick Actions</h3>
      <div class="action-buttons">
        <button class="btn btn-primary" on:click={navigateToCore}>
          ⚙️ Configure Settings
        </button>
        <button class="btn btn-secondary" on:click={navigateToAdvanced}>
          🚀 Advanced Features
        </button>
        <button class="btn btn-accent" on:click={navigateToCommands}>
          🎤 Voice Commands
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
            <p>Choose your preferred transcription method</p>
          </div>
        </div>
        <div class="step">
          <div class="step-number">2</div>
          <div class="step-content">
            <strong>Configure audio device</strong>
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
            <strong>Try voice commands</strong>
            <p>Use Double Shift tap to test AI-powered voice commands</p>
          </div>
        </div>
        <div class="step">
          <div class="step-number">5</div>
          <div class="step-content">
            <strong>Explore AI features</strong>
            <p>Discover advanced AI agent capabilities and MCP tools</p>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .welcome-page {
    padding: 24px;
    height: 100%;
  }

  .page-content {
    display: flex;
    flex-direction: column;
    gap: 24px;
    max-width: 100%;
  }

  .welcome-header {
    text-align: center;
    padding: 20px 0;
  }

  .app-icon {
    margin-bottom: 16px;
    display: flex;
    justify-content: center;
  }

  .brand-icon {
    width: 64px;
    height: 64px;
    border-radius: 12px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .welcome-title {
    margin: 0 0 8px 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .welcome-tagline {
    margin: 0;
    color: var(--text-secondary);
    font-size: 1rem;
  }

  .status-overview {
    padding: 20px;
  }

  .status-overview h3 {
    margin: 0 0 16px 0;
    font-size: 1.1rem;
    color: var(--text-primary);
  }

  .status-grid {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .status-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .status-label {
    color: var(--text-secondary);
    font-size: 0.9rem;
  }

  .status-value {
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.9rem;
  }

  .quick-actions {
    text-align: center;
  }

  .quick-actions h3 {
    margin: 0 0 16px 0;
    font-size: 1.1rem;
    color: var(--text-primary);
  }

  .action-buttons {
    display: flex;
    gap: 12px;
    justify-content: center;
    flex-wrap: wrap;
  }

  .btn-accent {
    background: linear-gradient(135deg, #9C27B0, #673AB7);
    color: white;
    border: none;
  }

  .btn-accent:hover {
    background: linear-gradient(135deg, #8E24AA, #5E35B1);
    transform: translateY(-1px);
  }

  .getting-started {
    padding: 20px;
  }

  .getting-started h3 {
    margin: 0 0 16px 0;
    font-size: 1.1rem;
    color: var(--text-primary);
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .step {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .step-number {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background-color: var(--accent-primary);
    color: white;
    border-radius: 50%;
    font-size: 0.8rem;
    font-weight: 600;
    flex-shrink: 0;
  }

  .step-content {
    flex: 1;
  }

  .step-content strong {
    display: block;
    color: var(--text-primary);
    font-size: 0.9rem;
    margin-bottom: 4px;
  }

  .step-content p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.85rem;
    line-height: 1.4;
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    .welcome-page {
      padding: 16px;
    }

    .action-buttons {
      flex-direction: column;
    }

    .welcome-title {
      font-size: 1.3rem;
    }
  }
</style> 