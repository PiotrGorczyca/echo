<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { hasUnsavedChanges = $bindable(false) } = $props();

  // Types
  interface AudioDevice {
    name: string;
    id: string;
  }

  interface RecordingState {
    is_recording: boolean;
    device_name: string;
  }

  interface AppSettings {
    api_key: string;
    selected_device_id: string;
    auto_paste: boolean;
    transcription_mode: TranscriptionMode;
    whisper_model_size: WhisperModelSize;
    whisper_model_path: string | null;
    device_type: DeviceType;
    enable_voice_activation: boolean;
    wake_words: string[];
    listening_device_id: string | null;
    wake_word_sensitivity: number;
    wake_word_timeout_ms: number;
    voice_energy_threshold: number | null;
    auto_calibrate_threshold: boolean;
    wake_word_model_size: WhisperModelSize;
    user_mcp_servers: any[];
  }

  type TranscriptionMode = "OpenAI" | "LocalWhisper" | "CandleWhisper";
  type WhisperModelSize = "Tiny" | "Base" | "Small" | "Medium" | "Large" | "LargeTurbo" | "DistilMedium" | "DistilLargeV2" | "DistilLargeV3";
  type DeviceType = "Cpu" | "Cuda" | "Metal" | "Rocm";

  // State
  let audioDevices: AudioDevice[] = $state([]);
  let settings: AppSettings = $state({
    api_key: "",
    selected_device_id: "",
    auto_paste: true,
    transcription_mode: "CandleWhisper" as TranscriptionMode,
    whisper_model_size: "DistilMedium" as WhisperModelSize,
    whisper_model_path: null,
    device_type: "Rocm" as DeviceType,
    enable_voice_activation: false,
    wake_words: [],
    listening_device_id: null,
    wake_word_sensitivity: 0.5,
    wake_word_timeout_ms: 5000,
    voice_energy_threshold: null,
    auto_calibrate_threshold: true,
    wake_word_model_size: "Base" as WhisperModelSize,
    user_mcp_servers: []
  });

  let recordingState: RecordingState = $state({ is_recording: false, device_name: "" });
  let status: string = $state("Ready");
  let error: string = $state("");
  let isSaving: boolean = $state(false);
  
  // Test recording state
  let isTestRecording: boolean = $state(false);
  let testAudioPath: string = $state("");
  let testRecordingTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(async () => {
    await loadAudioDevices();
    await loadSettings();
    await updateRecordingState();
    setInterval(updateRecordingState, 1000);
  });

  async function loadAudioDevices() {
    try {
      audioDevices = await invoke<AudioDevice[]>("get_audio_devices");
      if (audioDevices.length > 0 && !settings.selected_device_id) {
        settings.selected_device_id = audioDevices[0].id;
      }
    } catch (err) {
      error = `Failed to load audio devices: ${err}`;
    }
  }

  async function loadSettings() {
    try {
      settings = await invoke<AppSettings>("get_settings");
    } catch (err) {
      console.error("Failed to load settings:", err);
    }
  }

  async function updateRecordingState() {
    try {
      recordingState = await invoke<RecordingState>("get_recording_state");
      if (recordingState.is_recording) {
        status = "🎤 Recording... (Double-tap Alt to stop)";
      } else {
        status = "Ready - Double-tap Left Alt to record";
      }
    } catch (err) {
      console.error("Failed to get recording state:", err);
    }
  }

  async function saveSettings() {
    try {
      isSaving = true;
      error = "";
      await invoke("save_settings", { settings });
      await invoke("reload_transcription_service");
      status = "Settings saved successfully!";
      hasUnsavedChanges = false;
      setTimeout(() => {
        if (!recordingState.is_recording) {
          status = "Ready - Double-tap Left Alt to record";
        }
      }, 2000);
    } catch (err) {
      error = `Failed to save settings: ${err}`;
    } finally {
      isSaving = false;
    }
  }

  async function testRecording() {
    try {
      if (isTestRecording) {
        // Stop the test recording
        console.log("Stopping test recording...");
        await invoke("stop_test_recording");
        isTestRecording = false;
        if (testRecordingTimer) {
          clearTimeout(testRecordingTimer);
          testRecordingTimer = null;
        }
        
        // Get the recorded file path and play it back
        console.log("Getting last recording path...");
        const result = await invoke("get_last_recording_path");
        console.log("get_last_recording_path returned:", result);
        
        if (result) {
          testAudioPath = result as string;
          console.log("Test audio path set to:", testAudioPath);
          status = "Test recording completed - click Play to hear it";
          
          // Immediately test if the file exists by checking its info
          try {
            const fileInfo = await invoke("get_audio_file_info", { filePath: testAudioPath });
            console.log("File info immediately after recording:", fileInfo);
          } catch (infoError) {
            console.error("File doesn't seem to exist:", infoError);
            error = "Test recording file not found after recording";
          }
        } else {
          console.error("get_last_recording_path returned null/undefined");
          error = "Failed to get recorded audio file";
        }
      } else {
        // Start a test recording (5 seconds)
        console.log("Starting test recording...");
        isTestRecording = true;
        testAudioPath = "";
        status = "Test recording... (5 seconds)";
        error = "";
        
        await invoke("start_test_recording", { deviceId: settings.selected_device_id });
        console.log("start_test_recording completed");
        
        // Auto-stop after 5 seconds
        testRecordingTimer = setTimeout(async () => {
          try {
            console.log("Auto-stopping test recording after 5 seconds...");
            await invoke("stop_test_recording");
            isTestRecording = false;
            
            // Get the recorded file path
            console.log("Getting recording path after auto-stop...");
            const result = await invoke("get_last_recording_path");
            console.log("Auto-stop get_last_recording_path returned:", result);
            
            if (result) {
              testAudioPath = result as string;
              console.log("Auto-stop test audio path set to:", testAudioPath);
              status = "Test recording completed - click Play to hear it";
              
              // Test file existence
              try {
                const fileInfo = await invoke("get_audio_file_info", { filePath: testAudioPath });
                console.log("Auto-stop file info:", fileInfo);
              } catch (infoError) {
                console.error("Auto-stop file doesn't exist:", infoError);
                error = "Test recording file not found after auto-stop";
              }
            } else {
              console.error("Auto-stop get_last_recording_path returned null/undefined");
              error = "Failed to get recorded audio file";
              status = "Ready - Double-tap Left Alt to record";
            }
          } catch (err) {
            console.error("Auto-stop error:", err);
            error = `Failed to stop test recording: ${err}`;
            isTestRecording = false;
            status = "Ready - Double-tap Left Alt to record";
          }
        }, 5000);
      }
    } catch (err) {
      console.error("Test recording error:", err);
      error = `Failed to start test recording: ${err}`;
      isTestRecording = false;
      status = "Ready - Double-tap Left Alt to record";
    }
  }
  
  async function playTestRecording() {
    if (!testAudioPath) {
      console.error("No test audio path available");
      error = "No test recording available to play";
      return;
    }
    
    try {
      console.log("🎵 Playing test recording natively from path:", testAudioPath);
      status = "Playing test recording...";
      
      // First, verify the audio file format
      try {
        const verification = await invoke("verify_audio_playback", { filePath: testAudioPath }) as string;
        console.log("🔍 Audio verification:", verification);
      } catch (verifyError: any) {
        console.error("❌ Audio verification failed:", verifyError);
        error = `Audio file verification failed: ${verifyError}`;
        return;
      }
      
      // Play the audio file using native system audio player
      try {
        console.log("🔊 Starting native audio playback...");
        const result = await invoke("play_audio_file_native", { filePath: testAudioPath }) as string;
        console.log("✅ Native audio playback result:", result);
        
        status = "Test playback finished";
        error = ""; // Clear any previous errors
        
        // Show success message
        setTimeout(() => {
          status = "Ready - Double-tap Left Alt to record";
        }, 2000);
        
      } catch (playError: any) {
        console.error("❌ Native audio playback failed:", playError);
        error = `Failed to play audio: ${playError}`;
        status = "Playback failed";
      }
      
      // Clean up the test recording file after playback attempt
      setTimeout(() => {
        cleanupTestRecording();
      }, 3000); // Give a bit more time for playback to complete
      
    } catch (err: any) {
      console.error("💥 Unexpected error in playTestRecording:", err);
      error = `Unexpected error: ${err.message}`;
      status = "Error occurred";
      cleanupTestRecording();
    }
  }

  async function stopAudioPlayback() {
    try {
      console.log("🛑 Stopping audio playback...");
      const result = await invoke("stop_audio_playback") as string;
      console.log("✅ Stop playback result:", result);
      status = result;
      
      // Clear status after a moment
      setTimeout(() => {
        status = "Ready - Double-tap Left Alt to record";
      }, 2000);
      
    } catch (err: any) {
      console.error("❌ Failed to stop audio playback:", err);
      error = `Failed to stop playback: ${err}`;
    }
  }

  function markAsChanged() {
    hasUnsavedChanges = true;
  }

  async function cleanupTestRecording() {
    if (testAudioPath) {
      try {
        await invoke("cleanup_test_recording");
        console.log("Test recording cleaned up:", testAudioPath);
        testAudioPath = "";
      } catch (err) {
        console.error("Failed to clean up test recording:", err);
      }
    }
  }
