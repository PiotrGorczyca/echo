// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod transcription;
mod voice_activation;
mod rocm_detection;
mod mcp;
mod ai_agent;
mod voice_command;
mod recording_manager;

// New modular structure
mod state;
mod settings;
mod commands;

use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::io::BufWriter;
use std::fs::File;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, SampleFormat};
use hound::{WavSpec, WavWriter};
use tempfile::NamedTempFile;
use anyhow::Result;
// device_query imports are used locally in monitor_alt_keys function
use enigo::{Enigo, Settings, Keyboard};
use notify_rust::Notification;
use tauri::{
    AppHandle, Manager, State, generate_handler,
    menu::{MenuBuilder, MenuItem, MenuEvent},
    tray::{TrayIconBuilder}, Emitter, Listener,
};
use mouse_position::mouse_position::{Mouse};

// Import from our new modules
use state::*;
use settings::*;
use commands::window::show_and_focus_main_window;
use transcription::{TranscriptionService, TranscriptionConfig};
use recording_manager::{RecordingManager, RecordingMode, RecordingConfig};

// Helper function to perform transcription safely without holding locks across await
async fn perform_transcription_safe(
    service: Arc<Mutex<TranscriptionService>>, 
    file_path: &str
) -> Result<String, String> {
    // Clone the file path to avoid borrowing issues
    let file_path = file_path.to_string();
    
    // Extract the transcription logic into a separate function call
    // This avoids holding the lock across await by calling transcribe directly
    let transcription_config = {
        let service_guard = service.lock()
            .map_err(|e| format!("Failed to lock transcription service: {}", e))?;
        
        // Clone the config so we can release the lock
        service_guard.get_config().clone()
    };
    
    // Create a new TranscriptionService instance for this call
    // This avoids the lock issue entirely
    let temp_service = TranscriptionService::new(transcription_config)
        .map_err(|e| format!("Failed to create transcription service: {}", e))?;
    
    // Now we can call transcribe without holding any locks
    temp_service.transcribe(&file_path).await
        .map_err(|e| format!("Transcription failed: {}", e))
}

// Simple test recording - completely separate from main recording system
static mut TEST_RECORDING_STREAM: Option<Stream> = None;
static mut TEST_WAV_WRITER: Option<Arc<Mutex<WavWriter<BufWriter<File>>>>> = None;
static mut TEST_RECORDING_FILE_PATH: Option<String> = None;

// Actual recording system - separate from RecordingManager state management
static mut RECORDING_STREAM: Option<Stream> = None;
static mut WAV_WRITER: Option<Arc<Mutex<WavWriter<BufWriter<File>>>>> = None;
static mut TEMP_FILE_PATH: Option<String> = None;
static mut TEMP_FILE: Option<NamedTempFile> = None;

// Global RecordingManager instance
static mut RECORDING_MANAGER: Option<Arc<RecordingManager>> = None;

// Simple test recording functions
fn start_simple_test_recording(device_id: &str) -> Result<String, String> {
    println!("🎬 Starting simple test recording with device_id: {}", device_id);
    
    // Check if test recording is already running
    unsafe {
        if TEST_RECORDING_STREAM.is_some() {
            println!("⚠️ Test recording already in progress, stopping existing one first");
            let _ = stop_simple_test_recording();
        }
    }
    
    let host = cpal::default_host();
    
    // Get device
    let device = if let Ok(device_index) = device_id.parse::<usize>() {
        println!("🎯 Trying to use device index: {}", device_index);
        let devices: Vec<_> = host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?
            .collect();
            
        println!("📋 Found {} audio devices", devices.len());
        devices.into_iter().nth(device_index)
            .unwrap_or_else(|| {
                println!("⚠️ Device index {} not found, falling back to default", device_index);
                host.default_input_device().unwrap()
            })
    } else {
        println!("⚠️ Invalid device_id format: {}, using default device", device_id);
        host.default_input_device()
            .ok_or_else(|| "No default input device available".to_string())?
    };
    
    println!("🎤 Selected device: {:?}", device.name().unwrap_or("Unknown".to_string()));
    
    let config = device.default_input_config()
        .map_err(|e| format!("Failed to get default input config: {}", e))?;
    
    println!("⚙️ Device config: {}Hz, {} channels, {:?}", 
             config.sample_rate().0, config.channels(), config.sample_format());
    
    // Extract values we need before moving config
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    
    // Create simple temp file
    println!("📁 Creating temporary file...");
    let temp_file = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    
    // Persist the temp file so it doesn't get deleted
    let (_file, persistent_path) = temp_file.keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e))?;
    
    let final_path = persistent_path.to_string_lossy().to_string();
    println!("📂 Created persistent file: {}", final_path);
    
    // Create WAV writer with standard settings for browser compatibility
    let spec = WavSpec {
        channels: 1, // Mono for simplicity and compatibility
        sample_rate: 44100, // Standard sample rate
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    
    println!("🎵 Creating WAV writer with spec: {}Hz, {} channels, {} bits", 
             spec.sample_rate, spec.channels, spec.bits_per_sample);
    
    let wav_writer = WavWriter::create(&final_path, spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
    
    let wav_writer = Arc::new(Mutex::new(wav_writer));
    
    // Create and start stream
    println!("🔧 Building audio stream...");
    let stream = match sample_format {
        SampleFormat::F32 => {
            println!("📡 Using F32 sample format");
            device.build_input_stream(
                &config.into(),
                {
                    let writer = wav_writer.clone();
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if let Ok(mut wav_writer) = writer.lock() {
                            // Convert stereo to mono if needed and write
                            if channels == 1 {
                                // Already mono
                                for &sample in data {
                                    let _ = wav_writer.write_sample((sample * i16::MAX as f32) as i16);
                                }
                            } else {
                                // Convert to mono by averaging channels
                                for chunk in data.chunks(channels) {
                                    let avg = chunk.iter().sum::<f32>() / channels as f32;
                                    let _ = wav_writer.write_sample((avg * i16::MAX as f32) as i16);
                                }
                            }
                        }
                    }
                },
                |err| eprintln!("Test recording stream error: {}", err),
                None,
            )
        },
        SampleFormat::I16 => {
            println!("📡 Using I16 sample format");
            device.build_input_stream(
                &config.into(),
                {
                    let writer = wav_writer.clone();
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if let Ok(mut wav_writer) = writer.lock() {
                            if channels == 1 {
                                // Already mono
                                for &sample in data {
                                    let _ = wav_writer.write_sample(sample);
                                }
                            } else {
                                // Convert to mono by averaging channels
                                for chunk in data.chunks(channels) {
                                    let avg = chunk.iter().map(|&x| x as i32).sum::<i32>() / channels as i32;
                                    let _ = wav_writer.write_sample(avg as i16);
                                }
                            }
                        }
                    }
                },
                |err| eprintln!("Test recording stream error: {}", err),
                None,
            )
        },
        _ => return Err(format!("Unsupported sample format for test recording: {:?}", sample_format)),
    }.map_err(|e| format!("Failed to build test recording stream: {}", e))?;
    
    println!("▶️ Starting audio stream...");
    stream.play().map_err(|e| format!("Failed to start test recording stream: {}", e))?;
    
    // Store in test recording globals
    unsafe {
        TEST_RECORDING_STREAM = Some(stream);
        TEST_WAV_WRITER = Some(wav_writer);
        TEST_RECORDING_FILE_PATH = Some(final_path.clone());
    }
    
    println!("✅ Simple test recording started successfully: {}", final_path);
    Ok(final_path)
}

fn stop_simple_test_recording() -> Result<Option<String>, String> {
    println!("🛑 Stopping simple test recording...");
    
    let file_path = unsafe {
        // Stop stream
        if let Some(stream) = TEST_RECORDING_STREAM.take() {
            drop(stream);
            println!("🔇 Test recording stream stopped");
        } else {
            println!("⚠️ No test recording stream to stop");
        }
        
        // Finalize WAV writer
        if let Some(wav_writer) = TEST_WAV_WRITER.take() {
            println!("💾 Finalizing WAV writer...");
            if let Ok(writer) = Arc::try_unwrap(wav_writer) {
                if let Ok(writer) = writer.into_inner() {
                    match writer.finalize() {
                        Ok(_) => println!("✅ Test WAV writer finalized successfully"),
                        Err(e) => {
                            eprintln!("❌ Failed to finalize test WAV writer: {}", e);
                            return Err(format!("Failed to finalize WAV writer: {}", e));
                        }
                    }
                }
            } else {
                eprintln!("⚠️ Could not unwrap WAV writer Arc");
            }
        } else {
            println!("⚠️ No WAV writer to finalize");
        }
        
        TEST_RECORDING_FILE_PATH.clone()
    };
    
    if let Some(ref path) = file_path {
        // Verify file was created properly
        println!("🔍 Verifying file: {}", path);
        match std::fs::metadata(path) {
            Ok(metadata) => {
                let file_size = metadata.len();
                println!("✅ Test recording file verified: {} bytes", file_size);
                if file_size == 0 {
                    return Err("Test recording file is empty".to_string());
                }
                if file_size < 100 {
                    println!("⚠️ Test recording file is very small: {} bytes", file_size);
                }
            }
            Err(e) => {
                println!("❌ File verification failed: {}", e);
                return Err(format!("Test recording file verification failed: {}", e));
            }
        }
    } else {
        println!("❌ No file path available");
    }
    
    println!("🎉 Test recording stopped successfully");
    Ok(file_path)
}

