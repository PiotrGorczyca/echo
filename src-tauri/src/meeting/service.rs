use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::transcription::TranscriptionService;
use super::{
    Meeting, MeetingRecordingManager, MeetingRecordingConfig, MeetingStatus,
    storage::{MeetingStorage, MeetingSummary},
    transcription::MeetingTranscriptionPipeline,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingProcessingStatus {
    pub meeting_id: String,
    pub status: ProcessingState,
    pub progress: f64, // 0.0 to 1.0
    pub current_step: String,
    pub estimated_completion: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingState {
    Pending,
    Transcribing,
    ExtractingActionItems,
    Finalizing,
    Completed,
    Failed,
}

pub struct MeetingService {
    recording_manager: Arc<MeetingRecordingManager>,
    storage: Arc<MeetingStorage>,
    transcription_pipeline: Arc<RwLock<MeetingTranscriptionPipeline>>,
    processing_status: Arc<RwLock<HashMap<String, MeetingProcessingStatus>>>,
}

impl MeetingService {
    pub fn new(
        transcription_service: Option<Arc<TranscriptionService>>,
        storage_directory: std::path::PathBuf,
        config: MeetingRecordingConfig,
        api_key: Option<String>,
    ) -> Result<Self, String> {
        let storage = Arc::new(
            MeetingStorage::new(storage_directory)
                .map_err(|e| format!("Failed to initialize meeting storage: {}", e))?
        );

        let recording_manager = Arc::new(MeetingRecordingManager::new(config));
        
        let transcription_pipeline = Arc::new(RwLock::new(MeetingTranscriptionPipeline::new(
            transcription_service,
            Arc::clone(&storage),
            3, // Max concurrent chunks
            api_key,
        )?));

        Ok(Self {
            recording_manager,
            storage,
            transcription_pipeline,
            processing_status: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    // Meeting lifecycle methods
    pub async fn start_meeting(&self, title: String, participants: Vec<String>) -> Result<String, String> {
        self.recording_manager.start_meeting(title, participants)
    }

    pub async fn start_recording(&self) -> Result<(), String> {
        self.recording_manager.start_recording()
    }

    pub async fn pause_recording(&self) -> Result<(), String> {
        self.recording_manager.pause_recording()
    }

    pub async fn resume_recording(&self) -> Result<(), String> {
        self.recording_manager.resume_recording()
    }

    pub async fn stop_recording(&self) -> Result<(), String> {
        self.recording_manager.stop_recording()
    }

    pub async fn end_meeting(&self) -> Result<Meeting, String> {
        let meeting = self.recording_manager.end_meeting()?;
        
        // Save the meeting
        self.storage.save_meeting(&meeting).await?;
        
        // Start background processing
        self.start_meeting_processing(&meeting).await?;
        
        Ok(meeting)
    }

    // Meeting processing pipeline
    async fn start_meeting_processing(&self, meeting: &Meeting) -> Result<(), String> {
        println!("🚀 Starting background processing for meeting: {}", meeting.title);

        // Initialize processing status
        let status = MeetingProcessingStatus {
            meeting_id: meeting.id.clone(),
            status: ProcessingState::Pending,
            progress: 0.0,
            current_step: "Initializing processing".to_string(),
            estimated_completion: None,
            error_message: None,
        };

        self.update_processing_status(status).await;

        // Start processing in background
        let meeting_clone = meeting.clone();
        let pipeline = Arc::clone(&self.transcription_pipeline);
        let service = Arc::new(self.clone_for_background());

        tokio::spawn(async move {
            let meeting_id_for_error = meeting_clone.id.clone();
            let service_for_error = Arc::clone(&service);
            
            if let Err(e) = Self::process_meeting_background_static(meeting_clone, pipeline, service).await {
                println!("❌ Meeting processing failed: {}", e);
                
                let failed_status = MeetingProcessingStatus {
                    meeting_id: meeting_id_for_error,
                    status: ProcessingState::Failed,
                    progress: 0.0,
                    current_step: "Processing failed".to_string(),
                    estimated_completion: None,
                    error_message: Some(e),
                };
                
                service_for_error.update_processing_status(failed_status).await;
            }
        });

        Ok(())
    }

    async fn process_meeting_background_static(
        meeting: Meeting,
        pipeline: Arc<RwLock<MeetingTranscriptionPipeline>>,
        service_handle: Arc<MeetingServiceHandle>,
    ) -> Result<(), String> {
        let meeting_id = meeting.id.clone();

        // Step 1: Transcription
        service_handle.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.clone(),
            status: ProcessingState::Transcribing,
            progress: 0.1,
            current_step: "Transcribing audio chunks".to_string(),
            estimated_completion: Some(Utc::now() + chrono::Duration::minutes(5)),
            error_message: None,
        }).await;

        let transcript = {
            let pipeline = pipeline.read().await;
            pipeline.process_meeting(&meeting).await
                .map_err(|e| format!("Transcription failed: {}", e))?
        };

        // Step 2: Action item extraction is now handled automatically in the transcription pipeline
        service_handle.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.clone(),
            status: ProcessingState::ExtractingActionItems,
            progress: 0.7,
            current_step: "AI processing and action item extraction completed".to_string(),
            estimated_completion: Some(Utc::now() + chrono::Duration::minutes(1)),
            error_message: None,
        }).await;

        // Step 3: Finalization
        service_handle.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.clone(),
            status: ProcessingState::Finalizing,
            progress: 0.9,
            current_step: "Finalizing meeting data".to_string(),
            estimated_completion: Some(Utc::now() + chrono::Duration::seconds(30)),
            error_message: None,
        }).await;

        // Save final results
        service_handle.storage.update_transcript(&meeting_id, transcript.full_text).await?;
        service_handle.storage.update_meeting_status(&meeting_id, MeetingStatus::Completed).await?;

        // Mark as completed
        service_handle.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.clone(),
            status: ProcessingState::Completed,
            progress: 1.0,
            current_step: "Processing completed".to_string(),
            estimated_completion: None,
            error_message: None,
        }).await;

        println!("✅ Meeting processing completed: {} words transcribed", transcript.word_count);
        Ok(())
    }

    async fn update_processing_status(&self, status: MeetingProcessingStatus) {
        let mut status_map = self.processing_status.write().await;
        status_map.insert(status.meeting_id.clone(), status);
    }

    // Query methods
    pub async fn get_current_meeting(&self) -> Result<Option<Meeting>, String> {
        self.recording_manager.get_current_meeting()
    }

    pub async fn get_recording_state(&self) -> Result<super::MeetingRecordingState, String> {
        self.recording_manager.get_recording_state()
    }

    pub async fn list_meetings(&self) -> Result<Vec<MeetingSummary>, String> {
        self.storage.list_meetings().await
    }

    pub async fn get_meeting(&self, meeting_id: &str) -> Result<Meeting, String> {
        self.storage.load_meeting(meeting_id).await
    }

    pub async fn delete_meeting(&self, meeting_id: &str) -> Result<(), String> {
        // Remove from processing status if present
        {
            let mut status_map = self.processing_status.write().await;
            status_map.remove(meeting_id);
        }

        // Delete from storage
        self.storage.delete_meeting(meeting_id).await
    }

    pub async fn get_processing_status(&self, meeting_id: &str) -> Option<MeetingProcessingStatus> {
        let status_map = self.processing_status.read().await;
        status_map.get(meeting_id).cloned()
    }

    pub async fn get_all_processing_statuses(&self) -> HashMap<String, MeetingProcessingStatus> {
        let status_map = self.processing_status.read().await;
        status_map.clone()
    }

    // Chunk management
    pub async fn rotate_chunk_if_needed(&self) -> Result<bool, String> {
        if self.recording_manager.should_create_new_chunk()? {
            self.recording_manager.rotate_chunk()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // Retry and recovery methods
    pub async fn retry_meeting_processing(&self, meeting_id: &str) -> Result<(), String> {
        let meeting = self.storage.load_meeting(meeting_id).await?;
        
        // Reset status
        self.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.to_string(),
            status: ProcessingState::Pending,
            progress: 0.0,
            current_step: "Retrying processing".to_string(),
            estimated_completion: None,
            error_message: None,
        }).await;

        // Restart processing
        self.start_meeting_processing(&meeting).await
    }

    pub async fn cancel_meeting_processing(&self, meeting_id: &str) -> Result<(), String> {
        // Update status to indicate cancellation
        self.update_processing_status(MeetingProcessingStatus {
            meeting_id: meeting_id.to_string(),
            status: ProcessingState::Failed,
            progress: 0.0,
            current_step: "Processing cancelled".to_string(),
            estimated_completion: None,
            error_message: Some("Processing was cancelled by user".to_string()),
        }).await;

        // Note: This doesn't actually cancel running tasks - 
        // a more sophisticated implementation would use tokio::sync::CancellationToken
        
        Ok(())
    }

    // Transcription service management
    pub async fn set_transcription_service(&self, service: Arc<TranscriptionService>) -> Result<(), String> {
        let mut pipeline = self.transcription_pipeline.write().await;
        pipeline.set_transcription_service(service);
        println!("✅ Updated transcription service for meeting service");
        Ok(())
    }

    pub async fn has_transcription_service(&self) -> bool {
        let pipeline = self.transcription_pipeline.read().await;
        pipeline.has_transcription_service()
    }

    pub async fn set_api_key(&self, api_key: String) -> Result<(), String> {
        let pipeline = self.transcription_pipeline.read().await;
        pipeline.set_api_key(api_key)?;
        println!("✅ Updated OpenAI API key for meeting service");
        Ok(())
    }

    pub async fn has_api_key(&self) -> bool {
        let pipeline = self.transcription_pipeline.read().await;
        pipeline.has_api_key()
    }

    // Helper method for background task cloning
    fn clone_for_background(&self) -> MeetingServiceHandle {
        MeetingServiceHandle {
            storage: Arc::clone(&self.storage),
            processing_status: Arc::clone(&self.processing_status),
        }
    }

    // Access to storage for external use
    pub fn get_storage(&self) -> Arc<crate::meeting::storage::MeetingStorage> {
        Arc::clone(&self.storage)
    }

    // Clear current meeting (for external control)
    pub async fn clear_current_meeting(&self) -> Result<(), String> {
        self.recording_manager.clear_current_meeting()
    }
}

