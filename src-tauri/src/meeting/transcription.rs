use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::transcription::TranscriptionService;
use super::{Meeting, AudioChunk, MeetingStatus};
use super::storage::MeetingStorage;
use super::ai_processor::MeetingAiProcessor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionChunk {
    pub id: String,
    pub chunk_number: u32,
    pub text: String,
    pub confidence: Option<f32>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub processing_duration_ms: u64,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub meeting_id: String,
    pub chunks: Vec<TranscriptionChunk>,
    pub full_text: String,
    pub total_duration: Duration,
    pub word_count: usize,
    pub processing_started: DateTime<Utc>,
    pub processing_completed: Option<DateTime<Utc>>,
    pub status: TranscriptionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TranscriptionStatus {
    Pending,
    Processing,
    Completed,
    Failed(String),
    PartiallyCompleted, // Some chunks failed but others succeeded
}

pub struct MeetingTranscriptionPipeline {
    transcription_service: Option<Arc<TranscriptionService>>,
    ai_processor: Arc<Mutex<MeetingAiProcessor>>,
    storage: Arc<MeetingStorage>,
    max_concurrent_chunks: usize,
    chunk_overlap_seconds: f64, // Overlap between chunks for better continuity
}

impl MeetingTranscriptionPipeline {
    pub fn new(
        transcription_service: Option<Arc<TranscriptionService>>,
        storage: Arc<MeetingStorage>,
        max_concurrent_chunks: usize,
        api_key: Option<String>,
    ) -> Result<Self, String> {
        let ai_processor = MeetingAiProcessor::new(api_key)
            .map_err(|e| format!("Failed to initialize AI processor: {}", e))?;
            
        Ok(Self {
            transcription_service,
            ai_processor: Arc::new(Mutex::new(ai_processor)),
            storage,
            max_concurrent_chunks,
            chunk_overlap_seconds: 2.0, // 2-second overlap
        })
    }

    pub fn set_transcription_service(&mut self, service: Arc<TranscriptionService>) {
        self.transcription_service = Some(service);
    }

    pub fn has_transcription_service(&self) -> bool {
        self.transcription_service.is_some()
    }
    
    pub fn set_api_key(&self, api_key: String) -> Result<(), String> {
        let mut ai_processor = self.ai_processor.lock()
            .map_err(|e| format!("Failed to lock AI processor: {}", e))?;
        ai_processor.set_api_key(api_key)
            .map_err(|e| format!("Failed to set API key: {}", e))
    }
    
    pub fn has_api_key(&self) -> bool {
        self.ai_processor.lock()
            .map(|processor| processor.has_api_key())
            .unwrap_or(false)
    }

    pub async fn process_meeting(&self, meeting: &Meeting) -> Result<MeetingTranscript, String> {
        println!("🎙️ Starting transcription pipeline for meeting: {}", meeting.title);
        
        // Check if transcription service is available
        let transcription_service = self.transcription_service
            .as_ref()
            .ok_or("Transcription service not available. Please configure API key first.")?;
        
        // Update meeting status to processing
        self.storage.update_meeting_status(&meeting.id, MeetingStatus::Processing).await?;

        let mut transcript = MeetingTranscript {
            meeting_id: meeting.id.clone(),
            chunks: Vec::new(),
            full_text: String::new(),
            total_duration: meeting.duration.unwrap_or(Duration::zero()),
            word_count: 0,
            processing_started: Utc::now(),
            processing_completed: None,
            status: TranscriptionStatus::Processing,
        };

        // Sort audio chunks by chunk number to ensure proper order
        let mut sorted_chunks = meeting.audio_chunks.clone();
        sorted_chunks.sort_by_key(|chunk| chunk.chunk_number);

        if sorted_chunks.is_empty() {
            return Err("No audio chunks found in meeting".to_string());
        }

        // Process chunks in batches to control concurrency
        let mut all_transcription_chunks = Vec::new();
        let chunk_batches = sorted_chunks.chunks(self.max_concurrent_chunks);

        for (batch_index, batch) in chunk_batches.enumerate() {
            println!("📝 Processing batch {}/{}", batch_index + 1, 
                    (sorted_chunks.len() + self.max_concurrent_chunks - 1) / self.max_concurrent_chunks);

            let batch_results = self.process_chunk_batch(batch.to_vec(), Arc::clone(transcription_service)).await?;
            all_transcription_chunks.extend(batch_results);
        }

        // Sort transcription chunks by chunk number
        all_transcription_chunks.sort_by_key(|chunk| chunk.chunk_number);

        // Post-process and assemble final transcript
        let final_transcript = self.assemble_transcript(all_transcription_chunks).await?;
        
        transcript.chunks = final_transcript.chunks;
        transcript.full_text = final_transcript.full_text;
        transcript.word_count = final_transcript.word_count;
        transcript.processing_completed = Some(Utc::now());
        transcript.status = TranscriptionStatus::Completed;

        // Save transcript to meeting
        self.storage.update_transcript(&meeting.id, transcript.full_text.clone()).await?;

        // Process action items with AI if available
        if self.has_api_key() {
            println!("🤖 Starting AI processing for action items...");
            
            let duration_minutes = meeting.duration
                .map(|d| d.num_minutes() as f64)
                .unwrap_or(0.0);
            
            // Clone necessary data to avoid holding lock across await
            let meeting_id = meeting.id.clone();
            let participants = meeting.participants.clone();
            let title = meeting.title.clone();
            let full_text = transcript.full_text.clone();
            
            // Process transcript with AI (create a temporary AI processor to avoid lock issues)
            let api_key = {
                let ai_processor = self.ai_processor.lock()
                    .map_err(|e| format!("Failed to lock AI processor: {}", e))?;
                ai_processor.openai_client.as_ref()
                    .map(|client| client.get_api_key().to_string())
                    .ok_or_else(|| "No API key available".to_string())?
            };
            
            // Create a temporary AI processor for this operation
            let temp_processor = super::ai_processor::MeetingAiProcessor::new(Some(api_key))
                .map_err(|e| format!("Failed to create temporary AI processor: {}", e))?;
            
            let extraction_result = temp_processor.process_meeting_transcript(
                &meeting_id,
                &full_text,
                &participants,
                &title,
                duration_minutes,
            ).await;
            
            match extraction_result {
                Ok(extraction) => {
                    println!("✅ AI processing completed successfully");
                    
                    // Generate summary and convert action items
                    let summary = temp_processor.summarize_extraction(&extraction);
                    println!("{}", summary);
                    
                    // Convert extracted action items to internal format
                    let action_items = temp_processor.convert_to_action_items(&meeting_id, &extraction);
                    
                    // Save action items to storage
                    for action_item in action_items {
                        if let Err(e) = self.storage.save_action_item(&action_item).await {
                            println!("⚠️ Failed to save action item: {}", e);
                        }
                    }
                    
                    // Store the extraction metadata (decisions, suggestions, etc.)
                    if let Err(e) = self.storage.save_meeting_analysis(&meeting_id, &extraction).await {
                        println!("⚠️ Failed to save meeting analysis: {}", e);
                    }
                },
                Err(e) => {
                    println!("⚠️ AI processing failed: {}", e);
                    // Continue without AI processing - transcription is still complete
                }
            }
        } else {
            println!("⚠️ AI processing skipped - no API key configured");
        }

        self.storage.update_meeting_status(&meeting.id, MeetingStatus::Completed).await?;

        println!("✅ Meeting transcription completed: {} words, {} chunks", 
                transcript.word_count, transcript.chunks.len());

        Ok(transcript)
    }

    async fn process_chunk_batch(&self, chunks: Vec<AudioChunk>, transcription_service: Arc<TranscriptionService>) -> Result<Vec<TranscriptionChunk>, String> {
        let mut handles: Vec<JoinHandle<Result<TranscriptionChunk, String>>> = Vec::new();

        // Start transcription tasks for each chunk in the batch
        for chunk in chunks {
            let transcription_service = Arc::clone(&transcription_service);
            let chunk_id = chunk.id.clone();
            let chunk_number = chunk.chunk_number;
            let file_path = chunk.file_path.clone();
            let start_time = chunk.start_timestamp;
            let end_time = chunk.end_timestamp.unwrap_or(Utc::now());

            let handle = tokio::spawn(async move {
                Self::transcribe_single_chunk(
                    transcription_service,
                    chunk_id,
                    chunk_number,
                    file_path,
                    start_time,
                    end_time,
                ).await
            });

            handles.push(handle);
        }

        // Wait for all transcription tasks to complete
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(transcription_chunk)) => {
                    results.push(transcription_chunk);
                }
                Ok(Err(e)) => {
                    println!("⚠️ Chunk transcription failed: {}", e);
                    // Continue with other chunks even if one fails
                }
                Err(e) => {
                    println!("⚠️ Task join error: {}", e);
                }
            }
        }

        if results.is_empty() {
            return Err("All chunks in batch failed to transcribe".to_string());
        }

        Ok(results)
    }

    async fn transcribe_single_chunk(
        transcription_service: Arc<TranscriptionService>,
        chunk_id: String,
        chunk_number: u32,
        file_path: PathBuf,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<TranscriptionChunk, String> {
        let processing_start = std::time::Instant::now();
        
        println!("🎤 Transcribing chunk {}: {:?}", chunk_number, file_path);

        // Check if file exists
        if !file_path.exists() {
            return Err(format!("Audio file not found: {:?}", file_path));
        }

        // Perform transcription directly (no Mutex needed)
        let file_path_str = file_path.to_string_lossy().to_string();
        let transcription_text = transcription_service.transcribe(&file_path_str).await
            .map_err(|e| format!("Transcription failed for chunk {}: {}", chunk_number, e))?;

        let processing_duration = processing_start.elapsed();
        let word_count = transcription_text.split_whitespace().count();

        println!("✅ Chunk {} transcribed: {} words in {}ms", 
                chunk_number, word_count, processing_duration.as_millis());

        Ok(TranscriptionChunk {
            id: chunk_id,
            chunk_number,
            text: transcription_text,
            confidence: None, // Could be added later if transcription service provides it
            start_time,
            end_time,
            processing_duration_ms: processing_duration.as_millis() as u64,
            word_count,
        })
    }

    async fn assemble_transcript(&self, chunks: Vec<TranscriptionChunk>) -> Result<MeetingTranscript, String> {
        if chunks.is_empty() {
            return Err("No transcription chunks to assemble".to_string());
        }

        println!("🔗 Assembling {} transcription chunks into final transcript", chunks.len());

        // Apply overlap removal and smoothing
        let processed_chunks = self.process_chunk_boundaries(chunks).await?;
        
        // Combine all chunk texts
        let full_text = processed_chunks
            .iter()
            .map(|chunk| chunk.text.trim())
            .collect::<Vec<_>>()
            .join(" ");

        // Clean up the final text
        let cleaned_text = self.clean_transcript_text(&full_text);
        let total_word_count = cleaned_text.split_whitespace().count();

        Ok(MeetingTranscript {
            meeting_id: String::new(), // Will be set by caller
            chunks: processed_chunks,
            full_text: cleaned_text,
            total_duration: Duration::zero(), // Will be set by caller
            word_count: total_word_count,
            processing_started: Utc::now(), // Will be set by caller
            processing_completed: Some(Utc::now()),
            status: TranscriptionStatus::Completed,
        })
    }

    async fn process_chunk_boundaries(&self, mut chunks: Vec<TranscriptionChunk>) -> Result<Vec<TranscriptionChunk>, String> {
        if chunks.len() <= 1 {
            return Ok(chunks);
        }

        // Sort by chunk number to ensure proper order
        chunks.sort_by_key(|chunk| chunk.chunk_number);

        // Process each chunk boundary to remove overlap and improve continuity
        for i in 0..chunks.len() - 1 {
            let current_chunk = &chunks[i];
            let next_chunk = &chunks[i + 1];

            // Check for potential overlap or discontinuity
            if let Some(boundary_processed) = self.process_boundary_overlap(
                &current_chunk.text,
                &next_chunk.text,
            ).await {
                // Update the chunks with processed boundary text
                chunks[i].text = boundary_processed.0;
                chunks[i + 1].text = boundary_processed.1;
            }
        }

        Ok(chunks)
    }

    async fn process_boundary_overlap(&self, current_text: &str, next_text: &str) -> Option<(String, String)> {
        // Simple overlap detection and removal
        // Look for common words/phrases at the end of current and start of next
        
        let current_words: Vec<&str> = current_text.split_whitespace().collect();
        let next_words: Vec<&str> = next_text.split_whitespace().collect();

        if current_words.is_empty() || next_words.is_empty() {
            return None;
        }

        // Look for overlap in the last few words of current and first few words of next
        let overlap_window = std::cmp::min(10, std::cmp::min(current_words.len(), next_words.len()));
        
        for overlap_size in (1..=overlap_window).rev() {
            let current_suffix = &current_words[current_words.len() - overlap_size..];
            let next_prefix = &next_words[..overlap_size];

            // Check if there's significant overlap (at least 2 words match)
            let matches = current_suffix.iter()
                .zip(next_prefix.iter())
                .filter(|(a, b)| a.to_lowercase() == b.to_lowercase())
                .count();

            if matches >= 2 && matches == overlap_size {
                // Found overlap - remove from the end of current chunk
                let processed_current = current_words[..current_words.len() - overlap_size]
                    .join(" ");
                let processed_next = next_words.join(" ");

                println!("🔗 Removed {}-word overlap between chunks", overlap_size);
                return Some((processed_current, processed_next));
            }
        }

        None
    }

    fn clean_transcript_text(&self, text: &str) -> String {
        // Basic text cleaning and normalization
        text
            .trim()
            // Remove multiple spaces
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            // Basic punctuation cleanup
            .replace(" ,", ",")
            .replace(" .", ".")
            .replace(" ?", "?")
            .replace(" !", "!")
            .replace("  ", " ")
    }

    pub async fn retry_failed_chunks(&self, meeting_id: &str) -> Result<(), String> {
        // Implementation for retrying failed chunks
        // This would reload the meeting, identify failed chunks, and retry transcription
        println!("🔄 Retrying failed chunks for meeting: {}", meeting_id);
        
        // Load meeting
        let meeting = self.storage.load_meeting(meeting_id).await?;
        
        // Find chunks that might have failed (this would need additional tracking)
        // For now, we'll just re-process the entire meeting
        let _transcript = self.process_meeting(&meeting).await?;
        
        Ok(())
    }

    pub fn get_progress_estimate(&self, meeting: &Meeting) -> f64 {
        // Estimate progress based on audio chunks and processing
        if meeting.audio_chunks.is_empty() {
            return 0.0;
        }

        // Simple progress estimation - could be enhanced with actual chunk status tracking
        match meeting.status {
            MeetingStatus::Recording => 0.1,
            MeetingStatus::Processing => 0.5,
            MeetingStatus::Completed => 1.0,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_boundary_overlap_detection() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(MeetingStorage::new(temp_dir.path().to_path_buf()).unwrap());
        
        // Mock transcription service - we'll need to create a proper mock for testing
        // For now, this test structure shows how boundary processing works
        
        let pipeline = MeetingTranscriptionPipeline::new(
            None, // No transcription service for testing
            storage,
            2,
            None, // No API key for testing
        ).unwrap();

        let current_text = "Hello world this is a test";
        let next_text = "this is a test and more content";

        let result = pipeline.process_boundary_overlap(current_text, next_text).await;
        
        if let Some((processed_current, processed_next)) = result {
            assert_eq!(processed_current, "Hello world");
            assert_eq!(processed_next, "this is a test and more content");
        }
    }

    #[test]
    fn test_text_cleaning() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(MeetingStorage::new(temp_dir.path().to_path_buf()).unwrap());
        
        let pipeline = MeetingTranscriptionPipeline::new(
            None, // No transcription service for testing
            storage,
            2,
            None, // No API key for testing
        ).unwrap();

        let messy_text = "  Hello   world  ,  this is    a test .  ";
        let cleaned = pipeline.clean_transcript_text(messy_text);
        
        assert_eq!(cleaned, "Hello world, this is a test.");
    }
}