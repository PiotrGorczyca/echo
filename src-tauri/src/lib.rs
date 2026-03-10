// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod transcription;
mod voice_activation;
mod rocm_detection;
mod mcp;
mod ai_agent;
mod voice_command;
mod recording_manager;
mod openai_client;
mod meeting;
mod audio_capture;
mod history;

// Task management & Claude Code integration
mod tasks;
mod workspace;
mod claude;

// New modular structure
mod state;
mod settings;
mod commands;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::Instant;
use std::io::BufWriter;
use std::fs::File;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, SampleFormat, SupportedStreamConfig};
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

/// Get the preferred audio host. On Linux, use ALSA directly to skip JACK probing
/// which adds ~3-5s delay even when JACK isn't running.
fn preferred_audio_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    {
        cpal::host_from_id(cpal::HostId::Alsa).unwrap_or_else(|_| cpal::default_host())
    }
    #[cfg(not(target_os = "linux"))]
    {
        cpal::default_host()
    }
}
use commands::window::show_and_focus_main_window;
use transcription::{TranscriptionService, TranscriptionConfig};
use recording_manager::{RecordingManager, RecordingMode, RecordingConfig};
use history::HistoryManager;

// Helper function to perform transcription using the shared service directly.
// Since TranscriptionService::transcribe takes &self and backends are Send + Sync,
// we can call it through Arc without any Mutex.
async fn perform_transcription(
    service: Arc<TranscriptionService>,
    file_path: &str
) -> Result<String, String> {
    service.transcribe(file_path).await
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

/// Set to true while paste_text is typing — hotkey detection should be suppressed.
static IS_TYPING: AtomicBool = AtomicBool::new(false);

// Cached audio device — pre-initialized at startup to avoid slow ALSA probing on each recording.
use std::sync::OnceLock;

struct CachedAudioDevice {
    device: cpal::Device,
    config: SupportedStreamConfig,
}

static CACHED_AUDIO: OnceLock<Mutex<Option<CachedAudioDevice>>> = OnceLock::new();

/// Pre-warm the audio device in a background thread. Called once at startup.
fn warm_up_audio_device(device_id: &str) {
    let device_id = device_id.to_string();
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        println!("[audio] Pre-warming audio device...");
        match resolve_audio_device(&device_id) {
            Ok(cached) => {
                let name = cached.device.name().unwrap_or_default();
                let lock = CACHED_AUDIO.get_or_init(|| Mutex::new(None));
                *lock.lock().unwrap() = Some(cached);
                println!("[audio] Device '{}' pre-warmed in {:.0}ms", name, t.elapsed().as_millis());
            }
            Err(e) => {
                eprintln!("[audio] Failed to pre-warm device: {}", e);
            }
        }
    });
}

/// Resolve a device by ID (index string or default). This is the slow part — ALSA enumeration.
fn resolve_audio_device(device_id: &str) -> Result<CachedAudioDevice, String> {
    let host = preferred_audio_host();
    let device = if let Ok(device_index) = device_id.parse::<usize>() {
        let devices: Vec<_> = host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?
            .collect();
        devices.into_iter().nth(device_index)
            .unwrap_or_else(|| {
                println!("[audio] Device index {} not found, using default", device_index);
                host.default_input_device().unwrap()
            })
    } else {
        host.default_input_device()
            .ok_or("No default input device available")?
    };
    let config = device.default_input_config()
        .map_err(|e| format!("Failed to get device config: {}", e))?;
    Ok(CachedAudioDevice { device, config })
}