fn cleanup_simple_test_recording() -> Result<(), String> {
    let file_path = unsafe {
        TEST_RECORDING_FILE_PATH.take()
    };
    
    if let Some(path) = file_path {
        match std::fs::remove_file(&path) {
            Ok(_) => {
                println!("✅ Test recording file cleaned up: {}", path);
                Ok(())
            }
            Err(e) => {
                eprintln!("❌ Failed to clean up test recording file: {}", e);
                Err(format!("Failed to clean up test recording file: {}", e))
            }
        }
    } else {
        Ok(()) // Nothing to clean up
    }
}

// Helper function to create WAV writer
fn create_wav_writer(path: &str, sample_rate: cpal::SampleRate, channels: u16) -> Result<WavWriter<BufWriter<File>>, hound::Error> {
    // Use browser-compatible settings for better playback compatibility
    // Most browsers work best with standard sample rates and mono/stereo audio
    let browser_compatible_sample_rate = match sample_rate.0 {
        // If already a standard rate, keep it
        44100 | 48000 | 22050 | 16000 => sample_rate.0,
        // Otherwise default to 44.1kHz (CD quality, widely supported)
        _ => {
            println!("Converting sample rate from {} to 44100 Hz for browser compatibility", sample_rate.0);
            44100
        }
    };
    
    // Use mono for test recordings to ensure compatibility and smaller file size
    let browser_compatible_channels = if channels > 2 {
        println!("Converting from {} channels to stereo for browser compatibility", channels);
        2
    } else {
        channels
    };
    
    let spec = WavSpec {
        channels: browser_compatible_channels,
        sample_rate: browser_compatible_sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    
    println!("Creating WAV file with spec: {}Hz, {} channels, 16-bit PCM", 
             browser_compatible_sample_rate, browser_compatible_channels);
    
    WavWriter::create(path, spec)
}

// Global state for the audio stream (not shared with Tauri) - removed duplicates

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
    show_overlay_status(app_handle, "Recording started...", "recording");
}

fn show_recording_stopped(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "Recording stopped", "success");
}

fn show_transcribing_status(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "Transcribing audio...", "transcribing");
}

fn show_transcription_success(app_handle: &AppHandle, text: &str) {
    let preview = if text.len() > 50 {
        format!("{}...", &text[..50])
    } else {
        text.to_string()
    };
    show_overlay_status(app_handle, &format!("{}", preview), "success");
}

fn show_transcription_error(app_handle: &AppHandle, error: &str) {
    show_overlay_status(app_handle, error, "error");
}

// Handle tray menu events
fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "show" => {
            let _ = show_and_focus_main_window(app);
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

// Helper functions for audio recording
fn get_audio_devices_sync() -> Result<Vec<AudioDevice>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    
    let host = cpal::default_host();
    let input_devices = host.input_devices().map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    let mut devices = Vec::new();
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("Device {}", i));
        devices.push(AudioDevice {
            id: i.to_string(),
            name,
        });
    }
    
    Ok(devices)
}

fn start_audio_recording(device_id: &str) -> Result<(), String> {
    
    let host = cpal::default_host();
    
    // Get the actual device to use
    let device = if let Ok(device_index) = device_id.parse::<usize>() {
        // Try to get the specific device by index
        let devices: Vec<_> = host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?
            .collect();
            
        if let Some(device) = devices.into_iter().nth(device_index) {
            println!("Using selected device index {}: {:?}", device_index, device.name().unwrap_or_default());
            device
        } else {
            println!("Device index {} not found, falling back to default device", device_index);
            host.default_input_device()
                .ok_or_else(|| "No default input device available".to_string())?
        }
    } else {
        println!("Invalid device ID format: {}, using default device", device_id);
        host.default_input_device()
            .ok_or_else(|| "No default input device available".to_string())?
    };
    
    let config = device.default_input_config()
        .map_err(|e| format!("Failed to get default input config: {}", e))?;
    
    println!("Recording config: channels={}, sample_rate={}, format={:?}", 
             config.channels(), config.sample_rate().0, config.sample_format());
    
    let sample_rate = config.sample_rate();
    let channels = config.channels();
    
    // Get temp file path from global state
    let temp_path = unsafe {
        TEMP_FILE_PATH.as_ref()
            .ok_or_else(|| "No temp file path available".to_string())?
            .clone()
    };
    
    // Create WAV writer
    let wav_writer = create_wav_writer(&temp_path, sample_rate, channels)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
    
    let wav_writer = Arc::new(Mutex::new(wav_writer));
    
    unsafe {
        WAV_WRITER = Some(wav_writer.clone());
    }
    
    // Build and start stream based on sample format
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                write_audio_data_f32(&wav_writer, data);
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                write_audio_data_i16(&wav_writer, data);
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                write_audio_data_u16(&wav_writer, data);
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        ),
        _ => return Err(format!("Unsupported sample format: {:?}", config.sample_format())),
    }.map_err(|e| format!("Failed to build input stream: {}", e))?;
    
    stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;
    
    unsafe {
        RECORDING_STREAM = Some(stream);
    }
    
    Ok(())
}

fn write_audio_data_f32(wav_writer: &Arc<Mutex<WavWriter<BufWriter<File>>>>, data: &[f32]) {
    if let Ok(mut writer) = wav_writer.lock() {
        for &sample in data {
            let sample_i16 = (sample * i16::MAX as f32) as i16;
            let _ = writer.write_sample(sample_i16);
        }
    }
}

fn write_audio_data_i16(wav_writer: &Arc<Mutex<WavWriter<BufWriter<File>>>>, data: &[i16]) {
    if let Ok(mut writer) = wav_writer.lock() {
        for &sample in data {
            let _ = writer.write_sample(sample);
        }
    }
}

fn write_audio_data_u16(wav_writer: &Arc<Mutex<WavWriter<BufWriter<File>>>>, data: &[u16]) {
    if let Ok(mut writer) = wav_writer.lock() {
        for &sample in data {
            let sample_i16 = ((sample as i32) - 32768) as i16;
            let _ = writer.write_sample(sample_i16);
        }
    }
}

// TODO: The recording functions and other commands will be moved to separate modules
// For now, keeping the essential ones here to maintain functionality

