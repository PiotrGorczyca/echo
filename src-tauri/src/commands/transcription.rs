use crate::transcription::{python_env, WhisperModelSize, TranscriptionService};
use tauri::AppHandle;

#[tauri::command]
pub async fn check_backend_deps(backend: String) -> Result<bool, String> {
    let packages = python_env::required_packages(&backend);
    if packages.is_empty() {
        return Ok(true); // No Python deps needed (LocalWhisper, OpenAI, Parakeet)
    }

    if !python_env::venv_exists() {
        return Ok(false);
    }

    let pkg_refs: Vec<&str> = packages.iter().copied().collect();
    Ok(python_env::check_packages(&pkg_refs))
}

#[tauri::command]
pub async fn install_backend_deps(backend: String) -> Result<String, String> {
    let packages = python_env::required_packages(&backend);
    if packages.is_empty() {
        return Ok("No dependencies needed for this backend.".to_string());
    }

    let pkg_refs: Vec<&str> = packages.iter().copied().collect();
    python_env::install_packages(&pkg_refs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_model_downloaded(model_size: WhisperModelSize) -> Result<bool, String> {
    if model_size.is_parakeet_model() {
        // Check if the parakeet model directory has all required files
        let model_dir = dirs::data_dir()
            .ok_or_else(|| "Could not find data directory".to_string())?
            .join("echo")
            .join("models")
            .join(model_size.model_filename());

        if !model_dir.exists() {
            return Ok(false);
        }

        let files = model_size.parakeet_download_files();
        for (_, local_name) in &files {
            let file_path = model_dir.join(local_name);
            if !file_path.exists() {
                return Ok(false);
            }
            if let Ok(meta) = std::fs::metadata(&file_path) {
                if meta.len() == 0 {
                    return Ok(false);
                }
            }
        }
        return Ok(true);
    }

    // For GGML whisper models, check if the single file exists
    if model_size.requires_python_backend() {
        // Python backends handle their own model downloads
        return Ok(true);
    }

    let model_path = dirs::data_dir()
        .ok_or_else(|| "Could not find data directory".to_string())?
        .join("echo")
        .join("models")
        .join(model_size.model_filename());

    Ok(model_path.exists() && std::fs::metadata(&model_path).map(|m| m.len() > 0).unwrap_or(false))
}

#[tauri::command]
pub async fn download_model(model_size: WhisperModelSize, app_handle: AppHandle) -> Result<String, String> {
    TranscriptionService::download_model_with_progress(&model_size, app_handle)
        .await
        .map_err(|e| e.to_string())
}
