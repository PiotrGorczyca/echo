use tauri::{AppHandle, State, Emitter};
use std::sync::{Arc, Mutex};

use crate::meeting::{
    MeetingRecordingConfig, Meeting, MeetingRecordingState,
    storage::MeetingSummary,
    service::{MeetingProcessingStatus, MeetingStatistics},
};
use crate::state::AppState;

#[tauri::command]
pub async fn start_meeting(
    title: String,
    participants: Vec<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<String, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    let meeting_id = meeting_service.start_meeting(title.clone(), participants.clone()).await?;

    // Emit event to frontend
    app.emit("meeting-started", serde_json::json!({
        "meeting_id": meeting_id,
        "title": title,
        "participants": participants
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(meeting_id)
}

#[tauri::command]
pub async fn start_meeting_recording(
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    // Use the existing recording system instead of separate meeting recording
    let recording_manager = unsafe {
        crate::RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Check if we can start recording
    recording_manager.can_start_recording(&crate::recording_manager::RecordingMode::Meeting)?;

    // Create temporary file for meeting recording
    let temp_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let (_file, persistent_path) = temp_file.keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e))?;
    let final_temp_path = persistent_path.to_string_lossy().to_string();

    let config = crate::recording_manager::RecordingConfig {
        mode: crate::recording_manager::RecordingMode::Meeting,
        auto_stop_duration_ms: None, // No auto-stop for meetings
        temp_file_path: final_temp_path.clone(),
    };

    // Start recording state management
    recording_manager.start_recording(config)?;

    // Get device ID from settings
    let device_id = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.settings.selected_device_id.clone()
    };

    // Start actual audio recording using existing infrastructure
    if let Err(e) = crate::start_actual_recording(&device_id, &final_temp_path, 16000, 1).await {
        // If audio recording fails, clean up the recording state
        let _ = recording_manager.stop_recording();
        return Err(e);
    }

    // Meeting service doesn't need to handle recording anymore since we use the global system
    // The global recording system will handle everything and call back to meeting service when done

    // Emit event to frontend
    app.emit("meeting-recording-started", serde_json::json!({
        "timestamp": chrono::Utc::now(),
        "file_path": final_temp_path
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn pause_meeting_recording(
    _state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    // For now, pause is not fully supported by the global recording system
    // We could extend this later to pause the actual audio recording
    // For now, just acknowledge the pause request
    println!("📝 Meeting recording pause requested (not fully implemented yet)");

    // Emit event to frontend
    app.emit("meeting-recording-paused", serde_json::json!({
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn resume_meeting_recording(
    _state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    // For now, resume is not fully supported by the global recording system
    // We could extend this later to resume the actual audio recording
    // For now, just acknowledge the resume request
    println!("📝 Meeting recording resume requested (not fully implemented yet)");

    // Emit event to frontend
    app.emit("meeting-recording-resumed", serde_json::json!({
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn stop_meeting_recording(
    _state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    // Use the existing recording system
    let recording_manager = unsafe {
        crate::RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Stop the global recording system - this will trigger transcription automatically
    let (mode, temp_file_path) = recording_manager.stop_recording()?;
    
    // Verify this was a meeting recording
    if mode != crate::recording_manager::RecordingMode::Meeting {
        return Err(format!("Expected meeting recording, found {:?}", mode));
    }

    // Stop actual audio recording
    crate::stop_actual_recording().await?;

    // Meeting service doesn't need to handle recording stop since global system handles it
    // The transcription will be automatically processed and saved to the meeting

    // The existing transcription system will handle the audio file automatically
    // through handle_recording_transcription for RecordingMode::Meeting

    // Emit event to frontend
    app.emit("meeting-recording-stopped", serde_json::json!({
        "timestamp": chrono::Utc::now(),
        "file_path": temp_file_path
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn end_meeting(
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<Meeting, String> {
    // First, check if we're currently recording a meeting and stop it
    let recording_manager = unsafe {
        crate::RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Check if we're recording a meeting
    let was_recording = if let Ok(current_state) = recording_manager.get_state() {
        current_state.is_recording && current_state.mode == Some(crate::recording_manager::RecordingMode::Meeting)
    } else {
        false
    };

    // Stop recording if it's active
    if was_recording {
        println!("🛑 Stopping meeting recording as part of ending meeting...");
        
        // Stop the global recording system
        let (mode, temp_file_path) = recording_manager.stop_recording()?;
        
        // Verify this was a meeting recording
        if mode != crate::recording_manager::RecordingMode::Meeting {
            println!("⚠️ Expected meeting recording, found {:?}", mode);
        }

        // Stop actual audio recording
        crate::stop_actual_recording().await?;

        println!("✅ Meeting recording stopped, starting transcription...");
        
        // Emit recording stopped event
        app.emit("meeting-recording-stopped", serde_json::json!({
            "timestamp": chrono::Utc::now(),
            "file_path": temp_file_path.clone()
        })).map_err(|e| format!("Failed to emit recording stopped event: {}", e))?;

        // Trigger transcription manually (since we bypassed stop_recording_new)
        let app_state = state.inner().clone();
        let app_handle = app.clone();
        let file_path = temp_file_path.clone();
        let mode_for_transcription = mode.clone();
        
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::handle_recording_transcription(app_state, app_handle, file_path, mode_for_transcription).await {
                println!("❌ Failed to handle meeting transcription: {}", e);
            }
        });

        println!("📝 Transcription task spawned for meeting audio");
    }

    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    // Get current meeting before ending it
    let meeting = match meeting_service.get_current_meeting().await? {
        Some(mut meeting) => {
            // End the meeting manually without triggering old processing logic
            meeting.end_time = Some(chrono::Utc::now());
            meeting.duration = meeting.end_time.map(|end| end.signed_duration_since(meeting.start_time));
            meeting.status = crate::meeting::MeetingStatus::Completed;
            
            // Save the meeting
            meeting_service.get_storage().save_meeting(&meeting).await
                .map_err(|e| format!("Failed to save meeting: {}", e))?;
            
            // Clear current meeting from recording manager
            if let Err(e) = meeting_service.clear_current_meeting().await {
                println!("⚠️ Failed to clear current meeting: {}", e);
            }
            
            println!("✅ Meeting ended: {}", meeting.title);
            
            meeting
        }
        None => {
            return Err("No meeting in progress".to_string());
        }
    };

    // Emit event to frontend
    app.emit("meeting-ended", serde_json::json!({
        "meeting_id": meeting.id,
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(meeting)
}

#[tauri::command]
pub async fn get_current_meeting(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<Meeting>, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.get_current_meeting().await
}

#[tauri::command]
pub async fn get_meeting_recording_state(
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<MeetingRecordingState, String> {
    // Use the global recording manager to get the actual recording state
    let recording_manager = unsafe {
        crate::RECORDING_MANAGER.as_ref()
            .ok_or("Recording manager not initialized")?
            .clone()
    };

    // Convert the global recording state to meeting recording state format
    let global_state = recording_manager.get_state()?;
    
    let meeting_recording_state = MeetingRecordingState {
        is_recording: global_state.is_recording && 
                     global_state.mode == Some(crate::recording_manager::RecordingMode::Meeting),
        is_paused: false, // Global system doesn't support pause yet
        current_chunk_number: if global_state.is_recording { 1 } else { 0 },
        current_chunk_path: global_state.temp_file_path.map(|p| std::path::PathBuf::from(p)),
        current_chunk_start_time: global_state.start_time.map(|t| {
            chrono::DateTime::from_timestamp(t as i64 / 1000, ((t % 1000) * 1_000_000) as u32)
                .unwrap_or_else(chrono::Utc::now)
        }),
        total_recording_duration: chrono::Duration::zero(), // Would need to calculate from start_time
        last_save_time: None,
    };

    Ok(meeting_recording_state)
}

#[tauri::command]
pub async fn list_meetings(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<MeetingSummary>, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.list_meetings().await
}

#[tauri::command]
pub async fn get_meeting(
    meeting_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Meeting, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.get_meeting(&meeting_id).await
}

#[tauri::command]
pub async fn delete_meeting(
    meeting_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.delete_meeting(&meeting_id).await?;

    // Emit event to frontend
    app.emit("meeting-deleted", serde_json::json!({
        "meeting_id": meeting_id,
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command] 
pub async fn rotate_meeting_chunk(
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    if meeting_service.rotate_chunk_if_needed().await? {
        
        // Emit event to frontend
        app.emit("meeting-chunk-rotated", serde_json::json!({
            "timestamp": chrono::Utc::now()
        })).map_err(|e| format!("Failed to emit event: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_meeting_config() -> Result<MeetingRecordingConfig, String> {
    Ok(MeetingRecordingConfig::default())
}

#[tauri::command]
pub async fn update_meeting_config(
    config: MeetingRecordingConfig,
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    // For now, we'll just validate the config
    // In a full implementation, you'd save this to persistent storage
    if config.chunk_duration_minutes == 0 {
        return Err("Chunk duration must be greater than 0".to_string());
    }
    
    if config.max_meeting_duration_hours == 0 {
        return Err("Max meeting duration must be greater than 0".to_string());
    }

    // TODO: Update the meeting manager with new config
    // This would require refactoring the manager to accept config updates
    
    Ok(())
}

#[tauri::command]
pub async fn get_meeting_processing_status(
    meeting_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<MeetingProcessingStatus>, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    Ok(meeting_service.get_processing_status(&meeting_id).await)
}

#[tauri::command]
pub async fn get_all_processing_statuses(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<std::collections::HashMap<String, MeetingProcessingStatus>, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    Ok(meeting_service.get_all_processing_statuses().await)
}

#[tauri::command]
pub async fn retry_meeting_processing(
    meeting_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.retry_meeting_processing(&meeting_id).await?;

    // Emit event to frontend
    app.emit("meeting-processing-restarted", serde_json::json!({
        "meeting_id": meeting_id,
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn cancel_meeting_processing(
    meeting_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.cancel_meeting_processing(&meeting_id).await?;

    // Emit event to frontend
    app.emit("meeting-processing-cancelled", serde_json::json!({
        "meeting_id": meeting_id,
        "timestamp": chrono::Utc::now()
    })).map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_meeting_statistics(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<MeetingStatistics, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    meeting_service.get_meeting_statistics().await
}

#[tauri::command]
pub async fn force_chunk_rotation(
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<bool, String> {
    let meeting_service = {
        let app_state = state.inner().lock()
            .map_err(|e| format!("Failed to lock app state: {}", e))?;
        app_state.meeting_service.clone()
    };

    let rotated = meeting_service.rotate_chunk_if_needed().await?;

    if rotated {
        // Emit event to frontend
        app.emit("meeting-chunk-rotated", serde_json::json!({
            "timestamp": chrono::Utc::now(),
            "forced": true
        })).map_err(|e| format!("Failed to emit event: {}", e))?;
    }

    Ok(rotated)
}