pub fn run() {
    // Load settings from file
    let saved_settings = load_settings_from_file();
    
    let mut initial_state = AppState::default();
    initial_state.settings = saved_settings.clone();
    
    // Initialize transcription service with loaded settings
    let transcription_config = TranscriptionConfig {
        mode: saved_settings.transcription_mode.clone(),
        openai_api_key: if saved_settings.api_key.is_empty() { None } else { Some(saved_settings.api_key.clone()) },
        whisper_model_path: saved_settings.whisper_model_path.clone(),
        whisper_model_size: saved_settings.whisper_model_size.clone(),
        device: saved_settings.device_type.clone(),
    };
    
    match TranscriptionService::new(transcription_config) {
        Ok(service) => {
            initial_state.transcription_service = Some(Arc::new(Mutex::new(service)));
            println!("✅ Transcription service initialized on startup with saved settings");
        }
        Err(e) => {
            eprintln!("⚠️  Failed to initialize transcription service on startup: {}", e);
            println!("   Service will be initialized when settings are saved");
        }
    }
    
    let app_state = Arc::new(Mutex::new(initial_state));
    
    // Debug: Print initial state
    if let Ok(state) = app_state.lock() {
        println!("Initial app state - is_recording: {}, device: {}", 
                 state.is_recording, state.current_device_name);
        println!("API key loaded: {}", if state.settings.api_key.is_empty() { "No" } else { "Yes" });
        println!("Selected device: {}", state.settings.selected_device_id);
        println!("Transcription service ready: {}", state.transcription_service.is_some());
    }
    
    tauri::Builder::default()
        .manage(app_state.clone())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let _handle = app.handle().clone();
            
            // Initialize voice command service
            let app_handle = app.handle().clone();
            let app_state_for_voice = app_state.clone();
            {
                let mut state = app_state_for_voice.lock().map_err(|e| format!("Failed to lock state for voice command initialization: {}", e))?;
                
                // Get references to existing services
                let transcription_service = state.transcription_service.clone();
                let ai_agent = state.ai_agent.clone();
                let mcp_client = state.mcp_client.clone();
                
                // Create voice command service
                let voice_service = voice_command::VoiceCommandService::new(
                    app_handle.clone(),
                    transcription_service,
                    ai_agent,
                    mcp_client,
                );
                
                state.voice_command_service = Some(Arc::new(voice_service));
                println!("✅ Voice command service initialized");
                
                // Initialize global recording manager
                let recording_manager = Arc::new(RecordingManager::new());
                unsafe {
                    RECORDING_MANAGER = Some(recording_manager.clone());
                }
                println!("✅ Recording manager initialized");
            }
            
            // Recording events are now handled directly in the recording functions
            
            // Keep legacy voice command event listener for compatibility
            let app_handle_for_legacy = app.handle().clone();
            let app_state_for_legacy = app_state.clone();
            
            app.listen("voice-command-start-recording", move |_event| {
                println!("🎤 Received legacy voice-command-start-recording event - redirecting to new system");
                
                let app_handle = app_handle_for_legacy.clone();
                let app_state = app_state_for_legacy.clone();
                
                // Use the new recording system
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<Arc<Mutex<AppState>>>();
                    
                    if let Err(e) = start_voice_command_recording(state.inner().clone(), Some(app_handle.clone())).await {
                        println!("❌ Failed to start voice command recording: {}", e);
                        
                        // Notify the voice command service of the error
                        if let Ok(app_state) = app_state.lock() {
                            if let Some(ref voice_service) = app_state.voice_command_service {
                                let _ = voice_service.handle_recording_error(&e);
                            }
                        }
                    } else {
                        println!("✅ Voice command recording started successfully via legacy event");
                    }
                });
            });

            
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
            // Import commands from the new modules
            commands::window::hide_window,
            commands::window::hide_main_window,
            commands::window::hide_overlay_window,
            commands::window::position_main_window,
            
            // TODO: Move these to their respective modules
            get_audio_devices,
            get_settings,
            save_settings,
            get_recording_state,
            toggle_recording,
            force_stop_recording_command,
            reload_transcription_service,
            get_cursor_position_command,
            start_recording,
            stop_recording,
            get_last_recording_path,
            start_test_recording,
            stop_test_recording,
            get_audio_file_info,
            cleanup_test_recording,
            verify_audio_playback,
            play_audio_file_native,
            stop_audio_playback,
            
            // MCP Commands
            get_mcp_servers,
            save_mcp_servers,
            connect_mcp_server,
            disconnect_mcp_server,
            get_available_mcp_tools,
            process_voice_command_mcp,
            install_mcp_package,
            
            // Voice Command Commands
            get_voice_command_state,
            get_voice_command_messages,
            clear_voice_command_messages,
            process_text_command,
            start_voice_command_test,
            start_voice_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// New recording functions using RecordingManager
async fn start_transcription_recording(state: Arc<Mutex<AppState>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    let (recording_manager, device_id) = {
        let app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        let device_id = app_state.settings.selected_device_id.clone();
        
        // Get global recording manager
        let manager = unsafe {
            RECORDING_MANAGER.as_ref()
                .ok_or("Recording manager not initialized")?
                .clone()
        };
        
        (manager, device_id)
    };

    // Check if we can start recording
    recording_manager.can_start_recording(&RecordingMode::Transcription)?;

    // Create temporary file
    let temp_file = NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let _temp_path = temp_file.path().to_string_lossy().to_string();
    let (_file, persistent_path) = temp_file.keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e))?;
    let final_temp_path = persistent_path.to_string_lossy().to_string();

    let config = RecordingConfig {
        mode: RecordingMode::Transcription,
        auto_stop_duration_ms: None, // Unlimited for transcription
        temp_file_path: final_temp_path.clone(),
    };

    // Start recording state management
    recording_manager.start_recording(config)?;

    // Start actual audio recording
    let recording_result = start_actual_recording(&device_id, &final_temp_path, 16000, 1).await;
    
    if let Err(e) = recording_result {
        // If audio recording fails, clean up the recording state
        let _ = recording_manager.stop_recording();
        return Err(e);
    }

    // Emit recording started event and show overlay
    if let Some(handle) = app_handle {
        show_overlay_status(&handle, "Recording started...", "recording");
        let _ = handle.emit("recording-started", serde_json::json!({
            "mode": "transcription",
            "file_path": final_temp_path
        }));
    }

    Ok(())
}

async fn start_voice_command_recording(state: Arc<Mutex<AppState>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    let (recording_manager, device_id) = {
        let app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        let device_id = app_state.settings.selected_device_id.clone();
        
        // Get global recording manager
        let manager = unsafe {
            RECORDING_MANAGER.as_ref()
                .ok_or("Recording manager not initialized")?
                .clone()
        };
        
        (manager, device_id)
    };

    // Check if we can start recording
    recording_manager.can_start_recording(&RecordingMode::VoiceCommand)?;

    // Create temporary file
    let temp_file = NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let _temp_path = temp_file.path().to_string_lossy().to_string();
    let (_file, persistent_path) = temp_file.keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e))?;
    let final_temp_path = persistent_path.to_string_lossy().to_string();

    let config = RecordingConfig {
        mode: RecordingMode::VoiceCommand,
        auto_stop_duration_ms: Some(10000), // 10 second timeout for voice commands
        temp_file_path: final_temp_path.clone(),
    };

    // Start recording state management
    recording_manager.start_recording(config)?;

    // Start actual audio recording
    let recording_result = start_actual_recording(&device_id, &final_temp_path, 16000, 1).await;
    
    if let Err(e) = recording_result {
        // If audio recording fails, clean up the recording state
        let _ = recording_manager.stop_recording();
        return Err(e);
    }

    // Set up auto-stop timer for voice commands
    let manager_clone = Arc::clone(&recording_manager);
    let handle_clone = app_handle.clone();
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(10000)).await;
        
        // Check if still recording in voice command mode
        if let Ok(Some(RecordingMode::VoiceCommand)) = manager_clone.get_current_mode() {
            if let Ok(true) = manager_clone.is_recording() {
                println!("⏰ Auto-stopping voice command recording after 10 seconds");
                let _ = stop_recording_new(state_clone, handle_clone).await;
            }
        }
    });

    // Emit recording started event and show overlay
    if let Some(handle) = app_handle {
        show_overlay_status(&handle, "Voice command recording...", "recording");
        let _ = handle.emit("recording-started", serde_json::json!({
            "mode": "voice_command",
            "file_path": final_temp_path
        }));
    }

    Ok(())
}

