use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::TranscriptionBackend;

#[derive(Debug, Serialize, Deserialize)]
struct WhisperResponse {
    text: String,
}

pub struct OpenAIBackend {
    api_key: String,
}

impl OpenAIBackend {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl TranscriptionBackend for OpenAIBackend {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        use std::time::Duration;
        
        // Create client with timeout
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30)) // 30 second timeout
            .build()?;
        
        // Read the audio file
        let audio_data = std::fs::read(audio_file_path)?;
        println!("📤 Uploading {} bytes to OpenAI API", audio_data.len());
        
        // Create multipart form
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(audio_data)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")?,
            )
            .text("model", "whisper-1");
        
        println!("🌐 Sending request to OpenAI API...");
        let response = client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;
        
        println!("📨 Received response from OpenAI API");
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("API Error {}: {}", status, error_text));
        }
        
        let whisper_response: WhisperResponse = response.json().await?;
        
        Ok(whisper_response.text)
    }
} 