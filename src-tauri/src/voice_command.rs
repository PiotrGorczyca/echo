use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use crate::transcription::TranscriptionService;
use crate::ai_agent::AiAgentCore;
use crate::mcp::McpClient;
use crate::openai_client::{OpenAiClient, IntentAnalysis, McpTool};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandMessage {
    pub id: String,
    pub timestamp: u64,
    pub r#type: String, // 'user' | 'system' | 'result' | 'error'
    pub content: String,
    pub metadata: Option<VoiceCommandMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandMetadata {
    pub transcription: Option<String>,
    pub intent: Option<String>,
    pub tool: Option<String>,
    pub server: Option<String>,
    pub confidence: Option<f32>,
    pub processing_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandState {
    pub is_recording: bool,
    pub is_processing: bool,
    pub current_state: String, // 'idle' | 'recording' | 'transcribing' | 'processing' | 'executing'
    pub recording_start_time: Option<u64>,
}

pub struct VoiceCommandService {
    transcription_service: Option<Arc<TranscriptionService>>,
    ai_agent: Option<Arc<AiAgentCore>>,
    mcp_client: Option<Arc<McpClient>>,
    openai_client: Option<Arc<OpenAiClient>>,
    state: Arc<Mutex<VoiceCommandState>>,
    app_handle: AppHandle,
    messages: Arc<Mutex<Vec<VoiceCommandMessage>>>,
}

impl VoiceCommandService {
    pub fn new(
        app_handle: AppHandle,
        transcription_service: Option<Arc<TranscriptionService>>,
        ai_agent: Option<Arc<AiAgentCore>>,
        mcp_client: Option<Arc<McpClient>>,
        openai_client: Option<Arc<OpenAiClient>>,
    ) -> Self {
        let initial_state = VoiceCommandState {
            is_recording: false,
            is_processing: false,
            current_state: "idle".to_string(),
            recording_start_time: None,
        };

        Self {
            transcription_service,
            ai_agent,
            mcp_client,
            openai_client,
            state: Arc::new(Mutex::new(initial_state)),
            app_handle,
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_state(&self) -> Result<VoiceCommandState, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|e| format!("Failed to get voice command state: {}", e))
    }

    pub fn update_state(&self, new_state: &str) -> Result<(), String> {
        let mut state = self.state
            .lock()
            .map_err(|e| format!("Failed to lock voice command state: {}", e))?;

        state.current_state = new_state.to_string();

        match new_state {
            "recording" => {
                state.is_recording = true;
                state.recording_start_time = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                );
            }
            "transcribing" | "processing" | "executing" => {
                state.is_recording = false;
                state.is_processing = true;
            }
            "idle" => {
                state.is_recording = false;
                state.is_processing = false;
                state.recording_start_time = None;
            }
            _ => {}
        }

        // Emit state change event
        let _ = self.app_handle.emit("voice-command-state", &*state);

        println!("Voice command state updated to: {}", new_state);
        Ok(())
    }

    pub fn add_message(&self, message_type: &str, content: &str, metadata: Option<VoiceCommandMetadata>) -> Result<(), String> {
        let message = VoiceCommandMessage {
            id: format!("{}{}", 
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                rand::random::<u16>()
            ),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            r#type: message_type.to_string(),
            content: content.to_string(),
            metadata,
        };

        {
            let mut messages = self.messages
                .lock()
                .map_err(|e| format!("Failed to lock messages: {}", e))?;
            messages.push(message.clone());
        }

        // Emit new message event
        let _ = self.app_handle.emit("voice-command-event", &message);

        Ok(())
    }

    pub fn get_messages(&self) -> Result<Vec<VoiceCommandMessage>, String> {
        self.messages
            .lock()
            .map(|messages| messages.clone())
            .map_err(|e| format!("Failed to get messages: {}", e))
    }

    pub fn clear_messages(&self) -> Result<(), String> {
        let mut messages = self.messages
            .lock()
            .map_err(|e| format!("Failed to lock messages: {}", e))?;
        messages.clear();
        Ok(())
    }

    pub fn set_openai_client(&mut self, api_key: Option<String>) -> Result<(), String> {
        if let Some(key) = api_key {
            match OpenAiClient::new(key) {
                Ok(client) => {
                    self.openai_client = Some(Arc::new(client));
                    println!("✅ OpenAI client initialized for voice commands");
                    Ok(())
                }
                Err(e) => {
                    println!("❌ Failed to initialize OpenAI client: {}", e);
                    Err(format!("Failed to initialize OpenAI client: {}", e))
                }
            }
        } else {
            self.openai_client = None;
            println!("🔄 OpenAI client disabled for voice commands");
            Ok(())
        }
    }

    pub fn has_openai_client(&self) -> bool {
        self.openai_client.is_some()
    }

    pub fn update_overlay_status(&self, message: &str, status_type: &str) -> Result<(), String> {
        // Call the show_overlay_status function from the main lib
        crate::show_overlay_status(&self.app_handle, message, status_type);
        Ok(())
    }

    pub async fn process_text_command(&self, command_text: &str) -> Result<(), String> {
        let start_time = std::time::Instant::now();
        
        // Add user message
        self.add_message("user", command_text, None)?;
        
        // Update state to processing
        self.update_state("processing")?;
        self.update_overlay_status("🧠 Analyzing command intent...", "processing")?;

        // Process with OpenAI if available, otherwise fall back to AI agent
        match self.process_with_openai(command_text).await {
            Ok(analysis) => {
                let processing_time = start_time.elapsed().as_millis() as u64;
                
                if let Some(tool_name) = &analysis.tool_name {
                    // Execute the tool
                    self.update_state("executing")?;
                    self.update_overlay_status(&format!("⚡ Executing: {}", tool_name), "executing")?;
                    
                    match self.execute_tool(&analysis).await {
                        Ok(result) => {
                            self.add_message("result", &result, Some(VoiceCommandMetadata {
                                transcription: Some(command_text.to_string()),
                                intent: Some(analysis.reasoning.clone()),
                                tool: analysis.tool_name.clone(),
                                server: analysis.server_name.clone(),
                                confidence: Some(analysis.confidence),
                                processing_time: Some(processing_time),
                            }))?;
                        }
                        Err(error) => {
                            self.add_message("error", &format!("❌ Tool execution failed: {}", error), Some(VoiceCommandMetadata {
                                transcription: Some(command_text.to_string()),
                                intent: Some(analysis.reasoning.clone()),
                                tool: analysis.tool_name.clone(),
                                server: analysis.server_name.clone(),
                                confidence: Some(analysis.confidence),
                                processing_time: Some(processing_time),
                            }))?;
                        }
                    }
                } else {
                    // No tool found - check if this needs direct OpenAI response
                    if analysis.reasoning.to_lowercase().contains("informational") || 
                       analysis.reasoning.to_lowercase().contains("direct llm") ||
                       analysis.reasoning.to_lowercase().contains("direct response") {
                        
                        self.update_overlay_status("🤔 Getting direct AI response...", "processing")?;
                        
                        // Try to answer directly with OpenAI
                        match self.answer_question_directly(command_text).await {
                            Ok(answer) => {
                                self.add_message("result", &format!("🤖 {}", answer), Some(VoiceCommandMetadata {
                                    transcription: Some(command_text.to_string()),
                                    intent: Some("direct_llm_response".to_string()),
                                    tool: None,
                                    server: Some("openai_direct".to_string()),
                                    confidence: Some(analysis.confidence),
                                    processing_time: Some(processing_time),
                                }))?;
                            }
                            Err(direct_error) => {
                                // Fallback to reasoning if direct answer fails
                                self.add_message("result", &format!("💭 {} (Note: Unable to provide direct answer: {})", analysis.reasoning, direct_error), Some(VoiceCommandMetadata {
                                    transcription: Some(command_text.to_string()),
                                    intent: Some(analysis.reasoning.clone()),
                                    tool: None,
                                    server: None,
                                    confidence: Some(analysis.confidence),
                                    processing_time: Some(processing_time),
                                }))?;
                            }
                        }
                    } else {
                        // Regular case - no tool found, provide explanation
                        self.add_message("result", &format!("💭 {}", analysis.reasoning), Some(VoiceCommandMetadata {
                            transcription: Some(command_text.to_string()),
                            intent: Some(analysis.reasoning.clone()),
                            tool: None,
                            server: None,
                            confidence: Some(analysis.confidence),
                            processing_time: Some(processing_time),
                        }))?;
                    }
                }
            }
            Err(error) => {
                // Fall back to AI agent
                let fallback_msg = if error.contains("rate_limit_exceeded") {
                    "⚠️ OpenAI rate limit exceeded. Using local AI agent..."
                } else if error.contains("Too Many Requests") {
                    "⚠️ OpenAI quota exceeded. Using local AI agent..."
                } else {
                    "🔄 OpenAI unavailable, using local AI agent..."
                };
                self.update_overlay_status(fallback_msg, "processing")?;
                match self.process_with_ai_agent(command_text).await {
                    Ok(result) => {
                        let processing_time = start_time.elapsed().as_millis() as u64;
                        self.add_message("result", &result, Some(VoiceCommandMetadata {
                            transcription: Some(command_text.to_string()),
                            intent: Some("ai_agent_fallback".to_string()),
                            tool: None,
                            server: None,
                            confidence: Some(0.7),
                            processing_time: Some(processing_time),
                        }))?;
                    }
                    Err(fallback_error) => {
                        self.add_message("error", &format!("❌ Processing failed: {} (OpenAI: {})", fallback_error, error), None)?;
                    }
                }
            }
        }

        // Reset state to idle
        self.update_state("idle")?;
        self.update_overlay_status("Command completed", "success")?;
        
        // Hide overlay after a short delay
        let app_handle = self.app_handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
            crate::hide_overlay_status(&app_handle);
        });

        Ok(())
    }

    async fn process_with_openai(&self, command_text: &str) -> Result<IntentAnalysis, String> {
        let openai_client = self.openai_client.as_ref()
            .ok_or("OpenAI client not available")?;
        
        // Get available MCP tools
        let available_tools = self.get_available_mcp_tools().await?;
        
        // Analyze intent with OpenAI
        openai_client.analyze_intent(command_text, &available_tools)
            .await
            .map_err(|e| format!("OpenAI analysis failed: {}", e))
    }

    async fn process_with_ai_agent(&self, command_text: &str) -> Result<String, String> {
        let ai_agent = self.ai_agent.as_ref()
            .ok_or("AI agent not available")?;
        
        // Create a ConversationInput from the command text
        let input = crate::ai_agent::ConversationInput {
            text: command_text.to_string(),
            audio_path: None,
            input_type: crate::ai_agent::InputType::Text,
            language: None,
        };
        
        // For now, pass empty user servers - this could be enhanced later
        let user_servers = vec![];
        
        match ai_agent.process_voice_command(input, &user_servers, None).await {
            Ok(conversation) => {
                // Extract the output text from the conversation
                Ok(conversation.output.text)
            }
            Err(e) => Err(format!("AI agent processing failed: {}", e))
        }
    }

    async fn get_available_mcp_tools(&self) -> Result<Vec<McpTool>, String> {
        let mcp_client = self.mcp_client.as_ref()
            .ok_or("MCP client not available")?;
        
        // Get available tools from MCP servers
        let tools_by_server = mcp_client.get_all_tools().await;
        
       
        
        // Convert to our McpTool format
        let mut converted_tools = Vec::new();
        for (server_name, tools) in tools_by_server {
            
            for tool in tools {
                println!("     - {}: {}", tool.name, tool.description);
                
                // Parse JSON schema to extract parameter information
                let parameters = self.parse_tool_schema(&tool.input_schema);
                
                converted_tools.push(McpTool {
                    name: tool.name.clone(),
                    server: server_name.clone(),
                    description: tool.description.clone(),
                    parameters,
                });
            }
        }
        
        println!("   Total converted tools for OpenAI: {}", converted_tools.len());
        
        if converted_tools.is_empty() {
            println!("⚠️ No MCP tools available! Check:");
            println!("   1. Are MCP servers configured in settings?");
            println!("   2. Are servers connected successfully?");
            println!("   3. Do servers provide any tools?");
            
            // Get server connection status for debugging
            let connected_servers = mcp_client.get_connected_servers().await;
            println!("   Connected servers list: {:?}", connected_servers);
        }
        
        Ok(converted_tools)
    }

    fn parse_tool_schema(&self, schema: &serde_json::Value) -> std::collections::HashMap<String, serde_json::Value> {
        let mut parameters = std::collections::HashMap::new();
        
        // Handle standard JSON Schema format
        if let Some(schema_obj) = schema.as_object() {
            // Get properties
            if let Some(properties) = schema_obj.get("properties").and_then(|p| p.as_object()) {
                for (param_name, param_def) in properties {
                    // Extract parameter info (type, description, etc.)
                    let mut param_info = serde_json::Map::new();
                    
                    if let Some(param_obj) = param_def.as_object() {
                        if let Some(param_type) = param_obj.get("type") {
                            param_info.insert("type".to_string(), param_type.clone());
                        }
                        if let Some(description) = param_obj.get("description") {
                            param_info.insert("description".to_string(), description.clone());
                        }
                        if let Some(required_array) = schema_obj.get("required").and_then(|r| r.as_array()) {
                            let is_required = required_array.iter().any(|r| r.as_str() == Some(param_name));
                            param_info.insert("required".to_string(), serde_json::Value::Bool(is_required));
                        }
                    }
                    
                    parameters.insert(param_name.clone(), serde_json::Value::Object(param_info));
                }
            }
        }
        
        parameters
    }

    async fn answer_question_directly(&self, question: &str) -> Result<String, String> {
        let openai_client = self.openai_client.as_ref()
            .ok_or("OpenAI client not available")?;
        
        openai_client.answer_question_directly(question)
            .await
            .map_err(|e| format!("Direct answer failed: {}", e))
    }

    async fn execute_tool(&self, analysis: &IntentAnalysis) -> Result<String, String> {
        let mcp_client = self.mcp_client.as_ref()
            .ok_or("MCP client not available")?;
        
        let tool_name = analysis.tool_name.as_ref()
            .ok_or("No tool specified")?;
        let server_name = analysis.server_name.as_ref()
            .ok_or("No server specified")?;
        
        // Convert parameters to serde_json::Value
        let arguments = if analysis.parameters.is_empty() {
            None
        } else {
            Some(serde_json::to_value(&analysis.parameters)
                .map_err(|e| format!("Failed to convert parameters: {}", e))?)
        };
        
        // Execute the tool with extracted parameters
        match mcp_client.call_tool(server_name, tool_name, arguments).await {
            Ok(result) => {
                // Format the result nicely
                let formatted_result = if result.content.is_empty() {
                    "Tool executed successfully".to_string()
                } else {
                    // Process the first content item
                    match &result.content[0] {
                        crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
                        crate::mcp::protocol::ToolContent::Image { .. } => "Image result (not displayed)".to_string(),
                        crate::mcp::protocol::ToolContent::Resource { .. } => "Resource result".to_string(),
                    }
                };
                Ok(format!("✅ {}", formatted_result))
            }
            Err(e) => Err(format!("Tool execution failed: {}", e))
        }
    }

    pub async fn start_voice_command(&self) -> Result<(), String> {
        // Update state to recording
        self.update_state("recording")?;
        
        // Add system message
        self.add_message("system", "🎤 Recording voice command...", None)?;

        // Start recording would go here - for now simulate
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        
        // Simulate transcription
        self.update_state("transcribing")?;
        self.add_message("system", "📝 Transcribing audio...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Simulate processing
        self.update_state("processing")?;
        self.add_message("system", "🧠 Processing command...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Simulate execution
        self.update_state("executing")?;
        self.add_message("system", "⚡ Executing command...", None)?;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Add result
        self.add_message("result", "Voice command test completed successfully", Some(VoiceCommandMetadata {
            transcription: Some("test voice command".to_string()),
            intent: Some("system_test".to_string()),
            tool: None,
            server: None,
            confidence: Some(0.98),
            processing_time: Some(5000),
        }))?;

        // Reset to idle
        self.update_state("idle")?;

        Ok(())
    }

    pub async fn start_voice_recording(&self) -> Result<(), String> {
        println!("🎤 Starting voice command recording...");
        
        // Update state to recording
        self.update_state("recording")?;
        
        // Add system message
        self.add_message("system", "🎤 Starting voice recording...", None)?;

        // Use the app handle to trigger the actual recording system
        // This integrates with the existing recording infrastructure
        let _ = self.app_handle.emit("voice-command-start-recording", ());
        
        Ok(())
    }

    pub async fn stop_voice_recording_and_process(&self) -> Result<(), String> {
        // Update state to transcribing  
        self.update_state("transcribing")?;
        self.add_message("system", "📝 Transcribing audio...", None)?;

        // Emit event to stop recording and get the audio file
        let _ = self.app_handle.emit("stop-voice-command-recording", ());

        // The actual transcription will be handled by the event listener
        // and will call process_transcription when ready
        
        Ok(())
    }

    pub async fn process_transcription(&self, transcription_text: &str) -> Result<(), String> {
        // Add detailed logging for transcription
        println!("🔍 Voice Command Transcription Result:");
        println!("   Raw text: '{}'", transcription_text);
        println!("   Length: {} characters", transcription_text.len());
        println!("   Words: {} words", transcription_text.split_whitespace().count());
        
        // Log transcription details but don't add to UI messages
        // Users can see the transcription in the overlay status and final result metadata

        // Use the new OpenAI integration via process_text_command
        self.process_text_command(transcription_text).await?;

        Ok(())
    }

    pub async fn handle_recording_error(&self, error: &str) -> Result<(), String> {
        self.update_state("idle")?;
        self.add_message("error", &format!("Recording failed: {}", error), None)?;
        Ok(())
    }
    
    pub async fn handle_transcription_from_system(&self, transcription_text: &str, _audio_path: &str) -> Result<(), String> {
        println!("🔄 Voice command service received transcription from system: '{}'", transcription_text);
        
        // Update state to transcribing first
        self.update_state("transcribing")?;
        
        // Process the transcription
        self.process_transcription(transcription_text).await?;
        
        Ok(())
    }

    pub async fn handle_recording_cancelled(&self) -> Result<(), String> {
        println!("🚫 Voice command recording cancelled");
        
        // Update state to idle
        self.update_state("idle")?;
        
        // Add cancellation message
        self.add_message("system", "🚫 Recording cancelled", Some(VoiceCommandMetadata {
            transcription: None,
            intent: Some("recording_cancelled".to_string()),
            tool: None,
            server: None,
            confidence: None,
            processing_time: None,
        }))?;

        // Update overlay to show cancellation
        self.update_overlay_status("Recording cancelled", "error")?;
        
        // Hide overlay after a short delay
        let app_handle = self.app_handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
            crate::hide_overlay_status(&app_handle);
        });

        Ok(())
    }
} 