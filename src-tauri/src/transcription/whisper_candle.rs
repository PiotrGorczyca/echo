use crate::transcription::{TranscriptionBackend, WhisperModelSize, DeviceType, python_env};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter};
use std::time::{Instant};

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
        model_size.hf_model_id()
    }

    fn get_model_id(&self) -> &str {
        Self::get_model_id_for_size(&self.model_size)
    }

    fn is_english_only(&self) -> bool {
        self.model_size.is_english_only()
    }

    fn is_moonshine(&self) -> bool {
        self.model_size.is_moonshine_model()
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
        
        let python_cmd = self.get_python_cmd();

        let mut command = tokio::process::Command::new(&python_cmd);
        command
            .arg(&python_script)
            .arg("--model")
            .arg(self.get_model_id())
            .arg("--device")
            .arg(match self.device_type {
                DeviceType::Cpu => "cpu",
                DeviceType::Cuda => "cuda",
                DeviceType::Metal => "mps",
                DeviceType::Rocm => "cuda",
            })
            .arg("--audio")
            .arg(audio_path);

        if self.device_type == DeviceType::Rocm {
            self.set_rocm_env(&mut command);
        }

        let output = command.output().await?;
        
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

        let python_cmd = self.get_python_cmd();

        let mut child = {
            let mut cmd = tokio::process::Command::new(&python_cmd);
            cmd.arg(&download_script)
                .arg("--model")
                .arg(self.get_model_id())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            if self.device_type == DeviceType::Rocm {
                self.set_rocm_env(&mut cmd);
            }

            cmd.spawn()?
        };
        
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
    
    async fn parse_and_emit_progress(&self, line: &str, app_handle: &AppHandle, current_stage: &mut &str, _start_time: Instant) {
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
        
        let python_cmd = self.get_python_cmd();

        let output = {
            let mut cmd = tokio::process::Command::new(&python_cmd);
            cmd.arg(&check_script)
                .arg("--model")
                .arg(self.get_model_id());

            if self.device_type == DeviceType::Rocm {
                self.set_rocm_env(&mut cmd);
            }

            cmd.output().await?
        };
        
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
        
        let python_cmd = self.get_python_cmd();

        let output = {
            let mut cmd = tokio::process::Command::new(&python_cmd);
            cmd.arg(&preload_script)
                .arg("--model")
                .arg(self.get_model_id())
                .arg("--device")
                .arg(match self.device_type {
                    DeviceType::Cpu => "cpu",
                    DeviceType::Cuda => "cuda",
                    DeviceType::Metal => "mps",
                    DeviceType::Rocm => "cuda",
                });

            if self.device_type == DeviceType::Rocm {
                self.set_rocm_env(&mut cmd);
            }

            cmd.output().await?
        };
        
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
import traceback

def get_model_class(model_id):
    """Return the appropriate model class for the given model ID."""
    if "moonshine" in model_id.lower():
        from transformers import MoonshineForConditionalGeneration
        return MoonshineForConditionalGeneration
    else:
        from transformers import AutoModelForSpeechSeq2Seq
        return AutoModelForSpeechSeq2Seq

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    args = parser.parse_args()

    try:
        from transformers import AutoProcessor
        ModelClass = get_model_class(args.model)

        print(f"Starting download for model: {args.model}")
        print("This may take several minutes depending on your internet connection...")

        # Temporarily disable offline mode for downloading
        if "HF_HUB_OFFLINE" in os.environ:
            del os.environ["HF_HUB_OFFLINE"]
        if "TRANSFORMERS_OFFLINE" in os.environ:
            del os.environ["TRANSFORMERS_OFFLINE"]

        print("Downloading processor...")
        processor = AutoProcessor.from_pretrained(args.model)
        print("Processor downloaded")

        print("Downloading model...")
        model = ModelClass.from_pretrained(
            args.model,
            torch_dtype=torch.float32,
            low_cpu_mem_usage=True,
        )
        print("Model downloaded")

        # Verify offline mode works
        print("Verifying local installation...")
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"

        test_processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
        test_model = ModelClass.from_pretrained(
            args.model,
            torch_dtype=torch.float32,
            low_cpu_mem_usage=True,
            local_files_only=True
        )

        print("Local installation verified")
        print(f"Model {args.model} downloaded and ready for offline use!")

    except Exception as e:
        print(f"Error downloading model: {e}", file=sys.stderr)
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

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    parser.add_argument("--device", default="cpu", help="Device to use")
    args = parser.parse_args()

    try:
        from transformers import AutoProcessor
        print(f"Preloading model: {args.model}")

        if args.device == "cuda" and torch.cuda.is_available():
            device, torch_dtype = "cuda", torch.float16
        elif args.device == "mps" and torch.backends.mps.is_available():
            device, torch_dtype = "mps", torch.float16
        else:
            device, torch_dtype = "cpu", torch.float32

        processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)

        if "moonshine" in args.model.lower():
            from transformers import MoonshineForConditionalGeneration
            model = MoonshineForConditionalGeneration.from_pretrained(
                args.model, local_files_only=True
            ).to(device).to(torch_dtype)
        else:
            from transformers import AutoModelForSpeechSeq2Seq
            model = AutoModelForSpeechSeq2Seq.from_pretrained(
                args.model, torch_dtype=torch_dtype, low_cpu_mem_usage=True,
                use_safetensors=True, local_files_only=True
            ).to(device)

        print(f"Model {args.model} preloaded successfully on {device}")

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
import torch

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    args = parser.parse_args()

    try:
        from transformers import AutoProcessor
        processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)

        if "moonshine" in args.model.lower():
            from transformers import MoonshineForConditionalGeneration
            model = MoonshineForConditionalGeneration.from_pretrained(
                args.model, torch_dtype=torch.float32, low_cpu_mem_usage=True, local_files_only=True
            )
        else:
            from transformers import AutoModelForSpeechSeq2Seq
            model = AutoModelForSpeechSeq2Seq.from_pretrained(
                args.model, torch_dtype=torch.float32, low_cpu_mem_usage=True,
                use_safetensors=True, local_files_only=True
            )
        print("True")
    except Exception:
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
import sys

