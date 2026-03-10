use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod storage;
pub mod transcription;
pub mod service;
pub mod ai_processor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration: Option<Duration>,
    pub participants: Vec<String>,
    pub audio_chunks: Vec<AudioChunk>,
    pub transcript: Option<String>,
    pub action_items: Vec<ActionItem>,
    pub status: MeetingStatus,
    pub audio_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioChunk {
    pub id: String,
    pub chunk_number: u32,
    pub file_path: PathBuf,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub duration_seconds: f64,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItem {
    pub id: String,
    pub meeting_id: String,
    pub text: String,
    pub assignee: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub priority: Priority,
    pub category: ActionItemType,
    pub context: String,
    pub status: ActionItemStatus,
    pub timestamp_in_meeting: Option<f64>, // seconds from meeting start
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeetingStatus {
    Scheduled,
    InProgress,
    Recording,
    Paused,
    Processing, // Transcribing and extracting action items
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionItemType {
    Task,
    Decision,
    FollowUp,
    Question,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionItemStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingRecordingConfig {
    pub chunk_duration_minutes: u32, // Split audio into chunks of this duration
    pub max_meeting_duration_hours: u32,
    pub audio_quality: AudioQuality,
    pub auto_save_interval_minutes: u32,
    pub voice_activation_enabled: bool,
    pub silence_detection_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioQuality {
    Low,    // 16kHz, mono
    Medium, // 44.1kHz, mono
    High,   // 48kHz, stereo
}

impl Default for MeetingRecordingConfig {
    fn default() -> Self {
        Self {
            chunk_duration_minutes: 15, // 15-minute chunks
            max_meeting_duration_hours: 8, // 8-hour max
            audio_quality: AudioQuality::Medium,
            auto_save_interval_minutes: 5,
            voice_activation_enabled: true,
            silence_detection_threshold: 0.02,
        }
    }
}

#[derive(Debug)]
pub struct MeetingRecordingManager {
    current_meeting: Arc<Mutex<Option<Meeting>>>,
    config: MeetingRecordingConfig,
    recording_state: Arc<Mutex<MeetingRecordingState>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingRecordingState {
    pub is_recording: bool,
    pub is_paused: bool,
    pub current_chunk_number: u32,
    pub current_chunk_path: Option<PathBuf>,
    pub current_chunk_start_time: Option<DateTime<Utc>>,
    pub total_recording_duration: Duration,
    pub last_save_time: Option<DateTime<Utc>>,
}

impl Default for MeetingRecordingState {
    fn default() -> Self {
        Self {
            is_recording: false,
            is_paused: false,
            current_chunk_number: 0,
            current_chunk_path: None,
            current_chunk_start_time: None,
            total_recording_duration: Duration::zero(),
            last_save_time: None,
        }
    }
}

impl MeetingRecordingManager {
    pub fn new(config: MeetingRecordingConfig) -> Self {
        Self {
            current_meeting: Arc::new(Mutex::new(None)),
            config,
            recording_state: Arc::new(Mutex::new(MeetingRecordingState::default())),
        }
    }

    pub fn start_meeting(&self, title: String, participants: Vec<String>) -> Result<String, String> {
        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        if meeting_guard.is_some() {
            return Err("A meeting is already in progress".to_string());
        }

        let meeting_id = Uuid::new_v4().to_string();
        let audio_directory = self.get_meeting_audio_directory(&meeting_id);
        
        // Create audio directory
        std::fs::create_dir_all(&audio_directory)
            .map_err(|e| format!("Failed to create meeting directory: {}", e))?;

        let meeting = Meeting {
            id: meeting_id.clone(),
            title,
            start_time: Utc::now(),
            end_time: None,
            duration: None,
            participants,
            audio_chunks: Vec::new(),
            transcript: None,
            action_items: Vec::new(),
            status: MeetingStatus::Scheduled,
            audio_directory,
        };

        *meeting_guard = Some(meeting);
        Ok(meeting_id)
    }

    pub fn start_recording(&self) -> Result<(), String> {
        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        let meeting = meeting_guard.as_mut()
            .ok_or("No meeting in progress")?;

        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if state_guard.is_recording && !state_guard.is_paused {
            return Err("Already recording".to_string());
        }

        // Start new chunk or resume
        if !state_guard.is_recording {
            state_guard.current_chunk_number += 1;
            state_guard.current_chunk_start_time = Some(Utc::now());
        }

        let chunk_filename = format!("chunk_{:03}.wav", state_guard.current_chunk_number);
        let chunk_path = meeting.audio_directory.join(chunk_filename);
        
        state_guard.current_chunk_path = Some(chunk_path);
        state_guard.is_recording = true;
        state_guard.is_paused = false;
        
        meeting.status = MeetingStatus::Recording;

        Ok(())
    }

    pub fn pause_recording(&self) -> Result<(), String> {
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if !state_guard.is_recording {
            return Err("Not currently recording".to_string());
        }

        state_guard.is_paused = true;

        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        if let Some(meeting) = meeting_guard.as_mut() {
            meeting.status = MeetingStatus::Paused;
        }

        Ok(())
    }

    pub fn resume_recording(&self) -> Result<(), String> {
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if !state_guard.is_recording || !state_guard.is_paused {
            return Err("Recording is not paused".to_string());
        }

        state_guard.is_paused = false;

        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        if let Some(meeting) = meeting_guard.as_mut() {
            meeting.status = MeetingStatus::Recording;
        }

        Ok(())
    }

    pub fn stop_recording(&self) -> Result<(), String> {
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if !state_guard.is_recording {
            return Err("Not currently recording".to_string());
        }

        // Finalize current chunk
        if let Some(chunk_path) = &state_guard.current_chunk_path {
            if let Some(start_time) = state_guard.current_chunk_start_time {
                let end_time = Utc::now();
                let duration = end_time.signed_duration_since(start_time);
                
                // Only create chunk if we actually have some recording duration
                if duration.num_seconds() > 0 {
                    let chunk = AudioChunk {
                        id: Uuid::new_v4().to_string(),
                        chunk_number: state_guard.current_chunk_number,
                        file_path: chunk_path.clone(),
                        start_timestamp: start_time,
                        end_timestamp: Some(end_time),
                        duration_seconds: duration.num_milliseconds() as f64 / 1000.0,
                        file_size_bytes: self.get_file_size(chunk_path).unwrap_or(0),
                    };

                    let mut meeting_guard = self.current_meeting.lock()
                        .map_err(|e| format!("Failed to lock meeting: {}", e))?;
                    
                    if let Some(meeting) = meeting_guard.as_mut() {
                        meeting.audio_chunks.push(chunk);
                    }
                }
            }
        }

        state_guard.is_recording = false;
        state_guard.is_paused = false;
        state_guard.current_chunk_path = None;
        state_guard.current_chunk_start_time = None;

        Ok(())
    }

    pub fn end_meeting(&self) -> Result<Meeting, String> {
        self.stop_recording()?;

        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        let mut meeting = meeting_guard.take()
            .ok_or("No meeting in progress")?;

        meeting.end_time = Some(Utc::now());
        meeting.duration = meeting.end_time.map(|end| end.signed_duration_since(meeting.start_time));
        meeting.status = MeetingStatus::Processing;

        // Reset recording state
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        *state_guard = MeetingRecordingState::default();

        Ok(meeting)
    }

    pub fn should_create_new_chunk(&self) -> Result<bool, String> {
        let state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if let Some(start_time) = state_guard.current_chunk_start_time {
            let current_duration = Utc::now().signed_duration_since(start_time);
            let chunk_limit = Duration::minutes(self.config.chunk_duration_minutes as i64);
            Ok(current_duration >= chunk_limit)
        } else {
            Ok(false)
        }
    }

    pub fn rotate_chunk(&self) -> Result<(), String> {
        if !self.should_create_new_chunk()? {
            return Ok(());
        }

        // Finalize current chunk
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;

        if let Some(chunk_path) = &state_guard.current_chunk_path {
            if let Some(start_time) = state_guard.current_chunk_start_time {
                let end_time = Utc::now();
                let duration = end_time.signed_duration_since(start_time);
                
                // Only create chunk if we actually have some recording duration
                if duration.num_seconds() > 0 {
                    let chunk = AudioChunk {
                        id: Uuid::new_v4().to_string(),
                        chunk_number: state_guard.current_chunk_number,
                        file_path: chunk_path.clone(),
                        start_timestamp: start_time,
                        end_timestamp: Some(end_time),
                        duration_seconds: duration.num_milliseconds() as f64 / 1000.0,
                        file_size_bytes: self.get_file_size(chunk_path).unwrap_or(0),
                    };

                    let mut meeting_guard = self.current_meeting.lock()
                        .map_err(|e| format!("Failed to lock meeting: {}", e))?;
                    
                    if let Some(meeting) = meeting_guard.as_mut() {
                        meeting.audio_chunks.push(chunk);
                    }
                }
            }
        }

        // Start new chunk
        state_guard.current_chunk_number += 1;
        state_guard.current_chunk_start_time = Some(Utc::now());

        let meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        
        if let Some(meeting) = meeting_guard.as_ref() {
            let chunk_filename = format!("chunk_{:03}.wav", state_guard.current_chunk_number);
            let chunk_path = meeting.audio_directory.join(chunk_filename);
            state_guard.current_chunk_path = Some(chunk_path);
        }

        Ok(())
    }

    pub fn get_current_meeting(&self) -> Result<Option<Meeting>, String> {
        let meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        Ok(meeting_guard.clone())
    }

    pub fn get_recording_state(&self) -> Result<MeetingRecordingState, String> {
        let state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        Ok(state_guard.clone())
    }

    fn get_meeting_audio_directory(&self, meeting_id: &str) -> PathBuf {
        std::env::temp_dir().join("echo_meeting_audio").join(meeting_id)
    }

    fn get_file_size(&self, path: &PathBuf) -> Result<u64, String> {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(_) => {
                // File doesn't exist yet (audio recording not implemented)
                // Return 0 for now - this will be updated when actual audio capture is implemented
                println!("⚠️  Audio file not found at {:?} - audio recording not yet implemented", path);
                Ok(0)
            }
        }
    }

    pub fn clear_current_meeting(&self) -> Result<(), String> {
        let mut meeting_guard = self.current_meeting.lock()
            .map_err(|e| format!("Failed to lock meeting: {}", e))?;
        *meeting_guard = None;
        
        // Also reset recording state
        let mut state_guard = self.recording_state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        *state_guard = MeetingRecordingState::default();
        
        Ok(())
    }
}