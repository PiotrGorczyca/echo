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
    if let Some(main_window) = app_handle.get_webview_window("main") {
        // Always use the primary monitor for consistent positioning
        if let Ok(monitors) = main_window.available_monitors() {
            let primary_monitor = monitors.iter().find(|m| {
                // Try to find primary monitor by checking if it's at position (0,0) or has "primary" in name
                let pos = m.position();
                pos.x == 0 && pos.y == 0
            }).or_else(|| monitors.first()).ok_or("No monitors available")?;
            
            let monitor_position = primary_monitor.position();
            let monitor_size = primary_monitor.size();
            let screen_width = monitor_size.width as i32;
            let screen_height = monitor_size.height as i32;
            
            // Set window size: width of 500px, full height minus a small margin for taskbar
            let window_width = 500;
            let window_height = screen_height - 40; // Leave space for taskbar
            
            // Position on the right side of the primary monitor
            let x_position = monitor_position.x + screen_width - window_width;
            let y_position = monitor_position.y;
            
            // Set size and position atomically to prevent flicker
            main_window.set_size(Size::Physical(tauri::PhysicalSize { 
                width: window_width as u32, 
                height: window_height as u32 
            })).map_err(|e| format!("Failed to set window size: {}", e))?;
            
            main_window.set_position(Position::Physical(tauri::PhysicalPosition { 
                x: x_position, 
                y: y_position 
            })).map_err(|e| format!("Failed to set window position: {}", e))?;
            
            println!("Main window positioned: {}x{} at ({}, {}) on primary monitor", 
                     window_width, window_height, x_position, y_position);
        } else {
            return Err("Failed to get available monitors".to_string());
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
            
            // Simple and reliable approach: just set focus multiple times
            let _ = window.set_focus();
            
            // Small delay then focus again - this helps with stubborn window managers
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = window_clone.set_focus();
            });
            
            println!("Window was already visible, focused without flashing");
        } else {
            // Window is hidden, show it and position it
            let _ = window.show();
            
            // Position the window after showing with a small delay
            let app_clone = app_handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                tauri::async_runtime::block_on(async {
                    let _ = position_main_window(app_clone).await;
                });
            });
            
            let _ = window.set_focus();
            println!("Window was hidden, showed and positioned");
        }
    }
    Ok(())
} 