// Simplified handle for background tasks
struct MeetingServiceHandle {
    storage: Arc<MeetingStorage>,
    processing_status: Arc<RwLock<HashMap<String, MeetingProcessingStatus>>>,
}

impl MeetingServiceHandle {
    async fn update_processing_status(&self, status: MeetingProcessingStatus) {
        let mut status_map = self.processing_status.write().await;
        status_map.insert(status.meeting_id.clone(), status);
    }
}

// Utility functions for meeting management
impl MeetingService {
    pub async fn get_meeting_statistics(&self) -> Result<MeetingStatistics, String> {
        let meetings = self.list_meetings().await?;
        
        let total_meetings = meetings.len();
        let completed_meetings = meetings.iter()
            .filter(|m| matches!(m.status, MeetingStatus::Completed))
            .count();
        
        let total_duration = meetings.iter()
            .filter_map(|m| m.duration)
            .fold(chrono::Duration::zero(), |acc, d| acc + d);

        let total_participants: usize = meetings.iter()
            .map(|m| m.participants.len())
            .sum();

        Ok(MeetingStatistics {
            total_meetings,
            completed_meetings,
            processing_meetings: total_meetings - completed_meetings,
            total_duration_hours: total_duration.num_minutes() as f64 / 60.0,
            average_participants: if total_meetings > 0 {
                total_participants as f64 / total_meetings as f64
            } else {
                0.0
            },
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingStatistics {
    pub total_meetings: usize,
    pub completed_meetings: usize,
    pub processing_meetings: usize,
    pub total_duration_hours: f64,
    pub average_participants: f64,
}