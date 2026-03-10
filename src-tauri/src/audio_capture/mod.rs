use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use cpal::{Device, Host, SampleFormat};
use cpal::traits::{DeviceTrait, HostTrait};

pub mod platform;
pub mod virtual_devices;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AudioDeviceType {
    Microphone,
    SystemOutput,      // System audio output (speakers/headphones)
    SystemLoopback,    // System audio loopback/monitor
    VirtualCable,      // Virtual audio cable
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedAudioDevice {
    pub id: String,
    pub name: String,
    pub device_type: AudioDeviceType,
    pub is_default: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub is_available: bool,
    pub platform_info: Option<PlatformDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformDeviceInfo {
    pub platform: String,
    pub device_id: String,
    pub driver_name: Option<String>,
    pub is_loopback_capable: bool,
    pub supports_exclusive_mode: bool,
}

pub struct AudioCaptureManager {
    host: Host,
    device_cache: Arc<Mutex<Vec<ExtendedAudioDevice>>>,
    last_scan: Arc<Mutex<std::time::Instant>>,
    scan_interval: std::time::Duration,
}

impl AudioCaptureManager {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
            device_cache: Arc::new(Mutex::new(Vec::new())),
            last_scan: Arc::new(Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(60))),
            scan_interval: std::time::Duration::from_secs(30), // Cache for 30 seconds
        }
    }

    /// Get all available audio devices including system audio and virtual cables
    pub fn get_all_audio_devices(&self) -> Result<Vec<ExtendedAudioDevice>, String> {
        // Check if we need to refresh the cache
        let needs_refresh = {
            let last_scan = self.last_scan.lock()
                .map_err(|e| format!("Failed to lock last_scan: {}", e))?;
            last_scan.elapsed() > self.scan_interval
        };

        if needs_refresh {
            self.refresh_device_cache()?;
        }

        let cache = self.device_cache.lock()
            .map_err(|e| format!("Failed to lock device cache: {}", e))?;
        Ok(cache.clone())
    }

    /// Refresh the device cache with current system state
    fn refresh_device_cache(&self) -> Result<(), String> {
        println!("🔍 Scanning for audio devices...");
        
        let mut devices = Vec::new();

        // 1. Get traditional input devices (microphones)
        devices.extend(self.get_input_devices()?);

        // 2. Get system audio output devices
        devices.extend(self.get_output_devices()?);

        // 3. Get system loopback/monitor devices (platform-specific)
        devices.extend(self.get_loopback_devices()?);

        // 4. Detect virtual audio cables
        devices.extend(self.detect_virtual_audio_cables()?);

        // 5. Sort devices by priority (loopback/virtual cables first for meetings)
        devices.sort_by(|a, b| {
            use AudioDeviceType::*;
            let priority_a = match a.device_type {
                SystemLoopback => 0,
                VirtualCable => 1,
                SystemOutput => 2,
                Microphone => 3,
                Unknown => 4,
            };
            let priority_b = match b.device_type {
                SystemLoopback => 0,
                VirtualCable => 1,
                SystemOutput => 2,
                Microphone => 3,
                Unknown => 4,
            };
            priority_a.cmp(&priority_b)
        });

        // Update cache
        {
            let mut cache = self.device_cache.lock()
                .map_err(|e| format!("Failed to lock device cache: {}", e))?;
            *cache = devices;
        }

        {
            let mut last_scan = self.last_scan.lock()
                .map_err(|e| format!("Failed to lock last_scan: {}", e))?;
            *last_scan = std::time::Instant::now();
        }

        println!("✅ Audio device scan completed");
        Ok(())
    }

    /// Get traditional microphone input devices
    fn get_input_devices(&self) -> Result<Vec<ExtendedAudioDevice>, String> {
        let input_devices = self.host.input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?;

        let default_input = self.host.default_input_device();
        let mut devices = Vec::new();

        for (i, device) in input_devices.enumerate() {
            let name = device.name().unwrap_or_else(|_| format!("Input Device {}", i));
            let is_default = default_input.as_ref()
                .map(|def| self.devices_equal(&device, def))
                .unwrap_or(false);

            let config = device.default_input_config().ok();
            
            devices.push(ExtendedAudioDevice {
                id: format!("input_{}", i),
                name: name.clone(),
                device_type: AudioDeviceType::Microphone,
                is_default,
                sample_rate: config.as_ref().map(|c| c.sample_rate().0),
                channels: config.as_ref().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: self.get_platform_name(),
                    device_id: format!("input_{}", i),
                    driver_name: self.get_device_driver_name(&device),
                    is_loopback_capable: false,
                    supports_exclusive_mode: false,
                }),
            });
        }

        Ok(devices)
    }

    /// Get output devices (speakers/headphones) - some can be used for loopback
    fn get_output_devices(&self) -> Result<Vec<ExtendedAudioDevice>, String> {
        let output_devices = self.host.output_devices()
            .map_err(|e| format!("Failed to get output devices: {}", e))?;

        let default_output = self.host.default_output_device();
        let mut devices = Vec::new();

        for (i, device) in output_devices.enumerate() {
            let name = device.name().unwrap_or_else(|_| format!("Output Device {}", i));
            let is_default = default_output.as_ref()
                .map(|def| self.devices_equal(&device, def))
                .unwrap_or(false);

            let config = device.default_output_config().ok();
            
            devices.push(ExtendedAudioDevice {
                id: format!("output_{}", i),
                name: name.clone(),
                device_type: AudioDeviceType::SystemOutput,
                is_default,
                sample_rate: config.as_ref().map(|c| c.sample_rate().0),
                channels: config.as_ref().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: self.get_platform_name(),
                    device_id: format!("output_{}", i),
                    driver_name: self.get_device_driver_name(&device),
                    is_loopback_capable: self.device_supports_loopback(&device),
                    supports_exclusive_mode: false,
                }),
            });
        }

        Ok(devices)
    }

    /// Get platform-specific loopback/monitor devices
    fn get_loopback_devices(&self) -> Result<Vec<ExtendedAudioDevice>, String> {
        platform::get_platform_loopback_devices(&self.host)
    }

    /// Detect virtual audio cable solutions
    fn detect_virtual_audio_cables(&self) -> Result<Vec<ExtendedAudioDevice>, String> {
        virtual_devices::detect_virtual_audio_devices(&self.host)
    }

    /// Get the best device for meeting recording (prioritizes system audio)
    pub fn get_recommended_meeting_device(&self) -> Result<Option<ExtendedAudioDevice>, String> {
        let devices = self.get_all_audio_devices()?;
        
        // Priority order for meeting recording:
        // 1. System loopback devices
        // 2. Virtual audio cables
        // 3. Default system output (if loopback capable)
        // 4. Default microphone (fallback)
        
        for device in devices {
            match device.device_type {
                AudioDeviceType::SystemLoopback => return Ok(Some(device)),
                AudioDeviceType::VirtualCable => return Ok(Some(device)),
                _ => continue,
            }
        }

        // Fallback to loopback-capable output device
        let devices = self.get_all_audio_devices()?;
        for device in devices {
            if device.device_type == AudioDeviceType::SystemOutput &&
               device.platform_info.as_ref()
                   .map(|info| info.is_loopback_capable)
                   .unwrap_or(false) {
                return Ok(Some(device));
            }
        }

        // Last resort: default microphone
        let devices = self.get_all_audio_devices()?;
        for device in devices {
            if device.device_type == AudioDeviceType::Microphone && device.is_default {
                return Ok(Some(device));
            }
        }

        Ok(None)
    }

    /// Create an audio stream from a device
    pub fn create_input_stream(
        &self,
        device_id: &str,
        mut callback: impl FnMut(&[f32]) + Send + 'static,
    ) -> Result<cpal::Stream, String> {
        let device = self.get_device_by_id(device_id)?;
        let config = device.default_input_config()
            .map_err(|e| format!("Failed to get device config: {}", e))?;

        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| callback(data),
                    |err| println!("Audio stream error: {}", err),
                    None,
                )
            }
            SampleFormat::I16 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let float_data: Vec<f32> = data.iter()
                            .map(|&sample| sample as f32 / i16::MAX as f32)
                            .collect();
                        callback(&float_data);
                    },
                    |err| println!("Audio stream error: {}", err),
                    None,
                )
            }
            SampleFormat::U16 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let float_data: Vec<f32> = data.iter()
                            .map(|&sample| (sample as f32 - 32768.0) / 32768.0)
                            .collect();
                        callback(&float_data);
                    },
                    |err| println!("Audio stream error: {}", err),
                    None,
                )
            }
            format => return Err(format!("Unsupported sample format: {:?}", format)),
        }.map_err(|e| format!("Failed to build input stream: {}", e))?;

        Ok(stream)
    }

    // Helper methods
    fn devices_equal(&self, device1: &Device, device2: &Device) -> bool {
        device1.name().unwrap_or_default() == device2.name().unwrap_or_default()
    }

    fn get_platform_name(&self) -> String {
        #[cfg(target_os = "windows")]
        return "Windows".to_string();
        #[cfg(target_os = "macos")]
        return "macOS".to_string();
        #[cfg(target_os = "linux")]
        return "Linux".to_string();
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return "Unknown".to_string();
    }

    fn get_device_driver_name(&self, _device: &Device) -> Option<String> {
        // Platform-specific driver name detection would go here
        // For now, return the host ID
        Some(self.host.id().name().to_string())
    }

    fn device_supports_loopback(&self, device: &Device) -> bool {
        // Check if this output device can be used for loopback recording
        // This is platform-specific and would need detailed implementation
        let name = device.name().unwrap_or_default().to_lowercase();
        
        // Basic heuristics - could be made more sophisticated
        name.contains("speakers") || 
        name.contains("headphones") || 
        name.contains("output") ||
        name.contains("playback")
    }

    fn get_device_by_id(&self, device_id: &str) -> Result<Device, String> {
        if device_id.starts_with("input_") {
            let index: usize = device_id.strip_prefix("input_")
                .and_then(|s| s.parse().ok())
                .ok_or("Invalid input device ID")?;
            
            let devices: Vec<_> = self.host.input_devices()
                .map_err(|e| format!("Failed to get input devices: {}", e))?
                .collect();
            
            devices.into_iter().nth(index)
                .ok_or("Device not found".to_string())
                
        } else if device_id.starts_with("output_") {
            let index: usize = device_id.strip_prefix("output_")
                .and_then(|s| s.parse().ok())
                .ok_or("Invalid output device ID")?;
            
            let devices: Vec<_> = self.host.output_devices()
                .map_err(|e| format!("Failed to get output devices: {}", e))?
                .collect();
            
            devices.into_iter().nth(index)
                .ok_or("Device not found".to_string())
                
        } else {
            Err("Unknown device ID format".to_string())
        }
    }
}