async fn stop_recording_new(state: Arc<Mutex<AppState>>, app_handle: Option<AppHandle>) -> Result<String, String> {
    let recording_manager = unsafe {
        RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Stop the RecordingManager state first
    let (mode, temp_file_path) = recording_manager.stop_recording()?;

    // Stop actual audio recording
    stop_actual_recording().await?;

    // Emit recording stopped event and show overlay
    if let Some(handle) = app_handle.clone() {
        show_overlay_status(&handle, "Recording stopped", "success");
        let _ = handle.emit("recording-stopped", serde_json::json!({
            "mode": match mode {
                RecordingMode::Transcription => "transcription",
                RecordingMode::VoiceCommand => "voice_command",
            },
            "file_path": temp_file_path.clone()
        }));
        
        // Handle transcription for the recorded file
        let app_state = state.clone();
        let file_path = temp_file_path.clone();
        let mode_for_transcription = mode.clone();
        
        tauri::async_runtime::spawn(async move {
            if let Err(e) = handle_recording_transcription(app_state, handle, file_path, mode_for_transcription).await {
                println!("❌ Failed to handle transcription: {}", e);
            }
        });
    }

    Ok(temp_file_path)
}

// Actual audio recording functions
async fn start_actual_recording(device_id: &str, temp_file_path: &str, sample_rate: u32, channels: u16) -> Result<(), String> {
    // Stop any existing recording first
    stop_actual_recording().await?;

    println!("🎤 Starting actual audio recording to: {}", temp_file_path);

    let host = cpal::default_host();
    
    // Get device
    let device = if let Ok(device_index) = device_id.parse::<usize>() {
        let devices: Vec<_> = host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?
            .collect();
        devices.into_iter().nth(device_index)
            .unwrap_or_else(|| {
                println!("⚠️ Device index {} not found, using default", device_index);
                host.default_input_device().unwrap()
            })
    } else {
        host.default_input_device()
            .ok_or("No default input device available")?
    };

    let device_name = device.name().unwrap_or("Unknown".to_string());
    println!("🎤 Using device: {}", device_name);

    // Get device config
    let device_config = device.default_input_config()
        .map_err(|e| format!("Failed to get device config: {}", e))?;

    // Create WAV writer
    let wav_spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let file = File::create(temp_file_path)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;
    let wav_writer = WavWriter::new(BufWriter::new(file), wav_spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
    let wav_writer = Arc::new(Mutex::new(wav_writer));

    // Create stream config
    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // Create audio stream
    let wav_writer_clone = Arc::clone(&wav_writer);
    let stream = match device_config.sample_format() {
        SampleFormat::F32 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = wav_writer_clone.lock() {
                        for &sample in data {
                            let sample_i16 = (sample * i16::MAX as f32) as i16;
                            let _ = writer.write_sample(sample_i16);
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = wav_writer_clone.lock() {
                        for &sample in data {
                            let _ = writer.write_sample(sample);
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
        }
        _ => return Err("Unsupported sample format".to_string()),
    }.map_err(|e| format!("Failed to create audio stream: {}", e))?;

    // Start the stream
    stream.play().map_err(|e| format!("Failed to start audio stream: {}", e))?;

    // Store globally
    unsafe {
        RECORDING_STREAM = Some(stream);
        WAV_WRITER = Some(wav_writer);
        TEMP_FILE_PATH = Some(temp_file_path.to_string());
    }

    println!("✅ Audio recording started successfully");
    Ok(())
}

async fn stop_actual_recording() -> Result<(), String> {
    println!("🛑 Stopping actual audio recording...");

    unsafe {
        // Stop the stream first
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream);
            println!("✅ Audio stream stopped");
        }

        // Finalize WAV writer
        if let Some(wav_writer) = WAV_WRITER.take() {
            if let Ok(writer) = Arc::try_unwrap(wav_writer) {
                if let Ok(writer) = writer.into_inner() {
                    writer.finalize()
                        .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;
                    println!("✅ WAV file finalized");
                }
            }
        }

        // Verify file exists and has content
        if let Some(file_path) = TEMP_FILE_PATH.take() {
            match std::fs::metadata(&file_path) {
                Ok(metadata) => {
                    println!("✅ Recording file verified: {} bytes", metadata.len());
                    if metadata.len() == 0 {
                        return Err("Recording file is empty".to_string());
                    }
                }
                Err(e) => return Err(format!("Cannot verify recording file: {}", e)),
            }
        }
    }

    println!("✅ Audio recording stopped successfully");
    Ok(())
}

async fn handle_recording_transcription(
    app_state: Arc<Mutex<AppState>>,
    app_handle: AppHandle,
    file_path: String,
    mode: RecordingMode,
) -> Result<(), String> {
    println!("🔄 Starting transcription for {:?} mode recording: {}", mode, file_path);
    
    // Get transcription service
    let transcription_service = {
        let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        state.transcription_service.clone()
            .ok_or("Transcription service not initialized")?
    };
    
    // Show transcribing status
    show_transcribing_status(&app_handle);
    
    // Perform transcription using a helper function to avoid holding locks across await
    let transcription_result = perform_transcription_safe(transcription_service.clone(), &file_path).await?;
    
    println!("✅ Transcription successful: {}", transcription_result);
    
    match mode {
        RecordingMode::Transcription => {
            // Handle regular transcription - auto-paste if enabled
            let auto_paste_enabled = {
                let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                state.settings.auto_paste
            };
            
            if auto_paste_enabled {
                // Auto-paste the transcription
                let mut enigo = Enigo::new(&Settings::default()).unwrap();
                enigo.text(&transcription_result).unwrap();
                show_transcription_success(&app_handle, &transcription_result);
            } else {
                show_transcription_success(&app_handle, &transcription_result);
            }
        }
        RecordingMode::VoiceCommand => {
            // Handle voice command transcription
            let voice_service = {
                let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                state.voice_command_service.clone()
                    .ok_or("Voice command service not initialized")?
            };
            
            // Notify voice command service of transcription
            if let Err(e) = voice_service.handle_transcription_from_system(&transcription_result, &file_path).await {
                println!("❌ Failed to notify voice command service: {}", e);
            }
        }
    }
    
    // Clean up temp file
    if let Err(e) = std::fs::remove_file(&file_path) {
        println!("⚠️ Failed to clean up temp file {}: {}", file_path, e);
    }
    
    Ok(())
}

// Legacy recording functions (kept for compatibility during transition)
// TODO: These functions will be moved to recording module
fn start_recording_hotkey(state: State<Arc<Mutex<AppState>>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    
    // Check if already recording
    {
        let app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        if app_state.is_recording {
            println!("Already recording, ignoring start request");
            return Ok(());
        }
    }
    
    println!("Starting audio recording...");
    
    // Update state to recording
    let (_device_name, device_id) = {
        let mut app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.is_recording = true;
        app_state.recording_start_time = Some(Instant::now());
        
        // Create temporary file that persists even when the NamedTempFile is dropped
        let temp_file = NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
        let temp_path = temp_file.path().to_string_lossy().to_string();
        app_state.temp_file_path = Some(temp_path.clone());
        
        // Get the underlying file handle and persist the temp file
        let (_file, persistent_path) = temp_file.keep().map_err(|e| format!("Failed to persist temp file: {}", e))?;
        let final_temp_path = persistent_path.to_string_lossy().to_string();
        app_state.temp_file_path = Some(final_temp_path.clone());
        
        println!("Created persistent temp file: {}", final_temp_path);
        
        unsafe {
            // Store the path instead of the NamedTempFile since we've persisted it
            TEMP_FILE = None; // We don't need to store the NamedTempFile anymore
            TEMP_FILE_PATH = Some(final_temp_path.clone());
        }
        
        (app_state.current_device_name.clone(), app_state.settings.selected_device_id.clone())
    };
    
    // Show recording started notification
    if let Some(app_handle) = &app_handle {
        show_recording_started(app_handle);
    }
    
    // Start audio recording
    match start_audio_recording(&device_id) {
        Ok(_) => {
            // Update current device name
            {
                let mut app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                if let Ok(devices) = get_audio_devices_sync() {
                    if let Some(device) = devices.iter().find(|d| d.id == device_id) {
                        app_state.current_device_name = device.name.clone();
                    }
                }
            }
            
            println!("Audio recording started successfully");
            Ok(())
        }
        Err(e) => {
            // Reset state on error
            {
                let mut app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                app_state.is_recording = false;
                app_state.recording_start_time = None;
                app_state.temp_file_path = None;
            }
            
            unsafe {
                TEMP_FILE = None;
                TEMP_FILE_PATH = None;
            }
            
            if let Some(app_handle) = &app_handle {
                show_transcription_error(app_handle, &e);
            }
            
            Err(e)
        }
    }
}

fn stop_recording_hotkey(state: State<Arc<Mutex<AppState>>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    
    // Check if actually recording
    let temp_file_path = {
        let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        
        if !app_state.is_recording {
            println!("Not recording, ignoring stop request");
            return Ok(());
        }
        
        // Update state
        app_state.is_recording = false;
        app_state.recording_start_time = None;
        
        app_state.temp_file_path.clone()
    };
    
    println!("Stopping audio recording...");
    
    // Stop the stream and clean up
    unsafe {
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream); // This stops the stream
            println!("Audio stream stopped");
        }
        
        if let Some(wav_writer) = WAV_WRITER.take() {
            // Extract the writer from the Arc<Mutex<...>> to finalize it
            println!("🎵 Finalizing WAV writer...");
            if let Ok(writer) = Arc::try_unwrap(wav_writer) {
                if let Ok(writer) = writer.into_inner() {
                    match writer.finalize() {
                        Ok(_) => {
                            println!("✅ WAV writer finalized successfully");
                            
                            // Double-check the file exists and has content
                            if let Some(ref file_path) = temp_file_path {
                                match std::fs::metadata(file_path) {
                                    Ok(metadata) => {
                                        println!("✅ File verified after finalization: {} bytes", metadata.len());
                                        if metadata.len() == 0 {
                                            eprintln!("⚠️ Warning: WAV file is empty after finalization");
                                        }
                                    }
                                    Err(e) => eprintln!("❌ Cannot verify file after finalization: {}", e),
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to finalize WAV file: {}", e);
                            return Err(format!("Failed to finalize WAV file: {}", e));
                        }
                    }
                } else {
                    eprintln!("❌ Failed to extract writer from mutex");
                    return Err("Failed to extract WAV writer from mutex".to_string());
                }
            } else {
                eprintln!("❌ Failed to unwrap Arc - writer may still be in use");
                return Err("WAV writer still in use, cannot finalize".to_string());
            }
        } else {
            eprintln!("⚠️ No WAV writer to finalize");
        }
    }
    
    // Check if this is a test recording (skip transcription)
    let is_test_recording = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.is_test_recording
    };
    
    if is_test_recording {
        // For test recordings, just show recording stopped and skip transcription
        if let Some(app_handle) = &app_handle {
            show_recording_stopped(app_handle);
        }
        
        // DON'T clean up temp file path yet - it's needed for playback
        // Just clear the global references to the stream and writer
        unsafe {
            TEMP_FILE = None;
            // Keep TEMP_FILE_PATH for now - it will be cleaned up later
        }
        
        println!("Test recording completed - skipping transcription");
        return Ok(());
    }
    
    // Show transcribing status for normal recordings
    if let Some(app_handle) = &app_handle {
        show_transcribing_status(app_handle);
    }
    
    // Start transcription if we have a temp file
    if let Some(temp_path) = temp_file_path {
        println!("Starting transcription for file: {}", temp_path);
        
        // Get the transcription service and settings
        let transcription_service = {
            let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
            app_state.transcription_service.clone()
        };
        
        // Perform transcription in a background task
        if let Some(service) = transcription_service {
            let app_handle_clone = app_handle.clone();
            let state_clone = state.inner().clone();
            
            tauri::async_runtime::spawn(async move {
                println!("🔄 Starting transcription thread for file: {}", temp_path);
                
                // Check file size and properties
                if let Ok(metadata) = std::fs::metadata(&temp_path) {
                    println!("📊 Audio file size: {} bytes ({:.2} MB)", 
                             metadata.len(), metadata.len() as f64 / 1024.0 / 1024.0);
                } else {
                    eprintln!("⚠️ Could not read audio file metadata");
                }
                
                println!("🌐 Starting API call to transcription service...");
                let start_time = std::time::Instant::now();
                
                // Perform transcription using the safe helper function
                let result = perform_transcription_safe(service.clone(), &temp_path).await;
                
                let duration = start_time.elapsed();
                println!("⏱️ Transcription call took: {:.2}s", duration.as_secs_f64());
                
                match result {
                    Ok(ref text) => {
                        println!("✅ Transcription successful: {}", text);
                        println!("📝 Transcribed text length: {} characters", text.len());
                        
                        // Check if this was a voice command recording by checking if voice command service is recording
                        let is_voice_command_recording = {
                            if let Ok(app_state) = state_clone.lock() {
                                if let Some(ref voice_service) = app_state.voice_command_service {
                                    if let Ok(voice_state) = voice_service.get_state() {
                                        voice_state.is_recording || voice_state.current_state == "recording"
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };
                        
                        // If this was a voice command recording, notify the voice command service
                        if is_voice_command_recording {
                            println!("🎤 This was a voice command recording, notifying voice command service");
                            if let Ok(app_state) = state_clone.lock() {
                                if let Some(ref voice_service) = app_state.voice_command_service {
                                    let voice_service_clone = Arc::clone(voice_service);
                                    let transcription_text = text.clone();
                                    let audio_path = temp_path.clone();
                                    
                                    // Notify voice command service in background
                                    tokio::spawn(async move {
                                        if let Err(e) = voice_service_clone.handle_transcription_from_system(&transcription_text, &audio_path).await {
                                            println!("❌ Failed to notify voice command service: {}", e);
                                        }
                                    });
                                }
                            }
                            
                            // For voice commands, we don't do the regular transcription processing
                            // The voice command service will handle it
                            return;
                        }
                        
                        // Try to process as voice command first
                        let mut voice_command_processed = false;
                        let (ai_agent_opt, user_servers) = {
                            if let Ok(app_state) = state_clone.lock() {
                                (app_state.ai_agent.clone(), app_state.settings.user_mcp_servers.clone())
                            } else {
                                (None, Vec::new())
                            }
                        };
                        
                        if let Some(ai_agent) = ai_agent_opt {
                            if !user_servers.is_empty() {
                                let input = crate::ai_agent::core::ConversationInput {
                                    text: text.clone(),
                                    audio_path: Some(temp_path.clone()),
                                    input_type: crate::ai_agent::core::InputType::Voice,
                                    language: None,
                                };
                                
                                // Try to process as voice command
                                if let Ok(conversation) = ai_agent.process_voice_command(input, &user_servers, None).await {
                                    if !conversation.output.actions.is_empty() {
                                        println!("🎯 Voice command processed successfully: {}", conversation.output.text);
                                        voice_command_processed = true;
                                        
                                        // Show success message for voice command
                                        if let Some(app_handle) = &app_handle_clone {
                                            show_transcription_success(app_handle, &format!("Command: {}", conversation.output.text));
                                        }
                                    }
                                }
                            }
                        }
                        
                        // If not processed as voice command, handle as regular transcription
                        if !voice_command_processed {
                            // Check if auto_paste is enabled
                            let auto_paste_enabled = {
                                if let Ok(app_state) = state_clone.lock() {
                                    app_state.settings.auto_paste
                                } else {
                                    true // Default to enabled if we can't read the setting
                                }
                            };
                        
                            if auto_paste_enabled {
                                // Use direct text input for reliable pasting at cursor position
                                let mut enigo = Enigo::new(&Settings::default()).expect("Failed to initialize Enigo");
                                if let Err(e) = enigo.text(&text) {
                                    eprintln!("Failed to type text: {}", e);
                                    if let Some(app_handle) = &app_handle_clone {
                                        show_transcription_error(app_handle, &format!("Failed to type transcribed text: {}", e));
                                    }
                                } else {
                                    // Success - don't show overlay to avoid stealing focus
                                    println!("Transcribed text typed successfully: {}", text);
                                    // Hide the transcribing overlay
                                    if let Some(app_handle) = &app_handle_clone {
                                        hide_overlay_status(app_handle);
                                    }
                                }
                            } else {
                                // Auto-paste disabled, just copy to clipboard
                                use arboard::Clipboard;
                                
                                if let Ok(mut clipboard) = Clipboard::new() {
                                    if let Err(e) = clipboard.set_text(text) {
                                        eprintln!("Failed to copy text to clipboard: {}", e);
                                        if let Some(app_handle) = &app_handle_clone {
                                            show_transcription_error(app_handle, &format!("Failed to copy text to clipboard: {}", e));
                                        }
                                    } else {
                                        println!("Transcribed text copied to clipboard: {}", text);
                                        // Use a system notification instead of overlay to avoid focus stealing
                                        show_notification("EchoType", &format!("Transcribed text copied to clipboard"), None);
                                        // Hide the transcribing overlay
                                        if let Some(app_handle) = &app_handle_clone {
                                            hide_overlay_status(app_handle);
                                        }
                                    }
                                } else {
                                    eprintln!("Failed to access clipboard");
                                    if let Some(app_handle) = &app_handle_clone {
                                        show_transcription_error(app_handle, "Failed to access clipboard");
                                    }
                                }
                            }
                        }
                    }
                    Err(ref e) => {
                        eprintln!("❌ Transcription failed: {}", e);
                        println!("🔍 Full error details: {:?}", e);
                        if let Some(app_handle) = &app_handle_clone {
                            // Provide more user-friendly error messages
                            let error_msg = if e.contains("API key") {
                                "Invalid or missing OpenAI API key. Please check your settings."
                            } else if e.contains("model") {
                                "Transcription model error. Please check your Whisper settings."
                            } else if e.contains("network") || e.contains("connection") {
                                "Network error. Please check your internet connection."
                            } else if e.contains("file") || e.contains("No such file") {
                                "Audio file error. Please try recording again."
                            } else {
                                &format!("Transcription failed: {}", e)
                            };
                            show_transcription_error(app_handle, error_msg);
                        }
                    }
                }
                
                // Clean up temp file
                if let Err(e) = std::fs::remove_file(&temp_path) {
                    eprintln!("Failed to remove temp file: {}", e);
                }
                
                // Clear temp file path from state
                if let Ok(mut app_state) = state_clone.lock() {
                    app_state.temp_file_path = None;
                }
                
                // Note: Error overlays will stay visible until manually dismissed
                // Success cases already hide the overlay above
            });
        } else {
            eprintln!("No transcription service available");
            if let Some(app_handle) = &app_handle {
                show_transcription_error(app_handle, "No transcription service configured. Please check your API key or Whisper model settings.");
            }
        }
    }
    
    // Clean up global temp file (but NOT for test recordings - they need the file for playback)
    if !is_test_recording {
        unsafe {
            TEMP_FILE = None;
            TEMP_FILE_PATH = None;
        }
    }
    
    Ok(())
}

fn force_stop_recording(state: State<Arc<Mutex<AppState>>>) -> Result<(), String> {
    println!("Force stopping recording...");
    
    // Update state
    {
        let mut app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.is_recording = false;
        app_state.recording_start_time = None;
        app_state.temp_file_path = None;
    }
    
    // Stop the stream and clean up immediately
    unsafe {
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream);
            println!("Audio stream force stopped");
        }
        
        if let Some(wav_writer) = WAV_WRITER.take() {
            // Drop the Arc to release the writer without calling finalize during force stop
            drop(wav_writer);
        }
        
        if let Some(temp_file) = TEMP_FILE.take() {
            drop(temp_file); // This will delete the temp file
            println!("Temp file cleaned up");
        }
        
        if let Some(temp_path) = TEMP_FILE_PATH.take() {
            // Clean up the persistent temp file
            if let Err(e) = std::fs::remove_file(&temp_path) {
                eprintln!("Failed to remove persistent temp file {}: {}", temp_path, e);
            } else {
                println!("Persistent temp file cleaned up: {}", temp_path);
            }
        }
    }
    
    Ok(())
}

fn monitor_alt_keys(app_handle: AppHandle) {
    use device_query::{DeviceQuery, DeviceState, Keycode};
    use std::time::{Duration, Instant};
    
    let device_state = DeviceState::new();
    let mut alt_tap_count = 0;
    let mut last_alt_tap_time: Option<Instant> = None;
    
    // Add Shift key tracking
    let mut shift_tap_count = 0;
    let mut last_shift_tap_time: Option<Instant> = None;
    
    // Track Alt key state to detect press/release transitions
    let mut alt_was_pressed = false;
    let mut alt_press_start: Option<Instant> = None;
    
    // Track Shift key state to detect press/release transitions  
    let mut shift_was_pressed = false;
    let mut shift_press_start: Option<Instant> = None;
    
    loop {
        let keys: Vec<Keycode> = device_state.get_keys();
        let alt_currently_pressed = keys.contains(&Keycode::LAlt);
        let shift_currently_pressed = keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
        let now = Instant::now();
        
        // Detect Alt key press (transition from not-pressed to pressed)
        if alt_currently_pressed && !alt_was_pressed {
            // Alt key was just pressed
            alt_press_start = Some(now);
            println!("Alt key pressed");
        }
        // Detect Alt key release (transition from pressed to not-pressed)
        else if !alt_currently_pressed && alt_was_pressed {
            // Alt key was just released
            if let Some(press_start) = alt_press_start {
                let press_duration = now.duration_since(press_start);
                
                // Only count as a tap if the press was short (< 200ms)
                if press_duration < Duration::from_millis(200) {
                    alt_tap_count += 1;
                    last_alt_tap_time = Some(now);
                    
                    println!("Alt tap detected (duration: {}ms), count: {}", 
                             press_duration.as_millis(), alt_tap_count);
                    
                    // Double tap detected
                    if alt_tap_count >= 2 {
                        alt_tap_count = 0;
                        let state = app_handle.state::<Arc<Mutex<AppState>>>();
                        
                        // Check if currently recording using RecordingManager
                        let is_recording = {
                            if let Ok(app_state) = state.inner().lock() {
                                unsafe {
                                    if let Some(ref manager) = RECORDING_MANAGER {
                                        manager.is_recording().unwrap_or(false)
                                    } else {
                                        app_state.is_recording // Fallback to legacy state
                                    }
                                }
                            } else {
                                false
                            }
                        };
                        
                        // Toggle recording using new RecordingManager
                        if is_recording {
                            println!("Double Alt-tap detected - stopping recording");
                            let app_handle_clone = app_handle.clone();
                            let state_clone = state.inner().clone();
                            
                            // Use app handle to run async function
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = stop_recording_new(state_clone, Some(app_handle_clone)).await {
                                    println!("❌ Failed to stop recording: {}", e);
                                }
                            });
                        } else {
                            println!("Double Alt-tap detected - starting transcription recording");
                            let app_handle_clone = app_handle.clone();
                            let state_clone = state.inner().clone();
                            
                            // Use app handle to run async function
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = start_transcription_recording(state_clone, Some(app_handle_clone)).await {
                                    println!("❌ Failed to start transcription recording: {}", e);
                                }
                            });
                        }
                    }
                } else {
                    println!("Alt hold detected (duration: {}ms) - ignoring", press_duration.as_millis());
                }
            }
            alt_press_start = None;
        }
        
        // === SHIFT KEY DETECTION (new logic) ===
        
        // Detect Shift key press (transition from not-pressed to pressed)
        if shift_currently_pressed && !shift_was_pressed {
            // Shift key was just pressed
            shift_press_start = Some(now);
            println!("Shift key pressed");
        }
        // Detect Shift key release (transition from pressed to not-pressed)
        else if !shift_currently_pressed && shift_was_pressed {
            // Shift key was just released
            if let Some(press_start) = shift_press_start {
                let press_duration = now.duration_since(press_start);
                
                // Only count as a tap if the press was short (< 200ms)
                if press_duration < Duration::from_millis(200) {
                    shift_tap_count += 1;
                    last_shift_tap_time = Some(now);
                    
                    println!("Shift tap detected (duration: {}ms), count: {}", 
                             press_duration.as_millis(), shift_tap_count);
                    
                    // Double tap detected
                    if shift_tap_count >= 2 {
                        shift_tap_count = 0;
                        println!("Double Shift-tap detected - showing voice commands and starting recording");
                        
                        // Show voice commands page
                        let _ = show_and_focus_main_window(&app_handle);
                        
                        // Navigate to voice commands page
                        let _ = app_handle.emit("navigate-to-voice-commands", ());
                        
                        // Start voice command recording using new RecordingManager
                        println!("🎤 Starting voice command recording");
                        let app_handle_clone = app_handle.clone();
                        let state_for_voice = app_handle.state::<Arc<Mutex<AppState>>>();
                        let state_clone = state_for_voice.inner().clone();
                        
                        // Use app handle to run async function
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = start_voice_command_recording(state_clone, Some(app_handle_clone)).await {
                                println!("❌ Failed to start voice command recording: {}", e);
                            }
                        });
                    }
                } else {
                    println!("Shift hold detected (duration: {}ms) - ignoring", press_duration.as_millis());
                }
            }
            shift_press_start = None;
        }
        
        // Update the previous states
        alt_was_pressed = alt_currently_pressed;
        shift_was_pressed = shift_currently_pressed;
        
        // Reset tap counts if too much time has passed since last tap
        if let Some(last_tap) = last_alt_tap_time {
            if now.duration_since(last_tap) > Duration::from_millis(500) {
                if alt_tap_count > 0 {
                    println!("Alt tap sequence timed out, resetting count");
                    alt_tap_count = 0;
                }
            }
        }
        
        if let Some(last_tap) = last_shift_tap_time {
            if now.duration_since(last_tap) > Duration::from_millis(500) {
                if shift_tap_count > 0 {
                    println!("Shift tap sequence timed out, resetting count");
                    shift_tap_count = 0;
                }
            }
        }
        
        std::thread::sleep(Duration::from_millis(10));
    }
}

