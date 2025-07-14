use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecordingMode {
    Transcription,
    VoiceCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    pub mode: RecordingMode,
    pub auto_stop_duration_ms: Option<u64>,
    pub temp_file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    pub is_recording: bool,
    pub mode: Option<RecordingMode>,
    pub start_time: Option<u64>, // timestamp in ms
    pub temp_file_path: Option<String>,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            is_recording: false,
            mode: None,
            start_time: None,
            temp_file_path: None,
        }
    }
}

#[derive(Debug)]
pub struct RecordingManager {
    state: Arc<Mutex<RecordingState>>,
}

impl RecordingManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::default())),
        }
    }

    pub fn start_recording(&self, config: RecordingConfig) -> Result<(), String> {
        let mut state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        
        if state.is_recording {
            return Err(format!("Already recording in {:?} mode", state.mode));
        }

        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        *state = RecordingState {
            is_recording: true,
            mode: Some(config.mode),
            start_time: Some(start_time),
            temp_file_path: Some(config.temp_file_path),
        };

        Ok(())
    }

    pub fn stop_recording(&self) -> Result<(RecordingMode, String), String> {
        let mut state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        
        if !state.is_recording {
            return Err("Not currently recording".to_string());
        }

        let mode = state.mode.clone().unwrap();
        let temp_file_path = state.temp_file_path.clone().unwrap();

        *state = RecordingState::default();

        Ok((mode, temp_file_path))
    }

    pub fn is_recording(&self) -> Result<bool, String> {
        let state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        Ok(state.is_recording)
    }

    pub fn get_current_mode(&self) -> Result<Option<RecordingMode>, String> {
        let state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        Ok(state.mode.clone())
    }

    pub fn get_state(&self) -> Result<RecordingState, String> {
        let state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        Ok(state.clone())
    }

    pub fn can_start_recording(&self, mode: &RecordingMode) -> Result<bool, String> {
        let state = self.state.lock()
            .map_err(|e| format!("Failed to lock recording state: {}", e))?;
        
        if !state.is_recording {
            return Ok(true);
        }

        // Check if we're trying to start the same mode that's already recording
        if let Some(current_mode) = &state.mode {
            if current_mode == mode {
                return Err(format!("Already recording in {:?} mode", mode));
            } else {
                return Err(format!("Cannot start {:?} recording while {:?} is active", mode, current_mode));
            }
        }

        Ok(false)
    }
}

// Async version for use in tokio tasks
pub struct AsyncRecordingManager {
    inner: Arc<RecordingManager>,
}

impl AsyncRecordingManager {
    pub fn new(manager: Arc<RecordingManager>) -> Self {
        Self { inner: manager }
    }

    pub async fn start_recording(&self, config: RecordingConfig) -> Result<(), String> {
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.start_recording(config))
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn stop_recording(&self) -> Result<(RecordingMode, String), String> {
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.stop_recording())
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn is_recording(&self) -> Result<bool, String> {
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.is_recording())
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn get_current_mode(&self) -> Result<Option<RecordingMode>, String> {
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.get_current_mode())
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn get_state(&self) -> Result<RecordingState, String> {
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.get_state())
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn can_start_recording(&self, mode: &RecordingMode) -> Result<bool, String> {
        let mode = mode.clone();
        let manager = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || manager.can_start_recording(&mode))
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }
} 