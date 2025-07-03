// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod transcription;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::fs;
use std::path::PathBuf;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use hound::{WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use enigo::{Enigo, Settings, Keyboard};
use anyhow::Result;
use device_query::{DeviceQuery, DeviceState, Keycode};
use notify_rust::Notification;
use tauri::{
    AppHandle, Manager, State, Window, generate_handler,
    menu::{MenuBuilder, MenuItem, MenuEvent},
    tray::{TrayIconBuilder}, Emitter,
};
use mouse_position::mouse_position::{Mouse};
use transcription::{TranscriptionMode, TranscriptionConfig, TranscriptionService, WhisperModelSize, DeviceType};

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordingState {
    pub is_recording: bool,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub api_key: String,
    pub selected_device_id: String,
    pub auto_paste: bool,
    pub transcription_mode: TranscriptionMode,
    pub whisper_model_size: WhisperModelSize,
    pub whisper_model_path: Option<String>,
    pub device_type: DeviceType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhisperResponse {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatusUpdate {
    pub message: String,
    pub r#type: String, // "recording", "transcribing", "success", "error", "idle"
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadProgress {
    pub model_name: String,
    pub progress_percent: f64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub download_speed_mbps: f64,
    pub eta_seconds: Option<u64>,
    pub stage: String, // "downloading", "verifying", "complete", "error"
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadEvent {
    pub event_type: String, // "progress", "error", "complete", "started"
    pub progress: Option<DownloadProgress>,
    pub message: String,
}

// Use a simpler state that doesn't store the Stream directly
pub struct AppState {
    pub is_recording: bool,
    pub current_device_name: String,
    pub temp_file_path: Option<String>,
    pub settings: AppSettings,
    pub last_alt_press: Option<Instant>,
    pub alt_tap_count: u32,
    pub recording_start_time: Option<Instant>,
    pub transcription_service: Option<Arc<Mutex<TranscriptionService>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            is_recording: false,
            current_device_name: "No Device Selected".to_string(),
            temp_file_path: None,
            settings: AppSettings {
                api_key: String::new(),
                selected_device_id: String::new(),
                auto_paste: true,
                transcription_mode: TranscriptionMode::OpenAI,
                whisper_model_size: WhisperModelSize::Small,
                whisper_model_path: None,
                device_type: DeviceType::default(),
            },
            last_alt_press: None,
            alt_tap_count: 0,
            recording_start_time: None,
            transcription_service: None,
        }
    }
}

fn get_settings_file_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or("Could not find config directory")?;
    let app_config_dir = config_dir.join("echotype");
    
    // Create directory if it doesn't exist
    if !app_config_dir.exists() {
        fs::create_dir_all(&app_config_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    
    Ok(app_config_dir.join("settings.json"))
}

fn load_settings_from_file() -> AppSettings {
    match get_settings_file_path() {
        Ok(path) => {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<AppSettings>(&content) {
                            Ok(settings) => {
                                println!("Loaded settings from: {}", path.display());
                                return settings;
                            }
                            Err(e) => println!("Failed to parse settings file: {}", e),
                        }
                    }
                    Err(e) => println!("Failed to read settings file: {}", e),
                }
            }
        }
        Err(e) => println!("Failed to get settings file path: {}", e),
    }
    
    println!("Using default settings");
    AppSettings {
        api_key: String::new(),
        selected_device_id: String::new(),
        auto_paste: true,
        transcription_mode: TranscriptionMode::OpenAI,
        whisper_model_size: WhisperModelSize::Base,
        whisper_model_path: None,
        device_type: DeviceType::default(),
    }
}

fn save_settings_to_file(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_file_path()?;
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;
    
    println!("Settings saved to: {}", path.display());
    Ok(())
}

// Global state for the audio stream (not shared with Tauri)
static mut RECORDING_STREAM: Option<Stream> = None;
static mut WAV_WRITER: Option<Arc<Mutex<WavWriter<std::io::BufWriter<std::fs::File>>>>> = None;
static mut TEMP_FILE: Option<NamedTempFile> = None;

// Overlay management functions
fn get_cursor_position() -> Result<(i32, i32), String> {
    match Mouse::get_mouse_position() {
        Mouse::Position { x, y } => Ok((x, y)),
        Mouse::Error => Err("Failed to get mouse position".to_string()),
    }
}

fn show_overlay_status(app_handle: &AppHandle, message: &str, status_type: &str) {
    println!("Attempting to show overlay status: {} - {}", status_type, message);
    
    // Get overlay window
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        println!("Found overlay window, attempting to show");
        
        // Show the window
        if let Err(e) = overlay_window.show() {
            eprintln!("Failed to show overlay window: {}", e);
        } else {
            println!("Overlay window shown successfully");
        }
        
        // Send status update to overlay window
        let status_update = StatusUpdate {
            message: message.to_string(),
            r#type: status_type.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        };
        
        println!("Emitting status update: {:?}", status_update);
        if let Err(e) = overlay_window.emit("status-update", status_update) {
            eprintln!("Failed to emit status update: {}", e);
        } else {
            println!("Status update emitted successfully");
        }
    } else {
        eprintln!("Overlay window not found - using fallback notification");
        // Fallback to regular notification
        show_notification("EchoType", message, None);
    }
}

fn hide_overlay_status(app_handle: &AppHandle) {
    println!("Attempting to hide overlay status");
    
    // Get overlay window
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        println!("Found overlay window, attempting to hide");
        
        // Send idle status to trigger frontend hide logic
        let status_update = StatusUpdate {
            message: "".to_string(),
            r#type: "idle".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        };
        
        println!("Emitting idle status to trigger hide: {:?}", status_update);
        if let Err(e) = overlay_window.emit("status-update", status_update) {
            eprintln!("Failed to emit idle status: {}", e);
        } else {
            println!("Idle status emitted successfully - overlay should hide");
        }
        
        // Also directly hide the window as a fallback
        if let Err(e) = overlay_window.hide() {
            eprintln!("Failed to hide overlay window directly: {}", e);
        } else {
            println!("Overlay window hidden directly as fallback");
        }
    } else {
        eprintln!("Overlay window not found when trying to hide");
    }
}

// Keep notification functions as fallback
fn show_notification(title: &str, body: &str, icon: Option<&str>) {
    let mut notification = Notification::new();
    notification.summary(title).body(body).timeout(5000);
    
    if let Some(icon_name) = icon {
        notification.icon(icon_name);
    }
    
    if let Err(e) = notification.show() {
        eprintln!("Failed to show notification: {}", e);
    }
}

fn show_recording_started(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "🎤 Recording started...", "recording");
}

