use crate::transcription::{TranscriptionBackend, WhisperModelSize, DeviceType, python_env};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use std::process::Stdio;

/// Persistent faster-whisper server script.
/// Loads the model once, then accepts JSON-line requests on stdin
/// and writes JSON-line responses to stdout.
const SERVER_SCRIPT: &str = r#"
import sys, json

def main():
    config = json.loads(sys.stdin.readline())
    model_id = config["model"]
    device = config["device"]
    compute_type = config["compute_type"]

    try:
        from faster_whisper import WhisperModel

        if device not in ("cpu", "cuda"):
            device = "cpu"

        print(f"Loading model {model_id} on {device} ({compute_type})...", file=sys.stderr, flush=True)
        model = WhisperModel(model_id, device=device, compute_type=compute_type)
        print(f"Model loaded successfully", file=sys.stderr, flush=True)

        # Signal ready
        print(json.dumps({"status": "ready"}), flush=True)

        # Process requests line by line
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                request = json.loads(line)
                if request.get("command") == "quit":
                    break
                if request.get("command") == "transcribe":
                    audio_path = request["audio"]
                    print(f"Transcribing {audio_path}...", file=sys.stderr, flush=True)
                    segments, info = model.transcribe(
                        audio_path,
                        beam_size=5,
                        vad_filter=True,
                        vad_parameters=dict(min_silence_duration_ms=300),
                    )
                    print(f"Detected language: {info.language} (prob {info.language_probability:.2f})", file=sys.stderr, flush=True)
                    text_parts = [segment.text.strip() for segment in segments]
                    transcription = " ".join(text_parts)
                    print(json.dumps({"status": "ok", "text": transcription}), flush=True)
                else:
                    print(json.dumps({"status": "error", "error": f"Unknown command: {request.get('command')}"}), flush=True)
            except Exception as e:
                print(json.dumps({"status": "error", "error": str(e)}), flush=True)

    except Exception as e:
        print(json.dumps({"status": "error", "error": str(e)}), flush=True)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;

struct ServerProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

pub struct FasterWhisperBackend {
    model_size: WhisperModelSize,
    device_type: DeviceType,
    /// Lazily-started persistent server process.
    /// The Mutex here serializes concurrent transcription requests to the single process,
    /// but crucially does NOT require re-loading the model each time.
    server: Mutex<Option<ServerProcess>>,
}

impl FasterWhisperBackend {
    pub fn new(model_size: WhisperModelSize, device_type: DeviceType) -> Result<Self> {
        println!(
            "Initializing faster-whisper backend with model: {:?}, device: {:?}",
            model_size, device_type
        );
        Ok(Self {
            model_size,
            device_type,
            server: Mutex::new(None),
        })
    }

    fn get_model_id(&self) -> &str {
        match &self.model_size {
            WhisperModelSize::Tiny => "tiny",
            WhisperModelSize::Base => "base",
            WhisperModelSize::Small | WhisperModelSize::SmallQ5 => "small",
            WhisperModelSize::Medium | WhisperModelSize::MediumQ5 => "medium",
            WhisperModelSize::Large | WhisperModelSize::LargeV3Q5 => "large-v3",
            WhisperModelSize::LargeTurbo
            | WhisperModelSize::LargeTurboQ5
            | WhisperModelSize::LargeTurboQ8 => "turbo",
            WhisperModelSize::DistilSmall => "distil-whisper/distil-small.en",
            WhisperModelSize::DistilMedium => "distil-whisper/distil-medium.en",
            WhisperModelSize::DistilLargeV2 => "distil-whisper/distil-large-v2",
            WhisperModelSize::DistilLargeV3 => "distil-whisper/distil-large-v3",
            WhisperModelSize::MoonshineTiny | WhisperModelSize::MoonshineBase => "base",
        }
    }

    fn compute_type(&self) -> &str {
        match &self.device_type {
            DeviceType::Cuda => "float16",
            DeviceType::Cpu | DeviceType::Rocm | DeviceType::Metal => "int8",
        }
    }

    fn device_str(&self) -> &str {
        match &self.device_type {
            DeviceType::Cuda => "cuda",
            _ => "cpu",
        }
    }

    /// Write the server script to a persistent location (not a temp file that gets deleted).
    fn write_server_script() -> Result<std::path::PathBuf> {
        let script_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("echo");
        std::fs::create_dir_all(&script_dir)?;
        let script_path = script_dir.join("faster_whisper_server.py");
        std::fs::write(&script_path, SERVER_SCRIPT)?;
        Ok(script_path)
    }

