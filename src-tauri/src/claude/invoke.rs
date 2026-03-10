//! Claude Code invocation
//!
//! Methods to invoke Claude Code from Echo.
//! Supports headless mode with full permissions for hands-off orchestration.

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;
use tokio::io::{AsyncBufReadExt, BufReader};
use std::process::Stdio;
use tauri::Emitter;

use std::process::Command;

/// Invocation method
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum InvocationMethod {
    /// Copy to clipboard, simulate keyboard to open Claude Code
    Clipboard,
    /// Write to a file that user can open
    File { path: PathBuf },
    /// Launch in a new terminal window (CLI)
    Terminal { cwd: Option<PathBuf> },
    /// Run headless (background process) with full permissions
    Headless { cwd: Option<PathBuf> },
    /// Run headless with streaming output to logs
    HeadlessStreaming { cwd: Option<PathBuf> },
}

/// Configuration for headless Claude Code execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessConfig {
    /// Allow all tools without permission prompts
    pub bypass_permissions: bool,
    /// Specific tools to allow (if not bypassing all)
    pub allowed_tools: Vec<String>,
    /// Enable verbose logging
    pub verbose: bool,
    /// Output format (text, json, stream-json)
    pub output_format: String,
    /// System prompt to append (clarifies working context)
    pub system_prompt: Option<String>,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            bypass_permissions: true,
            allowed_tools: vec![
                "Bash".to_string(),
                "Read".to_string(),
                "Write".to_string(),
                "Edit".to_string(),
                "MultiEdit".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "LS".to_string(),
            ],
            verbose: true,
            output_format: "stream-json".to_string(),
            system_prompt: None,
        }
    }
}

impl Default for InvocationMethod {
    fn default() -> Self {
        // Default to Terminal if claude is in PATH, otherwise Clipboard
        if which::which("claude").is_ok() {
            InvocationMethod::Terminal { cwd: None }
        } else {
            InvocationMethod::Clipboard
        }
    }
}

/// Result of invocation attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationResult {
    pub success: bool,
    pub method: InvocationMethod,
    pub message: String,
}

/// Invoke Claude Code with a prompt
pub async fn invoke_claude_code(prompt: &str, method: &InvocationMethod) -> Result<InvocationResult> {
    invoke_claude_code_with_handle(prompt, method, None).await
}

/// Invoke Claude Code with a prompt and optional app handle for event streaming
pub async fn invoke_claude_code_with_handle(
    prompt: &str, 
    method: &InvocationMethod,
    app_handle: Option<tauri::AppHandle>,
) -> Result<InvocationResult> {
    match method {
        InvocationMethod::Clipboard => invoke_via_clipboard(prompt).await,
        InvocationMethod::File { path } => invoke_via_file(prompt, path).await,
        InvocationMethod::Terminal { cwd } => invoke_via_terminal(prompt, cwd.clone()).await,
        InvocationMethod::Headless { cwd } => invoke_via_headless(prompt, cwd.clone()).await,
        InvocationMethod::HeadlessStreaming { cwd } => {
            invoke_via_headless_streaming(prompt, cwd.clone(), app_handle).await
        }
    }
}

/// Headless invocation (Background process) with full permissions
async fn invoke_via_headless(prompt: &str, cwd: Option<PathBuf>) -> Result<InvocationResult> {
    invoke_via_headless_with_config(prompt, cwd, HeadlessConfig::default()).await
}