fn show_recording_stopped(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "⏹️ Recording stopped, transcribing...", "transcribing");
}

fn show_transcription_success(app_handle: &AppHandle, text: &str) {
    let preview = if text.chars().count() > 50 {
        let truncated: String = text.chars().take(47).collect();
        format!("✅ Text: {}...", truncated)
    } else {
        format!("✅ Text: {}", text)
    };
    
    show_overlay_status(app_handle, &preview, "success");
}

fn show_transcription_error(app_handle: &AppHandle, error: &str) {
    show_overlay_status(app_handle, &format!("❌ {}", error), "error");
}

// Handle tray menu events
fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "show" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "start_recording" => {
            let state = app.state::<Arc<Mutex<AppState>>>();
            let _ = start_recording_hotkey(state, Some(app.clone()));
        }
        "stop_recording" => {
            let state = app.state::<Arc<Mutex<AppState>>>();
            let _ = stop_recording_hotkey(state, Some(app.clone()));
        }
        "force_stop" => {
            let state = app.state::<Arc<Mutex<AppState>>>();
            let _ = force_stop_recording(state);
        }
        "quit" => {
            // Clean up before quitting
            let state = app.state::<Arc<Mutex<AppState>>>();
            let _ = force_stop_recording(state);
            std::process::exit(0);
        }
        _ => {}
    }
}