</script>

<div class="core-settings-page">
  <div class="page-content">
    <!-- Status Section -->
    <section class="settings-section card">
      <h3>Status</h3>
      <div class="status-bar {recordingState.is_recording ? 'recording' : ''}">
        <span class="status-text">{status}</span>
        {#if recordingState.is_recording}
          <div class="recording-indicator">
            <span class="recording-dot"></span>
          </div>
        {/if}
      </div>
      {#if error}
        <div class="error-message">{error}</div>
      {/if}
    </section>

    <!-- Transcription Mode -->
    <section class="settings-section card">
      <h3>Transcription Mode</h3>
      <div class="radio-group">
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="OpenAI" bind:group={settings.transcription_mode} onchange={markAsChanged} />
          <span class="radio-text">OpenAI API (Cloud)</span>
        </label>
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="LocalWhisper" bind:group={settings.transcription_mode} onchange={markAsChanged} />
          <span class="radio-text">Whisper.cpp (Local)</span>
        </label>
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="CandleWhisper" bind:group={settings.transcription_mode} onchange={markAsChanged} />
          <span class="radio-text">🔥 Candle Whisper (Fast Local)</span>
        </label>
      </div>
    </section>

    <!-- API Configuration -->
    {#if settings.transcription_mode === "OpenAI"}
      <section class="settings-section card">
        <h3>OpenAI Configuration</h3>
        <div class="input-group">
          <input
            type="password"
            bind:value={settings.api_key}
            placeholder="Enter your OpenAI API key"
            class="input"
            oninput={markAsChanged}
          />
        </div>
        <p class="help-text">
          Get your API key from <a href="https://platform.openai.com/api-keys" target="_blank">OpenAI Platform</a>
        </p>
      </section>
    {/if}

    <!-- Model Configuration -->
    {#if settings.transcription_mode === "LocalWhisper" || settings.transcription_mode === "CandleWhisper"}
      <section class="settings-section card">
        <h3>Model Configuration</h3>
        <div class="model-settings">
          <div class="setting-group">
            <label for="model-size">Model Size</label>
            <select 
              id="model-size"
              bind:value={settings.whisper_model_size} 
              class="device-select"
              onchange={markAsChanged}
            >
              <option value="Tiny">Tiny (39 MB)</option>
              <option value="Base">Base (74 MB)</option>
              <option value="Small">Small (244 MB)</option>
              <option value="Medium">Medium (769 MB)</option>
              <option value="Large">Large (1550 MB)</option>
              <option value="LargeTurbo">Large Turbo (809 MB)</option>
              <option value="DistilMedium">Distil Medium (394 MB)</option>
              <option value="DistilLargeV2">Distil Large V2 (756 MB)</option>
              <option value="DistilLargeV3">Distil Large V3 (756 MB)</option>
            </select>
          </div>
          
          {#if settings.transcription_mode === "CandleWhisper"}
            <div class="setting-group">
              <label for="device-type">Device Type</label>
              <select 
                id="device-type"
                bind:value={settings.device_type} 
                class="device-select"
                onchange={markAsChanged}
              >
                <option value="Cpu">CPU</option>
                <option value="Cuda">CUDA (NVIDIA)</option>
                <option value="Metal">Metal (Apple Silicon)</option>
                <option value="Rocm">ROCm (AMD)</option>
              </select>
            </div>
          {/if}
          
          {#if settings.whisper_model_path}
            <div class="model-info">
              <span class="info-label">Model Path:</span>
              <span class="info-value">{settings.whisper_model_path}</span>
            </div>
          {/if}
        </div>
      </section>
    {/if}

    <!-- Audio Device -->
    <section class="settings-section card">
      <h3>Audio Device</h3>
      <div class="device-controls">
        <select bind:value={settings.selected_device_id} class="device-select" onchange={markAsChanged}>
          {#each audioDevices as device}
            <option value={device.id}>{device.name}</option>
          {/each}
        </select>
        <button onclick={loadAudioDevices} class="btn btn-secondary">Refresh</button>
      </div>
      {#if recordingState.device_name}
        <p class="device-info">Current: {recordingState.device_name}</p>
      {/if}
      
      <!-- Test Recording Controls -->
      <div class="test-recording-section">
        <h4>Test Recording</h4>
        <p class="help-text">Test the currently selected audio device with a 5-second recording</p>
        <div class="test-controls">
          <button onclick={testRecording} class="btn btn-secondary" disabled={recordingState.is_recording}>
            {isTestRecording ? "Stop Test Recording" : "🎤 Test Recording"}
          </button>
          {#if testAudioPath}
            <button onclick={playTestRecording} class="btn btn-accent">
              🔊 Play Test Recording
            </button>
          {/if}
          <button onclick={stopAudioPlayback} class="btn btn-danger">
            🛑 Stop Audio Playback
          </button>
        </div>
        {#if error && error.includes("test")}
          <p class="error-message">{error}</p>
        {/if}
      </div>
    </section>

    <!-- Voice Activation -->
    <section class="settings-section card">
      <h3>Voice Activation</h3>
      <div class="voice-activation-settings">
        <div class="checkbox-group">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={settings.enable_voice_activation} onchange={markAsChanged} />
            <span class="checkmark"></span>
            Enable voice activation
          </label>
        </div>
        
        {#if settings.enable_voice_activation}
          <div class="voice-settings-expanded">
            <div class="setting-group">
              <label for="listening-device">Listening Device</label>
              <select 
                id="listening-device"
                bind:value={settings.listening_device_id} 
                class="device-select"
                onchange={markAsChanged}
              >
                <option value={null}>Use recording device</option>
                {#each audioDevices as device}
                  <option value={device.id}>{device.name}</option>
                {/each}
              </select>
            </div>
            
            <div class="setting-group">
              <label for="wake-word-sensitivity">Wake Word Sensitivity</label>
              <div class="slider-container">
                <input 
                  type="range" 
                  id="wake-word-sensitivity"
                  bind:value={settings.wake_word_sensitivity}
                  min="0.1" 
                  max="1.0" 
                  step="0.1"
                  class="slider"
                  oninput={markAsChanged}
                />
                <span class="slider-value">{settings.wake_word_sensitivity.toFixed(1)}</span>
              </div>
            </div>
            
            <div class="setting-group">
              <label for="wake-word-timeout">Wake Word Timeout (ms)</label>
              <input 
                type="number" 
                id="wake-word-timeout"
                bind:value={settings.wake_word_timeout_ms}
                min="1000" 
                max="30000" 
                step="1000"
                class="input"
                oninput={markAsChanged}
              />
            </div>
            
            <div class="setting-group">
              <label for="wake-word-model">Wake Word Model</label>
              <select 
                id="wake-word-model"
                bind:value={settings.wake_word_model_size} 
                class="device-select"
                onchange={markAsChanged}
              >
                <option value="Tiny">Tiny</option>
                <option value="Base">Base</option>
                <option value="Small">Small</option>
                <option value="Medium">Medium</option>
              </select>
            </div>
            
            <div class="checkbox-group">
              <label class="checkbox-label">
                <input type="checkbox" bind:checked={settings.auto_calibrate_threshold} onchange={markAsChanged} />
                <span class="checkmark"></span>
                Auto-calibrate voice threshold
              </label>
            </div>
          </div>
        {/if}
      </div>
    </section>

    <!-- Options -->
    <section class="settings-section card">
      <h3>Options</h3>
      <div class="checkbox-group">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={settings.auto_paste} onchange={markAsChanged} />
          <span class="checkmark"></span>
          Automatically paste transcribed text
        </label>
      </div>
    </section>

    <!-- Actions -->
    <section class="actions-section">
      <div class="button-group">
        <button onclick={saveSettings} class="btn btn-primary" disabled={isSaving}>
          {isSaving ? "Saving..." : "Save Settings"}
        </button>
      </div>
    </section>
  </div>
</div>

<style>
  .core-settings-page {
    padding: 24px;
    height: 100%;
  }

  .page-content {
    display: flex;
    flex-direction: column;
    gap: 20px;
    max-width: 100%;
  }

  .settings-section {
    padding: 20px;
  }

  .settings-section h3 {
    margin: 0 0 16px 0;
    font-size: 1.1rem;
    color: var(--text-primary, #ffffff);
    border-bottom: 1px solid var(--border-primary, #404040);
    padding-bottom: 8px;
  }

  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-tertiary, #3a3a3a);
    border-radius: 6px;
    border-left: 3px solid var(--accent-primary, #4A90E2);
  }

  .status-bar.recording {
    border-left-color: var(--error, #F44336);
    background: rgba(244, 67, 54, 0.1);
  }

  .status-text {
    font-size: 0.95rem;
    font-weight: 500;
    color: var(--text-primary, #ffffff);
  }

  .recording-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .recording-dot {
    width: 8px;
    height: 8px;
    background: var(--error, #F44336);
    border-radius: 50%;
    animation: pulse 1s infinite;
  }

  @keyframes pulse {
    0% { opacity: 1; }
    50% { opacity: 0.5; }
    100% { opacity: 1; }
  }

  .error-message {
    margin-top: 12px;
    padding: 12px;
    background: rgba(244, 67, 54, 0.1);
    color: var(--error, #F44336);
    border-radius: 6px;
    border: 1px solid var(--error, #F44336);
    font-size: 0.9rem;
  }

  .radio-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    padding: 8px;
    border-radius: 4px;
    transition: background-color var(--duration-fast, 150ms);
  }

  .radio-label:hover {
    background: var(--hover-bg, #404040);
  }

  .radio-label input[type="radio"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary, #4A90E2);
  }

  .radio-text {
    font-size: 0.95rem;
    color: var(--text-primary, #ffffff);
  }

  .input-group {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .input {
    flex: 1;
    padding: 12px;
    border: 1px solid var(--border-primary, #404040);
    border-radius: 6px;
    background-color: var(--bg-tertiary, #3a3a3a);
    color: var(--text-primary, #ffffff);
    font-size: 0.9rem;
    transition: border-color var(--duration-fast, 150ms);
  }

  .input:focus {
    outline: none;
    border-color: var(--accent-primary, #4A90E2);
    box-shadow: 0 0 0 2px rgba(74, 144, 226, 0.3);
  }

  .input::placeholder {
    color: var(--text-muted, #808080);
  }

  .help-text {
    font-size: 0.85rem;
    color: var(--text-secondary, #b0b0b0);
    margin-top: 8px;
    line-height: 1.4;
  }

  .help-text a {
    color: var(--accent-primary, #4A90E2);
    text-decoration: none;
  }

  .help-text a:hover {
    text-decoration: underline;
  }

  .device-controls {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .device-select {
    flex: 1;
    padding: 12px;
    border: 1px solid var(--border-primary, #404040);
    border-radius: 6px;
    background-color: var(--bg-tertiary, #3a3a3a);
    color: var(--text-primary, #ffffff);
    font-size: 0.9rem;
  }

  .device-info {
    font-size: 0.85rem;
    color: var(--text-secondary, #b0b0b0);
    margin-top: 8px;
  }

  .checkbox-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    padding: 8px;
    border-radius: 4px;
    transition: background-color var(--duration-fast, 150ms);
  }

  .checkbox-label:hover {
    background: var(--hover-bg, #404040);
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary, #4A90E2);
  }

  /* Model Configuration Styles */
  .model-settings {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .setting-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .setting-group label {
    font-size: 0.9rem;
    font-weight: 500;
    color: var(--text-primary, #ffffff);
  }

  .model-info {
    padding: 12px;
    background: var(--bg-tertiary, #3a3a3a);
    border-radius: 6px;
    border: 1px solid var(--border-primary, #404040);
  }

  .info-label {
    font-size: 0.85rem;
    color: var(--text-secondary, #b0b0b0);
  }

  .info-value {
    font-size: 0.85rem;
    color: var(--text-primary, #ffffff);
    font-family: monospace;
  }

  /* Voice Activation Styles */
  .voice-activation-settings {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .voice-settings-expanded {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px;
    background: var(--bg-tertiary, #3a3a3a);
    border-radius: 6px;
    border: 1px solid var(--border-primary, #404040);
  }

  .slider-container {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .slider {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: var(--border-primary, #404040);
    outline: none;
    appearance: none;
  }

  .slider::-webkit-slider-thumb {
    appearance: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent-primary, #4A90E2);
    cursor: pointer;
    border: 2px solid white;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  }

  .slider::-moz-range-thumb {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent-primary, #4A90E2);
    cursor: pointer;
    border: 2px solid white;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  }

  .slider-value {
    font-size: 0.9rem;
    font-weight: 500;
    color: var(--text-primary, #ffffff);
    min-width: 32px;
    text-align: center;
  }

  .actions-section {
    padding-top: 20px;
  }

  .button-group {
    display: flex;
    gap: 12px;
    justify-content: center;
  }

  .btn-danger {
    background: var(--danger-bg, #dc3545);
    color: var(--danger-text, #ffffff);
    border: 1px solid var(--danger-border, #dc3545);
  }

  .btn-danger:hover {
    background: var(--danger-hover, #c82333);
    border-color: var(--danger-hover, #c82333);
    transform: translateY(-1px);
  }

  .btn-danger:active {
    background: var(--danger-active, #bd2130);
    transform: translateY(0);
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    .core-settings-page {
      padding: 16px;
    }

    .button-group {
      flex-direction: column;
    }

    .device-controls {
      flex-direction: column;
      align-items: stretch;
    }
  }

  .test-recording-section {
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid var(--border-secondary, #505050);
  }

  .test-recording-section h4 {
    margin: 0 0 8px 0;
    font-size: 1rem;
    color: var(--text-primary, #ffffff);
    font-weight: 600;
  }

  .test-controls {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
    margin-top: 12px;
  }
</style> 