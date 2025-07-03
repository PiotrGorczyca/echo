<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

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
  }

  type TranscriptionMode = "OpenAI" | "LocalWhisper" | "CandleWhisper";
  type WhisperModelSize = "Tiny" | "Base" | "Small" | "Medium" | "Large" | "LargeTurbo" | "DistilMedium" | "DistilLargeV2" | "DistilLargeV3";
  type DeviceType = "Cpu" | "Cuda" | "Metal";

  interface DownloadProgress {
    model_name: string;
    progress_percent: number;
    downloaded_bytes: number;
    total_bytes: number;
    download_speed_mbps: number;
    eta_seconds: number | null;
    stage: string;
    error_message: string | null;
  }

  interface DownloadEvent {
    event_type: string;
    progress: DownloadProgress | null;
    message: string;
  }

  let audioDevices: AudioDevice[] = $state([]);
  let settings: AppSettings = $state({
    api_key: "",
    selected_device_id: "",
    auto_paste: true,
    transcription_mode: "CandleWhisper" as TranscriptionMode,
    whisper_model_size: "DistilMedium" as WhisperModelSize,
    whisper_model_path: null,
    device_type: "Cpu" as DeviceType
  });
  let recordingState: RecordingState = $state({ is_recording: false, device_name: "" });
  let status: string = $state("Ready");
  let error: string = $state("");
  let isSaving: boolean = $state(false);
  let isDownloading: boolean = $state(false);
  let downloadProgress: DownloadProgress | null = $state(null);
  let downloadError: string = $state("");
  let downloadInitializing: boolean = $state(false);
  let isCandleDownloading: boolean = $state(false);
  let candleDownloadProgress: DownloadProgress | null = $state(null);
  let candleDownloadError: string = $state("");
  let candleDownloadInitializing: boolean = $state(false);
  let isCandlePreloading: boolean = $state(false);
  
  // Model availability state
  let isLocalModelAvailable: boolean = $state(false);
  let isCandleModelAvailable: boolean = $state(false);

  onMount(async () => {
    await loadAudioDevices();
    await loadSettings();
    await updateRecordingState();
    
    // Check initial model status
    await checkInitialModelStatus();
    
    // Setup download event listeners
    setupDownloadEventListeners();
    
    // Update status every second
    setInterval(updateRecordingState, 1000);
  });

  async function checkInitialModelStatus() {
    try {
      // Check both model types to initialize state
      await checkModelStatus();
      await checkCandleModelStatus();
    } catch (err) {
      console.error("Failed to check initial model status:", err);
    }
  }

  // Reactive effect to check model status when transcription mode or model size changes
  $effect(() => {
    // Watch for changes in transcription mode or model size
    if (settings.transcription_mode && settings.whisper_model_size) {
      checkModelStatusForCurrentMode();
    }
  });

  async function checkModelStatusForCurrentMode() {
    try {
      if (settings.transcription_mode === "LocalWhisper") {
        await checkModelStatus();
      } else if (settings.transcription_mode === "CandleWhisper") {
        await checkCandleModelStatus();
      }
      // OpenAI doesn't need model checking
    } catch (err) {
      console.error("Failed to check model status for current mode:", err);
    }
  }

  async function setupDownloadEventListeners() {
    const { listen } = await import("@tauri-apps/api/event");
    
    await listen<DownloadEvent>("download-event", (event) => {
      const downloadEvent = event.payload;
      console.log("Download event received:", downloadEvent);
      
      // Update the appropriate download state based on the model being downloaded
      if (downloadEvent.progress) {
        const isCandle = settings.transcription_mode === "CandleWhisper";
        
        if (isCandle) {
          // Clear initializing state once we get first progress update
          candleDownloadInitializing = false;
          candleDownloadProgress = downloadEvent.progress;
          if (downloadEvent.event_type === "error") {
            candleDownloadError = downloadEvent.progress.error_message || "Download failed";
          } else {
            candleDownloadError = "";
          }
        } else {
          // Clear initializing state once we get first progress update
          downloadInitializing = false;
          downloadProgress = downloadEvent.progress;
          if (downloadEvent.event_type === "error") {
            downloadError = downloadEvent.progress.error_message || "Download failed";
          } else {
            downloadError = "";
          }
        }
      }
      
      // Update status message
      status = downloadEvent.message;
      
      // Handle completion
      if (downloadEvent.event_type === "complete") {
        // Update model availability state
        if (settings.transcription_mode === "CandleWhisper") {
          isCandleModelAvailable = true;
          isCandleDownloading = false;
          candleDownloadInitializing = false;
          candleDownloadProgress = null;
        } else if (settings.transcription_mode === "LocalWhisper") {
          isLocalModelAvailable = true;
          isDownloading = false;
          downloadInitializing = false;
          downloadProgress = null;
        }
        
        setTimeout(() => {
          if (!recordingState.is_recording) {
            status = "Ready - Double-tap Left Alt to record";
          }
        }, 3000);
      }
    });
  }

  async function loadAudioDevices() {
    try {
      audioDevices = await invoke<AudioDevice[]>("get_audio_devices");
      if (audioDevices.length > 0 && !settings.selected_device_id) {
        settings.selected_device_id = audioDevices[0].id;
      }
    } catch (err) {
      error = `Failed to load audio devices: ${err}`;
      console.error(error);
    }
  }

  async function loadSettings() {
    try {
      settings = await invoke<AppSettings>("get_settings");
      // Initialize transcription service after loading settings
      await initializeTranscriptionService();
    } catch (err) {
      console.error("Failed to load settings:", err);
    }
  }

  async function initializeTranscriptionService() {
    try {
      console.log("Initializing transcription service with settings:", settings);
      const result = await invoke<string>("initialize_transcription_service", { settings });
      console.log("Transcription service initialized:", result);
    } catch (err) {
      console.error("Failed to initialize transcription service:", err);
      // Don't show this as an error to the user since it's not critical for UI
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
      // Reinitialize transcription service with new settings
      await initializeTranscriptionService();
      status = "Settings saved successfully!";
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
      const isNowRecording = await invoke<boolean>("toggle_recording");
      // Update the local state immediately to prevent double-clicks
      recordingState.is_recording = isNowRecording;
      // Then refresh from backend to ensure consistency
      setTimeout(updateRecordingState, 100);
    } catch (err) {
      error = `Failed to toggle recording: ${err}`;
      // On error, refresh the state to ensure UI is correct
      await updateRecordingState();
    }
  }

  async function hideWindow() {
    try {
      await invoke("hide_window");
    } catch (err) {
      console.error("Failed to hide window:", err);
    }
  }

  function clearError() {
    error = "";
  }

  async function checkModelStatus(): Promise<boolean> {
    try {
      const isDownloaded = await invoke<boolean>("check_whisper_model", { 
        modelSize: settings.whisper_model_size 
      });
      isLocalModelAvailable = isDownloaded;
      return isDownloaded;
    } catch (err) {
      console.error("Failed to check model status:", err);
      isLocalModelAvailable = false;
      throw err;
    }
  }

  async function downloadModel() {
    try {
      // Set initializing state immediately
      downloadInitializing = true;
      isDownloading = true;
      downloadProgress = null;
      downloadError = "";
      error = "";
      
      console.log("Starting download for model:", settings.whisper_model_size);
      status = `Initializing download for ${settings.whisper_model_size} model...`;
      
      const modelPath = await invoke<string>("download_whisper_model", { 
        modelSize: settings.whisper_model_size 
      });
      
      console.log("Download complete, model path:", modelPath);
      
      // Update settings with the model path
      settings.whisper_model_path = modelPath;
      await saveSettings();
      
      status = "Model downloaded successfully!";
      setTimeout(() => {
        if (!recordingState.is_recording) {
          status = "Ready - Double-tap Left Alt to record";
        }
      }, 2000);
    } catch (err) {
      console.error("Download failed:", err);
      downloadError = `Failed to download model: ${err}`;
      error = `Failed to download model: ${err}`;
      status = "Download failed";
    } finally {
      isDownloading = false;
      downloadInitializing = false;
      downloadProgress = null;
    }
  }

  async function checkCandleModelStatus(): Promise<boolean> {
    try {
      const isDownloaded = await invoke<boolean>("check_candle_model", { 
        modelSize: settings.whisper_model_size 
      });
      isCandleModelAvailable = isDownloaded;
      return isDownloaded;
    } catch (err) {
      console.error("Failed to check Candle model status:", err);
      isCandleModelAvailable = false;
      throw err;
    }
  }

  async function downloadCandleModel() {
    try {
      // Set initializing state immediately
      candleDownloadInitializing = true;
      isCandleDownloading = true;
      candleDownloadProgress = null;
      candleDownloadError = "";
      error = "";
      
      console.log("Starting Candle model download for:", settings.whisper_model_size);
      status = `Initializing download for ${settings.whisper_model_size} model...`;
      
      const result = await invoke<string>("download_candle_model", { 
        modelSize: settings.whisper_model_size 
      });
      
      console.log("Candle model download complete:", result);
      
      status = "Model downloaded successfully!";
      setTimeout(() => {
        if (!recordingState.is_recording) {
          status = "Ready - Double-tap Left Alt to record";
        }
      }, 2000);
    } catch (err) {
      console.error("Candle model download failed:", err);
      candleDownloadError = `Failed to download model: ${err}`;
      error = `Failed to download model: ${err}`;
      status = "Download failed";
    } finally {
      isCandleDownloading = false;
      candleDownloadInitializing = false;
      candleDownloadProgress = null;
    }
  }

  async function preloadCandleModel() {
    try {
      isCandlePreloading = true;
      error = "";
      
      console.log("Starting Candle model preload for:", settings.whisper_model_size, "on device:", settings.device_type);
      status = `Preloading ${settings.whisper_model_size} model...`;
      
      const result = await invoke<string>("preload_candle_model", { 
        modelSize: settings.whisper_model_size,
        deviceType: settings.device_type
      });
      
      console.log("Candle model preload complete:", result);
      
      status = "Model preloaded and ready for fast transcription!";
      setTimeout(() => {
        if (!recordingState.is_recording) {
          status = "Ready - Double-tap Left Alt to record";
        }
      }, 3000);
    } catch (err) {
      console.error("Candle model preload failed:", err);
      error = `Failed to preload model: ${err}`;
      status = "Preload failed";
    } finally {
      isCandlePreloading = false;
    }
  }