// Alt key detection and recording logic
fn start_recording_hotkey(state: State<Arc<Mutex<AppState>>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    if app_state.is_recording {
        return Ok(()); // Already recording
    }
    
    let device_id = app_state.settings.selected_device_id.clone();
    if device_id.is_empty() {
        println!("No device selected, attempting to use default device");
        // Try to use the first available device as default
        let host = cpal::default_host();
        let input_devices: Vec<_> = host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?
            .collect();
        
        if input_devices.is_empty() {
            return Err("No audio input devices found".to_string());
        }
        
        // Use the first device and update settings
        app_state.settings.selected_device_id = "0".to_string();
        println!("Auto-selected first available device");
    }
    
    // Start recording logic (similar to before but simplified)
    let host = cpal::default_host();
    let input_devices: Vec<_> = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?
        .collect();
    
    let device_index: usize = device_id.parse()
        .map_err(|_| "Invalid device ID".to_string())?;
    
    let device = input_devices.get(device_index)
        .ok_or("Device not found".to_string())?;
    
    let device_name = device.name().unwrap_or_else(|_| "Unknown Device".to_string());
    
    let config = device.default_input_config()
        .map_err(|e| format!("Failed to get default input config: {}", e))?;
    
    // Create temporary file for recording - use a simple approach
    let temp_file = NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    
    let temp_path = temp_file.path().to_string_lossy().to_string();
    println!("Created temp file: {}", temp_path);
    
    let spec = WavSpec {
        channels: config.channels(),
        sample_rate: config.sample_rate().0,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    
    let writer = WavWriter::create(temp_file.path(), spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
    let writer = Arc::new(Mutex::new(writer));
    let writer_clone = writer.clone();
    
    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            let sample_i16 = (sample * i16::MAX as f32) as i16;
                            let _ = writer.write_sample(sample_i16);
                        }
                    }
                },
                |err| eprintln!("An error occurred on the input audio stream: {}", err),
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            let _ = writer.write_sample(sample);
                        }
                    }
                },
                |err| eprintln!("An error occurred on the input audio stream: {}", err),
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            let sample_i16 = ((sample as i32) - 32768) as i16;
                            let _ = writer.write_sample(sample_i16);
                        }
                    }
                },
                |err| eprintln!("An error occurred on the input audio stream: {}", err),
                None,
            )
        }
        _ => {
            return Err("Unsupported sample format".to_string());
        }
    };
    
    let stream = stream.map_err(|e| format!("Failed to build input stream: {}", e))?;
    
    stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;
    
    // Store in global state (unsafe but necessary for the callback)
    unsafe {
        RECORDING_STREAM = Some(stream);
        WAV_WRITER = Some(writer);
        TEMP_FILE = Some(temp_file);
    }
    
    app_state.temp_file_path = Some(temp_path.clone());
    println!("Temp file stored at: {}", temp_path);
    
    app_state.is_recording = true;
    app_state.current_device_name = device_name;
    app_state.recording_start_time = Some(Instant::now());
    
    println!("Recording started using device: {}", app_state.current_device_name);
    
    // Show overlay status if app handle is available
    if let Some(handle) = app_handle {
        show_recording_started(&handle);
    }
    
    Ok(())
}

