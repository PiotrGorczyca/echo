use super::nlp::*;
use super::commands::*;
use super::context::*;
use super::integrations::{McpIntegrationManager, UserMcpServer};
use crate::mcp::*;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// AI Agent Core - orchestrates all AI agent functionality
pub struct AiAgentCore {
    mcp_client: Arc<McpClient>,
    command_processor: Arc<CommandProcessor>,
    nlp_processor: Arc<NlpProcessor>,
    context_manager: Arc<ContextManager>,
    conversation_history: Arc<RwLock<Vec<Conversation>>>,
    active_sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    builtin_server: Arc<BuiltInMcpServer>,
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub input: ConversationInput,
    pub output: ConversationOutput,
    pub context: ConversationContext,
    pub metadata: ConversationMetadata,
}

/// Conversation input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationInput {
    pub text: String,
    pub audio_path: Option<String>,
    pub input_type: InputType,
    pub language: Option<String>,
}

/// Conversation output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationOutput {
    pub text: String,
    pub actions: Vec<AgentAction>,
    pub generated_content: Vec<GeneratedContent>,
    pub confidence: f32,
}

/// Generated content (images, videos, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedContent {
    pub id: String,
    pub content_type: ContentType,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Content types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentType {
    Image,
    Video,
    Audio,
    Document,
    Code,
    Diagram,
}

/// Input types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputType {
    Voice,
    Text,
    Mixed,
}

/// Agent session for maintaining context across interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub context: SessionContext,
    pub active_workflow: Option<String>,
    pub conversation_ids: Vec<String>,
}

/// Session context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub user_preferences: HashMap<String, serde_json::Value>,
    pub active_projects: Vec<String>,
    pub recent_actions: Vec<AgentAction>,
    pub working_directory: Option<String>,
}

/// Conversation context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    pub session_id: String,
    pub previous_conversation_id: Option<String>,
    pub extracted_entities: Vec<Entity>,
    pub intent: Intent,
    pub workflow_state: Option<WorkflowState>,
}

/// Conversation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    pub processing_time_ms: u64,
    pub mcp_servers_used: Vec<String>,
    pub tools_called: Vec<String>,
    pub confidence_scores: HashMap<String, f32>,
}