# Force offline mode to prevent any network requests
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def detect_device(requested_device):
    """Detect and return the best available device."""
    if requested_device == "cuda":
        if torch.cuda.is_available():
            if torch.version.hip is not None:
                try:
                    device = "cuda"
                    torch_dtype = torch.float16
                    print(f"Attempting ROCm backend on device: {torch.cuda.get_device_name(0)}", file=sys.stderr)
                    test_tensor = torch.tensor([1.0], device=device, dtype=torch_dtype)
                    _ = test_tensor * 2
                    print("ROCm GPU compatibility test passed", file=sys.stderr)
                    return device, torch_dtype
                except Exception as e:
                    print(f"ROCm GPU failed ({str(e)[:50]}...), falling back to CPU", file=sys.stderr)
                    return "cpu", torch.float32
            else:
                print(f"Using CUDA backend on device: {torch.cuda.get_device_name(0)}", file=sys.stderr)
                return "cuda", torch.float16
        else:
            print("Warning: CUDA/ROCm not available, falling back to CPU", file=sys.stderr)
            return "cpu", torch.float32
    elif requested_device == "mps" and torch.backends.mps.is_available():
        print("Using Metal Performance Shaders (MPS) backend", file=sys.stderr)
        return "mps", torch.float16
    else:
        print("Using CPU backend", file=sys.stderr)
        return "cpu", torch.float32

def transcribe_moonshine(args, device, torch_dtype):
    """Transcribe using Moonshine model."""
    from transformers import AutoProcessor, MoonshineForConditionalGeneration

    processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
    model = MoonshineForConditionalGeneration.from_pretrained(
        args.model, local_files_only=True
    ).to(device).to(torch_dtype)

    audio, sample_rate = torchaudio.load(args.audio)
    target_rate = processor.feature_extractor.sampling_rate
    if sample_rate != target_rate:
        audio = torchaudio.transforms.Resample(sample_rate, target_rate)(audio)
    if audio.shape[0] > 1:
        audio = torch.mean(audio, dim=0, keepdim=True)

    inputs = processor(audio.squeeze().numpy(), return_tensors="pt", sampling_rate=target_rate)
    inputs = {k: v.to(device, torch_dtype) if v.dtype.is_floating_point else v.to(device) for k, v in inputs.items()}

    # Limit max length to avoid hallucinations (Moonshine-specific)
    token_limit_factor = 6.5 / target_rate
    seq_lens = inputs["attention_mask"].sum(dim=-1)
    max_length = int((seq_lens * token_limit_factor).max().item())

    with torch.no_grad():
        generated_ids = model.generate(**inputs, max_length=max(max_length, 1))
    return processor.decode(generated_ids[0], skip_special_tokens=True)