/// Headless invocation with custom configuration
pub async fn invoke_via_headless_with_config(
    prompt: &str,
    cwd: Option<PathBuf>,
    config: HeadlessConfig,
) -> Result<InvocationResult> {
    use crate::commands::tasks::add_orchestration_log;
    
    let start_dir = cwd.unwrap_or_else(|| dirs::home_dir().unwrap_or(PathBuf::from(".")));
    
    // Write prompt to file for complex prompts
    let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("No config dir"))?.join("echo");
    std::fs::create_dir_all(&config_dir)?;
    let prompt_path = config_dir.join("current_prompt.md");
    std::fs::write(&prompt_path, prompt)?;
    
    // Create log file
    let log_path = config_dir.join("claude_headless.log");
    let log_file = std::fs::File::create(&log_path)?;
    
    add_orchestration_log("info", format!("Starting Claude Code headless in {}", start_dir.display()));
    
    // Build the command with proper headless flags
    // See `claude --help` for available flags
    let mut cmd = Command::new("claude");
    cmd.current_dir(&start_dir);
    
    // Use -p for print/non-interactive mode
    cmd.arg("-p");
    cmd.arg(format!("Read and execute the instructions in {}", prompt_path.display()));
    
    // Permission handling
    if config.bypass_permissions {
        cmd.arg("--dangerously-skip-permissions");
    } else if !config.allowed_tools.is_empty() {
        cmd.arg("--allowedTools");
        cmd.arg(config.allowed_tools.join(","));
    }
    
    // Output format for structured parsing
    cmd.arg("--output-format");
    cmd.arg(&config.output_format);
    
    // Verbose logging
    if config.verbose {
        cmd.arg("--verbose");
    }
    
    // System prompt if provided (to clarify the working context)
    if let Some(system_prompt) = &config.system_prompt {
        cmd.arg("--append-system-prompt");
        cmd.arg(system_prompt);
    }
    
    // Redirect output to log file
    cmd.stdout(log_file.try_clone()?);
    cmd.stderr(log_file);
    
    let child = cmd.spawn();

    match child {
        Ok(child) => {
            let pid = child.id();
            add_orchestration_log("success", format!("Claude Code started (PID: {})", pid));
            
            Ok(InvocationResult {
                success: true,
                method: InvocationMethod::Headless { cwd: Some(start_dir) },
                message: format!(
                    "Started Claude Code headless (PID: {}). Logs at {}",
                    pid,
                    log_path.display()
                ),
            })
        }
        Err(e) => {
            add_orchestration_log("error", format!("Failed to start Claude Code: {}", e));
            Err(anyhow!("Failed to start headless Claude: {}", e))
        }
    }
}

/// Headless invocation with streaming output (spawns async task to stream logs)
async fn invoke_via_headless_streaming(prompt: &str, cwd: Option<PathBuf>, app_handle: Option<tauri::AppHandle>) -> Result<InvocationResult> {
    use crate::commands::tasks::add_orchestration_log;
    
    let start_dir = cwd.clone().unwrap_or_else(|| dirs::home_dir().unwrap_or(PathBuf::from(".")));
    
    // Write prompt to file
    let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("No config dir"))?.join("echo");
    std::fs::create_dir_all(&config_dir)?;
    let prompt_path = config_dir.join("current_prompt.md");
    std::fs::write(&prompt_path, prompt)?;
    
    // Log file for persistence
    let log_path = config_dir.join("claude_headless.log");
    
    add_orchestration_log("info", format!("Starting Claude Code headless (streaming) in {}", start_dir.display()));
    
    // Build command for streaming
    // See `claude --help` for available flags
    let mut cmd = tokio::process::Command::new("claude");
    cmd.current_dir(&start_dir);
    
    // Use -p for non-interactive print mode
    cmd.arg("-p");
    
    // The actual prompt - read instructions from the file
    cmd.arg(format!("Read and execute the instructions in {}", prompt_path.display()));
    
    // Skip all permission prompts for hands-off operation
    cmd.arg("--dangerously-skip-permissions");
    
    // Use stream-json for real-time output parsing
    cmd.arg("--output-format").arg("stream-json");
    
    // Enable verbose logging
    cmd.arg("--verbose");
    
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    
    let mut child = cmd.spawn().map_err(|e| anyhow!("Failed to spawn Claude Code: {}", e))?;
    let pid = child.id().unwrap_or(0);
    
    add_orchestration_log("success", format!("Claude Code started with streaming (PID: {})", pid));
    
    // Take stdout/stderr handles
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let log_path_clone = log_path.clone();
    let app_handle_clone = app_handle.clone();
    
    // Spawn task to stream output
    tokio::spawn(async move {
        let mut log_file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path_clone)
            .await
            .ok();
        
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            
            while let Ok(Some(line)) = lines.next_line().await {
                // Log to file
                if let Some(ref mut file) = log_file {
                    let _ = tokio::io::AsyncWriteExt::write_all(
                        file, 
                        format!("{}\n", line).as_bytes()
                    ).await;
                }
                
                // Parse for session_id and cwd to enable conversation resumption
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(sid) = parsed.get("session_id").and_then(|v| v.as_str()) {
                        crate::commands::tasks::update_claude_context(Some(sid.to_string()), None);
                    }
                    if let Some(cwd) = parsed.get("cwd").and_then(|v| v.as_str()) {
                        crate::commands::tasks::update_claude_context(None, Some(std::path::PathBuf::from(cwd)));
                    }
                }
                
                // Add to orchestration logs
                add_orchestration_log("info", format!("[claude] {}", &line));
                
                // Emit to frontend if app_handle available
                if let Some(ref handle) = app_handle_clone {
                    let _ = handle.emit("claude-output", &line);
                }
            }
        }
        
        // Wait for process to complete
        let status = child.wait().await;
        match status {
            Ok(exit) => {
                if exit.success() {
                    add_orchestration_log("success", "Claude Code completed successfully");
                } else {
                    add_orchestration_log("warn", format!("Claude Code exited with status: {}", exit));
                }
            }
            Err(e) => {
                add_orchestration_log("error", format!("Claude Code process error: {}", e));
            }
        }
        
        // Emit completion event
        if let Some(ref handle) = app_handle_clone {
            let _ = handle.emit("claude-complete", "done");
        }
    });
    
    Ok(InvocationResult {
        success: true,
        method: InvocationMethod::HeadlessStreaming { cwd: Some(start_dir) },
        message: format!("Started Claude Code with streaming output (PID: {})", pid),
    })
}

