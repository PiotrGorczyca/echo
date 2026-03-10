//! Manages a dedicated Python virtual environment for Echo's transcription backends.
//!
//! Instead of relying on system pip (which may not exist or may be "externally managed"),
//! we create `~/.local/share/echo/python-env/` and install everything there.

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

/// Get the path to Echo's dedicated Python venv.
pub fn venv_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow!("Could not determine data directory"))?;
    Ok(data_dir.join("echo").join("python-env"))
}

/// Get the python executable inside the venv.
pub fn venv_python() -> Result<String> {
    let dir = venv_dir()?;
    Ok(dir.join("bin").join("python").to_string_lossy().to_string())
}

/// Get the pip executable inside the venv.
pub fn venv_pip() -> Result<String> {
    let dir = venv_dir()?;
    Ok(dir.join("bin").join("pip").to_string_lossy().to_string())
}

/// Check if the venv exists and has a working python.
pub fn venv_exists() -> bool {
    match venv_python() {
        Ok(python) => Command::new(&python)
            .args(["--version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Find a working system python3.
fn find_system_python() -> Result<String> {
    for candidate in &["python3", "python"] {
        if Command::new(candidate)
            .args(["--version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(candidate.to_string());
        }
    }
    Err(anyhow!(
        "Python 3 not found. Please install it:\n\
         - Debian/Ubuntu: sudo apt install python3 python3-venv\n\
         - Arch: sudo pacman -S python\n\
         - Fedora: sudo dnf install python3\n\
         - macOS: brew install python"
    ))
}

/// Create the venv if it doesn't exist.
pub async fn ensure_venv() -> Result<()> {
    if venv_exists() {
        return Ok(());
    }

    let dir = venv_dir()?;
    println!("Creating Python virtual environment at {:?}...", dir);

    let system_python = find_system_python()?;

    // python3 -m venv <path>
    let output = tokio::process::Command::new(&system_python)
        .args(["-m", "venv", &dir.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| anyhow!("Failed to create venv: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Common issue: python3-venv not installed on Debian/Ubuntu
        if stderr.contains("ensurepip") || stderr.contains("No module named venv") {
            return Err(anyhow!(
                "Python venv module not available. Install it:\n\
                 - Debian/Ubuntu: sudo apt install python3-venv\n\
                 - Arch: already included with python\n\
                 - Fedora: sudo dnf install python3\n\n\
                 Then restart Echo."
            ));
        }
        return Err(anyhow!("Failed to create Python venv: {}", stderr));
    }

    // Upgrade pip inside the venv
    let pip = venv_pip()?;
    let _ = tokio::process::Command::new(&pip)
        .args(["install", "--upgrade", "pip"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await;

    println!("Python virtual environment created successfully");
    Ok(())
}

/// Check if specific Python packages are importable inside the venv.
pub fn check_packages(packages: &[&str]) -> bool {
    let python = match venv_python() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let import_stmt = packages
        .iter()
        .map(|p| {
            // Handle package names that differ from import names
            match *p {
                "faster-whisper" => "faster_whisper",
                "torchaudio" => "torchaudio",
                _ => p,
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let check = format!("import {}", import_stmt);
    Command::new(&python)
        .args(["-c", &check])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Install packages into the venv. Returns (success, log_output).
pub async fn install_packages(packages: &[&str]) -> Result<String> {
    ensure_venv().await?;

    let pip = venv_pip()?;
    println!("Installing packages: {:?}", packages);

    let output = tokio::process::Command::new(&pip)
        .arg("install")
        .args(packages)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| anyhow!("Failed to run pip: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut log = String::new();
    for line in stdout.lines() {
        println!("[pip] {}", line);
        log.push_str(line);
        log.push('\n');
    }
    for line in stderr.lines() {
        println!("[pip] {}", line);
        log.push_str(line);
        log.push('\n');
    }

    if !output.status.success() {
        return Err(anyhow!("pip install failed:\n{}", log));
    }

    Ok(log)
}

/// Get the packages needed for a given backend.
pub fn required_packages(backend: &str) -> Vec<&'static str> {
    match backend {
        "FasterWhisper" => vec!["faster-whisper"],
        "CandleWhisper" => vec!["torch", "transformers", "torchaudio"],
        _ => vec![],
    }
}
