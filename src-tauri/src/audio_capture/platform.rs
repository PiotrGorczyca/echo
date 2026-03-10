use super::{ExtendedAudioDevice, AudioDeviceType, PlatformDeviceInfo};
use cpal::Host;
use cpal::traits::{DeviceTrait, HostTrait};

/// Get platform-specific loopback/monitor audio devices
pub fn get_platform_loopback_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    #[cfg(target_os = "windows")]
    return get_windows_loopback_devices(host);
    
    #[cfg(target_os = "linux")]
    return get_linux_loopback_devices(host);
    
    #[cfg(target_os = "macos")]
    return get_macos_loopback_devices(host);
    
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        println!("⚠️ Platform-specific loopback devices not supported on this OS");
        Ok(Vec::new())
    }
}

#[cfg(target_os = "windows")]
fn get_windows_loopback_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    println!("🔍 Scanning for Windows WASAPI loopback devices...");
    
    let mut devices = Vec::new();
    
    // On Windows, we need to use WASAPI in loopback mode
    // CPAL doesn't directly expose this, so we'll look for devices that might support it
    
    // Check if we're using WASAPI host
    let host_id = host.id();
    if host_id.name() != "WASAPI" {
        println!("⚠️ Not using WASAPI host, loopback may not be available");
        return Ok(devices);
    }
    
    // Get output devices and mark them as potential loopback sources
    let output_devices = host.output_devices()
        .map_err(|e| format!("Failed to get output devices: {}", e))?;
    
    for (i, device) in output_devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("Windows Audio Device {}", i));
        
        // Skip devices that are clearly not system audio
        let name_lower = name.to_lowercase();
        if name_lower.contains("bluetooth") && !name_lower.contains("speakers") {
            continue;
        }
        
        // Create a loopback version of this output device
        let loopback_name = format!("{} (Loopback)", name);
        
        devices.push(ExtendedAudioDevice {
            id: format!("windows_loopback_{}", i),
            name: loopback_name,
            device_type: AudioDeviceType::SystemLoopback,
            is_default: i == 0, // Assume first device is default
            sample_rate: device.default_output_config().ok().map(|c| c.sample_rate().0),
            channels: device.default_output_config().ok().map(|c| c.channels()),
            is_available: true,
            platform_info: Some(PlatformDeviceInfo {
                platform: "Windows".to_string(),
                device_id: format!("windows_loopback_{}", i),
                driver_name: Some("WASAPI".to_string()),
                is_loopback_capable: true,
                supports_exclusive_mode: true,
            }),
        });
    }
    
    // Look for "Stereo Mix" if available
    let input_devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        let name_lower = name.to_lowercase();
        
        if name_lower.contains("stereo mix") || 
           name_lower.contains("wave out mix") ||
           name_lower.contains("what u hear") ||
           name_lower.contains("speakers") && name_lower.contains("mix") {
            
            devices.push(ExtendedAudioDevice {
                id: format!("windows_stereomix_{}", i),
                name: format!("{} (System Audio)", name),
                device_type: AudioDeviceType::SystemLoopback,
                is_default: false,
                sample_rate: device.default_input_config().ok().map(|c| c.sample_rate().0),
                channels: device.default_input_config().ok().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: "Windows".to_string(),
                    device_id: format!("windows_stereomix_{}", i),
                    driver_name: Some("DirectSound/WDM".to_string()),
                    is_loopback_capable: true,
                    supports_exclusive_mode: false,
                }),
            });
        }
    }
    
    println!("✅ Found {} Windows loopback devices", devices.len());
    Ok(devices)
}

#[cfg(target_os = "linux")]
fn get_linux_loopback_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    println!("🔍 Scanning for Linux PulseAudio/ALSA monitor devices...");
    
    let mut devices = Vec::new();
    
    // On Linux, we look for PulseAudio monitor sources and ALSA loopback devices
    let input_devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        let name_lower = name.to_lowercase();
        
        // Look for PulseAudio monitor sources
        if name_lower.contains("monitor") || 
           name_lower.contains(".monitor") ||
           name_lower.contains("built-in audio analog stereo - monitor") ||
           name_lower.contains("alsa_output") && name_lower.contains("monitor") {
            
            devices.push(ExtendedAudioDevice {
                id: format!("linux_monitor_{}", i),
                name: format!("{} (System Audio Monitor)", name),
                device_type: AudioDeviceType::SystemLoopback,
                is_default: name_lower.contains("built-in"),
                sample_rate: device.default_input_config().ok().map(|c| c.sample_rate().0),
                channels: device.default_input_config().ok().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: "Linux".to_string(),
                    device_id: format!("linux_monitor_{}", i),
                    driver_name: Some("PulseAudio".to_string()),
                    is_loopback_capable: true,
                    supports_exclusive_mode: false,
                }),
            });
        }
        
        // Look for ALSA loopback devices
        else if name_lower.contains("loopback") {
            devices.push(ExtendedAudioDevice {
                id: format!("linux_loopback_{}", i),
                name: format!("{} (ALSA Loopback)", name),
                device_type: AudioDeviceType::SystemLoopback,
                is_default: false,
                sample_rate: device.default_input_config().ok().map(|c| c.sample_rate().0),
                channels: device.default_input_config().ok().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: "Linux".to_string(),
                    device_id: format!("linux_loopback_{}", i),
                    driver_name: Some("ALSA".to_string()),
                    is_loopback_capable: true,
                    supports_exclusive_mode: false,
                }),
            });
        }
    }
    
    // Also check for PipeWire devices if available
    devices.extend(get_pipewire_devices(host)?);
    
    println!("✅ Found {} Linux loopback devices", devices.len());
    Ok(devices)
}

