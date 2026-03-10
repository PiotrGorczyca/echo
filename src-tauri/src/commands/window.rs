use tauri::{AppHandle, Window, Size, Position, Manager};

#[tauri::command]
pub async fn hide_window(window: Window) -> Result<(), String> {
    window.hide().map_err(|e| format!("Failed to hide window: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn hide_main_window(app_handle: AppHandle) -> Result<(), String> {
    if let Some(main_window) = app_handle.get_webview_window("main") {
        main_window.hide().map_err(|e| format!("Failed to hide main window: {}", e))?;
        println!("Main window hidden");
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_overlay_window(app_handle: AppHandle) -> Result<(), String> {
    if let Some(overlay_window) = app_handle.get_webview_window("overlay") {
        overlay_window.hide().map_err(|e| format!("Failed to hide overlay window: {}", e))?;
        println!("Overlay window hidden");
    }
    Ok(())
}

#[tauri::command]
pub async fn position_main_window(app_handle: AppHandle) -> Result<(), String> {
    // Standard window mode - let the OS/User handle positioning
    // We can still ensure it's on screen if needed, but for now we'll leave it alone
    // to respect the user's last position or default center.
    if let Some(main_window) = app_handle.get_webview_window("main") {
        if let Err(e) = main_window.set_focus() {
            println!("Failed to focus main window: {}", e);
        }
    }
    Ok(())
}

pub fn show_and_focus_main_window(app_handle: &AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window("main") {
        // Check if window is already visible
        let is_visible = window.is_visible().unwrap_or(false);
        
        if is_visible {
            // Window is already visible, bring it to front without flashing
            let _ = window.unminimize(); // In case it's minimized
            
            // Use multiple methods to ensure window comes to foreground
            let _ = window.set_focus();
            let _ = window.set_always_on_top(true);  // Temporarily set on top
            
            // Request user attention as a fallback
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
            
            // Small delay then remove always-on-top and focus again
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = window_clone.set_always_on_top(false);  // Remove always-on-top
                let _ = window_clone.set_focus();
                
                // Try one more time after another small delay
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = window_clone.set_focus();
            });
            
            println!("Window was already visible, focused without flashing");
        } else {
            // Window is hidden, show it and position it
            let _ = window.show();
            let _ = window.unminimize(); // In case it's minimized
            
            // Position the window after showing with a small delay
            let app_clone = app_handle.clone();
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                tauri::async_runtime::block_on(async {
                    let _ = position_main_window(app_clone).await;
                });
                
                // Focus after positioning
                let _ = window_clone.set_focus();
                let _ = window_clone.set_always_on_top(true);  // Temporarily set on top
                
                // Small delay then remove always-on-top and focus again
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = window_clone.set_always_on_top(false);
                let _ = window_clone.set_focus();
            });
            
            // Initial focus attempt
            let _ = window.set_focus();
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
            println!("Window was hidden, showed and positioned");
        }
    }
    Ok(())
} 