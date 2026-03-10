use super::{ExtendedAudioDevice, AudioDeviceType, PlatformDeviceInfo};
use cpal::Host;
use cpal::traits::{DeviceTrait, HostTrait};

/// Known virtual audio cable solutions and their identifiers
#[derive(Debug, Clone)]
pub struct VirtualAudioSolution {
    pub name: &'static str,
    pub identifiers: &'static [&'static str],
    pub platform: &'static str,
    pub download_url: Option<&'static str>,
    pub is_free: bool,
}

pub const VIRTUAL_AUDIO_SOLUTIONS: &[VirtualAudioSolution] = &[
    // Windows solutions
    VirtualAudioSolution {
        name: "VB-Audio Virtual Cable",
        identifiers: &["vb-audio", "cable", "vbcable", "virtual cable"],
        platform: "Windows",
        download_url: Some("https://vb-audio.com/Cable/"),
        is_free: true,
    },
    VirtualAudioSolution {
        name: "Virtual Audio Cable (VAC)",
        identifiers: &["virtual audio cable", "vac", "line 1"],
        platform: "Windows", 
        download_url: Some("https://vac.muzychenko.net/en/"),
        is_free: false,
    },
    VirtualAudioSolution {
        name: "VoiceMeeter",
        identifiers: &["voicemeeter", "vm-vaio", "vaio"],
        platform: "Windows",
        download_url: Some("https://vb-audio.com/Voicemeeter/"),
        is_free: true,
    },
    
    // macOS solutions
    VirtualAudioSolution {
        name: "BlackHole",
        identifiers: &["blackhole", "black hole"],
        platform: "macOS",
        download_url: Some("https://existential.audio/blackhole/"),
        is_free: true,
    },
    VirtualAudioSolution {
        name: "Soundflower",
        identifiers: &["soundflower"],
        platform: "macOS",
        download_url: Some("https://github.com/mattingalls/Soundflower"),
        is_free: true,
    },
    VirtualAudioSolution {
        name: "Loopback",
        identifiers: &["loopback", "rogue amoeba"],
        platform: "macOS",
        download_url: Some("https://rogueamoeba.com/loopback/"),
        is_free: false,
    },
    
    // Linux solutions
    VirtualAudioSolution {
        name: "JACK Audio",
        identifiers: &["jack", "jackd"],
        platform: "Linux",
        download_url: Some("https://jackaudio.org/"),
        is_free: true,
    },
    VirtualAudioSolution {
        name: "PulseAudio Module Loopback", 
        identifiers: &["module-loopback", "pulse loopback"],
        platform: "Linux",
        download_url: None,
        is_free: true,
    },
];

/// Detect virtual audio cable devices from all available audio devices
pub fn detect_virtual_audio_devices(host: &Host) -> Result<Vec<ExtendedAudioDevice>, String> {
    println!("🔍 Scanning for virtual audio cable devices...");
    
    let mut virtual_devices = Vec::new();
    
    // Check input devices
    let input_devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?;
        
    for (i, device) in input_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        if let Some(solution) = identify_virtual_device(&name) {
            virtual_devices.push(create_virtual_device_info(
                format!("virtual_input_{}", i),
                &name,
                solution,
                true, // is_input
                &device,
            )?);
        }
    }
    
    // Check output devices (some virtual solutions create output devices too)
    let output_devices = host.output_devices()
        .map_err(|e| format!("Failed to get output devices: {}", e))?;
        
    for (i, device) in output_devices.enumerate() {
        let name = device.name().unwrap_or_default();
        if let Some(solution) = identify_virtual_device(&name) {
            // Only add if it's an input-capable virtual device
            if is_input_capable_virtual_device(&name) {
                virtual_devices.push(create_virtual_device_info(
                    format!("virtual_output_{}", i),
                    &name,
                    solution,
                    false, // is_input
                    &device,
                )?);
            }
        }
    }
    
    println!("✅ Found {} virtual audio devices", virtual_devices.len());
    Ok(virtual_devices)
}

/// Identify which virtual audio solution a device belongs to
fn identify_virtual_device(device_name: &str) -> Option<&'static VirtualAudioSolution> {
    let name_lower = device_name.to_lowercase();
    
    for solution in VIRTUAL_AUDIO_SOLUTIONS {
        // Only check solutions for current platform
        if !is_current_platform(solution.platform) {
            continue;
        }
        
        for identifier in solution.identifiers {
            if name_lower.contains(&identifier.to_lowercase()) {
                return Some(solution);
            }
        }
    }
    
    None
}

/// Check if this virtual device can be used for input (recording)
fn is_input_capable_virtual_device(device_name: &str) -> bool {
    let name_lower = device_name.to_lowercase();
    
    // VB-Audio devices that can be used for input
    if name_lower.contains("cable") && name_lower.contains("output") {
        return true;
    }
    
    // VoiceMeeter VAIO devices
    if name_lower.contains("vaio") {
        return true;
    }
    
    // BlackHole devices
    if name_lower.contains("blackhole") {
        return true;
    }
    
    // Soundflower devices
    if name_lower.contains("soundflower") {
        return true;
    }
    
    // Generally, if it's a virtual device and doesn't explicitly say "input", 
    // it might be an output that we can route audio through
    false
}