/// Invalidate the cached audio device (e.g. when the user changes device in settings).
pub fn invalidate_audio_cache() {
    if let Some(lock) = CACHED_AUDIO.get() {
        if let Ok(mut guard) = lock.lock() {
            *guard = None;
        }
    }
}

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
    
    let host = preferred_audio_host();
    
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
    let native_sample_rate = config.sample_rate().0;
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

    // Create WAV writer using the device's native sample rate to avoid distortion
    let spec = WavSpec {
        channels: 1, // Mono for compatibility
        sample_rate: native_sample_rate,
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
    // Use the device's native sample rate to avoid distortion from rate mismatch.
    // The WAV header must match the actual audio data rate.
    let actual_channels = if channels > 2 {
        println!("Converting from {} channels to stereo for compatibility", channels);
        2
    } else {
        channels
    };

    let spec = WavSpec {
        channels: actual_channels,
        sample_rate: sample_rate.0,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    println!("Creating WAV file with spec: {}Hz, {} channels, 16-bit PCM",
             sample_rate.0, actual_channels);

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

fn position_overlay_near_cursor(overlay_window: &tauri::WebviewWindow) -> Result<(), String> {
    // Get cursor position
    let (cursor_x, cursor_y) = get_cursor_position()?;

    // Overlay dimensions (from config)
    let overlay_width = 400;
    let overlay_height = 120;

    // Offset from cursor (show below and to the right of cursor)
    let offset_x = 20;
    let offset_y = 20;

    // Calculate initial overlay position
    let overlay_x = cursor_x + offset_x;
    let overlay_y = cursor_y + offset_y;

    println!("Cursor position: ({}, {})", cursor_x, cursor_y);
    println!("Initial overlay position: ({}, {})", overlay_x, overlay_y);

    // Get all available monitors and find which one contains the cursor
    if let Ok(monitors) = overlay_window.available_monitors() {
        let mut cursor_monitor = None;

        // Find the monitor that contains the cursor
        for monitor in monitors {
            let monitor_size = monitor.size();
            let monitor_pos = monitor.position();

            let monitor_left = monitor_pos.x;
            let monitor_right = monitor_pos.x + monitor_size.width as i32;
            let monitor_top = monitor_pos.y;
            let monitor_bottom = monitor_pos.y + monitor_size.height as i32;

            println!("Checking monitor: pos=({}, {}), size={}x{}, bounds=[{},{},{},{}]",
                     monitor_pos.x, monitor_pos.y,
                     monitor_size.width, monitor_size.height,
                     monitor_left, monitor_top, monitor_right, monitor_bottom);

            // Check if cursor is within this monitor's bounds
            if cursor_x >= monitor_left && cursor_x < monitor_right &&
               cursor_y >= monitor_top && cursor_y < monitor_bottom {
                cursor_monitor = Some(monitor);
                println!("✓ Cursor is on this monitor!");
                break;
            }
        }

        // Use the monitor containing the cursor, or fall back to current monitor
        let monitor = if let Some(m) = cursor_monitor {
            m
        } else if let Ok(Some(m)) = overlay_window.current_monitor() {
            println!("⚠ Could not determine cursor monitor, using overlay's current monitor");
            m
        } else {
            println!("⚠ Could not determine any monitor, using primary");
            if let Ok(Some(m)) = overlay_window.primary_monitor() {
                m
            } else {
                return Err("Could not determine any monitor".to_string());
            }
        };

        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();

        println!("Selected monitor: pos=({}, {}), size={}x{}",
                 monitor_pos.x, monitor_pos.y, monitor_size.width, monitor_size.height);

        let monitor_right = monitor_pos.x + monitor_size.width as i32;
        let monitor_bottom = monitor_pos.y + monitor_size.height as i32;

        println!("Monitor bounds: right={}, bottom={}", monitor_right, monitor_bottom);

        // Start with cursor position + offset
        let mut final_x = cursor_x + offset_x;
        let mut final_y = cursor_y + offset_y;

        println!("Target position before bounds check: ({}, {}) + overlay size ({}x{})",
                 final_x, final_y, overlay_width, overlay_height);
        println!("Checking against monitor bounds: right={}, bottom={}", monitor_right, monitor_bottom);
        println!("Right edge test: {} + {} = {} > {} ? {}",
                 final_x, overlay_width, final_x + overlay_width, monitor_right,
                 (final_x + overlay_width) > monitor_right);
        println!("Bottom edge test: {} + {} = {} > {} ? {}",
                 final_y, overlay_height, final_y + overlay_height, monitor_bottom,
                 (final_y + overlay_height) > monitor_bottom);

        // Check if overlay would go off the right edge
        if (final_x + overlay_width) > monitor_right {
            // Position to the left of cursor instead
            final_x = cursor_x - overlay_width - offset_x;
            println!("Would go off right edge, positioning left of cursor: x={}", final_x);

            // If still off screen, clamp to monitor bounds
            if final_x < monitor_pos.x {
                final_x = monitor_pos.x + 20;
                println!("Still off left edge, clamping to monitor left + 20: x={}", final_x);
            }
        }

        // Check if overlay would go off the bottom edge
        if (final_y + overlay_height) > monitor_bottom {
            // Position above cursor instead
            final_y = cursor_y - overlay_height - offset_y;
            println!("Would go off bottom edge, positioning above cursor: y={}", final_y);

            // If still off screen, clamp to monitor bounds
            if final_y < monitor_pos.y {
                final_y = monitor_pos.y + 20;
                println!("Still off top edge, clamping to monitor top + 20: y={}", final_y);
            }
        }

        // Final safety checks - ensure we're within monitor bounds
        if final_x < monitor_pos.x {
            final_x = monitor_pos.x + 20;
            println!("Final safety: adjusted x to monitor left + 20: {}", final_x);
        }
        if final_y < monitor_pos.y {
            final_y = monitor_pos.y + 20;
            println!("Final safety: adjusted y to monitor top + 20: {}", final_y);
        }
        if (final_x + overlay_width) > monitor_right {
            final_x = monitor_right - overlay_width - 20;
            println!("Final safety: adjusted x to fit within right edge: {}", final_x);
        }
        if (final_y + overlay_height) > monitor_bottom {
            final_y = monitor_bottom - overlay_height - 20;
            println!("Final safety: adjusted y to fit within bottom edge: {}", final_y);
        }

        // Set the position
        use tauri::PhysicalPosition;
        let position = PhysicalPosition::new(final_x, final_y);

        println!("Setting overlay position to: ({}, {})", final_x, final_y);

        if let Err(e) = overlay_window.set_position(position) {
            eprintln!("Failed to set overlay position: {}", e);
            return Err(format!("Failed to set overlay position: {}", e));
        } else {
            println!("✓ Overlay positioned successfully at ({}, {})", final_x, final_y);
        }
    } else {
        return Err("Could not get available monitors".to_string());
    }

    Ok(())
}

pub fn show_overlay_status(app_handle: &AppHandle, message: &str, status_type: &str) {
    println!("Attempting to show overlay status: {} - {}", status_type, message);

    // Get overlay window
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        println!("Found overlay window, attempting to show");

        // Position overlay near cursor before showing it
        if let Err(e) = position_overlay_near_cursor(&overlay_window) {
            eprintln!("Failed to position overlay near cursor: {}", e);
        }

        // Show the window (without stealing focus)
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
        show_notification("Echo", message, None);
    }
}

pub fn hide_overlay_status(app_handle: &AppHandle) {
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        // Send idle status to trigger frontend hide logic
        let status_update = StatusUpdate {
            message: "".to_string(),
            r#type: "idle".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        };

        let _ = overlay_window.emit("status-update", status_update);
        let _ = overlay_window.hide();
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

/// Copy text to the system clipboard reliably.
/// On Wayland uses wl-copy (forks a daemon to serve requests), falls back to arboard.
async fn set_clipboard_reliable(text: &str) {
    if cfg!(target_os = "linux") {
        // wl-copy forks a background daemon — clipboard survives after our process moves on
        if let Ok(mut child) = tokio::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(text.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }
            if let Ok(status) = child.wait().await {
                if status.success() {
                    println!("Clipboard set via wl-copy");
                    return;
                }
            }
        }

        // xclip fallback (X11)
        if let Ok(mut child) = tokio::process::Command::new("xclip")
            .arg("-selection").arg("clipboard")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(text.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }
            if let Ok(status) = child.wait().await {
                if status.success() {
                    println!("Clipboard set via xclip");
                    return;
                }
            }
        }
    }

    // arboard fallback (all platforms, but unreliable on Wayland)
    use arboard::Clipboard;
    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(text);
        println!("Clipboard set via arboard");
    }
}

/// Insert text into the currently focused window.
/// Sets clipboard (wl-copy), detects window type (xprop, ~4ms),
/// then simulates the right paste shortcut (Ctrl+Shift+V for terminals, Ctrl+V otherwise).
/// Clipboard is always available — user can paste manually if auto-paste fails.
async fn paste_text(text: &str) -> Result<(), String> {
    let start = std::time::Instant::now();

    // Set clipboard — must complete before paste shortcut
    set_clipboard_reliable(text).await;

    IS_TYPING.store(true, Ordering::SeqCst);

    // Detect terminal via xprop (~4ms, no user interaction)
    let shortcut = detect_paste_shortcut().await;

    let result = tokio::process::Command::new("xdotool")
        .arg("key").arg("--clearmodifiers").arg(shortcut)
        .output().await;

    let paste_result = match result {
        Ok(output) if output.status.success() => {
            println!("Pasted via clipboard + {} in {:.0?}", shortcut, start.elapsed());
            Ok(())
        }
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            Err(format!("Paste failed: {}. Text is in clipboard.", err))
        }
        Err(e) => Err(format!("xdotool not found: {}. Text is in clipboard.", e)),
    };

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    IS_TYPING.store(false, Ordering::SeqCst);
    paste_result
}