/// Terminal-based invocation (Claude CLI)
async fn invoke_via_terminal(prompt: &str, cwd: Option<PathBuf>) -> Result<InvocationResult> {
    // 1. Write prompt to a temp file to avoid CLI arg length limits/escaping
    let config_dir = dirs::config_dir().ok_or_else(|| anyhow!("No config dir"))?.join("echo");
    std::fs::create_dir_all(&config_dir)?;
    let prompt_path = config_dir.join("current_prompt.md");
    std::fs::write(&prompt_path, prompt)?;

    // 2. Prepare the command string
    // We navigate to the specific CWD (or home) before running Claude
    // This allows Claude to see the project files immediately
    let start_dir = cwd.unwrap_or_else(|| dirs::home_dir().unwrap_or(PathBuf::from(".")));
    // Escape the directory path for shell
    let start_dir_str = start_dir.to_string_lossy();
    let quoted_start_dir = format!("'{}'", start_dir_str.replace("'", "'\\''"));
    
    // We construct a command that:
    // 1. Changes directory to the project/workspace root
    // 2. Runs Claude with the instructions file
    // 3. Execs bash at the end to keep the terminal open
    let claude_cmd = format!("cd {} && claude \"Read and execute the instructions in {}\"", quoted_start_dir, prompt_path.display());

    // 3. Detect and launch terminal
    let preferred_terminal = {
        crate::settings::load_settings_from_file().terminal_emulator
    };

    #[cfg(target_os = "linux")]
    {
        // Helper to construct args to avoid temporary value dropped while borrowed
        let exec_cmd = format!("{}; exec bash", claude_cmd);
        
        // If user specified a terminal in settings, try that first
        if let Some(term) = preferred_terminal {
             if which::which(&term).is_ok() {
                // Determine args based on known terminals or default to -e
                let args: Vec<&str> = match term.as_str() {
                    "gnome-terminal" => vec!["--", "bash", "-c", &exec_cmd],
                    "konsole" | "xterm" | "alacritty" => vec!["-e", "bash", "-c", &exec_cmd],
                    "kitty" => vec!["bash", "-c", &exec_cmd],
                    "wezterm" => vec!["start", "bash", "-c", &exec_cmd],
                    _ => vec!["-e", "bash", "-c", &exec_cmd], // Generic fallback
                };

                let result = Command::new(&term)
                    .args(&args)
                    .spawn();
                
                if let Ok(_) = result {
                    return Ok(InvocationResult {
                        success: true,
                        method: InvocationMethod::Terminal { cwd: Some(start_dir) },
                        message: format!("Launched Claude Code in {}.", term),
                    });
                }
             }
        }

        let terminals = [
            ("gnome-terminal", vec!["--", "bash", "-c", &exec_cmd]),
            ("konsole", vec!["-e", "bash", "-c", &exec_cmd]),
            ("kitty", vec!["bash", "-c", &exec_cmd]),
            ("alacritty", vec!["-e", "bash", "-c", &exec_cmd]),
            ("xterm", vec!["-e", "bash", "-c", &exec_cmd]),
            ("wezterm", vec!["start", "bash", "-c", &exec_cmd]),
        ];

        for (term, args) in &terminals {
            if which::which(term).is_ok() {
                // Handle gnome-terminal specially if needed, but the args above should work for most
                // For gnome-terminal, "--" separates terminal options from command
                
                let result = Command::new(term)
                    .args(args)
                    .spawn();

                match result {
                    Ok(_) => return Ok(InvocationResult {
                        success: true,
                        method: InvocationMethod::Terminal { cwd: Some(start_dir) },
                        message: format!("Launched Claude Code in {}.", term),
                    }),
                    Err(e) => {
                        eprintln!("Failed to launch {}: {}", term, e);
                        continue;
                    }
                }
            }
        }
        
        return Err(anyhow!("No supported terminal emulator found (tried gnome-terminal, konsole, kitty, alacritty, xterm, wezterm). Please install one or use Clipboard mode."));
    }

    #[cfg(target_os = "macos")]
    {
        // MacOS implementation
        let script = format!(
            "tell application \"Terminal\" to do script \"{}\"",
            claude_cmd.replace("\"", "\\\"")
        );
        
        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn()
            .map_err(|e| anyhow!("Failed to launch Terminal.app: {}", e))?;

        return Ok(InvocationResult {
            success: true,
            method: InvocationMethod::Terminal { cwd: Some(start_dir) },
            message: "Launched Claude Code in Terminal.app.".to_string(),
        });
    }

    #[cfg(target_os = "windows")]
    {
        // Windows implementation (PowerShell/CMD)
        Command::new("cmd")
            .args(["/C", "start", "cmd", "/k", &claude_cmd])
            .spawn()
            .map_err(|e| anyhow!("Failed to launch cmd: {}", e))?;

        return Ok(InvocationResult {
            success: true,
            method: InvocationMethod::Terminal { cwd: Some(start_dir) },
            message: "Launched Claude Code in new Command Prompt.".to_string(),
        });
    }
}