fn stop_recording_hotkey(state: State<Arc<Mutex<AppState>>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    if !app_state.is_recording {
        return Err("Not currently recording".to_string());
    }
    
    // Check minimum recording duration to prevent "too short" errors
    if let Some(start_time) = app_state.recording_start_time {
        let duration = start_time.elapsed();
        if duration < Duration::from_millis(500) {
            println!("Recording duration too short ({}ms), extending...", duration.as_millis());
            drop(app_state); // Release lock
            std::thread::sleep(Duration::from_millis(600) - duration);
            app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        }
    }
    
    let temp_file_path = app_state.temp_file_path.clone()
        .ok_or("No temp file path available".to_string())?;
    
    let settings = app_state.settings.clone();
    let auto_paste = settings.auto_paste;
    let transcription_service = app_state.transcription_service.clone();
    
    // Clean up recording resources
    unsafe {
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream);
        }
        if let Some(writer) = WAV_WRITER.take() {
            if let Ok(writer) = Arc::try_unwrap(writer) {
                if let Ok(writer) = writer.into_inner() {
                    let _ = writer.finalize();
                }
            }
        }
        // Don't drop the temp file yet - we need it for transcription
    }
    
    app_state.is_recording = false;
    app_state.current_device_name = "Recording Stopped".to_string();
    app_state.recording_start_time = None;
    
    println!("Recording stopped");
    
    // Show overlay status if app handle is available
    if let Some(ref handle) = app_handle {
        show_recording_stopped(handle);
    }
    
    // Transcribe in background using app handle if available
    let should_transcribe = match settings.transcription_mode {
        TranscriptionMode::OpenAI => !settings.api_key.is_empty(),
        TranscriptionMode::LocalWhisper => true, // Local whisper doesn't need API key
        TranscriptionMode::CandleWhisper => true, // Candle whisper doesn't need API key
    };
    
    if should_transcribe {
        if let Some(handle) = app_handle {
            // Use tauri::async_runtime to spawn in the correct context
            let handle_clone = handle.clone();
            tauri::async_runtime::spawn(async move {
                println!("Starting transcription for file: {}", temp_file_path);
                
                // Check if file exists before transcription
                if std::path::Path::new(&temp_file_path).exists() {
                    println!("Temp file exists, proceeding with transcription");
                } else {
                    println!("WARNING: Temp file does not exist at transcription time!");
                }
                
                match transcribe_and_paste(temp_file_path.clone(), settings, transcription_service).await {
                    Ok(text) => {
                        println!("Transcription: {}", text);
                        show_transcription_success(&handle_clone, &text);
                        if auto_paste && !text.trim().is_empty() {
                            if let Err(e) = paste_text_direct(text).await {
                                eprintln!("Failed to paste text: {}", e);
                                show_transcription_error(&handle_clone, &format!("Failed to paste text: {}", e));
                            }
                        }
                        
                        // Auto-hide overlay after showing success
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        hide_overlay_status(&handle_clone);
                    }
                    Err(e) => {
                        eprintln!("Transcription failed: {}", e);
                        show_transcription_error(&handle_clone, &e);
                        
                        // Auto-hide overlay after showing error
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        hide_overlay_status(&handle_clone);
                    }
                }
                
                // Clean up temp file after transcription
                println!("Cleaning up temp file after transcription");
                unsafe {
                    if let Some(temp_file) = TEMP_FILE.take() {
                        println!("Temp file cleaned up successfully");
                        // NamedTempFile will auto-delete when dropped
                    } else {
                        println!("No temp file to clean up");
                    }
                }
            });
        } else {
            eprintln!("No app handle available for background transcription");
        }
    }
    
    Ok(())
}

fn force_stop_recording(state: State<Arc<Mutex<AppState>>>) -> Result<(), String> {
    let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    // Force cleanup of recording resources regardless of state
    unsafe {
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream);
        }
        if let Some(writer) = WAV_WRITER.take() {
            if let Ok(writer) = Arc::try_unwrap(writer) {
                if let Ok(writer) = writer.into_inner() {
                    let _ = writer.finalize();
                }
            }
        }
        TEMP_FILE.take();
    }
    
    app_state.is_recording = false;
    app_state.current_device_name = "Recording Force Stopped".to_string();
    app_state.temp_file_path = None;
    app_state.recording_start_time = None;
    
    println!("Recording force stopped and resources cleaned up");
    
    Ok(())
}