/// Detect if the focused window is a terminal and return the right paste shortcut.
/// Uses xprop to read WM_CLASS (~4ms, no user interaction, works on X11 and XWayland).
async fn detect_paste_shortcut() -> &'static str {
    const TERMINAL_CLASSES: &[&str] = &[
        "konsole", "alacritty", "kitty", "foot", "wezterm", "terminator",
        "tilix", "gnome-terminal", "xterm", "urxvt", "st-256color",
        "yakuake", "guake", "terminology", "sakura", "xfce4-terminal",
    ];

    // Step 1: get active window ID from root window property
    let winid = match tokio::process::Command::new("xprop")
        .args(["-root", "_NET_ACTIVE_WINDOW"])
        .output().await
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            // Parse "window id # 0x2400001"
            text.split_whitespace()
                .last()
                .filter(|s| s.starts_with("0x") && *s != "0x0")
                .map(|s| s.to_string())
        }
        _ => None,
    };

    // Step 2: get WM_CLASS of that window
    if let Some(id) = winid {
        if let Ok(output) = tokio::process::Command::new("xprop")
            .args(["-id", &id, "WM_CLASS"])
            .output().await
        {
            if output.status.success() {
                let class = String::from_utf8_lossy(&output.stdout).to_lowercase();
                println!("Active window class: {}", class.trim());

                if class.contains("not found") {
                    // No WM_CLASS = native Wayland window (e.g. Konsole)
                    // Use Ctrl+Shift+V which works in terminals and most Wayland-native apps
                    println!("Native Wayland window detected, using ctrl+shift+v");
                    return "ctrl+shift+v";
                }

                if TERMINAL_CLASSES.iter().any(|t| class.contains(t)) {
                    return "ctrl+shift+v";
                }
                return "ctrl+v";
            }
        }
    }

    "ctrl+v"
}

// Simple transcription status functions show a small mini indicator near the cursor.
// The overlay frontend renders a compact bar for these status types.
// Crucially, the mini overlay does NOT call setFocus, so it won't steal focus.

fn show_recording_started(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "Recording...", "recording");
}

fn show_recording_stopped(_app_handle: &AppHandle) {
    println!("Recording stopped");
}

fn show_transcribing_status(app_handle: &AppHandle) {
    show_overlay_status(app_handle, "Transcribing...", "transcribing");
}

fn show_transcription_success(app_handle: &AppHandle, text: &str) {
    let preview = if text.len() > 50 {
        format!("{}...", text.chars().take(50).collect::<String>())
    } else {
        text.to_string()
    };
    show_overlay_status(app_handle, &preview, "success");
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

/// Read ALSA card descriptions from /proc/asound/cards for friendly names.
fn get_alsa_card_descriptions() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/proc/asound/cards") {
        // Format: " 3 [V3             ]: USB-Audio - Creative Live! Cam Sync V3\n"
        //         "                      Creative Technology Ltd Creative Live!..."
        for line in content.lines() {
            let trimmed = line.trim();
            // Match lines like " 3 [V3             ]: USB-Audio - Creative Live! Cam Sync V3"
            if let Some(bracket_start) = trimmed.find('[') {
                if let Some(bracket_end) = trimmed.find(']') {
                    let card_id = trimmed[bracket_start + 1..bracket_end].trim().to_string();
                    // Get the friendly name after " - "
                    if let Some(dash_pos) = trimmed.find(" - ") {
                        let friendly = trimmed[dash_pos + 3..].trim().to_string();
                        map.insert(card_id, friendly);
                    }
                }
            }
        }
    }
    map
}

/// Convert a raw ALSA device name to a user-friendly name.
fn friendly_device_name(alsa_name: &str, card_descriptions: &std::collections::HashMap<String, String>) -> Option<String> {
    // Skip virtual/routing devices that are just PipeWire/PulseAudio passthroughs
    match alsa_name {
        "pipewire" | "pulse" | "default" => {
            return Some(format!("System Default ({})", alsa_name));
        }
        _ => {}
    }

    // Extract CARD=xxx from names like "sysdefault:CARD=V3" or "front:CARD=Generic_1,DEV=0"
    if let Some(card_pos) = alsa_name.find("CARD=") {
        let after_card = &alsa_name[card_pos + 5..];
        let card_id = after_card.split(&[',', ' ', ':'][..]).next().unwrap_or(after_card);
        if let Some(friendly) = card_descriptions.get(card_id) {
            // Determine the ALSA device type prefix
            let prefix = alsa_name.split(':').next().unwrap_or("");
            let qualifier = match prefix {
                "sysdefault" | "hw" => "",
                "front" => " (Front)",
                "surround40" => " (Surround 4.0)",
                "surround51" => " (Surround 5.1)",
                "surround71" => " (Surround 7.1)",
                _ => "",
            };
            // Skip surround variants for input devices — they're not useful
            if prefix.starts_with("surround") {
                return None;
            }
            return Some(format!("{}{}", friendly, qualifier));
        }
    }

    Some(alsa_name.to_string())
}

fn get_audio_devices_sync() -> Result<Vec<AudioDevice>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = preferred_audio_host();
    let input_devices = host.input_devices().map_err(|e| format!("Failed to get input devices: {}", e))?;
    let card_descriptions = get_alsa_card_descriptions();

    let mut devices = Vec::new();
    for (i, device) in input_devices.enumerate() {
        let raw_name = device.name().unwrap_or_else(|_| format!("Device {}", i));
        if let Some(friendly) = friendly_device_name(&raw_name, &card_descriptions) {
            devices.push(AudioDevice {
                id: i.to_string(),
                name: friendly,
            });
        }
    }

    println!("Audio devices:");
    for d in &devices {
        println!("  [{}] {}", d.id, d.name);
    }

    Ok(devices)
}

