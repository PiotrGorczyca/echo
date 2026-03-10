use tauri::{AppHandle, State, Emitter};
use std::sync::{Arc, Mutex};
use cpal::traits::StreamTrait;

use crate::audio_capture::{
    AudioCaptureManager, ExtendedAudioDevice, AudioDeviceType,
    virtual_devices::{check_virtual_audio_installation, VirtualAudioSuggestion, VirtualAudioStatus},
};
use crate::state::AppState;

#[tauri::command]
pub async fn get_extended_audio_devices(
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<ExtendedAudioDevice>, String> {
    // Create a new AudioCaptureManager (could be cached in AppState later)
    let manager = AudioCaptureManager::new();
    manager.get_all_audio_devices()
}

#[tauri::command]
pub async fn get_recommended_meeting_device(
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<ExtendedAudioDevice>, String> {
    let manager = AudioCaptureManager::new();
    manager.get_recommended_meeting_device()
}

#[tauri::command]
pub async fn get_devices_by_type(
    device_type: String,
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<ExtendedAudioDevice>, String> {
    let manager = AudioCaptureManager::new();
    let all_devices = manager.get_all_audio_devices()?;
    
    let filter_type = match device_type.as_str() {
        "microphone" => AudioDeviceType::Microphone,
        "system_output" => AudioDeviceType::SystemOutput,
        "system_loopback" => AudioDeviceType::SystemLoopback,
        "virtual_cable" => AudioDeviceType::VirtualCable,
        _ => return Err("Invalid device type".to_string()),
    };
    
    let filtered_devices: Vec<ExtendedAudioDevice> = all_devices
        .into_iter()
        .filter(|device| device.device_type == filter_type)
        .collect();
    
    Ok(filtered_devices)
}

#[tauri::command]
pub async fn check_system_audio_capability(
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<SystemAudioCapability, String> {
    let manager = AudioCaptureManager::new();
    let devices = manager.get_all_audio_devices()?;
    
    let has_loopback = devices.iter().any(|d| d.device_type == AudioDeviceType::SystemLoopback);
    let has_virtual_cable = devices.iter().any(|d| d.device_type == AudioDeviceType::VirtualCable);
    let loopback_devices = devices.iter()
        .filter(|d| d.device_type == AudioDeviceType::SystemLoopback)
        .count();
    let virtual_devices = devices.iter()
        .filter(|d| d.device_type == AudioDeviceType::VirtualCable)
        .count();
    
    Ok(SystemAudioCapability {
        has_system_audio: has_loopback || has_virtual_cable,
        has_loopback_devices: has_loopback,
        has_virtual_audio_cables: has_virtual_cable,
        loopback_device_count: loopback_devices,
        virtual_device_count: virtual_devices,
        recommended_device: manager.get_recommended_meeting_device()?,
        setup_required: !has_loopback && !has_virtual_cable,
    })
}

#[tauri::command]
pub async fn get_virtual_audio_suggestions() -> Result<Vec<VirtualAudioSuggestion>, String> {
    Ok(crate::audio_capture::virtual_devices::get_virtual_audio_suggestions())
}

#[tauri::command]
pub async fn get_virtual_audio_status() -> Result<VirtualAudioStatus, String> {
    Ok(check_virtual_audio_installation())
}

#[tauri::command]
pub async fn get_system_audio_setup_instructions() -> Result<Vec<String>, String> {
    Ok(crate::audio_capture::platform::get_system_audio_setup_instructions())
}

#[tauri::command]
pub async fn test_device_recording(
    device_id: String,
    duration_seconds: u32,
    _state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<DeviceTestResult, String> {
    if duration_seconds > 10 {
        return Err("Test duration cannot exceed 10 seconds".to_string());
    }
    
    let manager = AudioCaptureManager::new();
    
    // Get device info
    let devices = manager.get_all_audio_devices()?;
    let device = devices.iter()
        .find(|d| d.id == device_id)
        .ok_or("Device not found")?;
    
    println!("🎤 Testing device: {} ({})", device.name, device.id);
    
    // Test recording for specified duration
    let test_start = std::time::Instant::now();
    
    // Use Arc<Mutex<>> to share data between the callback and main thread
    let samples_received = Arc::new(std::sync::Mutex::new(0usize));
    let peak_amplitude = Arc::new(std::sync::Mutex::new(0.0f32));
    let avg_amplitude = Arc::new(std::sync::Mutex::new(0.0f32));
    let sample_count = Arc::new(std::sync::Mutex::new(0usize));
    
    let samples_received_clone = Arc::clone(&samples_received);
    let peak_amplitude_clone = Arc::clone(&peak_amplitude);
    let avg_amplitude_clone = Arc::clone(&avg_amplitude);
    let sample_count_clone = Arc::clone(&sample_count);
    
    // Create a test recording stream
    let stream = manager.create_input_stream(&device_id, move |data: &[f32]| {
        if let Ok(mut sr) = samples_received_clone.lock() {
            *sr += data.len();
        }
        
        for &sample in data {
            let abs_sample = sample.abs();
            
            if let Ok(mut peak) = peak_amplitude_clone.lock() {
                if abs_sample > *peak {
                    *peak = abs_sample;
                }
            }
            
            if let Ok(mut avg) = avg_amplitude_clone.lock() {
                *avg += abs_sample;
            }
            
            if let Ok(mut count) = sample_count_clone.lock() {
                *count += 1;
            }
        }
    })?;
    
    // Start the stream
    stream.play().map_err(|e| format!("Failed to start test stream: {}", e))?;
    
    // Record for the specified duration
    std::thread::sleep(std::time::Duration::from_secs(duration_seconds as u64));
    
    // Stop the stream
    drop(stream);
    
    let test_duration = test_start.elapsed();
    
    // Extract final values from Arc<Mutex<>>
    let final_samples_received = samples_received.lock().map(|val| *val).unwrap_or(0);
    let final_peak_amplitude = peak_amplitude.lock().map(|val| *val).unwrap_or(0.0);
    let final_avg_amplitude = avg_amplitude.lock().map(|val| *val).unwrap_or(0.0);
    let final_sample_count = sample_count.lock().map(|val| *val).unwrap_or(0);
    
    let final_avg = if final_sample_count > 0 {
        final_avg_amplitude / final_sample_count as f32
    } else {
        0.0
    };
    
    let result = DeviceTestResult {
        device_id: device_id.clone(),
        device_name: device.name.clone(),
        test_duration_ms: test_duration.as_millis() as u32,
        samples_received: final_samples_received,
        peak_amplitude: final_peak_amplitude,
        average_amplitude: final_avg,
        has_audio_signal: final_peak_amplitude > 0.001, // Basic threshold
        is_working: final_samples_received > 0,
        sample_rate: device.sample_rate,
        channels: device.channels,
    };
    
    // Emit test result to frontend
    app.emit("device-test-completed", &result)
        .map_err(|e| format!("Failed to emit test result: {}", e))?;
    
    println!("✅ Device test completed: {} samples, peak: {:.4}", final_samples_received, final_peak_amplitude);
    
    Ok(result)
}

#[tauri::command]
pub async fn refresh_audio_devices(
    _state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<Vec<ExtendedAudioDevice>, String> {
    println!("🔄 Refreshing audio device list...");
    
    // Force refresh by creating a new manager
    let manager = AudioCaptureManager::new();
    let devices = manager.get_all_audio_devices()?;
    
    // Emit event to frontend
    app.emit("audio-devices-refreshed", serde_json::json!({
        "device_count": devices.len(),
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit refresh event: {}", e))?;
    
    println!("✅ Found {} audio devices after refresh", devices.len());
    
    Ok(devices)
}

// Data structures for responses

#[derive(Debug, serde::Serialize)]
pub struct SystemAudioCapability {
    pub has_system_audio: bool,
    pub has_loopback_devices: bool,
    pub has_virtual_audio_cables: bool,
    pub loopback_device_count: usize,
    pub virtual_device_count: usize,
    pub recommended_device: Option<ExtendedAudioDevice>,
    pub setup_required: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct DeviceTestResult {
    pub device_id: String,
    pub device_name: String,
    pub test_duration_ms: u32,
    pub samples_received: usize,
    pub peak_amplitude: f32,
    pub average_amplitude: f32,
    pub has_audio_signal: bool,
    pub is_working: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}