async fn transcribe_and_paste(
    audio_file_path: String, 
    settings: AppSettings,
    transcription_service: Option<Arc<Mutex<TranscriptionService>>>
) -> Result<String, String> {
    // Try to use cached service first
    if let Some(service_arc) = transcription_service {
        println!("Using cached transcription service");
        
        // Clone the Arc to use in an async context
        let service_clone = service_arc.clone();
        
        // Spawn a blocking task to handle the mutex and async transcription
        let audio_path_clone = audio_file_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            // This runs in a thread pool, so we can block here safely
            let service = service_clone.lock()
                .map_err(|e| format!("Failed to lock transcription service: {}", e))?;
            
            // Use tokio::runtime::Handle to run async code from blocking context
            let rt = tokio::runtime::Handle::current();
            rt.block_on(service.transcribe(&audio_path_clone))
                .map_err(|e| format!("Transcription failed: {}", e))
        }).await
        .map_err(|e| format!("Task join failed: {}", e))??;
        
        Ok(result)
    } else {
        println!("Creating new transcription service (no cached service available)");
        
        // Fallback: create new service instance
        let config = TranscriptionConfig {
            mode: settings.transcription_mode.clone(),
            openai_api_key: if settings.api_key.is_empty() { None } else { Some(settings.api_key.clone()) },
            whisper_model_path: settings.whisper_model_path.clone(),
            whisper_model_size: settings.whisper_model_size.clone(),
            device: settings.device_type.clone(),
        };
        
        let service = TranscriptionService::new(config)
            .map_err(|e| format!("Failed to create transcription service: {}", e))?;
        
        let text = service.transcribe(&audio_file_path).await
            .map_err(|e| format!("Transcription failed: {}", e))?;
        
        Ok(text)
    }
}

async fn paste_text_direct(text: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let settings = Settings::default();
        let mut enigo = Enigo::new(&settings).map_err(|e| format!("Failed to create Enigo: {}", e))?;
        
        enigo.text(&text).map_err(|e| format!("Failed to type text: {}", e))?;
        
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    let mut audio_devices = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("Device {}", index));
        audio_devices.push(AudioDevice {
            name,
            id: index.to_string(),
        });
    }
    
    Ok(audio_devices)
}

#[tauri::command]
async fn get_settings(state: State<'_, Arc<Mutex<AppState>>>) -> Result<AppSettings, String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(app_state.settings.clone())
}

#[tauri::command]
async fn save_settings(
    settings: AppSettings,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    // Save to file first
    save_settings_to_file(&settings)?;
    
    // Then update in-memory state
    let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    // Check if we need to reload the transcription service
    let should_reload_service = app_state.settings.transcription_mode != settings.transcription_mode ||
                               app_state.settings.whisper_model_size != settings.whisper_model_size ||
                               app_state.settings.whisper_model_path != settings.whisper_model_path ||
                               app_state.settings.device_type != settings.device_type;
    
    app_state.settings = settings.clone();
    
    // Reload transcription service if needed
    if should_reload_service && matches!(settings.transcription_mode, TranscriptionMode::LocalWhisper | TranscriptionMode::CandleWhisper) {
        println!("Reloading transcription service with new settings...");
        
        // Initialize the transcription service
        let config = TranscriptionConfig {
            mode: settings.transcription_mode.clone(),
            openai_api_key: settings.api_key.clone().into(),
            whisper_model_path: settings.whisper_model_path.clone(),
            whisper_model_size: settings.whisper_model_size.clone(),
            device: settings.device_type.clone(),
        };
        
        match TranscriptionService::new(config) {
            Ok(service) => {
                app_state.transcription_service = Some(Arc::new(Mutex::new(service)));
                println!("Transcription service loaded successfully");
            }
            Err(e) => {
                println!("Failed to initialize transcription service: {}", e);
                return Err(format!("Failed to initialize transcription service: {}", e));
            }
        }
    }
    
    Ok(())
}

#[tauri::command]
async fn get_recording_state(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<RecordingState, String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(RecordingState {
        is_recording: app_state.is_recording,
        device_name: app_state.current_device_name.clone(),
    })
}

