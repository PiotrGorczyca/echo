use std::fs;
use std::path::PathBuf;
use crate::state::AppSettings;
use crate::transcription::{TranscriptionMode, WhisperModelSize, DeviceType};

pub fn get_settings_file_path() -> Result<PathBuf, String> {
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

pub fn load_settings_from_file() -> AppSettings {
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
        enable_voice_activation: false,
        wake_words: Vec::new(),
        listening_device_id: None,
        wake_word_sensitivity: 0.5,
        wake_word_timeout_ms: 1000,
        voice_energy_threshold: None,
        auto_calibrate_threshold: true,
        wake_word_model_size: WhisperModelSize::Base,
        user_mcp_servers: Vec::new(),
    }
}

pub fn save_settings_to_file(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_file_path()?;
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;
    
    println!("Settings saved to: {}", path.display());
    Ok(())
} 