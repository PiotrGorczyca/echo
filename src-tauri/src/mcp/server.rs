use super::protocol::*;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

/// Built-in MCP Server for Echo-specific functionality
pub struct BuiltInMcpServer {
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    prompts: Vec<McpPrompt>,
    capabilities: ServerCapabilities,
}

impl BuiltInMcpServer {
    pub fn new() -> Self {
        let tools = vec![
            McpTool {
                name: "transcribe_audio".to_string(),
                description: "Transcribe audio using the configured transcription service".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "audio_path": {
                            "type": "string",
                            "description": "Path to the audio file to transcribe"
                        },
                        "language": {
                            "type": "string",
                            "description": "Language code for transcription (optional)"
                        }
                    },
                    "required": ["audio_path"]
                }),
            },
            McpTool {
                name: "record_audio".to_string(),
                description: "Start or stop audio recording".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["start", "stop"],
                            "description": "Whether to start or stop recording"
                        },
                        "duration": {
                            "type": "number",
                            "description": "Duration in seconds (for start action)"
                        }
                    },
                    "required": ["action"]
                }),
            },
            McpTool {
                name: "get_voice_settings".to_string(),
                description: "Get current voice activation and transcription settings".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            McpTool {
                name: "update_voice_settings".to_string(),
                description: "Update voice activation and transcription settings".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "enable_voice_activation": {
                            "type": "boolean",
                            "description": "Enable or disable voice activation"
                        },
                        "wake_words": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "List of wake words"
                        },
                        "sensitivity": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1,
                            "description": "Voice activation sensitivity"
                        }
                    }
                }),
            },
        ];

        let resources = vec![
            McpResource {
                uri: "echo://conversations".to_string(),
                name: "Conversation History".to_string(),
                description: "Access to conversation history and transcripts".to_string(),
                mime_type: "application/json".to_string(),
            },
            McpResource {
                uri: "echo://settings".to_string(),
                name: "Application Settings".to_string(),
                description: "Current application settings and configuration".to_string(),
                mime_type: "application/json".to_string(),
            },
        ];

        let prompts = vec![
            McpPrompt {
                name: "transcription_summary".to_string(),
                description: "Generate a summary of transcribed audio".to_string(),
                arguments: Some(vec![
                    PromptArgument {
                        name: "transcript".to_string(),
                        description: "The transcribed text to summarize".to_string(),
                        required: true,
                    },
                    PromptArgument {
                        name: "style".to_string(),
                        description: "Summary style (brief, detailed, bullet_points)".to_string(),
                        required: false,
                    },
                ]),
            },
            McpPrompt {
                name: "voice_command_parser".to_string(),
                description: "Parse voice commands into structured actions".to_string(),
                arguments: Some(vec![
                    PromptArgument {
                        name: "command".to_string(),
                        description: "The voice command to parse".to_string(),
                        required: true,
                    },
                ]),
            },
        ];

        let capabilities = ServerCapabilities {
            experimental: None,
            logging: Some(LoggingCapability {}),
            prompts: Some(PromptsCapability {
                list_changed: false,
            }),
            resources: Some(ResourcesCapability {
                subscribe: false,
                list_changed: false,
            }),
            tools: Some(ToolsCapability {
                list_changed: false,
            }),
        };

        Self {
            tools,
            resources,
            prompts,
            capabilities,
        }
    }

    pub fn get_capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    pub fn get_tools(&self) -> &Vec<McpTool> {
        &self.tools
    }

    pub fn get_resources(&self) -> &Vec<McpResource> {
        &self.resources
    }

    pub fn get_prompts(&self) -> &Vec<McpPrompt> {
        &self.prompts
    }

    pub async fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<CallToolResponse> {
        match name {
            "transcribe_audio" => self.handle_transcribe_audio(arguments).await,
            "record_audio" => self.handle_record_audio(arguments).await,
            "get_voice_settings" => self.handle_get_voice_settings(arguments).await,
            "update_voice_settings" => self.handle_update_voice_settings(arguments).await,
            _ => Err(anyhow!("Unknown tool: {}", name)),
        }
    }

    async fn handle_transcribe_audio(&self, arguments: Option<Value>) -> Result<CallToolResponse> {
        let args = arguments.ok_or_else(|| anyhow!("Missing arguments"))?;
        let audio_path = args.get("audio_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing audio_path"))?;

        // This would integrate with the existing transcription service
        // For now, return a placeholder response
        Ok(CallToolResponse {
            content: vec![ToolContent::Text {
                text: format!("Transcription request for: {}", audio_path),
            }],
            is_error: false,
        })
    }

    async fn handle_record_audio(&self, arguments: Option<Value>) -> Result<CallToolResponse> {
        let args = arguments.ok_or_else(|| anyhow!("Missing arguments"))?;
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing action"))?;

        match action {
            "start" => {
                let duration = args.get("duration")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(30.0);

                Ok(CallToolResponse {
                    content: vec![ToolContent::Text {
                        text: format!("Started recording for {} seconds", duration),
                    }],
                    is_error: false,
                })
            }
            "stop" => {
                Ok(CallToolResponse {
                    content: vec![ToolContent::Text {
                        text: "Stopped recording".to_string(),
                    }],
                    is_error: false,
                })
            }
            _ => Err(anyhow!("Invalid action: {}", action)),
        }
    }

    async fn handle_get_voice_settings(&self, _arguments: Option<Value>) -> Result<CallToolResponse> {
        // This would integrate with the existing settings system
        let settings = serde_json::json!({
            "enable_voice_activation": true,
            "wake_words": ["hey echo", "echo"],
            "sensitivity": 0.5,
            "transcription_mode": "CandleWhisper"
        });

        Ok(CallToolResponse {
            content: vec![ToolContent::Text {
                text: serde_json::to_string_pretty(&settings)?,
            }],
            is_error: false,
        })
    }

    async fn handle_update_voice_settings(&self, arguments: Option<Value>) -> Result<CallToolResponse> {
        let args = arguments.ok_or_else(|| anyhow!("Missing arguments"))?;
        
        // This would integrate with the existing settings system
        // For now, just acknowledge the update
        Ok(CallToolResponse {
            content: vec![ToolContent::Text {
                text: format!("Updated voice settings: {}", serde_json::to_string_pretty(&args)?),
            }],
            is_error: false,
        })
    }

    pub async fn get_resource(&self, uri: &str) -> Result<Value> {
        match uri {
            "echo://conversations" => {
                // Return conversation history
                Ok(serde_json::json!({
                    "conversations": [],
                    "total_count": 0
                }))
            }
            "echo://settings" => {
                // Return current settings
                Ok(serde_json::json!({
                    "voice_activation": true,
                    "transcription_mode": "CandleWhisper",
                    "api_key_set": false
                }))
            }
            _ => Err(anyhow!("Unknown resource: {}", uri)),
        }
    }

    pub async fn get_prompt(&self, name: &str, arguments: Option<HashMap<String, Value>>) -> Result<String> {
        match name {
            "transcription_summary" => {
                let transcript = arguments
                    .as_ref()
                    .and_then(|args| args.get("transcript"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing transcript argument"))?;

                let style = arguments
                    .as_ref()
                    .and_then(|args| args.get("style"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("brief");

                Ok(format!(
                    "Please provide a {} summary of the following transcript:\n\n{}",
                    style, transcript
                ))
            }
            "voice_command_parser" => {
                let command = arguments
                    .as_ref()
                    .and_then(|args| args.get("command"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing command argument"))?;

                Ok(format!(
                    "Parse the following voice command into a structured action:\n\n\"{}\"\n\nReturn a JSON object with the action type, parameters, and confidence level.",
                    command
                ))
            }
            _ => Err(anyhow!("Unknown prompt: {}", name)),
        }
    }
}

impl Default for BuiltInMcpServer {
    fn default() -> Self {
        Self::new()
    }
} 