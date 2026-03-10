use std::path::PathBuf;
use serde_json;
use tokio::fs;
use chrono::{DateTime, Utc};

use super::{Meeting, ActionItem, MeetingStatus};
use super::ai_processor::ActionItemExtraction;

pub struct MeetingStorage {
    storage_directory: PathBuf,
}

impl MeetingStorage {
    pub fn new(storage_directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&storage_directory)
            .map_err(|e| format!("Failed to create storage directory: {}", e))?;
        
        Ok(Self { storage_directory })
    }

    pub async fn save_meeting(&self, meeting: &Meeting) -> Result<(), String> {
        let meeting_file = self.get_meeting_file_path(&meeting.id);
        let meeting_json = serde_json::to_string_pretty(meeting)
            .map_err(|e| format!("Failed to serialize meeting: {}", e))?;

        fs::write(&meeting_file, meeting_json)
            .await
            .map_err(|e| format!("Failed to write meeting file: {}", e))?;

        Ok(())
    }

    pub async fn load_meeting(&self, meeting_id: &str) -> Result<Meeting, String> {
        let meeting_file = self.get_meeting_file_path(meeting_id);
        
        if !meeting_file.exists() {
            return Err(format!("Meeting {} not found", meeting_id));
        }

        let meeting_json = fs::read_to_string(&meeting_file)
            .await
            .map_err(|e| format!("Failed to read meeting file: {}", e))?;

        let meeting: Meeting = serde_json::from_str(&meeting_json)
            .map_err(|e| format!("Failed to deserialize meeting: {}", e))?;

        Ok(meeting)
    }

    pub async fn list_meetings(&self) -> Result<Vec<MeetingSummary>, String> {
        let mut meetings = Vec::new();
        
        let mut entries = fs::read_dir(&self.storage_directory)
            .await
            .map_err(|e| format!("Failed to read storage directory: {}", e))?;

        while let Some(entry) = entries.next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))? {
            
            let path = entry.path();
            if path.is_file() && path.extension() == Some(std::ffi::OsStr::new("json")) {
                if let Ok(meeting) = self.load_meeting_summary(&path).await {
                    meetings.push(meeting);
                }
            }
        }

        // Sort by start time, most recent first
        meetings.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        
        Ok(meetings)
    }

    pub async fn delete_meeting(&self, meeting_id: &str) -> Result<(), String> {
        let meeting_file = self.get_meeting_file_path(meeting_id);
        
        if meeting_file.exists() {
            fs::remove_file(&meeting_file)
                .await
                .map_err(|e| format!("Failed to delete meeting file: {}", e))?;
        }

        // Also delete audio directory if it exists
        let audio_directory = PathBuf::from("meeting_audio").join(meeting_id);
        if audio_directory.exists() {
            fs::remove_dir_all(&audio_directory)
                .await
                .map_err(|e| format!("Failed to delete meeting audio directory: {}", e))?;
        }

        Ok(())
    }

    pub async fn update_meeting_status(&self, meeting_id: &str, status: MeetingStatus) -> Result<(), String> {
        let mut meeting = self.load_meeting(meeting_id).await?;
        meeting.status = status;
        self.save_meeting(&meeting).await
    }

    pub async fn add_action_items(&self, meeting_id: &str, action_items: Vec<ActionItem>) -> Result<(), String> {
        let mut meeting = self.load_meeting(meeting_id).await?;
        meeting.action_items.extend(action_items);
        self.save_meeting(&meeting).await
    }

    pub async fn update_transcript(&self, meeting_id: &str, transcript: String) -> Result<(), String> {
        let mut meeting = self.load_meeting(meeting_id).await?;
        meeting.transcript = Some(transcript);
        self.save_meeting(&meeting).await
    }

    pub async fn save_action_item(&self, action_item: &ActionItem) -> Result<(), String> {
        let mut meeting = self.load_meeting(&action_item.meeting_id).await?;
        
        // Check if action item already exists and update it, or add new one
        if let Some(existing_index) = meeting.action_items.iter().position(|item| item.id == action_item.id) {
            meeting.action_items[existing_index] = action_item.clone();
        } else {
            meeting.action_items.push(action_item.clone());
        }
        
        self.save_meeting(&meeting).await
    }

    pub async fn save_meeting_analysis(&self, meeting_id: &str, extraction: &ActionItemExtraction) -> Result<(), String> {
        let analysis_file = self.get_meeting_analysis_file_path(meeting_id);
        let analysis_json = serde_json::to_string_pretty(extraction)
            .map_err(|e| format!("Failed to serialize meeting analysis: {}", e))?;

        fs::write(&analysis_file, analysis_json)
            .await
            .map_err(|e| format!("Failed to write meeting analysis file: {}", e))?;

        Ok(())
    }

    pub async fn load_meeting_analysis(&self, meeting_id: &str) -> Result<ActionItemExtraction, String> {
        let analysis_file = self.get_meeting_analysis_file_path(meeting_id);
        
        if !analysis_file.exists() {
            return Err(format!("Meeting analysis for {} not found", meeting_id));
        }

        let analysis_json = fs::read_to_string(&analysis_file)
            .await
            .map_err(|e| format!("Failed to read meeting analysis file: {}", e))?;

        let analysis: ActionItemExtraction = serde_json::from_str(&analysis_json)
            .map_err(|e| format!("Failed to deserialize meeting analysis: {}", e))?;

        Ok(analysis)
    }

    pub async fn get_action_items_for_meeting(&self, meeting_id: &str) -> Result<Vec<ActionItem>, String> {
        let meeting = self.load_meeting(meeting_id).await?;
        Ok(meeting.action_items)
    }

    pub async fn update_action_item_status(&self, meeting_id: &str, action_item_id: &str, status: super::ActionItemStatus) -> Result<(), String> {
        let mut meeting = self.load_meeting(meeting_id).await?;
        
        if let Some(action_item) = meeting.action_items.iter_mut().find(|item| item.id == action_item_id) {
            action_item.status = status;
            self.save_meeting(&meeting).await
        } else {
            Err(format!("Action item {} not found in meeting {}", action_item_id, meeting_id))
        }
    }

    async fn load_meeting_summary(&self, path: &PathBuf) -> Result<MeetingSummary, String> {
        let meeting_json = fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read meeting file: {}", e))?;

        let meeting: Meeting = serde_json::from_str(&meeting_json)
            .map_err(|e| format!("Failed to deserialize meeting: {}", e))?;

        Ok(MeetingSummary {
            id: meeting.id,
            title: meeting.title,
            start_time: meeting.start_time,
            end_time: meeting.end_time,
            duration: meeting.duration,
            participants: meeting.participants,
            status: meeting.status,
            action_item_count: meeting.action_items.len(),
            has_transcript: meeting.transcript.is_some(),
        })
    }

    fn get_meeting_file_path(&self, meeting_id: &str) -> PathBuf {
        self.storage_directory.join(format!("{}.json", meeting_id))
    }

    fn get_meeting_analysis_file_path(&self, meeting_id: &str) -> PathBuf {
        self.storage_directory.join(format!("{}_analysis.json", meeting_id))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MeetingSummary {
    pub id: String,
    pub title: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration: Option<chrono::Duration>,
    pub participants: Vec<String>,
    pub status: MeetingStatus,
    pub action_item_count: usize,
    pub has_transcript: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_save_and_load_meeting() {
        let temp_dir = TempDir::new().unwrap();
        let storage = MeetingStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let meeting = Meeting {
            id: Uuid::new_v4().to_string(),
            title: "Test Meeting".to_string(),
            start_time: Utc::now(),
            end_time: None,
            duration: None,
            participants: vec!["Alice".to_string(), "Bob".to_string()],
            audio_chunks: Vec::new(),
            transcript: None,
            action_items: Vec::new(),
            status: MeetingStatus::InProgress,
            audio_directory: temp_dir.path().to_path_buf(),
        };

        // Save meeting
        storage.save_meeting(&meeting).await.unwrap();

        // Load meeting
        let loaded_meeting = storage.load_meeting(&meeting.id).await.unwrap();
        assert_eq!(loaded_meeting.id, meeting.id);
        assert_eq!(loaded_meeting.title, meeting.title);
        assert_eq!(loaded_meeting.participants, meeting.participants);
    }

    #[tokio::test]
    async fn test_list_meetings() {
        let temp_dir = TempDir::new().unwrap();
        let storage = MeetingStorage::new(temp_dir.path().to_path_buf()).unwrap();

        // Create multiple meetings
        for i in 0..3 {
            let meeting = Meeting {
                id: Uuid::new_v4().to_string(),
                title: format!("Test Meeting {}", i),
                start_time: Utc::now(),
                end_time: None,
                duration: None,
                participants: Vec::new(),
                audio_chunks: Vec::new(),
                transcript: None,
                action_items: Vec::new(),
                status: MeetingStatus::InProgress,
                audio_directory: temp_dir.path().to_path_buf(),
            };
            storage.save_meeting(&meeting).await.unwrap();
        }

        let meetings = storage.list_meetings().await.unwrap();
        assert_eq!(meetings.len(), 3);
    }
}