/// Clipboard-based invocation
/// 
/// 1. Copy prompt to clipboard
/// 2. User can then paste into Claude Code (Cmd+L or Cmd+K in Cursor)
///
/// Note: We don't simulate keyboard shortcuts automatically because:
/// - Different OS have different key simulation mechanisms
/// - User might not have Cursor focused
/// - It's more predictable to let user paste when ready
async fn invoke_via_clipboard(prompt: &str) -> Result<InvocationResult> {
    let mut clipboard = Clipboard::new()
        .map_err(|e| anyhow!("Failed to access clipboard: {}", e))?;
    
    clipboard
        .set_text(prompt.to_string())
        .map_err(|e| anyhow!("Failed to copy to clipboard: {}", e))?;

    Ok(InvocationResult {
        success: true,
        method: InvocationMethod::Clipboard,
        message: "Prompt copied to clipboard. Open Claude Code (Cmd+L) and paste (Cmd+V).".to_string(),
    })
}

/// File-based invocation
///
/// Write prompt to a file that user can open in Cursor.
/// Useful for longer prompts or when clipboard isn't available.
async fn invoke_via_file(prompt: &str, path: &PathBuf) -> Result<InvocationResult> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write prompt with markdown extension for nice rendering
    let file_path = if path.extension().is_none() {
        path.with_extension("md")
    } else {
        path.clone()
    };

    std::fs::write(&file_path, prompt)?;

    Ok(InvocationResult {
        success: true,
        method: InvocationMethod::File { path: file_path.clone() },
        message: format!(
            "Prompt written to {}. Open this file and use Claude Code to process it.",
            file_path.display()
        ),
    })
}

/// Invoke Claude Code with keyboard simulation (Linux/X11)
/// 
/// This attempts to:
/// 1. Focus Cursor window
/// 2. Send Ctrl+L to open Claude Code
/// 3. Paste the prompt
///
/// Note: This is experimental and may not work on all systems.
#[cfg(target_os = "linux")]
pub async fn invoke_with_keyboard(prompt: &str) -> Result<InvocationResult> {
    use enigo::{Enigo, Key, Keyboard, Settings};

    // First copy to clipboard
    let mut clipboard = Clipboard::new()
        .map_err(|e| anyhow!("Failed to access clipboard: {}", e))?;
    
    clipboard
        .set_text(prompt.to_string())
        .map_err(|e| anyhow!("Failed to copy to clipboard: {}", e))?;

    // Small delay to ensure clipboard is ready
    sleep(Duration::from_millis(100)).await;

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("Failed to create Enigo: {:?}", e))?;

    // Try to focus Cursor window using wmctrl (if available)
    let wmctrl_result = std::process::Command::new("wmctrl")
        .args(["-a", "Cursor"])
        .output();
        
    if let Err(e) = wmctrl_result {
        println!("wmctrl failed or not installed: {}", e);
        // Fallback to xdotool
        // xdotool search --name "Cursor" windowactivate
        let _ = std::process::Command::new("xdotool")
            .args(["search", "--name", "Cursor", "windowactivate"])
            .output();
    }

    sleep(Duration::from_millis(200)).await;

    // Send Ctrl+L (Cursor uses Ctrl on Linux, not Cmd)
    if let Err(e) = enigo.key(Key::Control, enigo::Direction::Press) {
         println!("Enigo failed to press Control: {:?}", e);
    }
    let _ = enigo.key(Key::Unicode('l'), enigo::Direction::Click);
    let _ = enigo.key(Key::Control, enigo::Direction::Release);

    sleep(Duration::from_millis(300)).await;

    // Paste
    let _ = enigo.key(Key::Control, enigo::Direction::Press);
    let _ = enigo.key(Key::Unicode('v'), enigo::Direction::Click);
    let _ = enigo.key(Key::Control, enigo::Direction::Release);

    sleep(Duration::from_millis(100)).await;

    Ok(InvocationResult {
        success: true,
        method: InvocationMethod::Clipboard,
        message: "Prompt copied to clipboard. Echo attempted to open Cursor and paste. If it didn't work, please switch to Cursor and press Ctrl+V.".to_string(),
    })
}

