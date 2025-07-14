pub mod openai;
pub mod whisper_local;
pub mod whisper_candle;

use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use anyhow::Result;
use tauri::{AppHandle, Emitter};
use std::time::{Duration, Instant};

use crate::{DownloadProgress, DownloadEvent};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TranscriptionMode {
    OpenAI,
    LocalWhisper,
    CandleWhisper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    pub mode: TranscriptionMode,
    pub openai_api_key: Option<String>,
    pub whisper_model_path: Option<String>,
    pub whisper_model_size: WhisperModelSize,
    pub device: DeviceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Cpu,
    Cuda,
    Metal,
    Rocm,
}

impl DeviceType {
    /// Get all available device types for the current system
    pub fn get_available_devices() -> Vec<DeviceType> {
        let mut devices = vec![DeviceType::Cpu];
        
        #[cfg(feature = "cuda")]
        {
            devices.push(DeviceType::Cuda);
        }
        
        #[cfg(all(feature = "metal", target_os = "macos"))]
        {
            devices.push(DeviceType::Metal);
        }
        
        #[cfg(feature = "rocm")]
        {
            // Only include ROCm if we have compatible hardware
            if crate::rocm_detection::detect_rocm_compatibility().is_supported {
                devices.push(DeviceType::Rocm);
            }
        }
        
        devices
    }
}

impl Default for DeviceType {
    fn default() -> Self {
        // Auto-detect best available device
        #[cfg(feature = "rocm")]
        {
            // Only default to ROCm if we have compatible hardware
            if crate::rocm_detection::detect_rocm_compatibility().is_supported {
                DeviceType::Rocm
            } else {
                DeviceType::Cpu
            }
        }
        #[cfg(all(feature = "cuda", not(feature = "rocm")))]
        {
            DeviceType::Cuda
        }
        #[cfg(all(feature = "metal", target_os = "macos", not(any(feature = "rocm", feature = "cuda"))))]
        {
            DeviceType::Metal
        }
        #[cfg(not(any(feature = "rocm", feature = "cuda", all(feature = "metal", target_os = "macos"))))]
        {
            DeviceType::Cpu
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
    LargeTurbo,
    // Distil-Whisper models
    DistilMedium,
    DistilLargeV2,
    DistilLargeV3,
}

impl WhisperModelSize {
    pub fn model_filename(&self) -> &str {
        match self {
            WhisperModelSize::Tiny => "ggml-tiny.bin",
            WhisperModelSize::Base => "ggml-base.bin", 
            WhisperModelSize::Small => "ggml-small.bin",
            WhisperModelSize::Medium => "ggml-medium.bin",
            WhisperModelSize::Large => "ggml-large-v3.bin",
            WhisperModelSize::LargeTurbo => "ggml-large-v3-turbo.bin",
            WhisperModelSize::DistilMedium => "distil-medium.en",
            WhisperModelSize::DistilLargeV2 => "distil-large-v2",
            WhisperModelSize::DistilLargeV3 => "distil-large-v3",
        }
    }
    
    pub fn download_url(&self) -> &str {
        match self {
            // Using the latest multilingual models from ggerganov's repository
            WhisperModelSize::Tiny => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
            WhisperModelSize::Base => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
            WhisperModelSize::Small => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
            WhisperModelSize::Medium => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
            WhisperModelSize::Large => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
            WhisperModelSize::LargeTurbo => "https://huggingface.co/openai/whisper-large-v3-turbo/resolve/main/model.safetensors",
            // Distil-Whisper models - these are Hugging Face model IDs, not direct URLs
            WhisperModelSize::DistilMedium => "distil-whisper/distil-medium.en",
            WhisperModelSize::DistilLargeV2 => "distil-whisper/distil-large-v2", 
            WhisperModelSize::DistilLargeV3 => "distil-whisper/distil-large-v3",
        }
    }
    
    pub fn is_distil_model(&self) -> bool {
        matches!(self, 
            WhisperModelSize::DistilMedium | 
            WhisperModelSize::DistilLargeV2 | 
            WhisperModelSize::DistilLargeV3
        )
    }
    
    pub fn is_turbo_model(&self) -> bool {
        matches!(self, WhisperModelSize::LargeTurbo)
    }
}

#[async_trait]
pub trait TranscriptionBackend: Send + Sync {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String>;
}

pub struct TranscriptionService {
    config: TranscriptionConfig,
    backend: Box<dyn TranscriptionBackend>,
}

impl TranscriptionService {
    pub fn new(config: TranscriptionConfig) -> Result<Self> {
        let backend: Box<dyn TranscriptionBackend> = match &config.mode {
            TranscriptionMode::OpenAI => {
                let api_key = config.openai_api_key.clone()
                    .ok_or_else(|| anyhow::anyhow!("OpenAI API key is required"))?;
                Box::new(openai::OpenAIBackend::new(api_key))
            }
            TranscriptionMode::LocalWhisper => {
                let model_path = config.whisper_model_path.clone()
                    .or_else(|| Self::default_model_path(&config.whisper_model_size))
                    .ok_or_else(|| anyhow::anyhow!("Whisper model path is required"))?;
                Box::new(whisper_local::WhisperLocalBackend::new(model_path)?)
            }
            TranscriptionMode::CandleWhisper => {
                Box::new(whisper_candle::CandleWhisperBackend::new(
                    config.whisper_model_size.clone(),
                    config.device.clone(),
                )?)
            }
        };
        
        Ok(Self { config, backend })
    }
    
    pub async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        self.backend.transcribe(audio_file_path).await
    }
    
    pub fn get_config(&self) -> &TranscriptionConfig {
        &self.config
    }
    
    pub fn is_candle_whisper(&self) -> bool {
        matches!(self.config.mode, TranscriptionMode::CandleWhisper)
    }
    
    fn default_model_path(model_size: &WhisperModelSize) -> Option<String> {
        dirs::data_dir().map(|data_dir| {
            let model_dir = data_dir.join("echotype").join("models");
            model_dir.join(model_size.model_filename()).to_string_lossy().to_string()
        })
    }
    
    pub async fn download_model_with_progress(model_size: &WhisperModelSize, app_handle: AppHandle) -> Result<String> {
        let model_name = format!("{:?}", model_size);
        
        // Emit download started event
        let start_event = DownloadEvent {
            event_type: "started".to_string(),
            progress: None,
            message: format!("Starting download of {} model...", model_name),
        };
        
        if let Err(e) = app_handle.emit("download-event", start_event) {
            eprintln!("Failed to emit download start event: {}", e);
        }
        
        Self::download_model_internal(model_size, Some(app_handle)).await
    }
    
    pub async fn download_model(model_size: &WhisperModelSize) -> Result<String> {
        Self::download_model_internal(model_size, None).await
    }
    
    async fn download_model_internal(model_size: &WhisperModelSize, app_handle: Option<AppHandle>) -> Result<String> {
        let model_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
            .join("echotype")
            .join("models");
        
        std::fs::create_dir_all(&model_dir)?;
        
        let model_path = model_dir.join(model_size.model_filename());
        
        // For Distil-Whisper models, we don't download them manually
        // They'll be downloaded by the Candle backend when needed
        if model_size.is_distil_model() {
            return Ok(model_size.download_url().to_string());
        }
        
        // Check if file exists and has non-zero size
        if model_path.exists() {
            let metadata = std::fs::metadata(&model_path)?;
            if metadata.len() > 0 {
                println!("Model already exists at: {}", model_path.display());
                return Ok(model_path.to_string_lossy().to_string());
            } else {
                println!("Found empty model file, re-downloading...");
                std::fs::remove_file(&model_path)?;
            }
        }
        
        println!("Downloading Whisper model: {:?} from {}", model_size, model_size.download_url());
        
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600)) // 10 minute timeout
            .build()?;
            
