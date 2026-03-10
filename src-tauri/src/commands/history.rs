use tauri::State;
use crate::state::AppState;
use crate::history::TranscriptionHistoryEntry;
use std::sync::{Arc, Mutex};

#[tauri::command]
pub async fn get_transcription_history(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<TranscriptionHistoryEntry>, String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.get_history()
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn get_history_entry(
    state: State<'_, Arc<Mutex<AppState>>>,
    id: String,
) -> Result<Option<TranscriptionHistoryEntry>, String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.get_entry(&id)
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn delete_history_entry(
    state: State<'_, Arc<Mutex<AppState>>>,
    id: String,
) -> Result<bool, String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.delete_entry(&id)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub async fn toggle_history_pin(
    state: State<'_, Arc<Mutex<AppState>>>,
    id: String,
) -> Result<bool, String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.toggle_pin(&id)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub async fn clear_transcription_history(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.clear_history()
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn search_transcription_history(
    state: State<'_, Arc<Mutex<AppState>>>,
    query: String,
) -> Result<Vec<TranscriptionHistoryEntry>, String> {
    let app_state = state.inner().lock().map_err(|e| e.to_string())?;

    if let Some(history_manager) = &app_state.history_manager {
        history_manager.search(&query)
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn repaste_transcription(
    state: State<'_, Arc<Mutex<AppState>>>,
    id: String,
) -> Result<String, String> {
    use enigo::{Enigo, Keyboard, Settings};

    // Get the text we need to paste, then release the lock
    let text_to_paste = {
        let app_state = state.inner().lock().map_err(|e| e.to_string())?;

        if let Some(history_manager) = &app_state.history_manager {
            if let Some(entry) = history_manager.get_entry(&id)? {
                entry.text.clone()
            } else {
                return Err("Entry not found".to_string());
            }
        } else {
            return Err("History manager not initialized".to_string());
        }
    }; // Lock is released here

    // Wait a bit to let the app lose focus if user clicked from the UI
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Paste the text
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.text(&text_to_paste).map_err(|e| e.to_string())?;

    Ok(text_to_paste)
}