#[cfg(target_os = "linux")]
fn get_pipewire_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    let input_devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    let mut devices = Vec::new();
    
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        let name_lower = name.to_lowercase();
        
        if name_lower.contains("pipewire") && 
           (name_lower.contains("monitor") || name_lower.contains("sink")) {
            
            devices.push(ExtendedAudioDevice {
                id: format!("pipewire_monitor_{}", i),
                name: format!("{} (PipeWire Monitor)", name),
                device_type: AudioDeviceType::SystemLoopback,
                is_default: false,
                sample_rate: device.default_input_config().ok().map(|c| c.sample_rate().0),
                channels: device.default_input_config().ok().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: "Linux".to_string(),
                    device_id: format!("pipewire_monitor_{}", i),
                    driver_name: Some("PipeWire".to_string()),
                    is_loopback_capable: true,
                    supports_exclusive_mode: false,
                }),
            });
        }
    }
    
    Ok(devices)
}

#[cfg(target_os = "macos")]
fn get_macos_loopback_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    println!("🔍 Scanning for macOS Core Audio loopback devices...");
    
    let mut devices = Vec::new();
    
    // On macOS, system audio capture requires special setup
    // Look for virtual devices that might provide system audio
    let input_devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
    
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        let name_lower = name.to_lowercase();
        
        // Look for devices that might capture system audio
        if name_lower.contains("soundflower") ||
           name_lower.contains("blackhole") ||
           name_lower.contains("loopback") ||
           name_lower.contains("system audio") ||
           (name_lower.contains("built-in") && name_lower.contains("input")) {
            
            // Determine if this is a virtual device or system device
            let device_type = if name_lower.contains("soundflower") || 
                               name_lower.contains("blackhole") ||
                               name_lower.contains("loopback") {
                AudioDeviceType::VirtualCable
            } else {
                AudioDeviceType::SystemLoopback
            };
            
            devices.push(ExtendedAudioDevice {
                id: format!("macos_system_{}", i),
                name: format!("{} (System Audio)", name),
                device_type,
                is_default: name_lower.contains("built-in"),
                sample_rate: device.default_input_config().ok().map(|c| c.sample_rate().0),
                channels: device.default_input_config().ok().map(|c| c.channels()),
                is_available: true,
                platform_info: Some(PlatformDeviceInfo {
                    platform: "macOS".to_string(),
                    device_id: format!("macos_system_{}", i),
                    driver_name: Some("Core Audio".to_string()),
                    is_loopback_capable: true,
                    supports_exclusive_mode: false,
                }),
            });
        }
    }
    
    println!("✅ Found {} macOS loopback devices", devices.len());
    Ok(devices)
}

/// Check if a device name indicates system audio capability
pub fn is_system_audio_device(device_name: &str) -> bool {
    let name_lower = device_name.to_lowercase();
    
    // Windows indicators
    if name_lower.contains("stereo mix") ||
       name_lower.contains("wave out mix") ||
       name_lower.contains("what u hear") ||
       name_lower.contains("loopback") {
        return true;
    }
    
    // Linux indicators  
    if name_lower.contains("monitor") ||
       name_lower.contains(".monitor") ||
       name_lower.contains("alsa_output") {
        return true;
    }
    
    // macOS indicators
    if name_lower.contains("soundflower") ||
       name_lower.contains("blackhole") ||
       name_lower.contains("system audio") {
        return true;
    }
    
    false
}

/// Get platform-specific setup instructions for system audio capture
pub fn get_system_audio_setup_instructions() -> Vec<String> {
    let mut instructions = Vec::new();
    
    #[cfg(target_os = "windows")]
    {
        instructions.push("Windows System Audio Setup:".to_string());
        instructions.push("1. Right-click on speaker icon in system tray".to_string());
        instructions.push("2. Select 'Recording devices'".to_string());
        instructions.push("3. Right-click in empty area → 'Show Disabled Devices'".to_string());
        instructions.push("4. Enable 'Stereo Mix' if available".to_string());
        instructions.push("5. Alternative: Install VB-Audio Cable or Virtual Audio Cable".to_string());
    }
    
    #[cfg(target_os = "linux")]
    {
        instructions.push("Linux System Audio Setup:".to_string());
        instructions.push("1. PulseAudio: Monitor sources should be auto-detected".to_string());
        instructions.push("2. Alternative: Install 'pavucontrol' for PulseAudio management".to_string());
        instructions.push("3. PipeWire: Monitor devices should be available automatically".to_string());
        instructions.push("4. ALSA: May need to configure loopback module".to_string());
    }
    
    #[cfg(target_os = "macos")]
    {
        instructions.push("macOS System Audio Setup:".to_string());
        instructions.push("1. Install BlackHole or Soundflower for system audio capture".to_string());
        instructions.push("2. BlackHole: Download from existential.audio/blackhole".to_string());
        instructions.push("3. Soundflower: Download from github.com/mattingalls/Soundflower".to_string());
        instructions.push("4. Configure system audio output to route through virtual device".to_string());
        instructions.push("5. Use Audio MIDI Setup to create aggregate devices if needed".to_string());
    }
    
    instructions
}