</script>

<main class="container">
  <header>
    <h1>🎙️ EchoType Settings</h1>
    <p>Background voice-to-text service</p>
    <button onclick={hideWindow} class="hide-btn">Minimize to Tray</button>
  </header>

  {#if error}
    <div class="error-banner">
      <span>{error}</span>
      <button onclick={clearError} class="close-btn">×</button>
    </div>
  {/if}

  <div class="content">
    <!-- Status Section -->
    <section class="status-section">
      <div class="status-bar {recordingState.is_recording ? 'recording' : ''}">
        <span class="status-text">{status}</span>
        {#if recordingState.is_recording}
          <div class="recording-indicator">
            <span class="recording-dot"></span>
          </div>
        {/if}
      </div>
      <div class="instructions">
        <h3>How to use:</h3>
        <ol>
          <li><strong>Double-tap Left Alt</strong> to start/stop recording</li>
          <li>Speak clearly into your microphone</li>
          <li>Text will be automatically transcribed and pasted</li>
        </ol>
      </div>
    </section>

    <!-- Transcription Mode Selection -->
    <section class="settings-section">
      <h2>Transcription Mode</h2>
      <div class="radio-group">
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="OpenAI" bind:group={settings.transcription_mode} />
          <span class="radio-text">OpenAI API (Cloud)</span>
        </label>
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="LocalWhisper" bind:group={settings.transcription_mode} />
          <span class="radio-text">Whisper.cpp (Local)</span>
        </label>
        <label class="radio-label">
          <input type="radio" name="transcription_mode" value="CandleWhisper" bind:group={settings.transcription_mode} />
          <span class="radio-text">🔥 Candle Whisper (Fast Local)</span>
        </label>
      </div>
      <p class="help-text">
        Choose between cloud OpenAI API, traditional local Whisper, or fast Candle-based inference with turbo models
      </p>
    </section>

    <!-- API Key Section -->
    {#if settings.transcription_mode === "OpenAI"}
      <section class="settings-section">
        <h2>OpenAI Configuration</h2>
        <div class="input-group">
          <input
            type="password"
            bind:value={settings.api_key}
            placeholder="Enter your OpenAI API key"
            class="api-key-input"
          />
        </div>
        <p class="help-text">
          Get your API key from <a href="https://platform.openai.com/api-keys" target="_blank">OpenAI Platform</a>
        </p>
      </section>
    {/if}

    <!-- Local Whisper Configuration -->
    {#if settings.transcription_mode === "LocalWhisper"}
      <section class="settings-section">
        <h2>Whisper.cpp Model Configuration</h2>
        <div class="model-select-group">
          <label for="model-size">Model Size:</label>
          <select id="model-size" bind:value={settings.whisper_model_size} class="model-select">
            <option value="Tiny">Tiny (39 MB - Fastest, multilingual)</option>
            <option value="Base">Base (74 MB - Fast, multilingual)</option>
            <option value="Small">Small (244 MB - Balanced, multilingual)</option>
            <option value="Medium">Medium (769 MB - Good quality, multilingual)</option>
            <option value="Large">Large v3 (1550 MB - Best quality, multilingual)</option>
          </select>
        </div>
        {#if isLocalModelAvailable}
          <p class="model-status success">✅ Model downloaded and ready</p>
        {:else}
            <p class="model-status warning">⚠️ Model not downloaded</p>
            <div class="download-section">
              <button onclick={downloadModel} class="download-btn" disabled={isDownloading}>
                {downloadInitializing ? "Initializing..." : isDownloading ? "Downloading..." : "Download Model"}
              </button>
              
              {#if downloadInitializing}
                <div class="progress-container">
                  <div class="progress-bar">
                    <div class="progress-fill initializing" style="width: 100%"></div>
                  </div>
                  <div class="progress-info">
                    <span class="progress-text">Initializing download...</span>
                  </div>
                  <div class="stage-text">Preparing download request</div>
                </div>
              {:else if isDownloading && downloadProgress}
                <div class="progress-container">
                  <div class="progress-bar">
                    <div class="progress-fill" style="width: {downloadProgress.progress_percent}%"></div>
                  </div>
                  <div class="progress-info">
                    <span class="progress-text">
                      {downloadProgress.progress_percent.toFixed(1)}% 
                      {#if downloadProgress.total_bytes > 0}
                        ({(downloadProgress.downloaded_bytes / 1024 / 1024).toFixed(1)} MB / {(downloadProgress.total_bytes / 1024 / 1024).toFixed(1)} MB)
                      {/if}
                    </span>
                    {#if downloadProgress.download_speed_mbps > 0}
                      <span class="speed-text">{downloadProgress.download_speed_mbps.toFixed(1)} MB/s</span>
                    {/if}
                    {#if downloadProgress.eta_seconds}
                      <span class="eta-text">ETA: {Math.floor(downloadProgress.eta_seconds / 60)}m {downloadProgress.eta_seconds % 60}s</span>
                    {/if}
                  </div>
                  <div class="stage-text">{downloadProgress.stage.replace('_', ' ')}</div>
                </div>
              {/if}
              
              {#if downloadError}
                <div class="error-message">{downloadError}</div>
              {/if}
            </div>
          {/if}
        <p class="help-text">
          Traditional Whisper.cpp backend. Models need to be downloaded and cached locally.
        </p>
      </section>
    {/if}

    <!-- Candle Whisper Configuration -->
    {#if settings.transcription_mode === "CandleWhisper"}
      <section class="settings-section">
        <h2>🔥 Candle Whisper Configuration</h2>
        <div class="model-select-group">
          <label for="candle-model-size">Model Size:</label>
          <select id="candle-model-size" bind:value={settings.whisper_model_size} class="model-select">
            <optgroup label="Standard Whisper Models (Multilingual, Auto-detect)">
              <option value="Tiny">Tiny (~39 MB - Fastest, multilingual)</option>
              <option value="Base">Base (~74 MB - Fast, multilingual)</option>  
              <option value="Small">Small (~244 MB - Balanced, multilingual)</option>
              <option value="Medium">Medium (~769 MB - Good quality, multilingual)</option>
              <option value="Large">Large v3 (~1550 MB - Best quality, multilingual)</option>
              <option value="LargeTurbo">Large v3 Turbo (~1550 MB - 8x faster, multilingual!)</option>
            </optgroup>
            <optgroup label="Distil-Whisper Models (Fast and Accurate)">
              <option value="DistilMedium">Distil Medium.en (~394 MB - 6x faster, English only)</option>
              <option value="DistilLargeV2">Distil Large v2 (~756 MB - 6x faster, multilingual, auto-detect)</option>
              <option value="DistilLargeV3">Distil Large v3 (~756 MB - 6x faster, multilingual, auto-detect)</option>
            </optgroup>
          </select>
        </div>
        
        <div class="device-type-group">
          <label for="device-type">Compute Device:</label>
          <select id="device-type" bind:value={settings.device_type} class="device-select">
            <option value="Cpu">CPU (Universal)</option>
            <option value="Cuda">CUDA GPU (NVIDIA)</option>
            <option value="Metal">Metal GPU (Apple)</option>
          </select>
        </div>
        
        {#if isCandleModelAvailable}
          <p class="model-status success">✅ Model downloaded and ready</p>
          <div class="model-actions">
            <button onclick={preloadCandleModel} class="preload-btn" disabled={isCandlePreloading}>
              {isCandlePreloading ? "Preloading..." : "🚀 Preload Model"}
            </button>
            <p class="help-text">Preload the model for faster transcription (recommended)</p>
          </div>
        {:else}
            <p class="model-status warning">⚠️ Model not downloaded</p>
            <div class="download-section">
              <button onclick={downloadCandleModel} class="download-btn" disabled={isCandleDownloading}>
                {candleDownloadInitializing ? "Initializing..." : isCandleDownloading ? "Downloading..." : "Download Model"}
              </button>
              
              {#if candleDownloadInitializing}
                <div class="progress-container">
                  <div class="progress-bar">
                    <div class="progress-fill initializing" style="width: 100%"></div>
                  </div>
                  <div class="progress-info">
                    <span class="progress-text">Initializing download...</span>
                  </div>
                  <div class="stage-text">Setting up Python environment</div>
                </div>
              {:else if isCandleDownloading && candleDownloadProgress}
                <div class="progress-container">
                  <div class="progress-bar">
                    <div class="progress-fill" style="width: {candleDownloadProgress.progress_percent}%"></div>
                  </div>
                  <div class="progress-info">
                    <span class="progress-text">
                      {candleDownloadProgress.progress_percent.toFixed(1)}%
                      {#if candleDownloadProgress.total_bytes > 0}
                        ({(candleDownloadProgress.downloaded_bytes / 1024 / 1024).toFixed(1)} MB / {(candleDownloadProgress.total_bytes / 1024 / 1024).toFixed(1)} MB)
                      {/if}
                    </span>
                    {#if candleDownloadProgress.download_speed_mbps > 0}
                      <span class="speed-text">{candleDownloadProgress.download_speed_mbps.toFixed(1)} MB/s</span>
                    {/if}
                    {#if candleDownloadProgress.eta_seconds}
                      <span class="eta-text">ETA: {Math.floor(candleDownloadProgress.eta_seconds / 60)}m {candleDownloadProgress.eta_seconds % 60}s</span>
                    {/if}
                  </div>
                  <div class="stage-text">{candleDownloadProgress.stage.replace('_', ' ')}</div>
                </div>
              {/if}
              
              {#if candleDownloadError}
                <div class="error-message">{candleDownloadError}</div>
              {/if}
            </div>
          {/if}
        
                  <div class="help-box">
          <h4>🚀 Performance Tips:</h4>
          <ul>
            <li><strong>Turbo and Distil models</strong> are 6-8x faster than standard models</li>
            <li><strong>GPU acceleration</strong> provides significant speedup when available</li>
            <li><strong>Distil models</strong> maintain high accuracy while being much faster</li>
            <li><strong>LargeTurbo</strong> offers the best balance of speed and quality</li>
          </ul>
          <h4>🌐 Language Support:</h4>
          <ul>
            <li><strong>Most models support 99+ languages</strong> with automatic detection</li>
            <li><strong>Distil Medium.en</strong> is English-only but fastest</li>
            <li><strong>Polish, German, Spanish, French, etc.</strong> all supported automatically</li>
          </ul>
          <p><strong>Note:</strong> Requires Python with transformers and torch packages installed</p>
        </div>
      </section>
    {/if}

    <!-- Device Selection -->
    <section class="settings-section">
      <h2>Audio Device</h2>
      <div class="device-controls">
        <select bind:value={settings.selected_device_id} class="device-select">
          {#each audioDevices as device}
            <option value={device.id}>{device.name}</option>
          {/each}
        </select>
        <button onclick={loadAudioDevices} class="refresh-btn">Refresh</button>
      </div>
      {#if recordingState.device_name && recordingState.device_name !== "No Device Selected"}
        <p class="device-info">Current: {recordingState.device_name}</p>
      {/if}
    </section>

    <!-- Options -->
    <section class="settings-section">
      <h2>Options</h2>
      <div class="checkbox-group">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={settings.auto_paste} />
          <span class="checkmark"></span>
          Automatically paste transcribed text
        </label>
      </div>
    </section>

    <!-- Actions -->
    <section class="actions-section">
      <div class="button-group">
        <button onclick={saveSettings} class="save-btn" disabled={isSaving}>
          {isSaving ? "Saving..." : "Save Settings"}
        </button>
        <button onclick={testRecording} class="test-btn">
          {recordingState.is_recording ? "Stop Test Recording" : "Test Recording"}
        </button>
      </div>
    </section>
  </div>
</main>

<style>
  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
    margin: 0;
    padding: 0;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    min-height: 100vh;
    color: #333;
  }

  .container {
    max-width: 600px;
    margin: 0 auto;
    padding: 20px;
    min-height: 100vh;
  }

  header {
    text-align: center;
    margin-bottom: 30px;
    color: white;
    position: relative;
  }

  header h1 {
    font-size: 2.5rem;
    margin: 0;
    text-shadow: 2px 2px 4px rgba(0,0,0,0.3);
  }

  header p {
    font-size: 1.1rem;
    margin: 10px 0;
    opacity: 0.9;
  }

  .hide-btn {
    position: absolute;
    top: 0;
    right: 0;
    background: rgba(255,255,255,0.2);
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.9rem;
    transition: background-color 0.3s ease;
  }

  .hide-btn:hover {
    background: rgba(255,255,255,0.3);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 25px;
  }

  section {
    background: white;
    border-radius: 12px;
    padding: 20px;
    box-shadow: 0 4px 20px rgba(0,0,0,0.1);
  }

  h2 {
    margin: 0 0 15px 0;
    font-size: 1.3rem;
    color: #333;
    border-bottom: 2px solid #f0f0f0;
    padding-bottom: 8px;
  }

  .error-banner {
    background: #fee;
    border: 1px solid #fcc;
    border-radius: 8px;
    padding: 15px;
    margin-bottom: 20px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #c33;
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: #c33;
    padding: 0;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 15px;
    background: #f8f9fa;
    border-radius: 8px;
    border-left: 4px solid #667eea;
  }

  .status-bar.recording {
    background: #fff3f3;
    border-left-color: #f44336;
  }

  .status-text {
    font-size: 1.1rem;
    font-weight: 500;
  }

  .recording-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .recording-dot {
    width: 12px;
    height: 12px;
    background: #f44336;
    border-radius: 50%;
    animation: pulse 1s infinite;
  }

  @keyframes pulse {
    0% { opacity: 1; }
    50% { opacity: 0.5; }
    100% { opacity: 1; }
  }

  .instructions {
    margin-top: 15px;
  }

  .instructions h3 {
    margin: 0 0 10px 0;
    font-size: 1.1rem;
    color: #555;
  }

  .instructions ol {
    margin: 0;
    padding-left: 20px;
    line-height: 1.6;
  }

  .instructions li {
    margin-bottom: 5px;
  }

  .input-group {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .api-key-input {
    flex: 1;
    padding: 12px;
    border: 2px solid #ddd;
    border-radius: 8px;
    font-size: 1rem;
    transition: border-color 0.3s ease;
  }

  .api-key-input:focus {
    outline: none;
    border-color: #667eea;
  }

  .help-text {
    font-size: 0.9rem;
    color: #666;
    margin-top: 8px;
  }

  .help-text a {
    color: #667eea;
    text-decoration: none;
  }

  .device-controls {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .device-select {
    flex: 1;
    padding: 12px;
    border: 2px solid #ddd;
    border-radius: 8px;
    font-size: 1rem;
    background: white;
  }

  .refresh-btn {
    background: #667eea;
    color: white;
    border: none;
    padding: 12px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    transition: background-color 0.3s ease;
  }

  .refresh-btn:hover {
    background: #5a6fd8;
  }

  .device-info {
    font-size: 0.9rem;
    color: #666;
    margin-top: 8px;
  }

  .checkbox-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    font-size: 1rem;
  }

  .checkbox-label input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: #667eea;
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
    font-size: 1rem;
  }

  .radio-label input[type="radio"] {
    width: 18px;
    height: 18px;
    accent-color: #667eea;
  }

  .radio-text {
    font-size: 1rem;
  }

  .model-select-group {
    display: flex;
    align-items: center;
    gap: 15px;
    margin-bottom: 15px;
  }

  .model-select-group label {
    font-weight: 500;
  }

  .model-select {
    flex: 1;
    padding: 10px;
    border: 2px solid #ddd;
    border-radius: 8px;
    font-size: 1rem;
    background: white;
  }

  .device-type-group {
    display: flex;
    align-items: center;
    gap: 15px;
    margin-bottom: 15px;
  }

  .device-type-group label {
    font-weight: 500;
  }

  .help-box {
    background: #f8f9fa;
    border: 1px solid #e9ecef;
    border-radius: 8px;
    padding: 15px;
    margin-top: 15px;
  }

  .help-box h4 {
    margin: 0 0 10px 0;
    color: #495057;
    font-size: 1rem;
  }

  .help-box ul {
    margin: 0;
    padding-left: 20px;
  }

  .help-box li {
    margin-bottom: 5px;
    font-size: 0.9rem;
    color: #6c757d;
  }

  .model-status {
    font-size: 0.95rem;
    margin: 10px 0;
    padding: 10px;
    border-radius: 6px;
  }

  .model-status.success {
    background: #e8f5e9;
    color: #2e7d32;
  }

  .model-status.warning {
    background: #fff3e0;
    color: #f57c00;
  }

  .model-status.error {
    background: #ffebee;
    color: #c62828;
  }

  .model-status.info {
    background: #e3f2fd;
    color: #1565c0;
  }

  .download-btn {
    background: #4caf50;
    color: white;
    border: none;
    padding: 10px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    transition: background-color 0.3s ease;
    margin-top: 10px;
  }

  .download-btn:hover:not(:disabled) {
    background: #45a049;
  }

  .download-btn:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .button-group {
    display: flex;
    gap: 15px;
    justify-content: center;
  }

  .save-btn, .test-btn {
    padding: 12px 24px;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    font-weight: 500;
    transition: all 0.3s ease;
  }

  .save-btn {
    background: #28a745;
    color: white;
  }

  .save-btn:hover:not(:disabled) {
    background: #218838;
  }

  .save-btn:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .test-btn {
    background: #667eea;
    color: white;
  }

  .test-btn:hover {
    background: #5a6fd8;
  }

  .download-section {
    margin-top: 10px;
  }

  .progress-container {
    margin-top: 15px;
    padding: 10px;
    background: #f8f9fa;
    border-radius: 8px;
    border: 1px solid #e9ecef;
  }

  .progress-bar {
    width: 100%;
    height: 8px;
    background: #e9ecef;
    border-radius: 4px;
    overflow: hidden;
    margin-bottom: 8px;
  }

  .progress-fill {
    height: 100%;
    background: linear-gradient(90deg, #4caf50, #45a049);
    transition: width 0.3s ease;
    animation: progress-shimmer 1.5s infinite;
  }

  .progress-fill.initializing {
    background: linear-gradient(45deg, #667eea, #764ba2, #667eea, #764ba2);
    background-size: 400% 400%;
    animation: initializing-pulse 2s ease-in-out infinite;
  }

  @keyframes progress-shimmer {
    0% {
      background-position: -100% 0;
    }
    100% {
      background-position: 100% 0;
    }
  }

  @keyframes initializing-pulse {
    0% {
      background-position: 0% 50%;
      opacity: 0.8;
    }
    50% {
      background-position: 100% 50%;
      opacity: 1;
    }
    100% {
      background-position: 0% 50%;
      opacity: 0.8;
    }
  }

  .progress-info {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 0.875rem;
    color: #495057;
  }

  .progress-text {
    font-weight: 500;
  }

  .speed-text {
    color: #28a745;
    font-weight: 500;
  }

  .eta-text {
    color: #6c757d;
  }

  .stage-text {
    margin-top: 5px;
    font-size: 0.8rem;
    color: #6c757d;
    font-style: italic;
    text-transform: capitalize;
  }

  .error-message {
    margin-top: 10px;
    padding: 10px;
    background: #ffebee;
    color: #c62828;
    border-radius: 6px;
    border: 1px solid #ffcdd2;
    font-size: 0.9rem;
  }

  .preload-btn {
    background: #ff9800;
    color: white;
    border: none;
    padding: 10px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 1rem;
    transition: background-color 0.3s ease;
    margin-top: 10px;
  }

  .preload-btn:hover:not(:disabled) {
    background: #f57c00;
  }

  .preload-btn:disabled {
    background: #ccc;
    cursor: not-allowed;
  }

  .model-actions {
    margin-top: 10px;
  }

  @media (max-width: 600px) {
    .container {
      padding: 15px;
    }

    header h1 {
      font-size: 2rem;
    }

    .button-group {
      flex-direction: column;
    }

    .device-controls {
      flex-direction: column;
    }

    .device-select {
      width: 100%;
    }
  }
</style>
