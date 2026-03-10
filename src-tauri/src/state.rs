use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use crate::transcription::{TranscriptionService, TranscriptionMode, WhisperModelSize, DeviceType};
use crate::voice_activation::VoiceActivationService;
use crate::voice_command::VoiceCommandService;
use crate::mcp::McpClient;
use crate::ai_agent::AiAgentCore;
use crate::meeting::{service::MeetingService, MeetingRecordingConfig};
use crate::history::HistoryManager;


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

/// Preferred IDE for "Work on this" feature
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub enum PreferredIde {
    #[default]
    ClaudeCode,
    Cursor,
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
    // Voice activation settings
    pub enable_voice_activation: bool,
    pub wake_words: Vec<String>,
    pub listening_device_id: Option<String>,
    pub wake_word_sensitivity: f32, // 0.0 to 1.0
    pub wake_word_timeout_ms: u64, // How long to wait after wake word before starting recording
    pub voice_energy_threshold: Option<f32>, // Custom energy threshold for voice detection
    pub auto_calibrate_threshold: bool, // Whether to auto-calibrate the energy threshold
    pub wake_word_model_size: WhisperModelSize, // Model size for wake word detection
    pub terminal_emulator: Option<String>,
    // IDE preference for "Work on this" feature
    #[serde(default)]
    pub preferred_ide: PreferredIde,
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

// Main application state
pub struct AppState {
    // Legacy recording state (kept for compatibility during transition)
    pub is_recording: bool,
    pub current_device_name: String,
    pub temp_file_path: Option<String>,
    pub settings: AppSettings,
    pub last_alt_press: Option<Instant>,
    pub alt_tap_count: u32,
    pub recording_start_time: Option<Instant>,
    pub transcription_service: Option<Arc<TranscriptionService>>,
    // Voice activation state
    pub is_voice_listening: bool,
    pub wake_word_detected_at: Option<Instant>,
    pub voice_activation_service: Option<Arc<Mutex<VoiceActivationService>>>,
    // Voice command state
    pub voice_command_service: Option<Arc<VoiceCommandService>>,
    // AI Agent state
    pub ai_agent: Option<Arc<AiAgentCore>>,
    pub mcp_client: Option<Arc<McpClient>>,
    // Meeting state
    pub meeting_service: Arc<MeetingService>,
    // Test recording mode (skips transcription)
    pub is_test_recording: bool,
    // Transcription history
    pub history_manager: Option<Arc<HistoryManager>>,
    // Watcher for Cursor handoff
    pub handoff_watcher: Option<Arc<Mutex<crate::workspace::CursorHandoffWatcher>>>,
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
                transcription_mode: TranscriptionMode::FasterWhisper,
                whisper_model_size: WhisperModelSize::LargeTurboQ5,
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
                terminal_emulator: None,
                preferred_ide: PreferredIde::default(),
            },
            last_alt_press: None,
            alt_tap_count: 0,
            recording_start_time: None,
            transcription_service: None,
            is_voice_listening: false,
            wake_word_detected_at: None,
            voice_activation_service: None,
            voice_command_service: None,
            ai_agent: None,
            mcp_client: None,
            meeting_service: Arc::new(MeetingService::new(
                None, // Will be connected to main transcription service when initialized
                std::env::temp_dir().join("echo_meetings"),
                MeetingRecordingConfig::default(),
                None, // API key will be set later when available
            ).unwrap()),
            is_test_recording: false,
            history_manager: None,
            handoff_watcher: None,
        }
    }
}