#[tauri::command]
async fn toggle_recording(
    _app_handle: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let is_recording = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.is_recording
    };
    
    if is_recording {
        stop_recording_hotkey(state, Some(_app_handle.clone()))?;
    } else {
        start_recording_hotkey(state, Some(_app_handle))?;
    }
    
    Ok(!is_recording)
}

#[tauri::command]
async fn hide_window(window: Window) -> Result<(), String> {
    window.hide().map_err(|e| format!("Failed to hide window: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn force_stop_recording_command(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    force_stop_recording(state)
}

#[tauri::command]
async fn download_whisper_model(
    app_handle: AppHandle,
    model_size: WhisperModelSize,
) -> Result<String, String> {
    transcription::TranscriptionService::download_model_with_progress(&model_size, app_handle)
        .await
        .map_err(|e| format!("Failed to download model: {}", e))
}

#[tauri::command]
async fn check_whisper_model(
    model_size: WhisperModelSize,
) -> Result<bool, String> {
    let model_path = dirs::data_dir()
        .ok_or_else(|| "Could not find data directory".to_string())?
        .join("echotype")
        .join("models")
        .join(model_size.model_filename());
    
    Ok(model_path.exists())
}

#[tauri::command]
async fn download_candle_model(
    app_handle: AppHandle,
    model_size: WhisperModelSize,
) -> Result<String, String> {
    use crate::transcription::whisper_candle::CandleWhisperBackend;
    use crate::transcription::DeviceType;
    
    // Create a temporary backend instance to use the download functionality
    let backend = CandleWhisperBackend::new(model_size.clone(), DeviceType::Cpu)
        .map_err(|e| format!("Failed to create backend: {}", e))?;
    
    backend.download_model_with_progress(app_handle).await
        .map_err(|e| format!("Failed to download model: {}", e))
}

#[tauri::command]
async fn check_candle_model(
    model_size: WhisperModelSize,
) -> Result<bool, String> {
    use crate::transcription::whisper_candle::CandleWhisperBackend;
    use crate::transcription::DeviceType;
    
    // Create a temporary backend instance to check model status
    let backend = CandleWhisperBackend::new(model_size.clone(), DeviceType::Cpu)
        .map_err(|e| format!("Failed to create backend: {}", e))?;
    
    backend.check_model_downloaded().await
        .map_err(|e| format!("Failed to check model: {}", e))
}

#[tauri::command]
async fn preload_candle_model(
    model_size: WhisperModelSize,
    device_type: DeviceType,
) -> Result<String, String> {
    use crate::transcription::whisper_candle::CandleWhisperBackend;
    
    // Create a backend instance to preload the model
    let backend = CandleWhisperBackend::new(model_size.clone(), device_type)
        .map_err(|e| format!("Failed to create backend: {}", e))?;
    
    backend.preload_model().await
        .map_err(|e| format!("Failed to preload model: {}", e))
}

#[tauri::command]
async fn show_overlay_status_command(
    app_handle: AppHandle,
    message: String,
    status_type: String,
) -> Result<(), String> {
    show_overlay_status(&app_handle, &message, &status_type);
    Ok(())
}

#[tauri::command]
async fn hide_overlay_status_command(
    app_handle: AppHandle,
) -> Result<(), String> {
    hide_overlay_status(&app_handle);
    Ok(())
}

#[tauri::command]
async fn hide_overlay_window(app_handle: AppHandle) -> Result<(), String> {
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        overlay_window.set_always_on_top(false).map_err(|e| format!("Failed to remove always-on-top: {}", e))?;
        overlay_window.hide().map_err(|e| format!("Failed to hide overlay window: {}", e))?;
        println!("Overlay window properly hidden and removed from always-on-top");
    }
    Ok(())
}

#[tauri::command]
async fn get_cursor_position_command() -> Result<(i32, i32), String> {
    get_cursor_position()
}

#[tauri::command]
async fn initialize_transcription_service(
    settings: AppSettings,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    println!("Initializing transcription service with mode: {:?}", settings.transcription_mode);
    
    // Create transcription config
    let config = TranscriptionConfig {
        mode: settings.transcription_mode.clone(),
        openai_api_key: if settings.api_key.is_empty() { None } else { Some(settings.api_key.clone()) },
        whisper_model_path: settings.whisper_model_path.clone(),
        whisper_model_size: settings.whisper_model_size.clone(),
        device: settings.device_type.clone(),
    };
    
    match TranscriptionService::new(config) {
        Ok(service) => {
            // Store the service and settings in state
            {
                let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                app_state.transcription_service = Some(Arc::new(Mutex::new(service)));
                app_state.settings = settings.clone();
                println!("Transcription service initialized successfully");
            } // Lock is released here
            
            // For Candle models, also try to preload (outside the lock)
            if matches!(settings.transcription_mode, TranscriptionMode::CandleWhisper) {
                match preload_candle_model(settings.whisper_model_size.clone(), settings.device_type.clone()).await {
                    Ok(msg) => println!("Model preloaded: {}", msg),
                    Err(e) => println!("Failed to preload model: {}", e),
                }
            }
            
            Ok("Transcription service initialized successfully".to_string())
        }
        Err(e) => {
            let error_msg = format!("Failed to initialize transcription service: {}", e);
            println!("{}", error_msg);
            Err(error_msg)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load settings from file
    let saved_settings = load_settings_from_file();
    
    let mut initial_state = AppState::default();
    initial_state.settings = saved_settings;
    
    let app_state = Arc::new(Mutex::new(initial_state));
    
    // Debug: Print initial state
    if let Ok(state) = app_state.lock() {
        println!("Initial app state - is_recording: {}, device: {}", 
                 state.is_recording, state.current_device_name);
        println!("API key loaded: {}", if state.settings.api_key.is_empty() { "No" } else { "Yes" });
        println!("Selected device: {}", state.settings.selected_device_id);
    }
    
    tauri::Builder::default()
        .manage(app_state.clone())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let _handle = app.handle().clone();
            
            // Debug: Print all available windows
            println!("Available windows during setup:");
            if let Some(main_window) = app.get_webview_window("main") {
                println!("  - main window found");
            } else {
                println!("  - main window NOT found");
            }
            if let Some(overlay_window) = app.get_webview_window("overlay") {
                println!("  - overlay window found");
                
                // Try to show the overlay window immediately for testing
                println!("  - Attempting to show overlay window for testing...");
                if let Err(e) = overlay_window.show() {
                    println!("  - Failed to show overlay: {}", e);
                } else {
                    println!("  - Overlay window should now be visible!");
                }
                
                // Test event emission
                println!("  - Testing event emission...");
                let test_status = StatusUpdate {
                    message: "TEST: Overlay window loaded".to_string(),
                    r#type: "recording".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                };
                if let Err(e) = overlay_window.emit("status-update", test_status) {
                    println!("  - Failed to emit test event: {}", e);
                } else {
                    println!("  - Test event emitted successfully");
                }
                
                // Check window properties
                if let Ok(is_visible) = overlay_window.is_visible() {
                    println!("  - Overlay window is_visible: {}", is_visible);
                }
                if let Ok(position) = overlay_window.outer_position() {
                    println!("  - Overlay window position: {:?}", position);
                }
                if let Ok(size) = overlay_window.outer_size() {
                    println!("  - Overlay window size: {:?}", size);
                }
            } else {
                println!("  - overlay window NOT found");
            }
            
            // Create menu items
            let show_item = MenuItem::with_id(app, "show", "Show Settings", true, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "start_recording", "Start Recording (Alt+Alt)", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "stop_recording", "Stop Recording", true, None::<&str>)?;
            let force_stop_item = MenuItem::with_id(app, "force_stop", "Force Stop Recording", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&start_item)
                .item(&stop_item)
                .item(&force_stop_item)
                .separator()
                .item(&quit_item)
                .build()?;
            
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(move |app, event| handle_menu_event(app, event))
                .build(app)?;
            
            // Start the Alt key monitoring thread
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                monitor_alt_keys(app_handle);
            });
            
            Ok(())
        })
        .invoke_handler(generate_handler![
            get_audio_devices,
            get_settings,
            save_settings,
            get_recording_state,
            toggle_recording,
            hide_window,
            force_stop_recording_command,
            download_whisper_model,
            check_whisper_model,
            download_candle_model,
            check_candle_model,
            preload_candle_model,
            initialize_transcription_service,
            show_overlay_status_command,
            hide_overlay_status_command,
            hide_overlay_window,
            get_cursor_position_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn monitor_alt_keys(app_handle: AppHandle) {
    let device_state = DeviceState::new();
    let mut last_keys = vec![];
    
    loop {
        let keys = device_state.get_keys();
        
        // Check for Alt key press
        if keys.contains(&Keycode::LAlt) && !last_keys.contains(&Keycode::LAlt) {
            // Alt key was just pressed
            let state = app_handle.state::<Arc<Mutex<AppState>>>();
            let mut app_state = match state.inner().lock() {
                Ok(state) => state,
                Err(_) => continue,
            };
            
            println!("Alt key pressed! Current state - recording: {}, tap_count: {}", 
                     app_state.is_recording, app_state.alt_tap_count);
            
            let now = Instant::now();
            
            if let Some(last_press) = app_state.last_alt_press {
                let duration_since_last = now.duration_since(last_press);
                println!("Time since last Alt press: {}ms", duration_since_last.as_millis());
                
                if duration_since_last < Duration::from_millis(800) {
                    app_state.alt_tap_count += 1;
                    println!("Alt tap count increased to: {}", app_state.alt_tap_count);
                    
                    if app_state.alt_tap_count >= 2 {
                        // Double tap detected!
                        let is_recording = app_state.is_recording;
                        println!("Double Alt detected! Current recording state: {}", is_recording);
                        
                        // Reset the counters immediately to prevent multiple triggers
                        app_state.alt_tap_count = 0;
                        app_state.last_alt_press = None;
                        drop(app_state); // Release the lock before calling recording functions
                        
                        if is_recording {
                            println!("Attempting to stop recording...");
                            match stop_recording_hotkey(state, Some(app_handle.clone())) {
                                Ok(_) => println!("Stop recording completed successfully"),
                                Err(e) => println!("Stop recording failed: {}", e),
                            }
                        } else {
                            println!("Attempting to start recording...");
                            match start_recording_hotkey(state, Some(app_handle.clone())) {
                                Ok(_) => println!("Start recording completed successfully"),
                                Err(e) => println!("Start recording failed: {}", e),
                            }
                        }
                        
                        // Small delay to prevent rapid re-triggering
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                } else {
                    println!("Alt press too late ({}ms), resetting count", duration_since_last.as_millis());
                    app_state.alt_tap_count = 1;
                }
            } else {
                println!("First Alt press detected");
                app_state.alt_tap_count = 1;
            }
            
            app_state.last_alt_press = Some(now);
        } else if !keys.contains(&Keycode::LAlt) {
            // Reset if Alt is not pressed for too long
            if let Ok(mut app_state) = app_handle.state::<Arc<Mutex<AppState>>>().inner().lock() {
                if let Some(last_press) = app_state.last_alt_press {
                    let time_since_last = Instant::now().duration_since(last_press);
                    if time_since_last > Duration::from_millis(1000) {
                        if app_state.alt_tap_count > 0 {
                            println!("Resetting Alt tap count due to timeout ({}ms)", time_since_last.as_millis());
                        }
                        app_state.alt_tap_count = 0;
                        app_state.last_alt_press = None;
                    }
                }
            }
        }
        
        last_keys = keys;
        std::thread::sleep(Duration::from_millis(50));
    }
}
