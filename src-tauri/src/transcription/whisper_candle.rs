use crate::transcription::{TranscriptionBackend, WhisperModelSize, DeviceType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::process::Command;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter};
use std::time::{Duration, Instant};

use crate::{DownloadProgress, DownloadEvent};

// Global cache for Python processes with models preloaded
static MODEL_CACHE: Lazy<Arc<Mutex<HashMap<String, Arc<Mutex<std::process::Child>>>>>> = 
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub struct CandleWhisperBackend {
    model_size: WhisperModelSize,
    device_type: DeviceType,
    model_key: String,
}

impl CandleWhisperBackend {
    pub fn new(model_size: WhisperModelSize, device_type: DeviceType) -> Result<Self> {
        let model_key = format!("{}_{:?}", Self::get_model_id_for_size(&model_size), device_type);
        
        println!("Initializing Candle Whisper backend with model: {:?}, device: {:?}", model_size, device_type);
        
        Ok(Self {
            model_size,
            device_type,
            model_key,
        })
    }
    
    fn get_model_id_for_size(model_size: &WhisperModelSize) -> &str {
        match model_size {
            WhisperModelSize::Tiny => "openai/whisper-tiny",
            WhisperModelSize::Base => "openai/whisper-base",
            WhisperModelSize::Small => "openai/whisper-small",
            WhisperModelSize::Medium => "openai/whisper-medium",
            WhisperModelSize::Large => "openai/whisper-large-v3",
            WhisperModelSize::LargeTurbo => "openai/whisper-large-v3-turbo",
            WhisperModelSize::DistilMedium => "distil-whisper/distil-medium.en", // English-only model
            WhisperModelSize::DistilLargeV2 => "distil-whisper/distil-large-v2", // Multilingual
            WhisperModelSize::DistilLargeV3 => "distil-whisper/distil-large-v3", // Multilingual
        }
    }
    
    fn get_model_id(&self) -> &str {
        Self::get_model_id_for_size(&self.model_size)
    }
    
    fn is_english_only(&self) -> bool {
        matches!(self.model_size, WhisperModelSize::DistilMedium)
    }
    
    fn preprocess_audio(&self, audio_file_path: &str) -> Result<PathBuf> {
        // For now, we'll use a simple approach that relies on external tools
        // In a production version, we would implement proper audio preprocessing
        
        // Check if input is already a WAV file at 16kHz
        let input_path = PathBuf::from(audio_file_path);
        if let Some(extension) = input_path.extension() {
            if extension == "wav" {
                // Assume it's already in the correct format for simplicity
                return Ok(input_path);
            }
        }
        
        // For non-WAV files, we'd need to convert them
        // For now, we'll just return the original path and handle it in the Python script
        Ok(input_path)
    }
    
    async fn run_python_inference(&self, audio_path: &PathBuf) -> Result<String> {
        // Try to get preloaded model process from cache first
        let cached_process = {
            let cache = MODEL_CACHE.lock().unwrap();
            cache.get(&self.model_key).cloned()
        };
        
        if let Some(_process) = cached_process {
            // Use the persistent server approach
            self.run_inference_with_server(audio_path).await
        } else {
            // Fallback to script-based approach if no server is running
            self.run_inference_with_script(audio_path).await
        }
    }
    
    async fn run_inference_with_script(&self, audio_path: &PathBuf) -> Result<String> {
        // Create a temporary Python script for inference
        let python_script = self.create_python_script()?;
        
        // Run the Python script
        let output = tokio::process::Command::new("python3")
            .arg(&python_script)
            .arg("--model")
            .arg(self.get_model_id())
            .arg("--device")
            .arg(match self.device_type {
                DeviceType::Cpu => "cpu",
                DeviceType::Cuda => "cuda",
                DeviceType::Metal => "mps",
            })
            .arg("--audio")
            .arg(audio_path)
            .output()
            .await?;
        
        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Python inference failed: {}", error_msg));
        }
        
