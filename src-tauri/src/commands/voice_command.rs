use tauri::{AppHandle, State};
use std::sync::{Arc, Mutex};
use crate::state::AppState;
use crate::openai_client::OpenAiClient;

#[tauri::command]
pub async fn update_openai_api_key(
    app_handle: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
    api_key: String,
) -> Result<(), String> {
    println!("🔧 Updating OpenAI API key for voice commands");
    
    let app_state = state.inner().clone();
    let mut state_guard = app_state.lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    
    // Update settings
    state_guard.settings.api_key = api_key.clone();
    
    // Recreate the voice command service with new OpenAI client
    if let Some(_voice_service) = &state_guard.voice_command_service {
        // Get existing service components
        let transcription_service = state_guard.transcription_service.clone();
        let ai_agent = state_guard.ai_agent.clone();
        let mcp_client = state_guard.mcp_client.clone();
        
        // Initialize OpenAI client if API key is available
        let openai_client = if !api_key.is_empty() {
            match OpenAiClient::new(api_key) {
                Ok(client) => {
                    println!("✅ OpenAI client created successfully for voice commands");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    println!("❌ Failed to create OpenAI client: {}", e);
                    return Err(format!("Failed to create OpenAI client: {}", e));
                }
            }
        } else {
            println!("🔄 OpenAI API key cleared for voice commands");
            None
        };
        
        // Create new voice command service
        let new_voice_service = crate::voice_command::VoiceCommandService::new(
            app_handle.clone(),
            transcription_service,
            ai_agent,
            mcp_client,
            openai_client,
        );
        
        // Replace the service in state
        state_guard.voice_command_service = Some(Arc::new(new_voice_service));
        println!("✅ Voice command service updated with new OpenAI client");
    }
    
    // Save settings to file
    crate::settings::save_settings_to_file(&state_guard.settings)
        .map_err(|e| format!("Failed to save settings: {}", e))?;
    
    println!("✅ OpenAI API key updated successfully");
    Ok(())
}

#[tauri::command]
pub async fn get_openai_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let app_state = state.inner().clone();
    let state_guard = app_state.lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    
    let has_api_key = !state_guard.settings.api_key.is_empty();
    
    if let Some(_voice_service) = &state_guard.voice_command_service {
        // Check if voice service has OpenAI client
        // For now, we'll just return whether we have an API key
        Ok(has_api_key)
    } else {
        Ok(false)
    }
}