// TODO: These commands will be moved to their respective modules
#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    
    let host = cpal::default_host();
    let input_devices = host.input_devices().map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    let mut devices = Vec::new();
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("Device {}", i));
        devices.push(AudioDevice {
            id: i.to_string(),
            name,
        });
    }
    
    println!("Found {} audio input devices", devices.len());
    for device in &devices {
        println!("  - {}: {}", device.id, device.name);
    }
    
    Ok(devices)
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
    // Update state
    {
        let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.settings = settings.clone();
    }
    
    // Save to file
    save_settings_to_file(&settings)?;
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
    app_handle: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let is_recording = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.is_recording
    };
    
    if is_recording {
        stop_recording_hotkey(state, Some(app_handle))?;
        Ok(false)
    } else {
        start_recording_hotkey(state, Some(app_handle))?;
        Ok(true)
    }
}

#[tauri::command]
async fn force_stop_recording_command(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    force_stop_recording(state)
}

#[tauri::command]
async fn reload_transcription_service(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    let config = TranscriptionConfig {
        mode: app_state.settings.transcription_mode.clone(),
        openai_api_key: if app_state.settings.api_key.is_empty() { None } else { Some(app_state.settings.api_key.clone()) },
        whisper_model_path: app_state.settings.whisper_model_path.clone(),
        whisper_model_size: app_state.settings.whisper_model_size.clone(),
        device: app_state.settings.device_type.clone(),
    };
    
    match TranscriptionService::new(config) {
        Ok(service) => {
            app_state.transcription_service = Some(Arc::new(Mutex::new(service)));
            println!("Transcription service reloaded successfully");
            Ok(())
        }
        Err(e) => Err(format!("Failed to reload transcription service: {}", e))
    }
}

