//! File watcher for automated Cursor handoff
//!
//! Watches for `CURSOR_INSTRUCTIONS.md` in the active workspace and
//! automatically pastes the content into Cursor's Agent chat.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use notify::{Watcher, RecursiveMode, Result as NotifyResult, Event};
use tauri::{AppHandle, Emitter};
use crate::claude::invoke::invoke_with_keyboard;

pub struct CursorHandoffWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    current_watch_path: Option<PathBuf>,
    app_handle: AppHandle,
    last_event_time: Arc<Mutex<Instant>>,
}

impl CursorHandoffWatcher {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            watcher: None,
            current_watch_path: None,
            app_handle,
            last_event_time: Arc::new(Mutex::new(Instant::now().checked_sub(Duration::from_secs(10)).unwrap())),
        }
    }

    pub fn start_watching(&mut self, path: &Path) -> NotifyResult<()> {
        if let Some(current) = &self.current_watch_path {
            if current == path {
                return Ok(());
            }
        }

        let app_handle = self.app_handle.clone();
        let last_event_time = self.last_event_time.clone();
        let watch_path = path.to_path_buf();

        let event_handler = move |res: NotifyResult<Event>| {
            match res {
                Ok(event) => {
                    // Check if it's the target file
                    let is_target = event.paths.iter().any(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s == "CURSOR_INSTRUCTIONS.md")
                            .unwrap_or(false)
                    });

                    if is_target && event.kind.is_modify() {
                        // Debounce
                        let mut last = last_event_time.lock().unwrap();
                        if last.elapsed() < Duration::from_secs(2) {
                            return;
                        }
                        *last = Instant::now();
                        drop(last);

                        println!("👀 Detected change in CURSOR_INSTRUCTIONS.md");
                        
                        // Read and process
                        let file_path = event.paths.iter().find(|p| p.ends_with("CURSOR_INSTRUCTIONS.md")).unwrap();
                        if let Ok(content) = std::fs::read_to_string(file_path) {
                            if !content.trim().is_empty() {
                                println!("🚀 Handing off to Cursor...");
                                let app_handle_clone = app_handle.clone();
                                let content_clone = content.clone();
                                tauri::async_runtime::spawn(async move {
                                    // Small delay to ensure file write is fully flush
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                    
                                    // Use keyboard invocation to paste into Cursor
                                    if let Err(e) = invoke_with_keyboard(&content_clone).await {
                                        println!("❌ Failed to handoff to Cursor: {}", e);
                                    } else {
                                        println!("✅ Successfully pasted instructions to Cursor");
                                        let _ = app_handle_clone.emit("orchestration-log", serde_json::json!({
                                            "level": "success",
                                            "message": "Auto-pasted instructions to Cursor Agent"
                                        }));
                                    }
                                });
                            }
                        }
                    }
                },
                Err(e) => println!("Watch error: {:?}", e),
            }
        };

        let mut watcher = notify::recommended_watcher(event_handler)?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;

        println!("Started watching for CURSOR_INSTRUCTIONS.md in {}", path.display());
        self.watcher = Some(watcher);
        self.current_watch_path = Some(path.to_path_buf());

        Ok(())
    }
}