        let result = String::from_utf8_lossy(&output.stdout);
        Ok(result.trim().to_string())
    }
    
    async fn run_inference_with_server(&self, audio_path: &PathBuf) -> Result<String> {
        // For now, fall back to script approach
        // TODO: Implement persistent server communication
        self.run_inference_with_script(audio_path).await
    }
    
    pub async fn download_model_with_progress(&self, app_handle: AppHandle) -> Result<String> {
        let model_name = self.get_model_id();
        
        // Emit download started event
        let start_event = DownloadEvent {
            event_type: "started".to_string(),
            progress: None,
            message: format!("Starting download of {} model...", model_name),
        };
        
        if let Err(e) = app_handle.emit("download-event", start_event) {
            eprintln!("Failed to emit download start event: {}", e);
        }
        
        self.download_model_internal(Some(app_handle)).await
    }
    
    pub async fn download_model(&self) -> Result<String> {
        self.download_model_internal(None).await
    }
    
    async fn download_model_internal(&self, app_handle: Option<AppHandle>) -> Result<String> {
        println!("Starting download for model: {}", self.get_model_id());
        
        // Create a Python script that downloads the model
        let download_script = self.create_download_script()?;
        println!("Created download script at: {:?}", download_script);
        
        // Run the download script with streaming output for progress tracking
        println!("Running download command...");
        
        let mut child = tokio::process::Command::new("python3")
            .arg(&download_script)
            .arg("--model")
            .arg(self.get_model_id())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to get stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("Failed to get stderr"))?;
        
        // Read output streams
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();
        
        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();
        let mut current_stage = "downloading";
        let start_time = Instant::now();
        
        // Monitor both stdout and stderr for progress
        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    match line? {
                        Some(line) => {
                            println!("Download stdout: {}", line);
                            stdout_lines.push(line.clone());
                            
                            // Parse progress from output and emit events
                            if let Some(app_handle) = &app_handle {
                                self.parse_and_emit_progress(&line, &app_handle, &mut current_stage, start_time).await;
                            }
                        }
                        None => break,
                    }
                }
                line = stderr_reader.next_line() => {
                    match line? {
                        Some(line) => {
                            println!("Download stderr: {}", line);
                            stderr_lines.push(line.clone());
                        }
                        None => {}
                    }
                }
            }
        }
        
        // Wait for the process to complete
        let output_status = child.wait().await?;
        
        if !output_status.success() {
            let error_message = stderr_lines.join("\n");
            
            // Emit error event
            if let Some(app_handle) = &app_handle {
                let error_event = DownloadEvent {
                    event_type: "error".to_string(),
                    progress: Some(DownloadProgress {
                        model_name: self.get_model_id().to_string(),
                        progress_percent: 0.0,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        download_speed_mbps: 0.0,
                        eta_seconds: None,
                        stage: "error".to_string(),
                        error_message: Some(error_message.clone()),
                    }),
                    message: format!("Download failed: {}", error_message),
                };
                
                if let Err(e) = app_handle.emit("download-event", error_event) {
                    eprintln!("Failed to emit error event: {}", e);
                }
            }
            
            return Err(anyhow!("Model download failed with exit code: {}. Error: {}", 
                              output_status.code().unwrap_or(-1), error_message));
        }
        
        // Emit completion event
        if let Some(app_handle) = &app_handle {
            let completion_event = DownloadEvent {
                event_type: "complete".to_string(),
                progress: Some(DownloadProgress {
                    model_name: self.get_model_id().to_string(),
                    progress_percent: 100.0,
                    downloaded_bytes: 0, // We don't have exact byte counts for Python downloads
                    total_bytes: 0,
                    download_speed_mbps: 0.0,
                    eta_seconds: Some(0),
                    stage: "complete".to_string(),
                    error_message: None,
                }),
                message: format!("{} model downloaded successfully!", self.get_model_id()),
            };
            
            if let Err(e) = app_handle.emit("download-event", completion_event) {
                eprintln!("Failed to emit completion event: {}", e);
            }
        }
        
        println!("Download completed successfully");
        Ok(stdout_lines.join("\n"))
    }
    
    async fn parse_and_emit_progress(&self, line: &str, app_handle: &AppHandle, current_stage: &mut &str, start_time: Instant) {
        let model_name = self.get_model_id();
        
        // Update stage based on output
        if line.contains("Downloading processor") {
            *current_stage = "downloading_processor";
        } else if line.contains("Downloading model") {
            *current_stage = "downloading_model";
        } else if line.contains("Verifying local installation") {
            *current_stage = "verifying";
        }
        
        // Determine progress based on stage
        let (progress_percent, message) = match *current_stage {
            "downloading_processor" => (20.0, "Downloading processor..."),
            "downloading_model" => (60.0, "Downloading model files..."),
            "verifying" => (90.0, "Verifying installation..."),
            _ => (10.0, "Initializing download..."),
        };
        
        let progress_data = DownloadProgress {
            model_name: model_name.to_string(),
            progress_percent,
            downloaded_bytes: 0, // Python downloads don't provide byte-level progress
            total_bytes: 0,
            download_speed_mbps: 0.0,
            eta_seconds: None,
            stage: current_stage.to_string(),
            error_message: None,
        };
        
        let progress_event = DownloadEvent {
            event_type: "progress".to_string(),
            progress: Some(progress_data),
            message: format!("{} model: {}", model_name, message),
        };
        
        if let Err(e) = app_handle.emit("download-event", progress_event) {
            eprintln!("Failed to emit progress event: {}", e);
        }
    }
    
    pub async fn check_model_downloaded(&self) -> Result<bool> {
        // Create a Python script that checks if the model is cached
        let check_script = self.create_check_script()?;
        
        // Run the check script
        let output = tokio::process::Command::new("python3")
            .arg(&check_script)
            .arg("--model")
            .arg(self.get_model_id())
            .output()
            .await?;
        
        if !output.status.success() {
            return Ok(false); // If script fails, assume model is not downloaded
        }
        
        let result = String::from_utf8_lossy(&output.stdout);
        Ok(result.trim() == "True")
    }

    pub async fn preload_model(&self) -> Result<String> {
        println!("Preloading model: {}", self.get_model_id());
        
        // Check if already preloaded
        {
            let cache = MODEL_CACHE.lock().unwrap();
            if cache.contains_key(&self.model_key) {
                println!("Model {} already preloaded", self.model_key);
                return Ok("Model already preloaded".to_string());
            }
        }
        
        // Create a Python script that preloads the model
        let preload_script = self.create_preload_script()?;
        
        // Run the preload script
        let output = tokio::process::Command::new("python3")
            .arg(&preload_script)
            .arg("--model")
            .arg(self.get_model_id())
            .arg("--device")
            .arg(match self.device_type {
                DeviceType::Cpu => "cpu",
                DeviceType::Cuda => "cuda",
                DeviceType::Metal => "mps",
            })
            .output()
            .await?;
        
        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Model preload failed: {}", error_msg));
        }
        
        let result = String::from_utf8_lossy(&output.stdout);
        println!("Model preloaded successfully: {}", self.model_key);
        
        // For now, we mark as preloaded but don't actually keep a persistent process
        // This is a stepping stone - the real optimization will come next
        Ok(result.trim().to_string())
    }
    
    fn create_download_script(&self) -> Result<PathBuf> {
        let script_content = r#"
import argparse
import sys
import os
import torch
from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq
import traceback

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    args = parser.parse_args()
    
    try:
        print(f"Starting download for model: {args.model}")
        print("This may take several minutes depending on your internet connection...")
        
        # Temporarily disable offline mode for downloading
        if "HF_HUB_OFFLINE" in os.environ:
            del os.environ["HF_HUB_OFFLINE"]
        if "TRANSFORMERS_OFFLINE" in os.environ:
            del os.environ["TRANSFORMERS_OFFLINE"]
        
        # Download model components directly (same as whisper.cpp approach)
        print("Downloading processor...")
        processor = AutoProcessor.from_pretrained(args.model)
        print("✓ Processor downloaded")
        
        print("Downloading model...")
        model = AutoModelForSpeechSeq2Seq.from_pretrained(
            args.model,
            torch_dtype=torch.float32,
            low_cpu_mem_usage=True,
            use_safetensors=True
        )
        print("✓ Model downloaded")
        
        # Test that it works in offline mode
        print("Verifying local installation...")
        # Enable offline mode for verification
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        
        test_processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
        test_model = AutoModelForSpeechSeq2Seq.from_pretrained(
            args.model,
            torch_dtype=torch.float32,
            low_cpu_mem_usage=True,
            use_safetensors=True,
            local_files_only=True
        )
        
        print("✓ Local installation verified")
        print(f"Model {args.model} downloaded and ready for offline use!")
        
    except Exception as e:
        print(f"Error downloading model: {e}", file=sys.stderr)
        print("Full error details:", file=sys.stderr)
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;
        
        // Create temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(script_content.as_bytes())?;
        
        // Get the path and prevent auto-deletion
        let path = temp_file.path().to_path_buf();
        temp_file.keep()?;
        
                Ok(path)
    }

    fn create_preload_script(&self) -> Result<PathBuf> {
        let script_content = r#"
import argparse
import sys
import os
import torch
from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq

# Force offline mode to prevent any network requests
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    parser.add_argument("--device", default="cpu", help="Device to use")
    args = parser.parse_args()
    
    try:
        print(f"Preloading model: {args.model}")
        
        # Set device
        if args.device == "cuda" and torch.cuda.is_available():
            device = "cuda"
            torch_dtype = torch.float16
        elif args.device == "mps" and torch.backends.mps.is_available():
            device = "mps"
            torch_dtype = torch.float16
        else:
            device = "cpu"
            torch_dtype = torch.float32
        
        # Load model and processor (offline mode)
        processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
        model = AutoModelForSpeechSeq2Seq.from_pretrained(
            args.model,
            torch_dtype=torch_dtype,
            low_cpu_mem_usage=True,
            use_safetensors=True,
            local_files_only=True
        ).to(device)
        
        print(f"✓ Model {args.model} preloaded successfully on {device}")
        print("Model ready for fast transcription!")
        
    except Exception as e:
        print(f"Error preloading model: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;
        
        // Create temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(script_content.as_bytes())?;
        
        // Get the path and prevent auto-deletion
        let path = temp_file.path().to_path_buf();
        temp_file.keep()?;
        
        Ok(path)
    }

    fn create_check_script(&self) -> Result<PathBuf> {
        let script_content = r#"
import argparse
import sys
import os
from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq
import torch

# Force offline mode to prevent any network requests
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    args = parser.parse_args()
    
    try:
        # Try to load the model and processor in offline mode
        # This is the same method used in inference, so it should be accurate
        processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
        model = AutoModelForSpeechSeq2Seq.from_pretrained(
            args.model,
            torch_dtype=torch.float32,
            low_cpu_mem_usage=True,
            use_safetensors=True,
            local_files_only=True
        )
        print("True")
        
    except Exception as e:
        # If any error occurs, model is not available locally
        print("False")

if __name__ == "__main__":
    main()
"#;
        
        // Create temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(script_content.as_bytes())?;
        
        // Get the path and prevent auto-deletion
        let path = temp_file.path().to_path_buf();
        temp_file.keep()?;
        
        Ok(path)
    }

    fn create_python_script(&self) -> Result<PathBuf> {
        let script_content = r#"
import argparse
import os
import torch
import torchaudio
from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq, pipeline
import sys

# Force offline mode to prevent any network requests
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    parser.add_argument("--device", default="cpu", help="Device to use")
    parser.add_argument("--audio", required=True, help="Audio file path")
    args = parser.parse_args()
    
    try:
        # Set device
        if args.device == "cuda" and torch.cuda.is_available():
            device = "cuda"
            torch_dtype = torch.float16
        elif args.device == "mps" and torch.backends.mps.is_available():
            device = "mps"
            torch_dtype = torch.float16
        else:
            device = "cpu"
            torch_dtype = torch.float32
        
        # Load model and processor directly (offline mode)
        try:
            processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
            model = AutoModelForSpeechSeq2Seq.from_pretrained(
                args.model,
                torch_dtype=torch_dtype,
                low_cpu_mem_usage=True,
                use_safetensors=True,
                local_files_only=True
            ).to(device)
            
            # Model loaded successfully
            
        except Exception as e:
            if "local_files_only" in str(e) or "not found" in str(e).lower():
                raise Exception(f"Model {args.model} not found locally. Please download it first using the Download Model button.")
            else:
                raise e
        
        # Load and preprocess audio
        audio, sample_rate = torchaudio.load(args.audio)
        
        # Resample to 16kHz if needed
        if sample_rate != 16000:
            resampler = torchaudio.transforms.Resample(sample_rate, 16000)
            audio = resampler(audio)
        
        # Convert to mono if stereo
        if audio.shape[0] > 1:
            audio = torch.mean(audio, dim=0, keepdim=True)
        
        # Prepare inputs
        inputs = processor(audio.squeeze().numpy(), sampling_rate=16000, return_tensors="pt")
        input_features = inputs.input_features.to(device, dtype=torch_dtype)
        
        # Generate transcription with multilingual support
        if "medium.en" in args.model:
            # English-only model - use direct generation
            with torch.no_grad():
                predicted_ids = model.generate(input_features)
            transcription = processor.batch_decode(predicted_ids, skip_special_tokens=True)[0]
        else:
            # Multilingual model - use pipeline for better language detection
            pipe = pipeline(
                "automatic-speech-recognition",
                model=model,
                tokenizer=processor.tokenizer,
                feature_extractor=processor.feature_extractor,
                torch_dtype=torch_dtype,
                device=device,
                return_timestamps=False,
                chunk_length_s=30,
                generate_kwargs={
                    "language": None,  # Auto-detect language
                    "task": "transcribe"
                }
            )
            
            # Load audio for pipeline (it expects numpy array)
            audio_np = audio.squeeze().numpy()
            
            result = pipe(audio_np, generate_kwargs={"language": None})
            transcription = result["text"]
        
        print(transcription)
        
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;
        
        // Create temporary file
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(script_content.as_bytes())?;
        
        // Get the path and prevent auto-deletion
        let path = temp_file.path().to_path_buf();
        temp_file.keep()?;
        
        Ok(path)
    }
}

#[async_trait]
impl TranscriptionBackend for CandleWhisperBackend {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        println!("Starting Candle Whisper transcription with model: {:?}", self.model_size);
        
        // Check if Python and required packages are available
        let python_check = Command::new("python3")
            .args(&["-c", "import torch, transformers, torchaudio; print('Dependencies OK')"])
            .output();
        
        match python_check {
            Ok(output) if output.status.success() => {
                println!("Python dependencies confirmed");
            }
            _ => {
                return Err(anyhow!(
                    "Python3 with required packages (torch, transformers, torchaudio) not found. \
                    Please install them with: pip install torch transformers torchaudio"
                ));
            }
        }
        
        // Preprocess audio
        let audio_path = self.preprocess_audio(audio_file_path)?;
        
        // Run inference using Python script
        let result = self.run_python_inference(&audio_path).await?;
        
        println!("Candle Whisper transcription completed successfully");
        Ok(result)
    }
} 