#[tauri::command]
async fn get_cursor_position_command() -> Result<(i32, i32), String> {
    get_cursor_position()
}

#[tauri::command]
async fn start_recording(
    app_handle: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    start_recording_hotkey(state, Some(app_handle))
}

#[tauri::command]
async fn stop_recording(
    app_handle: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    stop_recording_hotkey(state, Some(app_handle))
}

#[tauri::command]
async fn get_last_recording_path(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<String>, String> {
    // First check if there's a test recording available
    let test_path = unsafe {
        TEST_RECORDING_FILE_PATH.clone()
    };
    
    if let Some(ref test_path_str) = test_path {
        println!("🎵 Returning test recording path: {}", test_path_str);
        
        // Verify test recording file
        if let Ok(metadata) = std::fs::metadata(test_path_str) {
            println!("📁 Test file exists, size: {} bytes", metadata.len());
            if metadata.len() > 0 {
                return Ok(test_path);
            } else {
                println!("⚠️ Test recording file is empty");
                return Err("Test recording file is empty".to_string());
            }
        } else {
            println!("⚠️ Test recording file does not exist");
            return Err("Test recording file does not exist".to_string());
        }
    }
    
    // Fall back to main recording path
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    let path = app_state.temp_file_path.clone();
    
    if let Some(ref path_str) = path {
        println!("🎵 Returning main recording path: {}", path_str);
        
        // Check if file exists and get its size
        if let Ok(metadata) = std::fs::metadata(path_str) {
            println!("📁 File exists, size: {} bytes", metadata.len());
            
            // Try to read the first few bytes to test accessibility
            match std::fs::File::open(path_str) {
                Ok(_) => println!("✅ File can be opened for reading"),
                Err(e) => println!("❌ File cannot be opened: {}", e),
            }
        } else {
            println!("⚠️ File does not exist or cannot be accessed");
            return Err("Recording file does not exist".to_string());
        }
    } else {
        println!("❌ No recording path available");
    }
    
    Ok(path)
}

#[tauri::command]
async fn start_test_recording(
    device_id: String,
) -> Result<(), String> {
    println!("🎬 Starting test recording with device: {}", device_id);
    
    // Use the simple test recording system with the provided device_id
    match start_simple_test_recording(&device_id) {
        Ok(file_path) => {
            println!("Test recording started successfully: {}", file_path);
            Ok(())
        }
        Err(e) => Err(e)
    }
}

#[tauri::command]
async fn stop_test_recording() -> Result<(), String> {
    match stop_simple_test_recording() {
        Ok(file_path) => {
            if let Some(path) = file_path {
                println!("Test recording stopped successfully: {}", path);
            }
            Ok(())
        }
        Err(e) => Err(e)
    }
}

#[tauri::command]
async fn cleanup_test_recording() -> Result<(), String> {
    cleanup_simple_test_recording()
}

#[tauri::command]
async fn verify_audio_playback(file_path: String) -> Result<String, String> {
    // Try to read the WAV file and verify its format
    match std::fs::File::open(&file_path) {
        Ok(mut file) => {
            use std::io::Read;
            let mut header = [0u8; 44]; // WAV header is typically 44 bytes
            
            match file.read_exact(&mut header) {
                Ok(_) => {
                    // Check for RIFF header
                    if &header[0..4] != b"RIFF" {
                        return Err("Not a valid RIFF file".to_string());
                    }
                    
                    // Check for WAVE format
                    if &header[8..12] != b"WAVE" {
                        return Err("Not a valid WAVE file".to_string());
                    }
                    
                    // Extract basic format info
                    let channels = u16::from_le_bytes([header[22], header[23]]);
                    let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
                    let bits_per_sample = u16::from_le_bytes([header[34], header[35]]);
                    
                    let info = format!(
                        "Valid WAV: {}Hz, {} channels, {} bits - Size: {} bytes", 
                        sample_rate, 
                        channels, 
                        bits_per_sample,
                        std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0)
                    );
                    
                    // Check for browser compatibility
                    let mut warnings = Vec::new();
                    if sample_rate != 44100 && sample_rate != 48000 {
                        warnings.push(format!("Sample rate {} may not be browser-compatible", sample_rate));
                    }
                    if channels > 2 {
                        warnings.push(format!("{} channels may not be browser-compatible", channels));
                    }
                    if bits_per_sample != 16 {
                        warnings.push(format!("{} bits per sample may not be browser-compatible", bits_per_sample));
                    }
                    
                    if warnings.is_empty() {
                        Ok(info)
                    } else {
                        Ok(format!("{} - Warnings: {}", info, warnings.join(", ")))
                    }
                }
                Err(e) => Err(format!("Failed to read WAV header: {}", e)),
            }
        }
        Err(e) => Err(format!("Failed to open file: {}", e)),
    }
}

