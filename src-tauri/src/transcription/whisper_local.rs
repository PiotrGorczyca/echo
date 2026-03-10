use async_trait::async_trait;
use anyhow::Result;
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
use std::path::Path;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::TranscriptionBackend;

// Global cache for whisper models
static MODEL_CACHE: Lazy<Arc<Mutex<HashMap<String, Arc<WhisperContext>>>>> = 
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub struct WhisperLocalBackend {
    ctx: Arc<WhisperContext>,
}

impl WhisperLocalBackend {
    pub fn new(model_path: String) -> Result<Self> {
        let path = Path::new(&model_path);
        if !path.exists() {
            return Err(anyhow::anyhow!("Model file does not exist: {}", model_path));
        }
        
        // Check if model is already cached
        let mut cache = MODEL_CACHE.lock().unwrap();
        
        if let Some(cached_ctx) = cache.get(&model_path) {
            println!("Using cached Whisper model from: {}", model_path);
            return Ok(Self { ctx: cached_ctx.clone() });
        }
        
        println!("Loading Whisper model from: {}", model_path);
        let ctx = WhisperContext::new_with_params(
            &model_path,
            WhisperContextParameters::default(),
        )?;
        
        let ctx = Arc::new(ctx);
        cache.insert(model_path.clone(), ctx.clone());
        
        Ok(Self { ctx })
    }
}

#[async_trait]
impl TranscriptionBackend for WhisperLocalBackend {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        // Load and decode audio file
        let audio_data = std::fs::read(audio_file_path)?;
        
        // Convert WAV to PCM f32 samples
        let reader = hound::WavReader::new(std::io::Cursor::new(audio_data))?;
        let spec = reader.spec();
        
        // Resample to 16kHz if needed (Whisper expects 16kHz)
        let samples: Vec<f32> = if spec.sample_rate != 16000 {
            // Simple resampling - for production, use a proper resampling library
            let samples: Vec<f32> = reader.into_samples::<i16>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / i16::MAX as f32)
                .collect();
            
            resample_audio(&samples, spec.sample_rate, 16000)
        } else {
            reader.into_samples::<i16>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / i16::MAX as f32)
                .collect()
        };
        
        // Create parameters for transcription
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
        
        // Set some parameters
        // Use all available CPU threads for better performance
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        params.set_n_threads(num_threads);
        params.set_translate(false);
        params.set_language(None); // Auto-detect language
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        
        // Additional parameters to reduce hallucinations
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);
        
        // Speed optimizations
        params.set_token_timestamps(false); // Disable token-level timestamps for speed
        
        // Run the transcription
        let mut state = self.ctx.create_state()?;
        state.full(params, &samples)?;
        
        // Get the transcribed text
        let num_segments = state.full_n_segments()?;
        let mut text = String::new();
        
        for i in 0..num_segments {
            let segment = state.full_get_segment_text(i)?;
            text.push_str(&segment);
            text.push(' ');
        }
        
        Ok(text.trim().to_string())
    }
}

// Simple linear interpolation resampling
fn resample_audio(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f32 / to_rate as f32;
    let new_len = (samples.len() as f32 / ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);
    
    for i in 0..new_len {
        let src_idx = i as f32 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f32;
        
        if idx + 1 < samples.len() {
            let sample = samples[idx] * (1.0 - frac) + samples[idx + 1] * frac;
            resampled.push(sample);
        } else if idx < samples.len() {
            resampled.push(samples[idx]);
        }
    }
    
    resampled
} 