/// Create device info for a virtual audio device
fn create_virtual_device_info(
    id: String,
    name: &str,
    solution: &VirtualAudioSolution,
    is_input: bool,
    device: &cpal::Device,
) -> Result<ExtendedAudioDevice, String> {
    let config = if is_input {
        device.default_input_config().ok()
    } else {
        device.default_output_config().ok()
    };
    
    Ok(ExtendedAudioDevice {
        id: id.clone(),
        name: format!("{} (Virtual Audio)", name),
        device_type: AudioDeviceType::VirtualCable,
        is_default: false,
        sample_rate: config.as_ref().map(|c| c.sample_rate().0),
        channels: config.as_ref().map(|c| c.channels()),
        is_available: true,
        platform_info: Some(PlatformDeviceInfo {
            platform: solution.platform.to_string(),
            device_id: id,
            driver_name: Some(solution.name.to_string()),
            is_loopback_capable: true,
            supports_exclusive_mode: false,
        }),
    })
}

/// Check if a platform matches the current system
fn is_current_platform(platform: &str) -> bool {
    #[cfg(target_os = "windows")]
    return platform == "Windows";
    #[cfg(target_os = "macos")]
    return platform == "macOS";
    #[cfg(target_os = "linux")]
    return platform == "Linux";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return false;
}

/// Get installation suggestions for virtual audio solutions
pub fn get_virtual_audio_suggestions() -> Vec<VirtualAudioSuggestion> {
    let mut suggestions = Vec::new();
    
    for solution in VIRTUAL_AUDIO_SOLUTIONS {
        if is_current_platform(solution.platform) {
            suggestions.push(VirtualAudioSuggestion {
                name: solution.name.to_string(),
                description: get_solution_description(solution),
                download_url: solution.download_url.map(|s| s.to_string()),
                is_free: solution.is_free,
                difficulty: get_solution_difficulty(solution),
                recommended_for_meetings: is_recommended_for_meetings(solution),
            });
        }
    }
    
    // Sort by recommendation for meetings, then by ease of use
    suggestions.sort_by(|a, b| {
        match (a.recommended_for_meetings, b.recommended_for_meetings) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.difficulty.cmp(&b.difficulty),
        }
    });
    
    suggestions
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VirtualAudioSuggestion {
    pub name: String,
    pub description: String,
    pub download_url: Option<String>,
    pub is_free: bool,
    pub difficulty: u8, // 1-5, where 1 is easiest
    pub recommended_for_meetings: bool,
}

fn get_solution_description(solution: &VirtualAudioSolution) -> String {
    match solution.name {
        "VB-Audio Virtual Cable" => "Simple virtual audio cable for routing audio between applications. Perfect for meeting recording.".to_string(),
        "VoiceMeeter" => "Advanced audio mixer with virtual cables. Great for complex audio routing but may be overkill for simple meeting recording.".to_string(),
        "BlackHole" => "Modern virtual audio driver for macOS. Excellent for system audio capture and meeting recording.".to_string(),
        "Soundflower" => "Classic macOS virtual audio solution. Reliable but less actively maintained than BlackHole.".to_string(),
        "Loopback" => "Professional audio routing app for macOS. Very powerful but paid software.".to_string(),
        "JACK Audio" => "Professional audio system for Linux. Very powerful but complex to set up.".to_string(),
        "PulseAudio Module Loopback" => "Built-in PulseAudio solution for creating virtual audio devices.".to_string(),
        _ => "Virtual audio solution for routing audio between applications.".to_string(),
    }
}

fn get_solution_difficulty(solution: &VirtualAudioSolution) -> u8 {
    match solution.name {
        "VB-Audio Virtual Cable" => 2, // Easy to install and use
        "BlackHole" => 2, // Easy installation on macOS
        "VoiceMeeter" => 3, // More complex interface
        "Soundflower" => 2, // Simple but older  
        "Loopback" => 2, // Easy but paid
        "JACK Audio" => 5, // Complex setup
        "PulseAudio Module Loopback" => 4, // Requires command line
        _ => 3,
    }
}

fn is_recommended_for_meetings(solution: &VirtualAudioSolution) -> bool {
    matches!(solution.name, 
        "VB-Audio Virtual Cable" | 
        "BlackHole" | 
        "Soundflower"
    )
}

/// Check if any virtual audio devices are currently installed
pub fn check_virtual_audio_installation() -> VirtualAudioStatus {
    let host = cpal::default_host();
    
    let mut found_solutions = Vec::new();
    let mut total_devices = 0;
    
    // Check input devices
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            total_devices += 1;
            if let Ok(name) = device.name() {
                if let Some(solution) = identify_virtual_device(&name) {
                    if !found_solutions.iter().any(|s: &String| s == solution.name) {
                        found_solutions.push(solution.name.to_string());
                    }
                }
            }
        }
    }
    
    // Check output devices  
    if let Ok(output_devices) = host.output_devices() {
        for device in output_devices {
            total_devices += 1;
            if let Ok(name) = device.name() {
                if let Some(solution) = identify_virtual_device(&name) {
                    if !found_solutions.iter().any(|s: &String| s == solution.name) {
                        found_solutions.push(solution.name.to_string());
                    }
                }
            }
        }
    }
    
    VirtualAudioStatus {
        has_virtual_devices: !found_solutions.is_empty(),
        installed_solutions: found_solutions,
        total_audio_devices: total_devices,
        recommended_for_platform: get_platform_recommendation(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VirtualAudioStatus {
    pub has_virtual_devices: bool,
    pub installed_solutions: Vec<String>,
    pub total_audio_devices: usize,
    pub recommended_for_platform: Option<String>,
}

fn get_platform_recommendation() -> Option<String> {
    #[cfg(target_os = "windows")]
    return Some("VB-Audio Virtual Cable".to_string());
    #[cfg(target_os = "macos")]
    return Some("BlackHole".to_string());
    #[cfg(target_os = "linux")]
    return Some("PulseAudio Monitor (built-in)".to_string());
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return None;
}