// MCP Commands
#[tauri::command]
async fn get_mcp_servers(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<crate::ai_agent::integrations::UserMcpServer>, String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(app_state.settings.user_mcp_servers.clone())
}

#[tauri::command]
async fn save_mcp_servers(
    servers: Vec<crate::ai_agent::integrations::UserMcpServer>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    {
        let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.settings.user_mcp_servers = servers;
    }
    
    // Save to file
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    crate::settings::save_settings_to_file(&app_state.settings)
        .map_err(|e| format!("Failed to save settings: {}", e))?;
    
    Ok(())
}

#[tauri::command]
async fn connect_mcp_server(
    server_name: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mcp_client = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.mcp_client.clone()
    };
    
    if let Some(mcp_client) = mcp_client {
        mcp_client.connect_server(&server_name).await
            .map_err(|e| format!("Failed to connect to MCP server: {}", e))?;
    } else {
        return Err("MCP client not initialized".to_string());
    }
    
    Ok(())
}

#[tauri::command]
async fn disconnect_mcp_server(
    server_name: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mcp_client = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.mcp_client.clone()
    };
    
    if let Some(mcp_client) = mcp_client {
        mcp_client.disconnect_server(&server_name).await
            .map_err(|e| format!("Failed to disconnect from MCP server: {}", e))?;
    } else {
        return Err("MCP client not initialized".to_string());
    }
    
    Ok(())
}

#[tauri::command]
async fn get_available_mcp_tools(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<std::collections::HashMap<String, Vec<crate::mcp::protocol::McpTool>>, String> {
    let mcp_client = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.mcp_client.clone()
    };
    
    if let Some(mcp_client) = mcp_client {
        Ok(mcp_client.get_all_tools().await)
    } else {
        Ok(std::collections::HashMap::new())
    }
}

// Voice Command Commands
#[tauri::command]
async fn get_voice_command_state(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<voice_command::VoiceCommandState, String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    if let Some(ref voice_service) = app_state.voice_command_service {
        voice_service.get_state()
    } else {
        Ok(voice_command::VoiceCommandState {
            is_recording: false,
            is_processing: false,
            current_state: "idle".to_string(),
            recording_start_time: None,
        })
    }
}

#[tauri::command]
async fn get_voice_command_messages(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<voice_command::VoiceCommandMessage>, String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    if let Some(ref voice_service) = app_state.voice_command_service {
        voice_service.get_messages()
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
async fn clear_voice_command_messages(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
    
    if let Some(ref voice_service) = app_state.voice_command_service {
        voice_service.clear_messages()
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn process_text_command(
    command_text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let service_clone = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        
        if let Some(ref voice_service) = app_state.voice_command_service {
            Arc::clone(voice_service)
        } else {
            return Err("Voice command service not available".to_string());
        }
    }; // app_state is dropped here
    
    service_clone.process_text_command(&command_text).await
}

#[tauri::command]
async fn start_voice_command_test(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let service_clone = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        
        if let Some(ref voice_service) = app_state.voice_command_service {
            Arc::clone(voice_service)
        } else {
            return Err("Voice command service not available".to_string());
        }
    }; // app_state is dropped here
    
    service_clone.start_voice_command().await
}

#[tauri::command]
async fn start_voice_recording(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let service_clone = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        
        if let Some(ref voice_service) = app_state.voice_command_service {
            Arc::clone(voice_service)
        } else {
            return Err("Voice command service not available".to_string());
        }
    }; // app_state is dropped here
    
    service_clone.start_voice_recording().await
}

#[tauri::command]
async fn process_voice_command_mcp(
    command_text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<crate::ai_agent::integrations::IntegrationResult>, String> {
    let (ai_agent, user_servers) = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        (app_state.ai_agent.clone(), app_state.settings.user_mcp_servers.clone())
    };
    
    if let Some(ai_agent) = ai_agent {
        let input = crate::ai_agent::core::ConversationInput {
            text: command_text,
            audio_path: None,
            input_type: crate::ai_agent::core::InputType::Voice,
            language: None,
        };
        
        match ai_agent.process_voice_command(input, &user_servers, None).await {
            Ok(conversation) => {
                // Extract the result from the conversation
                if !conversation.output.actions.is_empty() {
                    let action = &conversation.output.actions[0];
                    if let (Some(server_name), Some(tool_name)) = (&action.mcp_server, &action.tool_name) {
                        return Ok(Some(crate::ai_agent::integrations::IntegrationResult {
                            success: true,
                            message: conversation.output.text,
                            data: None,
                            server_name: server_name.clone(),
                            tool_name: tool_name.clone(),
                        }));
                    }
                }
                Ok(None)
            }
            Err(e) => Err(format!("Failed to process voice command: {}", e)),
        }
    } else {
        Err("AI agent not initialized".to_string())
    }
}

#[tauri::command]
async fn install_mcp_package(
    package_name: String,
) -> Result<String, String> {
    // Determine the package manager and install command
    let install_command = if package_name.starts_with("@") || package_name.contains("/") {
        // Scoped package or GitHub package
        format!("bun add -g {}", package_name)
    } else {
        // Regular package
        format!("bun add -g {}", package_name)
    };

    // Execute the installation
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&install_command)
        .output()
        .await
        .map_err(|e| format!("Failed to execute install command: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(format!("Successfully installed {}\n{}", package_name, stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Installation failed for {}: {}", package_name, stderr))
    }
}

