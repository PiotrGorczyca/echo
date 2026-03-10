use crate::transcription::{TranscriptionService, TranscriptionConfig, TranscriptionMode, WhisperModelSize, DeviceType};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{StreamConfig, SampleFormat};
use hound::{WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use tauri::{AppHandle, Emitter};
use anyhow::Result;

const WAKE_WORD_BUFFER_DURATION_SEC: f32 = 2.0; // Keep 2 seconds of audio for wake word detection (reduced for faster processing)
const WAKE_WORD_CHECK_INTERVAL_MS: u64 = 50; // Check for voice activity every 50ms (more responsive)
const SAMPLE_RATE: u32 = 16000; // 16kHz for whisper compatibility

// Voice Activity Detection parameters
const DEFAULT_VAD_ENERGY_THRESHOLD: f32 = 0.001; // More sensitive default threshold
const VAD_MIN_SPEECH_DURATION_MS: u64 = 300; // Minimum speech duration before transcribing
const VAD_SILENCE_TIMEOUT_MS: u64 = 500; // How long to wait after speech ends before processing
const VAD_WINDOW_SIZE: usize = 800; // 50ms window at 16kHz for energy calculation

// Auto-calibration parameters
const CALIBRATION_SAMPLE_COUNT: usize = 50; // Number of samples to use for calibration (faster)
const AMBIENT_MULTIPLIER: f32 = 2.5; // Threshold = ambient_noise_level * multiplier (more sensitive)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeWordDetection {
    pub detected: bool,
    pub confidence: f32,
    pub word: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceActivityInfo {
    pub is_listening: bool,
    pub current_energy: f32,
    pub threshold: f32,
    pub is_speech_detected: bool,
    pub speech_duration_ms: u64,
}

pub struct VoiceActivationService {
    config: VoiceActivationConfig,
    audio_buffer: Arc<Mutex<VecDeque<f32>>>,
    transcription_service: Option<Arc<TranscriptionService>>, // External service, can be None
    is_listening: Arc<Mutex<bool>>,
    last_check_time: Arc<Mutex<Instant>>,
    app_handle: AppHandle,
    wake_word_callback: Option<Arc<dyn Fn() + Send + Sync>>,
    
    // Voice Activity Detection state
    vad_state: Arc<Mutex<VadState>>,
    
    // Function to check if main recording is active (to avoid conflicts)
    is_main_recording_active: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct VoiceActivationConfig {
    pub wake_words: Vec<String>,
    pub device_id: String,
    pub sensitivity: f32,
    pub timeout_ms: u64,
    pub energy_threshold: Option<f32>, // Custom threshold, auto-calibrated if None
    pub auto_calibrate: bool, // Whether to auto-calibrate threshold
    pub wake_word_model_size: WhisperModelSize, // Model size for wake word detection
}

#[derive(Debug)]
struct VadState {
    is_speech_active: bool,
    speech_start_time: Option<Instant>,
    last_speech_time: Option<Instant>,
    speech_buffer: VecDeque<f32>, // Buffer to store speech audio for transcription
    energy_history: VecDeque<f32>, // Recent energy levels for smoothing
    
    // Calibration state
    calibration_samples: VecDeque<f32>, // Samples for ambient noise calibration
    current_threshold: f32, // Current dynamic threshold
    ambient_noise_level: f32, // Measured ambient noise level
    is_calibrated: bool, // Whether calibration is complete
}

impl VadState {
    fn new() -> Self {
        Self {
            is_speech_active: false,
            speech_start_time: None,
            last_speech_time: None,
            speech_buffer: VecDeque::new(),
            energy_history: VecDeque::with_capacity(10), // Keep last 10 energy readings
            
            // Initialize calibration state
            calibration_samples: VecDeque::with_capacity(CALIBRATION_SAMPLE_COUNT),
            current_threshold: DEFAULT_VAD_ENERGY_THRESHOLD,
            ambient_noise_level: 0.0,
            is_calibrated: false,
        }
    }
    
    fn new_with_config(config: &VoiceActivationConfig) -> Self {
        let mut state = Self::new();
        
        // If a custom threshold is provided, use it and mark as calibrated
        if let Some(custom_threshold) = config.energy_threshold {
            state.current_threshold = custom_threshold;
            state.is_calibrated = !config.auto_calibrate; // Don't auto-calibrate if manual threshold
            println!("🎛️ Using custom energy threshold: {:.6}", custom_threshold);
        } else if !config.auto_calibrate {
            state.is_calibrated = true; // Use default threshold, no calibration
            println!("🎛️ Using default energy threshold: {:.6}", DEFAULT_VAD_ENERGY_THRESHOLD);
        } else {
            println!("🎛️ Auto-calibration enabled - will determine threshold from ambient noise");
        }
        
        state
    }
    
    fn calibrate_threshold(&mut self, energy: f32) {
        // Add sample for calibration
        self.calibration_samples.push_back(energy);
        
        // Keep only the required number of samples
        if self.calibration_samples.len() > CALIBRATION_SAMPLE_COUNT {
            self.calibration_samples.pop_front();
        }
        
        // Calculate ambient noise level and threshold when we have enough samples
        if self.calibration_samples.len() >= CALIBRATION_SAMPLE_COUNT {
            // Calculate mean and standard deviation of ambient noise
            let mean: f32 = self.calibration_samples.iter().sum::<f32>() / self.calibration_samples.len() as f32;
            let variance: f32 = self.calibration_samples.iter()
                .map(|&x| (x - mean).powi(2))
                .sum::<f32>() / self.calibration_samples.len() as f32;
            let std_dev = variance.sqrt();
            
            // Set ambient noise level and threshold
            self.ambient_noise_level = mean;
            // Use a more aggressive threshold for speech detection
            self.current_threshold = (mean + std_dev).max(mean * AMBIENT_MULTIPLIER).max(0.01);
            self.is_calibrated = true;
            
            println!("🎛️ Audio threshold calibrated: ambient={:.4}, threshold={:.4} (samples: {})", 
                     self.ambient_noise_level, self.current_threshold, self.calibration_samples.len());
        }
    }
    
    fn get_current_threshold(&self) -> f32 {
        if self.is_calibrated {
            self.current_threshold
        } else {
            DEFAULT_VAD_ENERGY_THRESHOLD
        }
    }
}

impl VoiceActivationService {
    pub fn new(
        config: VoiceActivationConfig,
        app_handle: AppHandle,
        external_transcription_service: Option<Arc<TranscriptionService>>, // Check for existing service first
    ) -> Result<Self> {
        println!("🎙️ Creating VoiceActivationService with config: device_id='{}', wake_words={:?}, sensitivity={}", 
                 config.device_id, config.wake_words, config.sensitivity);
        
        let transcription_service = if let Some(existing_service) = external_transcription_service {
            // Check if existing service is compatible (Candle Whisper)
            let is_compatible = existing_service.is_candle_whisper();
            
            if is_compatible {
                println!("✅ Reusing existing Candle Whisper transcription service for wake words");
                Some(existing_service)
            } else {
                println!("⚠️  Existing service is not Candle Whisper, creating dedicated service...");
                Self::create_dedicated_wake_word_service(&config)?
            }
        } else {
            println!("🔧 No existing transcription service found, creating dedicated service for wake words...");
            Self::create_dedicated_wake_word_service(&config)?
        };
        
        let buffer_size = (SAMPLE_RATE as f32 * WAKE_WORD_BUFFER_DURATION_SEC) as usize;
        let mut audio_buffer = VecDeque::with_capacity(buffer_size);
        audio_buffer.resize(buffer_size, 0.0);
        
        println!("📊 Audio buffer initialized: size={}, duration={}s", buffer_size, WAKE_WORD_BUFFER_DURATION_SEC);
        
        Ok(Self {
            config: config.clone(),
            audio_buffer: Arc::new(Mutex::new(audio_buffer)),
            transcription_service,
            is_listening: Arc::new(Mutex::new(false)),
            last_check_time: Arc::new(Mutex::new(Instant::now())),
            app_handle,
            wake_word_callback: None,
            
            // Voice Activity Detection state with configuration
            vad_state: Arc::new(Mutex::new(VadState::new_with_config(&config))),
            
            // Initialize without main recording check function
            is_main_recording_active: None,
        })
    }
    
    fn create_dedicated_wake_word_service(config: &VoiceActivationConfig) -> Result<Option<Arc<TranscriptionService>>> {
        println!("🔧 Creating dedicated transcription service for wake word detection...");
        let wake_word_config = TranscriptionConfig {
            mode: TranscriptionMode::CandleWhisper, // Use local option for wake words
            openai_api_key: None,
            whisper_model_path: None,
            whisper_model_size: config.wake_word_model_size.clone(), // Use configurable model size
            device: DeviceType::Cpu, // Use CPU to avoid conflicts with main transcription
        };
        
        match TranscriptionService::new(wake_word_config) {
            Ok(service) => {
                println!("✅ Dedicated wake word transcription service created successfully");
                Ok(Some(Arc::new(service)))
            }
            Err(e) => {
                eprintln!("❌ Failed to create wake word transcription service: {}", e);
                println!("⚠️  Continuing without transcription service - wake word detection will be disabled");
                Ok(None)
            }
        }
    }
    
    pub async fn start_listening(&self) -> Result<()> {
        // Check and set listening state
        {
            let mut is_listening = self.is_listening.lock().map_err(|e| anyhow::anyhow!("Failed to lock listening state: {}", e))?;
            if *is_listening {
                return Ok(()); // Already listening
            }
            *is_listening = true;
        }
        
        println!("Starting voice activation listening...");
        
        // Start audio capture stream
        self.start_audio_stream()?;
        
        // Start wake word detection loop
        self.start_wake_word_detection_loop();
        
        Ok(())
    }
    
    pub async fn stop_listening(&self) -> Result<()> {
        let mut is_listening = self.is_listening.lock().map_err(|e| anyhow::anyhow!("Failed to lock listening state: {}", e))?;
        *is_listening = false;
        println!("Stopped voice activation listening");
        Ok(())
    }
    
    pub fn is_listening(&self) -> bool {
        self.is_listening.lock().map(|state| *state).unwrap_or(false)
    }
    
    pub fn get_transcription_service(&self) -> &Option<Arc<TranscriptionService>> {
        &self.transcription_service
    }
    
    /// Force immediate wake word check (useful for testing or manual triggering)
    pub async fn force_wake_word_check(&self) -> Result<Option<WakeWordDetection>> {
        if let Some(ref service) = self.transcription_service {
            if let Ok(detection) = Self::check_for_wake_words_with_vad(
                &self.vad_state,
                service,
                &self.config.wake_words,
                self.config.sensitivity,
            ).await {
                Ok(Some(detection))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    
    pub fn set_wake_word_callback<F>(&mut self, callback: F) 
    where 
        F: Fn() + Send + Sync + 'static 
    {
        self.wake_word_callback = Some(Arc::new(callback));
    }
    
    pub fn set_main_recording_check<F>(&mut self, check_fn: F)
    where
        F: Fn() -> bool + Send + Sync + 'static
    {
        self.is_main_recording_active = Some(Arc::new(check_fn));
    }
    
    pub async fn get_activity_info(&self) -> VoiceActivityInfo {
        let is_listening = self.is_listening.lock().map(|state| *state).unwrap_or(false);
        
        if !is_listening {
            return VoiceActivityInfo {
                is_listening: false,
                current_energy: 0.0,
                threshold: DEFAULT_VAD_ENERGY_THRESHOLD,
                is_speech_detected: false,
                speech_duration_ms: 0,
            };
        }
        
        // Get current energy from recent audio samples
        let current_energy = {
            let buffer = self.audio_buffer.lock().unwrap();
            let window_start = buffer.len().saturating_sub(VAD_WINDOW_SIZE);
            let recent_samples: Vec<f32> = buffer.iter().skip(window_start).cloned().collect();
            Self::calculate_audio_energy(&recent_samples)
        };
        
        // Get VAD state information and current threshold
        let (is_speech_detected, speech_duration_ms, current_threshold) = {
            if let Ok(vad) = self.vad_state.lock() {
                let speech_duration = if let Some(start_time) = vad.speech_start_time {
                    Instant::now().duration_since(start_time).as_millis() as u64
                } else {
                    0
                };
                (vad.is_speech_active, speech_duration, vad.get_current_threshold())
            } else {
                (false, 0, DEFAULT_VAD_ENERGY_THRESHOLD)
            }
        };
        
        VoiceActivityInfo {
            is_listening: true,
            current_energy,
            threshold: current_threshold,
            is_speech_detected,
            speech_duration_ms,
        }
    }
    
    fn start_audio_stream(&self) -> Result<()> {
        println!("🎤 Starting audio stream for voice activation...");
        
        let host = cpal::default_host();
        let devices: Vec<_> = host.input_devices()
            .map_err(|e| anyhow::anyhow!("Failed to get input devices: {}", e))?
            .collect();
        
        println!("🔍 Available devices: {}", devices.len());
        for (i, device) in devices.iter().enumerate() {
            let name = device.name().unwrap_or_else(|_| format!("Device {}", i));
            println!("  - Device {}: {}", i, name);
        }
        
        let device_index: usize = match self.config.device_id.parse() {
            Ok(idx) => {
                println!("📱 Parsed device index: {}", idx);
                idx
            }
            Err(e) => {
                eprintln!("❌ Invalid device ID '{}': {}", self.config.device_id, e);
                return Err(anyhow::anyhow!("Invalid device ID '{}': {}", self.config.device_id, e));
            }
        };
        
        let device = devices.get(device_index)
            .ok_or_else(|| {
                eprintln!("❌ Device index {} not found (only {} devices available)", device_index, devices.len());
                anyhow::anyhow!("Device index {} not found (only {} devices available)", device_index, devices.len())
            })?;
        
        let device_name = device.name().unwrap_or_else(|_| format!("Device {}", device_index));
        println!("✅ Using device: {}", device_name);
        
        let config = device.default_input_config()
            .map_err(|e| anyhow::anyhow!("Failed to get default input config: {}", e))?;
        
        println!("🔧 Device config: channels={}, sample_rate={}, format={:?}", 
                 config.channels(), config.sample_rate().0, config.sample_format());
        
        let stream_config = StreamConfig {
            channels: 1, // Mono for wake word detection
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };
        
        let audio_buffer = self.audio_buffer.clone();
        let is_listening = self.is_listening.clone();
        
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                println!("🎵 Building F32 audio stream...");
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if let Ok(is_listening_guard) = is_listening.lock() {
                            if *is_listening_guard {
                                Self::process_audio_data(&audio_buffer, data);
                            }
                        }
                    },
                    |err| eprintln!("❌ Audio stream error: {}", err),
                    None,
                )?
            }
            SampleFormat::I16 => {
                println!("🎵 Building I16 audio stream...");
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if let Ok(is_listening_guard) = is_listening.lock() {
                            if *is_listening_guard {
                                let float_data: Vec<f32> = data.iter()
                                    .map(|&sample| sample as f32 / i16::MAX as f32)
                                    .collect();
                                Self::process_audio_data(&audio_buffer, &float_data);
                            }
                        }
                    },
                    |err| eprintln!("❌ Audio stream error: {}", err),
                    None,
                )?
            }
            SampleFormat::U16 => {
                println!("🎵 Building U16 audio stream...");
                device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        if let Ok(is_listening_guard) = is_listening.lock() {
                            if *is_listening_guard {
                                let float_data: Vec<f32> = data.iter()
                                    .map(|&sample| ((sample as i32) - 32768) as f32 / i16::MAX as f32)
                                    .collect();
                                Self::process_audio_data(&audio_buffer, &float_data);
                            }
                        }
                    },
                    |err| eprintln!("❌ Audio stream error: {}", err),
                    None,
                )?
            }
            _ => {
                eprintln!("❌ Unsupported sample format: {:?}", config.sample_format());
                return Err(anyhow::anyhow!("Unsupported sample format"));
            }
        };
        
        stream.play().map_err(|e| {
            eprintln!("❌ Failed to start audio stream: {}", e);
            anyhow::anyhow!("Failed to start audio stream: {}", e)
        })?;
        
        println!("✅ Audio stream started successfully");
        
        // Store stream in a static variable to keep it alive
        // Note: In a production implementation, we'd want better lifetime management
        std::mem::forget(stream);
        
        Ok(())
    }
    
    fn process_audio_data(audio_buffer: &Arc<Mutex<VecDeque<f32>>>, data: &[f32]) {
        if let Ok(mut buffer) = audio_buffer.lock() {
            for &sample in data {
                if buffer.len() >= buffer.capacity() {
                    buffer.pop_front();
                }
                buffer.push_back(sample);
            }
        }
    }
    
    fn start_wake_word_detection_loop(&self) {
        let audio_buffer = self.audio_buffer.clone();
        let transcription_service = self.transcription_service.clone();
        let is_listening = self.is_listening.clone();
        let last_check_time = self.last_check_time.clone();
        let wake_words = self.config.wake_words.clone();
        let sensitivity = self.config.sensitivity;
        let app_handle = self.app_handle.clone();
        let wake_word_callback = self.wake_word_callback.clone();
        let vad_state = self.vad_state.clone();
        let main_recording_check = self.is_main_recording_active.clone();
        
        tokio::spawn(async move {
            while {
                let should_continue = if let Ok(guard) = is_listening.lock() {
                    *guard
                } else {
                    false
                };
                should_continue
            } {
                let should_check = {
                    let mut last_check = last_check_time.lock().unwrap();
                    let now = Instant::now();
                    if now.duration_since(*last_check).as_millis() >= WAKE_WORD_CHECK_INTERVAL_MS as u128 {
                        *last_check = now;
                        true
                    } else {
                        false
                    }
                };
                
                if should_check {
                    // Check if main recording is active to avoid conflicts
                    let main_recording_active = if let Some(ref check_fn) = main_recording_check {
                        check_fn()
                    } else {
                        false
                    };
                    
                    if main_recording_active {
                        // Skip wake word detection during main recording to avoid conflicts
                        tokio::time::sleep(tokio::time::Duration::from_millis(WAKE_WORD_CHECK_INTERVAL_MS)).await;
                        continue;
                    }
                    
                    // Check for voice activity instead of constant transcription
                    if let Ok(should_transcribe) = Self::check_voice_activity(
                        &audio_buffer,
                        &vad_state,
                    ).await {
                        if should_transcribe {
                            println!("🎙️ Speech detected - transcribing for wake words...");
                            
                            if let Some(ref service) = transcription_service {
                                if let Ok(detection) = Self::check_for_wake_words_with_vad(
                                    &vad_state,
                                    service,
                                    &wake_words,
                                    sensitivity,
                                ).await {
                                    if detection.detected {
                                        println!("🎉 WAKE WORD DETECTED: {} (confidence: {:.2})", detection.word, detection.confidence);
                                        
                                        // Emit wake word detection event
                                        let _ = app_handle.emit("wake-word-detected", &detection);
                                        
                                        // Trigger the callback if set
                                        if let Some(callback) = &wake_word_callback {
                                            callback();
                                        }
                                    }
                                }
                            } else {
                                println!("⚠️  No transcription service available for wake word detection");
                            }
                        }
                    }
                }
                
                tokio::time::sleep(Duration::from_millis(25)).await; // Faster polling for more responsive detection
            }
        });
    }
    
    // Voice Activity Detection based on energy levels
    async fn check_voice_activity(
        audio_buffer: &Arc<Mutex<VecDeque<f32>>>,
        vad_state: &Arc<Mutex<VadState>>,
    ) -> Result<bool> {
        let current_samples = {
            let buffer = audio_buffer.lock().map_err(|e| anyhow::anyhow!("Failed to lock audio buffer: {}", e))?;
            
            // Take the most recent window for energy calculation
            let window_start = buffer.len().saturating_sub(VAD_WINDOW_SIZE);
            buffer.iter().skip(window_start).cloned().collect::<Vec<f32>>()
        };
        
        if current_samples.len() < VAD_WINDOW_SIZE / 4 {
            return Ok(false); // Not enough samples
        }
        
        // Calculate energy level (RMS)
        let energy = Self::calculate_audio_energy(&current_samples);
        
        let mut vad = vad_state.lock().map_err(|e| anyhow::anyhow!("Failed to lock VAD state: {}", e))?;
        
        // Update energy history for smoothing
        vad.energy_history.push_back(energy);
        if vad.energy_history.len() > 10 {
            vad.energy_history.pop_front();
        }
        
        // Calculate smoothed energy
        let avg_energy: f32 = vad.energy_history.iter().sum::<f32>() / vad.energy_history.len() as f32;
        
        // Handle calibration if auto-calibration is enabled
        if !vad.is_calibrated {
            vad.calibrate_threshold(avg_energy);
        }
        
        let now = Instant::now();
        let current_threshold = vad.get_current_threshold();
        let is_speech = avg_energy > current_threshold;
        
        if is_speech && !vad.is_speech_active {
            // Speech started
            println!("🗣️  Speech started (energy: {:.6})", avg_energy);
            vad.is_speech_active = true;
            vad.speech_start_time = Some(now);
            vad.last_speech_time = Some(now);
            vad.speech_buffer.clear();
            
            // Add current audio buffer to speech buffer
            let full_buffer = audio_buffer.lock().unwrap();
            vad.speech_buffer.extend(full_buffer.iter());
        } else if is_speech && vad.is_speech_active {
            // Speech continuing
            vad.last_speech_time = Some(now);
            
            // Add new samples to speech buffer
            vad.speech_buffer.extend(current_samples.iter());
            
            // Limit speech buffer size (max 10 seconds)
            let max_samples = SAMPLE_RATE as usize * 10;
            if vad.speech_buffer.len() > max_samples {
                let excess = vad.speech_buffer.len() - max_samples;
                for _ in 0..excess {
                    vad.speech_buffer.pop_front();
                }
            }
        } else if !is_speech && vad.is_speech_active {
            // Check if silence timeout reached
            if let Some(last_speech) = vad.last_speech_time {
                let silence_duration = now.duration_since(last_speech).as_millis() as u64;
                
                if silence_duration >= VAD_SILENCE_TIMEOUT_MS {
                    // Speech ended - check if it was long enough
                    if let Some(start_time) = vad.speech_start_time {
                        let speech_duration = now.duration_since(start_time).as_millis() as u64;
                        
                        if speech_duration >= VAD_MIN_SPEECH_DURATION_MS {
                            println!("🔚 Speech ended (duration: {}ms) - ready for transcription", speech_duration);
                            vad.is_speech_active = false;
                            return Ok(true); // Signal to transcribe
                        } else {
                            println!("⏭️  Speech too short ({}ms) - ignoring", speech_duration);
                        }
                    }
                    
                    vad.is_speech_active = false;
                    vad.speech_buffer.clear();
                }
            }
        }
        
        Ok(false) // Don't transcribe yet
    }
    
    fn calculate_audio_energy(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        
        // Calculate RMS (Root Mean Square) energy with better scaling for speech detection
        let sum_squares: f32 = samples.iter().map(|&x| x * x).sum();
        let rms = (sum_squares / samples.len() as f32).sqrt();
        
        // Apply logarithmic scaling to better distinguish speech levels
        // This makes the threshold more intuitive for users
        if rms > 0.0 {
            (rms * 10.0).max(0.0001) // Scale up and ensure minimum value
        } else {
            0.0
        }
    }
    
    async fn check_for_wake_words_with_vad(
        vad_state: &Arc<Mutex<VadState>>,
        transcription_service: &Arc<TranscriptionService>,
        wake_words: &[String],
        sensitivity: f32,
    ) -> Result<WakeWordDetection> {
        // Extract speech audio from VAD buffer
        let audio_samples = {
            let vad = vad_state.lock().map_err(|e| anyhow::anyhow!("Failed to lock VAD state: {}", e))?;
            vad.speech_buffer.iter().cloned().collect::<Vec<f32>>()
        };
        
        if audio_samples.is_empty() {
            return Ok(WakeWordDetection {
                detected: false,
                confidence: 0.0,
                word: String::new(),
                timestamp: 0,
            });
        }
        
        let audio_duration = audio_samples.len() as f32 / SAMPLE_RATE as f32;
        println!("🔊 Processing speech audio: {:.2}s ({} samples)", audio_duration, audio_samples.len());
        
        let start_time = std::time::Instant::now();
        
        // Create temporary WAV file from speech buffer
        let temp_file = NamedTempFile::new()?;
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        
        let mut writer = WavWriter::create(temp_file.path(), spec)?;
        for sample in &audio_samples {
            let sample_i16 = (sample * i16::MAX as f32) as i16;
            writer.write_sample(sample_i16)?;
        }
        writer.finalize()?;
        
        // Transcribe the speech audio directly (no Mutex needed)
        let transcription = {
            let temp_file_path = temp_file.path().to_str().unwrap().to_string();
            match transcription_service.transcribe(&temp_file_path).await {
                Ok(text) => {
                    let transcription_time = start_time.elapsed();
                    if !text.trim().is_empty() {
                        println!("🎯 Transcribed speech: '{}' (took {:.2}ms)", text.trim(), transcription_time.as_millis());
                    } else {
                        println!("🔇 Empty transcription (took {:.2}ms)", transcription_time.as_millis());
                    }
                    text
                }
                Err(e) => {
                    println!("❌ Transcription failed: {}", e);
                    return Err(e);
                }
            }
        };
        
        // Check if any wake words are present
        let transcription_lower = transcription.to_lowercase();
        
        for wake_word in wake_words {
            let wake_word_lower = wake_word.to_lowercase();
            
            // Simple substring matching - in production, you'd want more sophisticated matching
            if transcription_lower.contains(&wake_word_lower) {
                println!("🎉 WAKE WORD MATCH! Found '{}' in '{}'", wake_word_lower, transcription_lower);
                
                // Calculate confidence based on how clearly the wake word appears
                let confidence = Self::calculate_confidence(&transcription_lower, &wake_word_lower, sensitivity);
                
                println!("📊 Confidence: {:.2} (threshold: {:.2})", confidence, sensitivity);
                
                if confidence >= sensitivity {
                    println!("✅ Confidence threshold met - wake word detected!");
                    
                    // Clear the speech buffer to avoid re-processing
                    if let Ok(mut vad) = vad_state.lock() {
                        vad.speech_buffer.clear();
                    }
                    
                    return Ok(WakeWordDetection {
                        detected: true,
                        confidence,
                        word: wake_word.clone(),
                        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
                    });
                } else {
                    println!("⚠️  Confidence too low - ignoring detection");
                }
            }
        }
        
        Ok(WakeWordDetection {
            detected: false,
            confidence: 0.0,
            word: String::new(),
            timestamp: 0,
        })
    }
    
    fn calculate_confidence(transcription: &str, wake_word: &str, base_sensitivity: f32) -> f32 {
        // Simple confidence calculation
        // In production, you'd want more sophisticated matching with edit distance, phonetic similarity, etc.
        
        if transcription.trim() == wake_word.trim() {
            return 1.0; // Perfect match
        }
        
        if transcription.contains(wake_word) {
            // Word is present, calculate based on context and clarity
            let word_ratio = wake_word.len() as f32 / transcription.len() as f32;
            return (0.7 + word_ratio * 0.3).min(1.0);
        }
        
        // Check for fuzzy matching (simple Levenshtein-like approach)
        let distance = Self::simple_edit_distance(transcription, wake_word);
        let similarity = 1.0 - (distance as f32 / wake_word.len().max(transcription.len()) as f32);
        
        if similarity >= base_sensitivity {
            return similarity;
        }
        
        0.0
    }
    
    fn simple_edit_distance(s1: &str, s2: &str) -> usize {
        let len1 = s1.chars().count();
        let len2 = s2.chars().count();
        
        if len1 == 0 { return len2; }
        if len2 == 0 { return len1; }
        
        let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];
        
        for i in 0..=len1 { matrix[i][0] = i; }
        for j in 0..=len2 { matrix[0][j] = j; }
        
        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();
        
        for i in 1..=len1 {
            for j in 1..=len2 {
                let cost = if s1_chars[i-1] == s2_chars[j-1] { 0 } else { 1 };
                matrix[i][j] = (matrix[i-1][j] + 1)
                    .min(matrix[i][j-1] + 1)
                    .min(matrix[i-1][j-1] + cost);
            }
        }
        
        matrix[len1][len2]
    }
} 