impl AiAgentCore {
    pub async fn new(mcp_client: Arc<McpClient>) -> Result<Self> {
        let command_processor = Arc::new(CommandProcessor::new());
        let nlp_processor = Arc::new(NlpProcessor::new());
        let context_manager = Arc::new(ContextManager::new());
        let builtin_server = Arc::new(BuiltInMcpServer::new());

        Ok(Self {
            mcp_client,
            command_processor,
            nlp_processor,
            context_manager,
            conversation_history: Arc::new(RwLock::new(Vec::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            builtin_server,
        })
    }

    /// Process a voice or text input and generate a response
    pub async fn process_input(
        &self,
        input: ConversationInput,
        session_id: Option<String>,
    ) -> Result<Conversation> {
        let start_time = std::time::Instant::now();
        
        // Get or create session
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let session = self.get_or_create_session(&session_id).await?;

        // Process input through NLP
        let nlp_result = self.nlp_processor.process_text(&input.text).await?;

        // Extract context and intent
        let context = ConversationContext {
            session_id: session_id.clone(),
            previous_conversation_id: session.conversation_ids.last().cloned(),
            extracted_entities: nlp_result.entities,
            intent: nlp_result.intent.clone(),
            workflow_state: None,
        };

        // Process commands and generate actions
        let actions = self.command_processor.process_intent(&nlp_result.intent, &context).await?;

        // Execute actions using MCP tools
        let mut executed_actions = Vec::new();
        let mut generated_content = Vec::new();
        let mut mcp_servers_used = Vec::new();
        let mut tools_called = Vec::new();

        for action in actions {
            match self.execute_action(&action, &context).await {
                Ok(result) => {
                    executed_actions.push(action.clone());
                    if let Some(server) = &action.mcp_server {
                        mcp_servers_used.push(server.clone());
                    }
                    if let Some(tool) = &action.tool_name {
                        tools_called.push(tool.clone());
                    }
                    if let Some(content) = result.generated_content {
                        generated_content.extend(content);
                    }
                }
                Err(e) => {
                    log::error!("Failed to execute action {:?}: {}", action, e);
                    // Continue with other actions
                }
            }
        }

        // Generate response text
        let response_text = self.generate_response_text(&executed_actions, &generated_content).await?;

        // Create conversation record
        let conversation = Conversation {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            input,
            output: ConversationOutput {
                text: response_text,
                actions: executed_actions,
                generated_content,
                confidence: nlp_result.confidence,
            },
            context,
            metadata: ConversationMetadata {
                processing_time_ms: start_time.elapsed().as_millis() as u64,
                mcp_servers_used,
                tools_called,
                confidence_scores: HashMap::new(),
            },
        };

        // Store conversation
        self.store_conversation(&conversation).await?;

        // Update session
        self.update_session_activity(&session_id, &conversation.id).await?;

        Ok(conversation)
    }

    /// Execute an agent action using MCP tools
    async fn execute_action(
        &self,
        action: &AgentAction,
        context: &ConversationContext,
    ) -> Result<ActionResult> {
        match &action.action_type {
            ActionType::McpToolCall => {
                let server_name = action.mcp_server.as_ref()
                    .ok_or_else(|| anyhow!("Missing MCP server for tool call"))?;
                let tool_name = action.tool_name.as_ref()
                    .ok_or_else(|| anyhow!("Missing tool name for MCP call"))?;

                let response = self.mcp_client.call_tool(
                    server_name,
                    tool_name,
                    Some(serde_json::Value::Object(action.parameters.clone().into_iter().collect())),
                ).await?;

                Ok(ActionResult {
                    success: !response.is_error,
                    message: self.extract_text_from_tool_content(&response.content),
                    data: Some(serde_json::to_value(response)?),
                    generated_content: None,
                })
            }
            ActionType::BuiltinToolCall => {
                let tool_name = action.tool_name.as_ref()
                    .ok_or_else(|| anyhow!("Missing tool name for builtin call"))?;

                let response = self.builtin_server.call_tool(tool_name, Some(serde_json::Value::Object(action.parameters.clone().into_iter().collect()))).await?;

                Ok(ActionResult {
                    success: !response.is_error,
                    message: self.extract_text_from_tool_content(&response.content),
                    data: Some(serde_json::to_value(response)?),
                    generated_content: None,
                })
            }
            ActionType::Transcription => {
                // Handle transcription action
                self.handle_transcription_action(action, context).await
            }
            ActionType::ContentGeneration => {
                // Handle content generation (images, videos, etc.)
                self.handle_content_generation_action(action, context).await
            }
            ActionType::WorkflowExecution => {
                // Handle complex workflow execution
                self.handle_workflow_action(action, context).await
            }
        }
    }

    /// Handle transcription action
    async fn handle_transcription_action(
        &self,
        _action: &AgentAction,
        _context: &ConversationContext,
    ) -> Result<ActionResult> {
        // This would integrate with the existing transcription service
        Ok(ActionResult {
            success: true,
            message: "Transcription completed".to_string(),
            data: None,
            generated_content: None,
        })
    }

    /// Handle content generation action
    async fn handle_content_generation_action(
        &self,
        _action: &AgentAction,
        _context: &ConversationContext,
    ) -> Result<ActionResult> {
        // This would integrate with Replicate API for content generation
        Ok(ActionResult {
            success: true,
            message: "Content generation initiated".to_string(),
            data: None,
            generated_content: Some(vec![GeneratedContent {
                id: Uuid::new_v4().to_string(),
                content_type: ContentType::Image,
                url: None,
                local_path: None,
                metadata: HashMap::new(),
            }]),
        })
    }

    /// Handle workflow action
    async fn handle_workflow_action(
        &self,
        _action: &AgentAction,
        _context: &ConversationContext,
    ) -> Result<ActionResult> {
        // This would handle complex multi-step workflows
        Ok(ActionResult {
            success: true,
            message: "Workflow executed".to_string(),
            data: None,
            generated_content: None,
        })
    }

    /// Extract text content from tool response
    fn extract_text_from_tool_content(&self, content: &[ToolContent]) -> String {
        content.iter()
            .filter_map(|c| match c {
                ToolContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Generate response text based on actions and content
    async fn generate_response_text(
        &self,
        actions: &[AgentAction],
        generated_content: &[GeneratedContent],
    ) -> Result<String> {
        if actions.is_empty() {
            return Ok("I understand, but I'm not sure how to help with that right now.".to_string());
        }

        let mut response_parts = Vec::new();

        for action in actions {
            match &action.action_type {
                ActionType::McpToolCall | ActionType::BuiltinToolCall => {
                    response_parts.push(format!("Executed {}", action.description));
                }
                ActionType::Transcription => {
                    response_parts.push("Transcribed the audio".to_string());
                }
                ActionType::ContentGeneration => {
                    response_parts.push("Generated content".to_string());
                }
                ActionType::WorkflowExecution => {
                    response_parts.push("Executed workflow".to_string());
                }
            }
        }

        if !generated_content.is_empty() {
            response_parts.push(format!("Generated {} item(s)", generated_content.len()));
        }

        Ok(response_parts.join(". "))
    }

    /// Get or create a session
    async fn get_or_create_session(&self, session_id: &str) -> Result<AgentSession> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.get(session_id) {
            Ok(session.clone())
        } else {
            let session = AgentSession {
                id: session_id.to_string(),
                created_at: Utc::now(),
                last_activity: Utc::now(),
                context: SessionContext {
                    user_preferences: HashMap::new(),
                    active_projects: Vec::new(),
                    recent_actions: Vec::new(),
                    working_directory: None,
                },
                active_workflow: None,
                conversation_ids: Vec::new(),
            };
            
            sessions.insert(session_id.to_string(), session.clone());
            Ok(session)
        }
    }

    /// Update session activity
    async fn update_session_activity(&self, session_id: &str, conversation_id: &str) -> Result<()> {
        let mut sessions = self.active_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_activity = Utc::now();
            session.conversation_ids.push(conversation_id.to_string());
            
            // Keep only last 50 conversations per session
            if session.conversation_ids.len() > 50 {
                session.conversation_ids.drain(0..session.conversation_ids.len() - 50);
            }
        }
        
        Ok(())
    }

    /// Store conversation in history
    async fn store_conversation(&self, conversation: &Conversation) -> Result<()> {
        let mut history = self.conversation_history.write().await;
        history.push(conversation.clone());
        
        // Keep only last 1000 conversations
        if history.len() > 1000 {
            let excess = history.len() - 1000;
            history.drain(0..excess);
        }
        
        Ok(())
    }

    /// Get conversation history
    pub async fn get_conversation_history(&self, limit: Option<usize>) -> Vec<Conversation> {
        let history = self.conversation_history.read().await;
        let limit = limit.unwrap_or(50);
        
        history.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get active sessions
    pub async fn get_active_sessions(&self) -> Vec<AgentSession> {
        let sessions = self.active_sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Get available MCP tools
    pub async fn get_available_tools(&self) -> HashMap<String, Vec<McpTool>> {
        let mut all_tools = self.mcp_client.get_all_tools().await;
        
        // Add builtin tools
        all_tools.insert("builtin".to_string(), self.builtin_server.get_tools().clone());
        
        all_tools
    }

    /// Process voice command with user-defined MCP servers
    pub async fn process_voice_command(
        &self,
        input: ConversationInput,
        user_servers: &[UserMcpServer],
        session_id: Option<String>,
    ) -> Result<Conversation> {
        let start_time = std::time::Instant::now();
        
        // Get or create session
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let session = self.get_or_create_session(&session_id).await?;

        // Create integration manager
        let integration_manager = McpIntegrationManager::new(
            // We'll need to get registry from somewhere - for now create a minimal one
            crate::mcp::McpServerRegistry::new(std::path::PathBuf::from("/tmp/mcp_servers.json")),
            (*self.mcp_client).clone(),
        );

        // Try to match voice command to MCP tool
        if let Some((server_name, tool_name, parameters)) = integration_manager
            .process_voice_command(&input.text, user_servers)
            .await
        {
            // Execute the matched MCP tool directly
            match integration_manager
                .execute_tool(&server_name, &tool_name, Some(serde_json::Value::Object(
                    parameters.into_iter().collect()
                )))
                .await
            {
                Ok(result) => {
                    // Create a simplified conversation for MCP tool execution
                    let conversation = Conversation {
                        id: Uuid::new_v4().to_string(),
                        timestamp: Utc::now(),
                        input: input.clone(),
                        output: ConversationOutput {
                            text: result.message.clone(),
                            actions: vec![AgentAction {
                                id: Uuid::new_v4().to_string(),
                                action_type: ActionType::McpToolCall,
                                parameters: HashMap::new(),
                                priority: ActionPriority::Medium,
                                conditions: Vec::new(),
                                description: format!("Executed {} on {}", tool_name, server_name),
                                mcp_server: Some(server_name.clone()),
                                tool_name: Some(tool_name.clone()),
                                dependencies: Vec::new(),
                                timeout_seconds: Some(30),
                            }],
                            generated_content: Vec::new(),
                            confidence: 0.9, // High confidence for direct voice command match
                        },
                        context: ConversationContext {
                            session_id: session_id.clone(),
                            previous_conversation_id: session.conversation_ids.last().cloned(),
                            extracted_entities: Vec::new(),
                            intent: Intent {
                                intent_type: IntentType::McpToolCall,
                                confidence: 0.9,
                                parameters: HashMap::new(),
                                context_required: false,
                            },
                            workflow_state: None,
                        },
                        metadata: ConversationMetadata {
                            processing_time_ms: start_time.elapsed().as_millis() as u64,
                            mcp_servers_used: vec![server_name],
                            tools_called: vec![tool_name],
                            confidence_scores: HashMap::from([
                                ("voice_command_match".to_string(), 0.9),
                                ("tool_execution".to_string(), if result.success { 1.0 } else { 0.0 }),
                            ]),
                        },
                    };

                    // Store conversation
                    self.store_conversation(&conversation).await?;
                    
                    // Update session
                    self.update_session_activity(&session_id, &conversation.id).await?;

                    return Ok(conversation);
                }
                Err(e) => {
                    log::error!("Failed to execute MCP tool {}/{}: {}", server_name, tool_name, e);
                    // Fall back to regular NLP processing
                }
            }
        }

        // Fall back to regular AI agent processing if no voice command matched
        self.process_input(input, Some(session_id)).await
    }
}

/// Action result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub generated_content: Option<Vec<GeneratedContent>>,
} 