#[cfg(not(target_os = "linux"))]
pub async fn invoke_with_keyboard(prompt: &str) -> Result<InvocationResult> {
    // For non-Linux, just use clipboard
    invoke_via_clipboard(prompt).await
}

/// Tauri command to invoke Claude Code
#[tauri::command]
pub async fn send_to_claude_code(
    prompt: String,
    method: Option<InvocationMethod>,
) -> Result<InvocationResult, String> {
    let method = method.unwrap_or_default();
    invoke_claude_code(&prompt, &method)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command for keyboard-based invocation
#[tauri::command]
pub async fn send_to_claude_code_keyboard(prompt: String) -> Result<InvocationResult, String> {
    invoke_with_keyboard(&prompt)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command for headless execution with streaming output
#[tauri::command]
pub async fn send_to_claude_code_headless(
    prompt: String,
    cwd: Option<String>,
    app_handle: tauri::AppHandle,
) -> Result<InvocationResult, String> {
    let cwd_path = cwd.map(PathBuf::from);
    let method = InvocationMethod::HeadlessStreaming { cwd: cwd_path };
    invoke_claude_code_with_handle(&prompt, &method, Some(app_handle))
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command to get headless execution logs
#[tauri::command]
pub async fn get_claude_headless_logs() -> Result<String, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "No config dir".to_string())?
        .join("echo");
    let log_path = config_dir.join("claude_headless.log");
    
    if log_path.exists() {
        tokio::fs::read_to_string(&log_path)
            .await
            .map_err(|e| e.to_string())
    } else {
        Ok(String::new())
    }
}

/// Open a task in Cursor IDE
/// 
/// This writes the task context to a file in the repo and opens Cursor.
/// The user can then use Cursor's built-in AI features with the context.
#[tauri::command]
pub async fn open_task_in_cursor(
    repo_path: String,
    task_context: String,
) -> Result<InvocationResult, String> {
    let repo_path = PathBuf::from(&repo_path);
    
    // Create .echo directory if it doesn't exist
    let echo_dir = repo_path.join(".echo");
    if !echo_dir.exists() {
        std::fs::create_dir_all(&echo_dir)
            .map_err(|e| format!("Failed to create .echo directory: {}", e))?;
    }
    
    // Write task context to a file that Cursor can use
    let context_file = echo_dir.join("CURRENT_TASK.md");
    std::fs::write(&context_file, &task_context)
        .map_err(|e| format!("Failed to write task context: {}", e))?;
    
    // Open the repository in Cursor
    let cursor_result = Command::new("cursor")
        .arg(&repo_path)
        .spawn();
    
    match cursor_result {
        Ok(_) => {
            // Also try to open the task file directly after a short delay
            // This gives Cursor time to start
            let context_file_clone = context_file.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let _ = Command::new("cursor")
                    .arg(&context_file_clone)
                    .spawn();
            });
            
            Ok(InvocationResult {
                success: true,
                method: InvocationMethod::File { path: context_file },
                message: format!(
                    "Opened {} in Cursor. Task context saved to .echo/CURRENT_TASK.md",
                    repo_path.display()
                ),
            })
        }
        Err(e) => {
            Err(format!(
                "Failed to open Cursor. Make sure 'cursor' command is in your PATH. Error: {}",
                e
            ))
        }
    }
}