fn start_audio_recording(device_id: &str) -> Result<(), String> {
    
    let host = preferred_audio_host();
    
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
    // Prevent cpal/ALSA from trying to connect to JACK (which adds ~1s delay if not running)
    std::env::set_var("JACK_NO_START_SERVER", "1");
    std::env::set_var("JACK_NO_AUDIO_RESERVATION", "1");

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
            let service_arc = Arc::new(service);
            initial_state.transcription_service = Some(service_arc.clone());

            // Meeting service API key will be set after Tauri runtime starts
            println!("📋 Meeting service API key will be initialized after Tauri starts");

            println!("✅ Transcription service initialized on startup with saved settings");
            println!("   Meeting service will be connected when reload_transcription_service is called");
        }
        Err(e) => {
            eprintln!("⚠️  Failed to initialize transcription service on startup: {}", e);
            println!("   Service will be initialized when settings are saved");

            // Meeting service API key will be set after Tauri runtime starts
            println!("📋 Meeting service API key will be initialized after Tauri starts");
        }
    }

    // Initialize history manager
    let history_path = dirs::config_dir()
        .map(|p| p.join("echo").join("history.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("echo_history.json"));

    match HistoryManager::new(history_path) {
        Ok(manager) => {
            initial_state.history_manager = Some(Arc::new(manager));
            println!("✅ Transcription history manager initialized");
        }
        Err(e) => {
            eprintln!("⚠️  Failed to initialize history manager: {}", e);
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
    
    // Initialize task service state
    let task_service_state = commands::tasks::TaskServiceState::new();
    
    // Initialize API server state for Claude Code integration
    let api_server_state = commands::tasks::ApiServerState::new();

    tauri::Builder::default()
        .manage(app_state.clone())
        .manage(task_service_state)
        .manage(api_server_state)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let _handle = app.handle().clone();
            
            // Initialize voice command service
            let app_handle = app.handle().clone();
            let app_state_for_voice = app_state.clone();
            
            // Use async runtime to handle the async initialization
            tauri::async_runtime::block_on(async {
                let mut state = app_state_for_voice.lock().map_err(|e| format!("Failed to lock state for voice command initialization: {}", e))?;
                
                // MCP integrations have been removed. (Claude Code handles external tools.)
                // Keep VoiceCommandService functional without MCP + AI agent wiring.
                state.mcp_client = None;
                state.ai_agent = None;

                // Get references to existing services
                let transcription_service = state.transcription_service.clone();
                
                // Initialize OpenAI client if API key is available
                let openai_client = if !state.settings.api_key.is_empty() {
                    println!("🔧 Initializing OpenAI client for voice commands with API key");
                    match crate::openai_client::OpenAiClient::new(state.settings.api_key.clone()) {
                        Ok(client) => {
                            println!("✅ OpenAI client initialized successfully for voice commands");
                            Some(Arc::new(client))
                        }
                        Err(e) => {
                            println!("❌ Failed to initialize OpenAI client: {}", e);
                            None
                        }
                    }
                } else {
                    println!("⚠️ No OpenAI API key configured, voice commands will use AI agent fallback");
                    None
                };
                
                // Clone the service reference before passing to voice command (which takes ownership)
                let transcription_service_for_warmup = transcription_service.clone();

                // Create voice command service
                let voice_service = voice_command::VoiceCommandService::new(
                    app_handle.clone(),
                    transcription_service,
                    None,
                    None,
                    openai_client,
                );

                state.voice_command_service = Some(Arc::new(voice_service));
                println!("✅ Voice command service initialized");

                // Initialize global recording manager
                let recording_manager = Arc::new(RecordingManager::new());
                unsafe {
                    RECORDING_MANAGER = Some(recording_manager.clone());
                }
                println!("✅ Recording manager initialized");

                // Pre-warm the audio device in a background thread so the first
                // recording starts instantly (ALSA probing takes several seconds).
                warm_up_audio_device(&state.settings.selected_device_id);

                // Pre-load the transcription model in the background so the first
                // transcription call is fast (avoids model loading delay on first use).
                if let Some(ref service) = transcription_service_for_warmup {
                    let service_clone = service.clone();
                    tokio::spawn(async move {
                        println!("🔄 Pre-loading transcription model...");
                        match service_clone.warm_up().await {
                            Ok(()) => println!("✅ Transcription model pre-loaded and ready"),
                            Err(e) => println!("⚠️ Failed to pre-load transcription model: {} (will load on first use)", e),
                        }
                    });
                }
                
                // Initialize meeting service API key if available
                if !state.settings.api_key.is_empty() {
                    let meeting_service = Arc::clone(&state.meeting_service);
                    drop(state); // Release the lock before async call
                    
                    if let Err(e) = meeting_service.set_api_key(app_state.lock().unwrap().settings.api_key.clone()).await {
                        println!("⚠️ Failed to set meeting service API key on startup: {}", e);
                    } else {
                        println!("✅ Meeting service API key initialized on startup");
                    }
                } else {
                    println!("⚠️ No API key available for meeting service initialization");
                }
                
                Ok::<(), String>(())
            })?;
            
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
            debug_monitor_info,
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
            
            // Voice Command Commands
            get_voice_command_state,
            get_voice_command_messages,
            clear_voice_command_messages,
            process_text_command,
            start_voice_command_test,
            start_voice_recording,
            commands::voice_command::update_openai_api_key,
            commands::voice_command::get_openai_status,
            
            // Meeting commands
            commands::meeting::start_meeting,
            commands::meeting::start_meeting_recording,
            commands::meeting::pause_meeting_recording,
            commands::meeting::resume_meeting_recording,
            commands::meeting::stop_meeting_recording,
            commands::meeting::end_meeting,
            commands::meeting::get_current_meeting,
            commands::meeting::get_meeting_recording_state,
            commands::meeting::list_meetings,
            commands::meeting::get_meeting,
            commands::meeting::delete_meeting,
            commands::meeting::rotate_meeting_chunk,
            commands::meeting::get_meeting_config,
            commands::meeting::update_meeting_config,
            commands::meeting::get_meeting_processing_status,
            commands::meeting::get_all_processing_statuses,
            commands::meeting::retry_meeting_processing,
            commands::meeting::cancel_meeting_processing,
            commands::meeting::get_meeting_statistics,
            commands::meeting::force_chunk_rotation,
            
            // Enhanced audio capture commands
            commands::audio::get_extended_audio_devices,
            commands::audio::get_recommended_meeting_device,
            commands::audio::get_devices_by_type,
            commands::audio::check_system_audio_capability,
            commands::audio::get_virtual_audio_suggestions,
            commands::audio::get_virtual_audio_status,
            commands::audio::get_system_audio_setup_instructions,
            commands::audio::test_device_recording,
            commands::audio::refresh_audio_devices,

            // History commands
            commands::history::get_transcription_history,
            commands::history::get_history_entry,
            commands::history::delete_history_entry,
            commands::history::clear_transcription_history,
            commands::history::search_transcription_history,
            commands::history::repaste_transcription,
            commands::history::toggle_history_pin,

            // Task management commands (markdown-based)
            commands::tasks::list_repositories,
            commands::tasks::add_repository,
            commands::tasks::remove_repository,
            commands::tasks::ensure_repository,
            commands::tasks::open_repo_in_cursor,
            commands::tasks::open_task_file,
            commands::tasks::get_task_file_path,
            commands::tasks::get_repo_tasks,
            commands::tasks::create_task,
            commands::tasks::quick_create_task,
            commands::tasks::get_task,
            commands::tasks::update_task,
            commands::tasks::update_task_status,
            commands::tasks::delete_task,
            commands::tasks::list_tasks,
            commands::tasks::add_checklist_item,
            commands::tasks::toggle_checklist_item,
            commands::tasks::get_workspace_context,
            commands::tasks::detect_workspace,
            commands::tasks::get_orchestration_prompt,
            commands::tasks::save_orchestration_prompt,
            commands::tasks::get_orchestration_logs,
            commands::tasks::clear_orchestration_logs,
            commands::tasks::orchestrate_claude_code_tasks,
            
            // Claude Code API server commands
            commands::tasks::start_claude_api_server,
            commands::tasks::stop_claude_api_server,
            commands::tasks::get_claude_api_port,
            
            // Claude Code conversation commands
            commands::tasks::send_claude_response,
            commands::tasks::start_voice_response_recording,
            commands::tasks::show_orchestration_overlay,
            commands::tasks::get_claude_api_info,
            
            // Quick task queries
            commands::tasks::sync_tasks_to_cache,
            commands::tasks::get_next_task,
            commands::tasks::get_task_summary,

            // Transcription dependency management
            commands::transcription::check_backend_deps,
            commands::transcription::install_backend_deps,

            // Claude Code integration
            claude::invoke::send_to_claude_code,
            claude::invoke::send_to_claude_code_keyboard,
            claude::invoke::send_to_claude_code_headless,
            claude::invoke::get_claude_headless_logs,
            claude::invoke::open_task_in_cursor,
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

    // Show mini recording indicator immediately (before audio device opens, which can be slow)
    if let Some(ref handle) = app_handle {
        show_recording_started(handle);
    }

    // Start actual audio recording
    let recording_result = start_actual_recording(&device_id, &final_temp_path, 16000, 1).await;

    if let Err(e) = recording_result {
        // If audio recording fails, clean up the recording state
        let _ = recording_manager.stop_recording();
        return Err(e);
    }

    // Emit event after recording actually started
    if let Some(handle) = app_handle {
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

async fn start_orchestration_recording(state: Arc<Mutex<AppState>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    let (recording_manager, device_id) = {
        let app_state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        let device_id = app_state.settings.selected_device_id.clone();

        // Get global recording manager
        let manager = unsafe {
            RECORDING_MANAGER
                .as_ref()
                .ok_or("Recording manager not initialized")?
                .clone()
        };

        (manager, device_id)
    };

    // Check if we can start recording
    recording_manager.can_start_recording(&RecordingMode::Orchestration)?;

    // Create temporary file
    let temp_file = NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    let (_file, persistent_path) = temp_file
        .keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e))?;
    let final_temp_path = persistent_path.to_string_lossy().to_string();

    let config = RecordingConfig {
        mode: RecordingMode::Orchestration,
        auto_stop_duration_ms: None, // User stops with double Shift
        temp_file_path: final_temp_path.clone(),
    };

    // Start recording state management
    recording_manager.start_recording(config)?;

    // Start actual audio recording
    let recording_result = start_actual_recording(&device_id, &final_temp_path, 16000, 1).await;
    if let Err(e) = recording_result {
        let _ = recording_manager.stop_recording();
        return Err(e);
    }

    // Emit events + overlay
    if let Some(handle) = app_handle {
        crate::commands::tasks::add_orchestration_log("info", "Orchestration recording started");
        show_overlay_status(&handle, "🎙️ Speak your task plan… (Shift x2 to stop)", "recording");
        let _ = handle.emit("recording-started", serde_json::json!({
            "mode": "orchestration",
            "file_path": final_temp_path
        }));
        let _ = handle.emit("orchestration-recording-started", serde_json::json!({}));
        // Show the overlay window for orchestration
        if let Some(overlay) = handle.get_webview_window("overlay") {
            let _ = overlay.show();
            let _ = overlay.set_focus();
        }
        let _ = handle.emit("orchestration-log", serde_json::json!({
            "level": "info",
            "message": "Orchestration recording started"
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
        if matches!(mode, RecordingMode::Orchestration) {
            crate::commands::tasks::add_orchestration_log("info", "Orchestration recording stopped; starting transcription");
            let _ = handle.emit("orchestration-log", serde_json::json!({
                "level": "info",
                "message": "Recording stopped; transcribing…"
            }));
        }
        // Only show the overlay for orchestration mode; simple transcription
        // should not pop up any window to avoid stealing focus.
        if matches!(mode, RecordingMode::Orchestration) {
            show_overlay_status(&handle, "Recording stopped", "success");
        }
        let _ = handle.emit("recording-stopped", serde_json::json!({
            "mode": match mode {
                RecordingMode::Transcription => "transcription",
                RecordingMode::VoiceCommand => "voice_command",
                RecordingMode::Orchestration => "orchestration",
                RecordingMode::Meeting => "meeting",
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

async fn cancel_recording(state: Arc<Mutex<AppState>>, app_handle: Option<AppHandle>) -> Result<(), String> {
    println!("🚫 Cancelling recording...");
    
    let recording_manager = unsafe {
        RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Get the current recording mode and file path before stopping
    let (mode, temp_file_path) = {
        let _current_mode = recording_manager.get_current_mode()?;
        let is_recording = recording_manager.is_recording()?;
        
        if !is_recording {
            return Err("No recording in progress to cancel".to_string());
        }
        
        // Stop the RecordingManager state
        let (mode, temp_file_path) = recording_manager.stop_recording()?;
        (mode, temp_file_path)
    };

    // Stop actual audio recording without finalizing WAV (to avoid corrupted file)
    cancel_actual_recording().await?;

    // Clean up the temporary file
    if let Err(e) = std::fs::remove_file(&temp_file_path) {
        println!("⚠️ Failed to remove cancelled recording file {}: {}", temp_file_path, e);
    } else {
        println!("✅ Cancelled recording file removed: {}", temp_file_path);
    }

    // Handle mode-specific cancellation
    match mode {
        RecordingMode::VoiceCommand => {
            // Notify voice command service of cancellation
            let voice_service = {
                let state = state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                state.voice_command_service.clone()
            };
            
            if let Some(service) = voice_service {
                if let Err(e) = service.handle_recording_cancelled().await {
                    println!("⚠️ Failed to notify voice command service of cancellation: {}", e);
                }
            }
        }
        RecordingMode::Transcription => {
            // For transcription, we just need to show the cancellation message
            println!("📝 Transcription recording cancelled");
        }
        RecordingMode::Orchestration => {
            println!("🧩 Orchestration recording cancelled");
        }
        RecordingMode::Meeting => {
            // Meeting cancellation would be handled by meeting service
            println!("📅 Meeting recording cancelled");
        }
    }

    // Show cancellation feedback and emit event
    if let Some(handle) = app_handle {
        show_overlay_status(&handle, "Recording cancelled", "error");
        let _ = handle.emit("recording-cancelled", serde_json::json!({
            "mode": match mode {
                RecordingMode::Transcription => "transcription",
                RecordingMode::VoiceCommand => "voice_command",
                RecordingMode::Orchestration => "orchestration",
                RecordingMode::Meeting => "meeting",
            }
        }));
        
        // Hide overlay after a delay
        let handle_clone = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
            hide_overlay_status(&handle_clone);
        });
    }

    println!("✅ Recording cancelled successfully");
    Ok(())
}

// Actual audio recording functions
async fn start_actual_recording(device_id: &str, temp_file_path: &str, _sample_rate: u32, _channels: u16) -> Result<(), String> {
    // All cpal work is synchronous/blocking — run it on a blocking thread
    // so it doesn't block the async runtime (and delay overlay updates).
    let device_id = device_id.to_string();
    let temp_file_path = temp_file_path.to_string();

    tokio::task::spawn_blocking(move || {
        start_actual_recording_sync(&device_id, &temp_file_path)
    }).await.map_err(|e| format!("Recording task panicked: {}", e))?
}

fn start_actual_recording_sync(device_id: &str, temp_file_path: &str) -> Result<(), String> {
    // Quick check: only stop if there's actually something running
    unsafe {
        if RECORDING_STREAM.is_some() || WAV_WRITER.is_some() {
            drop(RECORDING_STREAM.take());
            if let Some(wav_writer) = WAV_WRITER.take() {
                if let Ok(writer) = Arc::try_unwrap(wav_writer) {
                    if let Ok(writer) = writer.into_inner() {
                        let _ = writer.finalize();
                    }
                }
            }
            TEMP_FILE_PATH.take();
        }
    }

    let t = std::time::Instant::now();
    println!("🎤 Starting actual audio recording to: {}", temp_file_path);

    // Try to use the pre-warmed cached device (instant), fall back to fresh probe (slow)
    let cached = {
        let lock = CACHED_AUDIO.get_or_init(|| Mutex::new(None));
        lock.lock().unwrap().take()
    };

    let (device, device_config) = if let Some(c) = cached {
        println!("[audio] Using pre-warmed device ({}ms)", t.elapsed().as_millis());
        (c.device, c.config)
    } else {
        println!("[audio] No cached device, probing ALSA (this may be slow)...");
        let c = resolve_audio_device(device_id)?;
        (c.device, c.config)
    };

    let device_name = device.name().unwrap_or("Unknown".to_string());
    println!("🎤 Using device: {}", device_name);

    let native_channels = device_config.channels();
    let native_sample_rate = device_config.sample_rate().0;
    let native_format = device_config.sample_format();

    println!("⚙️ Device native config: {}Hz, {} channels, {:?}",
             native_sample_rate, native_channels, native_format);

    // Write mono WAV at the device's native sample rate
    let wav_spec = WavSpec {
        channels: 1,
        sample_rate: native_sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let file = File::create(temp_file_path)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;
    let wav_writer = WavWriter::new(BufWriter::new(file), wav_spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
    let wav_writer = Arc::new(Mutex::new(wav_writer));

    // Create audio stream using device's native config
    let wav_writer_clone = Arc::clone(&wav_writer);
    let ch = native_channels as usize;
    let stream = match native_format {
        SampleFormat::F32 => {
            device.build_input_stream(
                &device_config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = wav_writer_clone.lock() {
                        if ch == 1 {
                            for &sample in data {
                                let _ = writer.write_sample((sample * i16::MAX as f32) as i16);
                            }
                        } else {
                            for chunk in data.chunks(ch) {
                                let avg = chunk.iter().sum::<f32>() / ch as f32;
                                let _ = writer.write_sample((avg * i16::MAX as f32) as i16);
                            }
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                &device_config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut writer) = wav_writer_clone.lock() {
                        if ch == 1 {
                            for &sample in data {
                                let _ = writer.write_sample(sample);
                            }
                        } else {
                            for chunk in data.chunks(ch) {
                                let avg = chunk.iter().map(|&x| x as i32).sum::<i32>() / ch as i32;
                                let _ = writer.write_sample(avg as i16);
                            }
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
        }
        _ => return Err(format!("Unsupported sample format: {:?}", native_format)),
    }.map_err(|e| format!("Failed to create audio stream: {}", e))?;

    // Start the stream
    stream.play().map_err(|e| format!("Failed to start audio stream: {}", e))?;

    // Store globally
    unsafe {
        RECORDING_STREAM = Some(stream);
        WAV_WRITER = Some(wav_writer);
        TEMP_FILE_PATH = Some(temp_file_path.to_string());
    }

    println!("✅ Audio recording started successfully ({}ms)", t.elapsed().as_millis());

    // Re-warm the device cache in the background for the next recording
    let did = device_id.to_string();
    std::thread::spawn(move || {
        if let Ok(c) = resolve_audio_device(&did) {
            let lock = CACHED_AUDIO.get_or_init(|| Mutex::new(None));
            *lock.lock().unwrap() = Some(c);
        }
    });

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

async fn cancel_actual_recording() -> Result<(), String> {
    println!("🚫 Cancelling actual audio recording...");

    unsafe {
        // Stop the stream first
        if let Some(stream) = RECORDING_STREAM.take() {
            drop(stream);
            println!("✅ Audio stream stopped");
        }

        // Drop WAV writer WITHOUT finalizing to avoid creating a valid audio file
        if let Some(wav_writer) = WAV_WRITER.take() {
            drop(wav_writer);
            println!("✅ WAV writer dropped (not finalized)");
        }

        // Clear the temp file path
        if let Some(_file_path) = TEMP_FILE_PATH.take() {
            println!("✅ Temp file path cleared");
        }
    }

    println!("✅ Audio recording cancelled successfully");
    Ok(())
}

async fn handle_recording_transcription(
    app_state: Arc<Mutex<AppState>>,
    app_handle: AppHandle,
    file_path: String,
    mode: RecordingMode,
) -> Result<(), String> {
    println!("🔄 Starting transcription for {:?} mode recording: {}", mode, file_path);
    if matches!(mode, RecordingMode::Orchestration) {
        crate::commands::tasks::add_orchestration_log("info", format!("Transcribing audio: {}", file_path));
        let _ = app_handle.emit("orchestration-log", serde_json::json!({
            "level": "info",
            "message": "Transcribing audio…"
        }));
    }
    
    // Get transcription service
    let transcription_service = {
        let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        state.transcription_service.clone()
            .ok_or("Transcription service not initialized")?
    };
    
    // Show transcribing status
    show_transcribing_status(&app_handle);
    
    // Perform transcription using the shared service directly (no Mutex needed)
    let transcription_result = perform_transcription(transcription_service.clone(), &file_path).await?;

    println!("✅ Transcription successful: {}", transcription_result);
    if matches!(mode, RecordingMode::Orchestration) {
        crate::commands::tasks::add_orchestration_log("success", "Transcription complete");
        let _ = app_handle.emit("orchestration-log", serde_json::json!({
            "level": "success",
            "message": "Transcription complete"
        }));
    }

    // Save to history
    {
        let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        if let Some(history_manager) = &state.history_manager {
            let source = match mode {
                RecordingMode::Transcription => crate::history::TranscriptionSource::Manual,
                RecordingMode::VoiceCommand => crate::history::TranscriptionSource::VoiceCommand,
                RecordingMode::Orchestration => crate::history::TranscriptionSource::Manual,
                RecordingMode::Meeting => crate::history::TranscriptionSource::Meeting,
            };

            let model = Some(format!("{:?}", state.settings.transcription_mode));

            if let Err(e) = history_manager.add_transcription(
                transcription_result.clone(),
                source,
                None, // duration would need to be calculated if needed
                model,
            ) {
                println!("⚠️ Failed to save transcription to history: {}", e);
            } else {
                println!("💾 Transcription saved to history");
            }
        }
    }

    match mode {
        RecordingMode::Transcription => {
            // Handle regular transcription - auto-paste if enabled
            let auto_paste_enabled = {
                let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                state.settings.auto_paste
            };

            // Hide the overlay now that recording/transcription is done
            hide_overlay_status(&app_handle);

            if auto_paste_enabled {
                // paste_text sets clipboard (safety net) + types text directly
                if let Err(e) = paste_text(&transcription_result).await {
                    eprintln!("Failed to paste text: {}", e);
                }
            } else {
                // Just copy to clipboard without typing
                set_clipboard_reliable(&transcription_result).await;
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
        RecordingMode::Orchestration => {
            // Save as the orchestration prompt and surface it in the Tasks UI
            if let Err(e) = crate::commands::tasks::save_orchestration_prompt(transcription_result.clone()).await {
                println!("⚠️ Failed to save orchestration prompt: {}", e);
                crate::commands::tasks::add_orchestration_log("error", format!("Failed to save prompt: {}", e));
                let _ = app_handle.emit("orchestration-log", serde_json::json!({
                    "level": "error",
                    "message": format!("Failed to save prompt: {}", e)
                }));
            } else {
                crate::commands::tasks::add_orchestration_log("success", "Orchestration prompt saved");
                let _ = app_handle.emit("orchestration-log", serde_json::json!({
                    "level": "success",
                    "message": "Orchestration prompt saved"
                }));
            }

            // Emit the prompt to the overlay for review
            let _ = app_handle.emit("orchestration-prompt-updated", serde_json::json!({
                "prompt": transcription_result
            }));
            // The overlay will receive this and show the prompt for review

            // Auto-send the prompt to Claude Code
            crate::commands::tasks::add_orchestration_log("info", "Sending prompt to Claude Code…");
            let _ = app_handle.emit("orchestration-log", serde_json::json!({
                "level": "info",
                "message": "Sending prompt to Claude Code…"
            }));
            
            // Start watcher in the target workspace if available
            // We need to peek at the workspace path from the task service or context
            // For now, we rely on the orchestration command to set it up, but the watcher needs to be long-lived.
            // Let's initialize the watcher here if we can determine the path.
            
            // Actually, best to let the command handle it since it resolves the workspace path.
            
            let task_state = app_handle.state::<crate::commands::tasks::TaskServiceState>();
            // Use headless mode with API server for voice-triggered orchestration
            match crate::commands::tasks::process_orchestration_request(task_state, app_handle.clone(), transcription_result.clone(), None, None, true, None).await {
                Ok(result) => {
                    crate::commands::tasks::add_orchestration_log(if result.success { "success" } else { "error" }, result.message.clone());
                    let _ = app_handle.emit("orchestration-log", serde_json::json!({
                        "level": if result.success { "success" } else { "error" },
                        "message": result.message
                    }));
                }
                Err(e) => {
                    crate::commands::tasks::add_orchestration_log("error", format!("Failed to send to Claude Code: {}", e));
                    let _ = app_handle.emit("orchestration-log", serde_json::json!({
                        "level": "error",
                        "message": format!("Failed to send to Claude Code: {}", e)
                    }));
                }
            }
        }
        RecordingMode::Meeting => {
            // Handle meeting recording - integrate with meeting service
            println!("📝 Meeting recording completed: {} characters transcribed", transcription_result.len());
            
            // Get meeting service and add this transcription to the current meeting
            let meeting_service = {
                let state = app_state.lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                state.meeting_service.clone()
            };
            
            // Check if there's a current meeting to add this transcription to
            if let Ok(Some(mut current_meeting)) = meeting_service.get_current_meeting().await {
                // For now, append to the transcript (in a full implementation, this would be a chunk)
                let updated_transcript = match &current_meeting.transcript {
                    Some(existing) => format!("{}\n\n{}", existing, transcription_result),
                    None => transcription_result.clone(),
                };
                
                // Create a temporary audio chunk entry
                let chunk = crate::meeting::AudioChunk {
                    id: uuid::Uuid::new_v4().to_string(),
                    chunk_number: (current_meeting.audio_chunks.len() + 1) as u32,
                    file_path: std::path::PathBuf::from(&file_path),
                    start_timestamp: chrono::Utc::now(),
                    end_timestamp: Some(chrono::Utc::now()),
                    duration_seconds: 30.0, // Placeholder - would be calculated from actual audio
                    file_size_bytes: std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0),
                };
                
                current_meeting.audio_chunks.push(chunk);
                current_meeting.transcript = Some(updated_transcript);
                
                // Save the updated meeting (this is a simplified approach)
                if let Err(e) = meeting_service.get_storage().save_meeting(&current_meeting).await {
                    println!("⚠️ Failed to save meeting with transcription: {}", e);
                } else {
                    println!("✅ Meeting transcription saved successfully");
                }
            } else {
                println!("⚠️ No current meeting found to save transcription to");
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

                // Perform transcription using the shared service directly
                let result = perform_transcription(service.clone(), &temp_path).await;

                let duration = start_time.elapsed();
                println!("⏱️ Transcription call took: {:.2}s", duration.as_secs_f64());

                match result {
                    Ok(ref text) => {
                        println!("✅ Transcription successful: {}", text);
                        println!("📝 Transcribed text length: {} characters", text.len());

                        // Hide the overlay now that transcription is done
                        if let Some(app_handle) = &app_handle_clone {
                            hide_overlay_status(app_handle);
                        }

                        // Standard transcription path: paste or copy result
                        {
                            let auto_paste_enabled = {
                                if let Ok(app_state) = state_clone.lock() {
                                    app_state.settings.auto_paste
                                } else {
                                    true
                                }
                            };

                            if auto_paste_enabled {
                                if let Err(e) = paste_text(&text).await {
                                    eprintln!("Failed to paste text: {}", e);
                                }
                            } else {
                                set_clipboard_reliable(text).await;
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

    // Shift double-tap => Claude Code task orchestration
    let mut shift_tap_count = 0;
    let mut last_shift_tap_time: Option<Instant> = None;
    
    // Track Alt key state to detect press/release transitions
    let mut alt_was_pressed = false;
    let mut alt_press_start: Option<Instant> = None;

    // Track Shift key state to detect press/release transitions
    let mut shift_was_pressed = false;
    let mut shift_press_start: Option<Instant> = None;
    
    // Track ESC key state for recording cancellation
    let mut esc_was_pressed = false;
    
    loop {
        // Skip hotkey detection while paste_text is typing to avoid
        // synthetic key events (Shift for capitals etc.) triggering shortcuts
        if IS_TYPING.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
            // Reset state so we don't see stale press→release transitions after typing
            alt_was_pressed = false;
            shift_was_pressed = false;
            esc_was_pressed = false;
            alt_press_start = None;
            shift_press_start = None;
            alt_tap_count = 0;
            shift_tap_count = 0;
            continue;
        }

        let keys: Vec<Keycode> = device_state.get_keys();
        let alt_currently_pressed = keys.contains(&Keycode::LAlt);
        let shift_currently_pressed = keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
        let esc_currently_pressed = keys.contains(&Keycode::Escape);
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

        // === SHIFT KEY DETECTION (task orchestration) ===
        // Detect Shift key press (transition from not-pressed to pressed)
        if shift_currently_pressed && !shift_was_pressed {
            shift_press_start = Some(now);
        }
        // Detect Shift key release (transition from pressed to not-pressed)
        else if !shift_currently_pressed && shift_was_pressed {
            if let Some(press_start) = shift_press_start {
                let press_duration = now.duration_since(press_start);

                // Only count as a tap if the press was short (< 200ms)
                if press_duration < Duration::from_millis(200) {
                    shift_tap_count += 1;
                    last_shift_tap_time = Some(now);

                    // Double tap detected
                    if shift_tap_count >= 2 {
                        shift_tap_count = 0;
            println!("Double Shift detected - toggling orchestration dictation recording");

                        let app_handle_clone = app_handle.clone();
                        let state = app_handle.state::<Arc<Mutex<AppState>>>();
                        let state_clone = state.inner().clone();
                        tauri::async_runtime::spawn(async move {
                // Toggle: if currently recording orchestration -> stop; else -> start orchestration
                let current_mode = unsafe {
                    RECORDING_MANAGER
                        .as_ref()
                        .and_then(|m| m.get_current_mode().ok())
                        .flatten()
                };
                let is_recording = unsafe {
                    RECORDING_MANAGER
                        .as_ref()
                        .and_then(|m| m.is_recording().ok())
                        .unwrap_or(false)
                };

                if is_recording && current_mode == Some(RecordingMode::Orchestration) {
                    if let Err(e) = stop_recording_new(state_clone, Some(app_handle_clone)).await {
                        println!("❌ Failed to stop orchestration recording: {}", e);
                    }
                    return;
                }

                // Don't interrupt other recording modes
                if is_recording {
                    crate::show_overlay_status(&app_handle_clone, "⚠️ Another recording is active", "error");
                    return;
                }

                if let Err(e) = start_orchestration_recording(state_clone, Some(app_handle_clone)).await {
                    println!("❌ Failed to start orchestration recording: {}", e);
                }
                        });
                    }
                }
            }
            shift_press_start = None;
        }
        
        // === ESC KEY DETECTION (cancel recording) ===
        
        // Detect ESC key press (transition from not-pressed to pressed)
        if esc_currently_pressed && !esc_was_pressed {
            println!("ESC key pressed - checking for active recording to cancel");
            
            // Check if any recording is currently active
            let state = app_handle.state::<Arc<Mutex<AppState>>>();
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
            
            if is_recording {
                println!("🚫 ESC pressed during recording - cancelling recording");
                let app_handle_clone = app_handle.clone();
                let state_clone = state.inner().clone();
                
                // Cancel the recording (don't transcribe)
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = cancel_recording(state_clone, Some(app_handle_clone)).await {
                        println!("❌ Failed to cancel recording: {}", e);
                    }
                });
            } else {
                println!("No active recording to cancel");
            }
        }
        
        // Update the previous states
        alt_was_pressed = alt_currently_pressed;
        shift_was_pressed = shift_currently_pressed;
        esc_was_pressed = esc_currently_pressed;
        
        // Reset tap counts if too much time has passed since last tap
        if let Some(last_tap) = last_alt_tap_time {
            if now.duration_since(last_tap) > Duration::from_millis(500) {
                if alt_tap_count > 0 {
                    println!("Alt tap sequence timed out, resetting count");
                    alt_tap_count = 0;
                }
            }
        }

        // Reset Shift tap counts if too much time has passed since last tap
        if let Some(last_tap) = last_shift_tap_time {
            if now.duration_since(last_tap) > Duration::from_millis(500) {
                if shift_tap_count > 0 {
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
    // Use simple cpal enumeration with numeric indices that match the recording functions
    get_audio_devices_sync()
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
    // Update state and get meeting service reference
    let meeting_service = {
        let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.settings = settings.clone();
        app_state.meeting_service.clone()
    };
    
    // Update meeting service API key if available (outside the lock)
    if !settings.api_key.is_empty() {
        if let Err(e) = meeting_service.set_api_key(settings.api_key.clone()).await {
            println!("⚠️ Failed to update meeting service API key: {}", e);
        } else {
            println!("✅ Meeting service API key updated");
        }
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
    let (config, meeting_service) = {
        let app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
        let config = TranscriptionConfig {
            mode: app_state.settings.transcription_mode.clone(),
            openai_api_key: if app_state.settings.api_key.is_empty() { None } else { Some(app_state.settings.api_key.clone()) },
            whisper_model_path: app_state.settings.whisper_model_path.clone(),
            whisper_model_size: app_state.settings.whisper_model_size.clone(),
            device: app_state.settings.device_type.clone(),
        };
        let meeting_service = Arc::clone(&app_state.meeting_service);
        (config, meeting_service)
    }; // MutexGuard is dropped here
    
    match TranscriptionService::new(config) {
        Ok(service) => {
            let service_arc = Arc::new(service);

            // Update the main transcription service
            {
                let mut app_state = state.inner().lock().map_err(|e| format!("Failed to lock state: {}", e))?;
                app_state.transcription_service = Some(service_arc.clone());
            } // MutexGuard is dropped here

            // Update the meeting service with the new transcription service
            if let Err(e) = meeting_service.set_transcription_service(service_arc).await {
                println!("⚠️ Failed to update meeting service transcription: {}", e);
            }

            println!("✅ Transcription service reloaded successfully (including meeting service)");
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
async fn debug_monitor_info(app_handle: AppHandle) -> Result<String, String> {
    let (cursor_x, cursor_y) = get_cursor_position()?;

    let mut debug_info = format!("Cursor position: ({}, {})\n\n", cursor_x, cursor_y);

    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        if let Ok(monitors) = overlay_window.available_monitors() {
            debug_info.push_str(&format!("Found {} monitors:\n", monitors.len()));

            for (i, monitor) in monitors.iter().enumerate() {
                let monitor_size = monitor.size();
                let monitor_pos = monitor.position();

                let monitor_left = monitor_pos.x;
                let monitor_right = monitor_pos.x + monitor_size.width as i32;
                let monitor_top = monitor_pos.y;
                let monitor_bottom = monitor_pos.y + monitor_size.height as i32;

                let is_cursor_here = cursor_x >= monitor_left && cursor_x < monitor_right &&
                                   cursor_y >= monitor_top && cursor_y < monitor_bottom;

                debug_info.push_str(&format!(
                    "\nMonitor {}: {}x{} at ({}, {})\n  Bounds: [{}, {}, {}, {}]\n  Cursor here: {}\n",
                    i, monitor_size.width, monitor_size.height,
                    monitor_pos.x, monitor_pos.y,
                    monitor_left, monitor_top, monitor_right, monitor_bottom,
                    is_cursor_here
                ));
            }
        }

        if let Ok(pos) = overlay_window.outer_position() {
            debug_info.push_str(&format!("\nOverlay window position: ({}, {})\n", pos.x, pos.y));
        }
    }

    Ok(debug_info)
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

// MCP integrations removed (Claude Code handles external tools).

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