#[tauri::command]
async fn get_audio_file_info(
    file_path: String,
) -> Result<serde_json::Value, String> {
    use std::fs;
    
    // Check if file exists
    let metadata = fs::metadata(&file_path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    
    let file_size = metadata.len();
    
    // Try to read WAV header to get audio format info
    let audio_info = match std::fs::read(&file_path) {
        Ok(data) if data.len() >= 44 => {
            // Basic WAV header parsing (simplified)
            if &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
                // Find fmt chunk
                if let Some(fmt_pos) = data.windows(4).position(|w| w == b"fmt ") {
                    let fmt_start = fmt_pos + 8; // Skip "fmt " and size
                    if data.len() >= fmt_start + 16 {
                        // Read format info
                        let audio_format = u16::from_le_bytes([data[fmt_start], data[fmt_start + 1]]);
                        let channels = u16::from_le_bytes([data[fmt_start + 2], data[fmt_start + 3]]);
                        let sample_rate = u32::from_le_bytes([
                            data[fmt_start + 4], data[fmt_start + 5], 
                            data[fmt_start + 6], data[fmt_start + 7]
                        ]);
                        let bits_per_sample = u16::from_le_bytes([data[fmt_start + 14], data[fmt_start + 15]]);
                        
                        serde_json::json!({
                            "format": "WAV",
                            "audio_format": audio_format,
                            "channels": channels,
                            "sample_rate": sample_rate,
                            "bits_per_sample": bits_per_sample,
                            "is_pcm": audio_format == 1
                        })
                    } else {
                        serde_json::json!({
                            "format": "WAV",
                            "error": "Incomplete format chunk"
                        })
                    }
                } else {
                    serde_json::json!({
                        "format": "WAV",
                        "error": "No format chunk found"
                    })
                }
            } else {
                serde_json::json!({
                    "format": "Unknown",
                    "error": "Not a WAV file"
                })
            }
        }
        Ok(_) => serde_json::json!({
            "format": "Unknown",
            "error": "File too small"
        }),
        Err(e) => serde_json::json!({
            "format": "Unknown",
            "error": format!("Failed to read file: {}", e)
        })
    };
    
    Ok(serde_json::json!({
        "file_path": file_path,
        "file_size": file_size,
        "audio_info": audio_info
    }))
}

#[tauri::command]
async fn play_audio_file_native(file_path: String) -> Result<String, String> {
    use std::process::Command;
    
    // Check if file exists first
    if !std::path::Path::new(&file_path).exists() {
        return Err("Audio file does not exist".to_string());
    }
    
    println!("🔊 Playing audio file natively: {}", file_path);
    
    // Try different audio players based on the OS
    #[cfg(target_os = "linux")]
    {
        // Try common Linux audio players in order of preference
        let players = [
            ("paplay", vec![file_path.as_str()]), // PulseAudio
            ("aplay", vec![file_path.as_str()]),  // ALSA
            ("ffplay", vec!["-nodisp", "-autoexit", file_path.as_str()]), // FFmpeg
            ("mplayer", vec![file_path.as_str()]), // MPlayer
            ("vlc", vec!["--intf", "dummy", "--play-and-exit", file_path.as_str()]), // VLC
            ("mpv", vec!["--no-video", file_path.as_str()]), // MPV
        ];
        
        for (player, args) in &players {
            println!("🎵 Trying audio player: {} with args: {:?}", player, args);
            
            match Command::new(player)
                .args(args)
                .spawn()
            {
                Ok(mut child) => {
                    println!("✅ Successfully started {} for audio playback", player);
                    
                    // Wait for the player to finish in a non-blocking way
                    match child.wait() {
                        Ok(status) => {
                            if status.success() {
                                return Ok(format!("Audio played successfully using {}", player));
                            } else {
                                println!("⚠️ {} exited with status: {}", player, status);
                            }
                        }
                        Err(e) => {
                            println!("⚠️ Error waiting for {}: {}", player, e);
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️ Failed to start {}: {}", player, e);
                    continue; // Try next player
                }
            }
        }
        
        Err("No suitable audio player found. Please install paplay, aplay, ffplay, mplayer, vlc, or mpv.".to_string())
    }
    
    #[cfg(target_os = "windows")]
    {
        // Windows: Use PowerShell with Windows Media Player or built-in player
        match Command::new("powershell")
            .args(&[
                "-Command",
                &format!("(New-Object Media.SoundPlayer '{}').PlaySync()", file_path)
            ])
            .spawn()
        {
            Ok(mut child) => {
                match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            Ok("Audio played successfully using Windows Media Player".to_string())
                        } else {
                            Err(format!("Windows Media Player failed with status: {}", status))
                        }
                    }
                    Err(e) => Err(format!("Error waiting for Windows Media Player: {}", e))
                }
            }
            Err(e) => Err(format!("Failed to start Windows Media Player: {}", e))
        }
    }
    
    #[cfg(target_os = "macos")]
    {
        // macOS: Use afplay (built-in audio player)
        match Command::new("afplay")
            .arg(&file_path)
            .spawn()
        {
            Ok(mut child) => {
                match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            Ok("Audio played successfully using afplay".to_string())
                        } else {
                            Err(format!("afplay failed with status: {}", status))
                        }
                    }
                    Err(e) => Err(format!("Error waiting for afplay: {}", e))
                }
            }
            Err(e) => Err(format!("Failed to start afplay: {}", e))
        }
    }
}

#[tauri::command]
async fn stop_audio_playback() -> Result<String, String> {
    use std::process::Command;
    
    println!("🛑 Stopping all audio playback processes...");
    
    let mut stopped_processes = Vec::new();
    
    #[cfg(target_os = "linux")]
    {
        // Kill common audio players that might be running
        let players = ["paplay", "aplay", "ffplay", "mplayer", "vlc", "mpv"];
        
        for player in &players {
            match Command::new("pkill")
                .arg("-f")
                .arg(player)
                .output()
            {
                Ok(output) => {
                    if output.status.success() {
                        println!("✅ Stopped {} processes", player);
                        stopped_processes.push(*player);
                    }
                }
                Err(e) => {
                    println!("⚠️ Failed to stop {}: {}", player, e);
                }
            }
        }
        
        // Also try to kill any processes playing WAV files
        match Command::new("pkill")
            .arg("-f")
            .arg(".wav")
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    println!("✅ Stopped processes playing .wav files");
                    stopped_processes.push("wav-players");
                }
            }
            Err(e) => {
                println!("⚠️ Failed to stop wav players: {}", e);
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        // Windows: Kill Windows Media Player and any PowerShell audio processes
        let processes = ["wmplayer", "powershell"];
        
        for process in &processes {
            match Command::new("taskkill")
                .args(&["/F", "/IM", &format!("{}.exe", process)])
                .output()
            {
                Ok(output) => {
                    if output.status.success() {
                        println!("✅ Stopped {} processes", process);
                        stopped_processes.push(*process);
                    }
                }
                Err(e) => {
                    println!("⚠️ Failed to stop {}: {}", process, e);
                }
            }
        }
    }
    
    #[cfg(target_os = "macos")]
    {
        // macOS: Kill afplay processes
        match Command::new("pkill")
            .arg("afplay")
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    println!("✅ Stopped afplay processes");
                    stopped_processes.push("afplay");
                }
            }
            Err(e) => {
                println!("⚠️ Failed to stop afplay: {}", e);
            }
        }
    }
    
    if stopped_processes.is_empty() {
        Ok("No audio playback processes were running".to_string())
    } else {
        Ok(format!("Stopped audio playback: {}", stopped_processes.join(", ")))
    }
}

// Voice Command Commands