    /// Start the persistent Python server process and wait for it to load the model.
    async fn start_server(&self) -> Result<ServerProcess> {
        let packages = python_env::required_packages("FasterWhisper");
        let pkg_refs: Vec<&str> = packages.iter().map(|s| *s).collect();
        if !python_env::check_packages(&pkg_refs) {
            return Err(anyhow!(
                "faster-whisper is not installed. Please go to Settings and install dependencies first."
            ));
        }

        let python = python_env::venv_python()?;
        let script_path = Self::write_server_script()?;

        println!("[faster-whisper] Starting persistent server process...");

        let mut child = Command::new(&python)
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow!("Failed to get stdin of faster-whisper server"))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to get stdout of faster-whisper server"))?;
        let stderr = child.stderr.take();

        // Spawn a task to forward stderr lines for logging
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    println!("[faster-whisper] {}", line);
                }
            });
        }

        let mut server = ServerProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Send config to start model loading
        let config = serde_json::json!({
            "model": self.get_model_id(),
            "device": self.device_str(),
            "compute_type": self.compute_type(),
        });
        let mut config_line = serde_json::to_string(&config)?;
        config_line.push('\n');
        server.stdin.write_all(config_line.as_bytes()).await?;
        server.stdin.flush().await?;

        // Wait for the "ready" signal
        let mut response_line = String::new();
        server.stdout.read_line(&mut response_line).await?;
        let response: serde_json::Value = serde_json::from_str(response_line.trim())
            .map_err(|e| anyhow!("Invalid JSON from server: {} (raw: {})", e, response_line.trim()))?;

        if response.get("status").and_then(|s| s.as_str()) == Some("ready") {
            println!("[faster-whisper] Server ready - model loaded");
            Ok(server)
        } else {
            let err = response.get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error during model loading");
            Err(anyhow!("faster-whisper server failed to start: {}", err))
        }
    }

    /// Ensure the server is running (start if needed) and return a mutable reference.
    async fn ensure_server(&self) -> Result<tokio::sync::MutexGuard<'_, Option<ServerProcess>>> {
        let mut guard = self.server.lock().await;

        // Check if existing server is still alive
        if let Some(ref mut srv) = *guard {
            match srv.child.try_wait() {
                Ok(Some(_)) => {
                    println!("[faster-whisper] Server process exited, restarting...");
                    *guard = None;
                }
                Ok(None) => return Ok(guard), // Still running
                Err(_) => {
                    *guard = None;
                }
            }
        }

        // Start new server
        let server = self.start_server().await?;
        *guard = Some(server);
        Ok(guard)
    }
}

#[async_trait]
impl TranscriptionBackend for FasterWhisperBackend {
    async fn warm_up(&self) -> Result<()> {
        println!("[faster-whisper] Warming up - pre-loading model...");
        let _ = self.ensure_server().await?;
        Ok(())
    }

    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        println!(
            "Starting faster-whisper transcription with model: {:?}",
            self.model_size
        );

        if self.model_size.is_moonshine_model() {
            return Err(anyhow!(
                "Moonshine models are not supported by faster-whisper. \
                 Please use the HuggingFace Whisper backend for Moonshine models."
            ));
        }

        let mut guard = self.ensure_server().await?;
        let server = guard.as_mut()
            .ok_or_else(|| anyhow!("Server not available after ensure_server"))?;

        // Send transcription request
        let request = serde_json::json!({
            "command": "transcribe",
            "audio": audio_file_path,
        });
        let mut request_line = serde_json::to_string(&request)?;
        request_line.push('\n');
        server.stdin.write_all(request_line.as_bytes()).await?;
        server.stdin.flush().await?;

        // Read response
        let mut response_line = String::new();
        server.stdout.read_line(&mut response_line).await?;

        if response_line.is_empty() {
            // Process likely crashed
            *guard = None;
            return Err(anyhow!("faster-whisper server process terminated unexpectedly"));
        }

        let response: serde_json::Value = serde_json::from_str(response_line.trim())
            .map_err(|e| anyhow!("Invalid JSON from server: {} (raw: {})", e, response_line.trim()))?;

        match response.get("status").and_then(|s| s.as_str()) {
            Some("ok") => {
                let text = response.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                println!("faster-whisper transcription completed successfully");
                Ok(text)
            }
            Some("error") => {
                let err = response.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error");
                Err(anyhow!("faster-whisper inference failed: {}", err))
            }
            _ => Err(anyhow!("Unexpected response from server: {}", response_line.trim())),
        }
    }
}

impl Drop for FasterWhisperBackend {
    fn drop(&mut self) {
        // Try to gracefully shut down the server process
        if let Ok(mut guard) = self.server.try_lock() {
            if let Some(ref mut server) = *guard {
                // Best effort: kill the process (stdin write requires async)
                let _ = server.child.start_kill();
            }
        }
    }
}