def transcribe_whisper(args, device, torch_dtype):
    """Transcribe using Whisper or Distil-Whisper model."""
    from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq, pipeline

    try:
        processor = AutoProcessor.from_pretrained(args.model, local_files_only=True)
        model = AutoModelForSpeechSeq2Seq.from_pretrained(
            args.model,
            torch_dtype=torch_dtype,
            low_cpu_mem_usage=True,
            use_safetensors=True,
            local_files_only=True
        ).to(device)
    except Exception as e:
        if "local_files_only" in str(e) or "not found" in str(e).lower():
            raise Exception(f"Model {args.model} not found locally. Please download it first using the Download Model button.")
        raise

    audio, sample_rate = torchaudio.load(args.audio)
    if sample_rate != 16000:
        audio = torchaudio.transforms.Resample(sample_rate, 16000)(audio)
    if audio.shape[0] > 1:
        audio = torch.mean(audio, dim=0, keepdim=True)

    audio_np = audio.squeeze().numpy()

    # English-only models use direct generation
    if any(tag in args.model for tag in [".en", "moonshine"]):
        inputs = processor(audio_np, sampling_rate=16000, return_tensors="pt")
        input_features = inputs.input_features.to(device, dtype=torch_dtype)
        with torch.no_grad():
            predicted_ids = model.generate(input_features)
        return processor.batch_decode(predicted_ids, skip_special_tokens=True)[0]
    else:
        # Multilingual models use pipeline for language detection
        pipe = pipeline(
            "automatic-speech-recognition",
            model=model,
            tokenizer=processor.tokenizer,
            feature_extractor=processor.feature_extractor,
            torch_dtype=torch_dtype,
            device=device,
            return_timestamps=False,
            chunk_length_s=30,
            generate_kwargs={"language": None, "task": "transcribe"}
        )
        result = pipe(audio_np, generate_kwargs={"language": None})
        return result["text"]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, help="Model ID")
    parser.add_argument("--device", default="cpu", help="Device to use")
    parser.add_argument("--audio", required=True, help="Audio file path")
    args = parser.parse_args()

    try:
        device, torch_dtype = detect_device(args.device)

        if "moonshine" in args.model.lower():
            transcription = transcribe_moonshine(args, device, torch_dtype)
        else:
            transcription = transcribe_whisper(args, device, torch_dtype)

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

    fn get_python_cmd(&self) -> String {
        if self.device_type == DeviceType::Rocm {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
            format!("{}/.rocm_pytorch_env/bin/python", home)
        } else {
            python_env::venv_python().unwrap_or_else(|_| "python3".to_string())
        }
    }

    fn set_rocm_env(&self, cmd: &mut tokio::process::Command) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
        let rocm_site_packages = format!("{}/.rocm_pytorch_env/lib/python3.13/site-packages", home);
        cmd.env("HSA_OVERRIDE_GFX_VERSION", "11.0.2")
            .env("HIP_VISIBLE_DEVICES", "0")
            .env("ROCR_VISIBLE_DEVICES", "0")
            .env("CUDA_VISIBLE_DEVICES", "0")
            .env("PYTHONPATH", &rocm_site_packages);
    }
}

#[async_trait]
impl TranscriptionBackend for CandleWhisperBackend {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        println!("Starting HuggingFace Whisper transcription with model: {:?}", self.model_size);

        // Verify deps are present (should have been installed via Settings)
        let packages = python_env::required_packages("CandleWhisper");
        let pkg_refs: Vec<&str> = packages.iter().map(|s| *s).collect();
        if !python_env::check_packages(&pkg_refs) {
            return Err(anyhow!(
                "HuggingFace Whisper dependencies are not installed. \
                 Please go to Settings and install dependencies first."
            ));
        }

        // Preprocess audio
        let audio_path = self.preprocess_audio(audio_file_path)?;

        // Run inference using Python script
        let result = self.run_python_inference(&audio_path).await?;

        println!("HuggingFace Whisper transcription completed successfully");
        Ok(result)
    }
} 