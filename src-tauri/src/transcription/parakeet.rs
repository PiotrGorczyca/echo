use anyhow::{anyhow, Result};
use async_trait::async_trait;
use parakeet_rs::{ParakeetTDT, Transcriber};
use std::sync::Mutex;

use super::TranscriptionBackend;

pub struct ParakeetBackend {
    model: Mutex<ParakeetTDT>,
}

impl ParakeetBackend {
    pub fn new(model_dir: String) -> Result<Self> {
        let path = std::path::Path::new(&model_dir);
        if !path.exists() {
            return Err(anyhow!(
                "Parakeet model directory does not exist: {}. \
                 Please download the model first from Settings.",
                model_dir
            ));
        }

        // Verify required files exist
        let has_encoder = path.join("encoder-model.onnx").exists()
            || path.join("encoder.onnx").exists()
            || path.join("encoder-model.int8.onnx").exists();
        let has_decoder = path.join("decoder_joint-model.onnx").exists()
            || path.join("decoder_joint.onnx").exists()
            || path.join("decoder_joint-model.int8.onnx").exists();
        let has_vocab = path.join("vocab.txt").exists();

        if !has_encoder || !has_decoder || !has_vocab {
            return Err(anyhow!(
                "Parakeet model directory is missing required files (encoder, decoder, vocab.txt). \
                 Please re-download the model from Settings."
            ));
        }

        println!("[parakeet] Loading model from: {}", model_dir);
        let model = ParakeetTDT::from_pretrained(&model_dir, None)
            .map_err(|e| anyhow!("Failed to load Parakeet TDT model: {}", e))?;
        println!("[parakeet] Model loaded successfully");

        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

#[async_trait]
impl TranscriptionBackend for ParakeetBackend {
    async fn transcribe(&self, audio_file_path: &str) -> Result<String> {
        println!("[parakeet] Transcribing: {}", audio_file_path);

        let audio_path = audio_file_path.to_string();

        // Read WAV and resample to 16kHz mono
        let reader = hound::WavReader::open(&audio_path)
            .map_err(|e| anyhow!("Failed to open WAV file: {}", e))?;
        let spec = reader.spec();
        let channels = spec.channels;
        let sample_rate = spec.sample_rate;

        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                let max_val = (1u32 << (bits - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .filter_map(Result::ok)
                    .map(move |s| s as f32 / max_val)
                    .collect()
            }
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .filter_map(Result::ok)
                .collect(),
        };

        // Downmix to mono if stereo
        let mono_samples: Vec<f32> = if channels > 1 {
            samples
                .chunks(channels as usize)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            samples
        };

        // Resample to 16kHz if needed
        let samples_16k = if sample_rate != 16000 {
            resample_audio(&mono_samples, sample_rate, 16000)
        } else {
            mono_samples
        };

        let mut model = self
            .model
            .lock()
            .map_err(|e| anyhow!("Failed to lock Parakeet model: {}", e))?;

        let result = model
            .transcribe_samples(samples_16k, 16000, 1, None)
            .map_err(|e| anyhow!("Parakeet transcription failed: {}", e))?;

        println!("[parakeet] Transcription completed successfully");
        Ok(result.text.trim().to_string())
    }

    async fn warm_up(&self) -> Result<()> {
        println!("[parakeet] Model already loaded (warm-up is a no-op)");
        Ok(())
    }
}

/// Linear interpolation resampling (same approach as whisper_local)
fn resample_audio(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = (src_idx - idx as f64) as f32;

        if idx + 1 < samples.len() {
            let sample = samples[idx] * (1.0 - frac) + samples[idx + 1] * frac;
            resampled.push(sample);
        } else if idx < samples.len() {
            resampled.push(samples[idx]);
        }
    }

    resampled
}