        let response = client
            .get(model_size.download_url())
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download model: HTTP {}", response.status()));
        }
        
        let total_size = response
            .content_length()
            .ok_or_else(|| anyhow::anyhow!("Failed to get content length"))?;
        
        println!("Total size: {} MB", total_size / 1024 / 1024);
        
        let mut file = std::fs::File::create(&model_path)?;
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();
        let start_time = Instant::now();
        let mut last_progress_emit = Instant::now();
        
        use futures_util::StreamExt;
        use std::io::Write;
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            
            let progress = (downloaded as f64 / total_size as f64) * 100.0;
            let elapsed = start_time.elapsed();
            let speed_mbps = if elapsed.as_secs() > 0 {
                (downloaded as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64()
            } else {
                0.0
            };
            
            // Calculate ETA
            let eta_seconds = if speed_mbps > 0.0 {
                let remaining_bytes = total_size - downloaded;
                let remaining_mb = remaining_bytes as f64 / 1024.0 / 1024.0;
                Some((remaining_mb / speed_mbps) as u64)
            } else {
                None
            };
            
            // Emit progress event every 1MB or when complete, but not more than once per second
            let should_emit = downloaded % (1024 * 1024) == 0 || 
                             downloaded == total_size || 
                             last_progress_emit.elapsed() >= Duration::from_secs(1);
            
            if should_emit && app_handle.is_some() {
                let model_name = format!("{:?}", model_size);
                let progress_data = DownloadProgress {
                    model_name: model_name.clone(),
                    progress_percent: progress,
                    downloaded_bytes: downloaded,
                    total_bytes: total_size,
                    download_speed_mbps: speed_mbps,
                    eta_seconds,
                    stage: "downloading".to_string(),
                    error_message: None,
                };
                
                let progress_event = DownloadEvent {
                    event_type: "progress".to_string(),
                    progress: Some(progress_data),
                    message: format!("Downloading {} model: {:.1}% ({:.1} MB/s)", 
                                   model_name, progress, speed_mbps),
                };
                
                if let Err(e) = app_handle.as_ref().unwrap().emit("download-event", progress_event) {
                    eprintln!("Failed to emit progress event: {}", e);
                }
                
                last_progress_emit = Instant::now();
            }
            
            // Also log to console
            if downloaded % (1024 * 1024 * 10) == 0 || downloaded == total_size {
                println!("Download progress: {:.1}% ({} MB / {} MB, {:.1} MB/s)", 
                    progress, 
                    downloaded / 1024 / 1024, 
                    total_size / 1024 / 1024,
                    speed_mbps
                );
            }
        }
        
        file.sync_all()?; // Ensure all data is written to disk
        
        println!("Download complete! Model saved to: {}", model_path.display());
        
        // Emit completion event
        if let Some(app_handle) = &app_handle {
            let model_name = format!("{:?}", model_size);
            let completion_progress = DownloadProgress {
                model_name: model_name.clone(),
                progress_percent: 100.0,
                downloaded_bytes: total_size,
                total_bytes: total_size,
                download_speed_mbps: 0.0,
                eta_seconds: Some(0),
                stage: "complete".to_string(),
                error_message: None,
            };
            
            let completion_event = DownloadEvent {
                event_type: "complete".to_string(),
                progress: Some(completion_progress),
                message: format!("{} model downloaded successfully!", model_name),
            };
            
            if let Err(e) = app_handle.emit("download-event", completion_event) {
                eprintln!("Failed to emit completion event: {}", e);
            }
        }
        
        Ok(model_path.to_string_lossy().to_string())
    }
} 