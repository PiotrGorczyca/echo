use crate::transcription::python_env;

#[tauri::command]
pub async fn check_backend_deps(backend: String) -> Result<bool, String> {
    let packages = python_env::required_packages(&backend);
    if packages.is_empty() {
        return Ok(true); // No Python deps needed (LocalWhisper, OpenAI)
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
