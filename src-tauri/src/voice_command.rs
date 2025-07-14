use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use crate::transcription::TranscriptionService;
use crate::ai_agent::AiAgentCore;
use crate::mcp::McpClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandMessage {
    pub id: String,
    pub timestamp: u64,
    pub r#type: String, // 'user' | 'system' | 'result' | 'error'
    pub content: String,
    pub metadata: Option<VoiceCommandMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandMetadata {
    pub transcription: Option<String>,
    pub intent: Option<String>,
    pub tool: Option<String>,
    pub server: Option<String>,
    pub confidence: Option<f32>,
    pub processing_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandState {
    pub is_recording: bool,
    pub is_processing: bool,
    pub current_state: String, // 'idle' | 'recording' | 'transcribing' | 'processing' | 'executing'
    pub recording_start_time: Option<u64>,
}

pub struct VoiceCommandService {
    transcription_service: Option<Arc<Mutex<TranscriptionService>>>,
    ai_agent: Option<Arc<AiAgentCore>>,
    mcp_client: Option<Arc<McpClient>>,
    state: Arc<Mutex<VoiceCommandState>>,
    app_handle: AppHandle,
    messages: Arc<Mutex<Vec<VoiceCommandMessage>>>,
}

impl VoiceCommandService {
    pub fn new(
        app_handle: AppHandle,
        transcription_service: Option<Arc<Mutex<TranscriptionService>>>,
        ai_agent: Option<Arc<AiAgentCore>>,
        mcp_client: Option<Arc<McpClient>>,
    ) -> Self {
        let initial_state = VoiceCommandState {
            is_recording: false,
            is_processing: false,
            current_state: "idle".to_string(),
            recording_start_time: None,
        };

        Self {
            transcription_service,
            ai_agent,
            mcp_client,
            state: Arc::new(Mutex::new(initial_state)),
            app_handle,
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_state(&self) -> Result<VoiceCommandState, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|e| format!("Failed to get voice command state: {}", e))
    }

    pub fn update_state(&self, new_state: &str) -> Result<(), String> {
        let mut state = self.state
            .lock()
            .map_err(|e| format!("Failed to lock voice command state: {}", e))?;

        state.current_state = new_state.to_string();

        match new_state {
            "recording" => {
                state.is_recording = true;
                state.recording_start_time = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                );
            }
            "transcribing" | "processing" | "executing" => {
                state.is_recording = false;
                state.is_processing = true;
            }
            "idle" => {
                state.is_recording = false;
                state.is_processing = false;
                state.recording_start_time = None;
            }
            _ => {}
        }

        // Emit state change event
        let _ = self.app_handle.emit("voice-command-state", &*state);

        println!("Voice command state updated to: {}", new_state);
        Ok(())
    }

    pub fn add_message(&self, message_type: &str, content: &str, metadata: Option<VoiceCommandMetadata>) -> Result<(), String> {
        let message = VoiceCommandMessage {
            id: format!("{}{}", 
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                rand::random::<u16>()
            ),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            r#type: message_type.to_string(),
            content: content.to_string(),
            metadata,
        };

        {
            let mut messages = self.messages
                .lock()
                .map_err(|e| format!("Failed to lock messages: {}", e))?;
            messages.push(message.clone());
        }

        // Emit new message event
        let _ = self.app_handle.emit("voice-command-event", &message);

        Ok(())
    }

    pub fn get_messages(&self) -> Result<Vec<VoiceCommandMessage>, String> {
        self.messages
            .lock()
            .map(|messages| messages.clone())
            .map_err(|e| format!("Failed to get messages: {}", e))
    }

    pub fn clear_messages(&self) -> Result<(), String> {
        let mut messages = self.messages
            .lock()
            .map_err(|e| format!("Failed to lock messages: {}", e))?;
        messages.clear();
        Ok(())
    }

    pub async fn process_text_command(&self, command_text: &str) -> Result<(), String> {
        // Add user message
        self.add_message("user", command_text, None)?;
        
        // Update state to processing
        self.update_state("processing")?;

        // Add system message
        self.add_message("system", "Processing command...", Some(VoiceCommandMetadata {
            transcription: Some(command_text.to_string()),
            intent: None,
            tool: None,
            server: None,
            confidence: None,
            processing_time: Some(0),
        }))?;

        // Simulate processing for now - replace with actual AI processing later
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Add result message
        self.add_message("result", &format!("Command processed: \"{}\"", command_text), Some(VoiceCommandMetadata {
            transcription: Some(command_text.to_string()),
            intent: Some("test_command".to_string()),
            tool: None,
            server: None,
            confidence: Some(0.95),
            processing_time: Some(1000),
        }))?;

        // Reset state to idle
        self.update_state("idle")?;

        Ok(())
    }

    pub async fn start_voice_command(&self) -> Result<(), String> {
        // Update state to recording
        self.update_state("recording")?;
        
        // Add system message
        self.add_message("system", "🎤 Recording voice command...", None)?;

        // Start recording would go here - for now simulate
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        
        // Simulate transcription
        self.update_state("transcribing")?;
        self.add_message("system", "📝 Transcribing audio...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Simulate processing
        self.update_state("processing")?;
        self.add_message("system", "🧠 Processing command...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Simulate execution
        self.update_state("executing")?;
        self.add_message("system", "⚡ Executing command...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Add result
        self.add_message("result", "Voice command test completed successfully", Some(VoiceCommandMetadata {
            transcription: Some("test voice command".to_string()),
            intent: Some("system_test".to_string()),
            tool: None,
            server: None,
            confidence: Some(0.98),
            processing_time: Some(5000),
        }))?;

        // Reset to idle
        self.update_state("idle")?;

        Ok(())
    }

    pub async fn start_voice_recording(&self) -> Result<(), String> {
        println!("🎤 Starting voice command recording...");
        
        // Update state to recording
        self.update_state("recording")?;
        
        // Add system message
        self.add_message("system", "🎤 Starting voice recording...", None)?;

        // Use the app handle to trigger the actual recording system
        // This integrates with the existing recording infrastructure
        let _ = self.app_handle.emit("voice-command-start-recording", ());
        
        Ok(())
    }

    pub async fn stop_voice_recording_and_process(&self) -> Result<(), String> {
        // Update state to transcribing  
        self.update_state("transcribing")?;
        self.add_message("system", "📝 Transcribing audio...", None)?;

        // Emit event to stop recording and get the audio file
        let _ = self.app_handle.emit("stop-voice-command-recording", ());

        // The actual transcription will be handled by the event listener
        // and will call process_transcription when ready
        
        Ok(())
    }

    pub async fn process_transcription(&self, transcription_text: &str) -> Result<(), String> {
        // Add detailed logging for transcription
        println!("🔍 Voice Command Transcription Result:");
        println!("   Raw text: '{}'", transcription_text);
        println!("   Length: {} characters", transcription_text.len());
        println!("   Words: {} words", transcription_text.split_whitespace().count());
        
        // Update state to processing
        self.update_state("processing")?;
        
        // Add transcription result message with detailed info
        let transcription_info = format!(
            "📝 Transcription: \"{}\"\n💬 Length: {} chars, {} words", 
            transcription_text,
            transcription_text.len(),
            transcription_text.split_whitespace().count()
        );
        
        self.add_message("system", &transcription_info, Some(VoiceCommandMetadata {
            transcription: Some(transcription_text.to_string()),
            intent: None,
            tool: None,
            server: None,
            confidence: None,
            processing_time: None,
        }))?;

        // For now, simulate AI processing - this will be replaced with actual AI integration
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Simulate execution
        self.update_state("executing")?;
        self.add_message("system", "⚡ Executing command...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Add result with the actual transcription
        self.add_message("result", &format!("Command processed: \"{}\"", transcription_text), Some(VoiceCommandMetadata {
            transcription: Some(transcription_text.to_string()),
            intent: Some("voice_command".to_string()),
            tool: None,
            server: None,
            confidence: Some(0.95),
            processing_time: Some(3000),
        }))?;

        // Reset to idle
        self.update_state("idle")?;

        Ok(())
    }

    pub async fn handle_recording_error(&self, error: &str) -> Result<(), String> {
        self.update_state("idle")?;
        self.add_message("error", &format!("Recording failed: {}", error), None)?;
        Ok(())
    }
    
    pub async fn handle_transcription_from_system(&self, transcription_text: &str, _audio_path: &str) -> Result<(), String> {
        println!("🔄 Voice command service received transcription from system: '{}'", transcription_text);
        
        // Update state to transcribing first
        self.update_state("transcribing")?;
        self.add_message("system", "📝 Transcription received from system...", None)?;
        
        // Process the transcription
        self.process_transcription(transcription_text).await?;
        
        Ok(())
    }
} 