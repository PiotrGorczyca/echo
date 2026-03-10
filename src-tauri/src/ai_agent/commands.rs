use super::nlp::*;
use crate::ai_agent::ConversationContext;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Command processor for converting intents into executable actions
pub struct CommandProcessor {
    action_templates: HashMap<IntentType, Vec<ActionTemplate>>,
}

/// Agent action to be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub id: String,
    pub action_type: ActionType,
    pub parameters: HashMap<String, serde_json::Value>,
    pub priority: ActionPriority,
    pub conditions: Vec<String>,
    pub description: String,
    pub mcp_server: Option<String>,
    pub tool_name: Option<String>,
    pub dependencies: Vec<String>,
    pub timeout_seconds: Option<u64>,
}

/// Action types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    McpToolCall,
    BuiltinToolCall,
    Transcription,
    ContentGeneration,
    WorkflowExecution,
}

/// Action priority
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Action template for generating actions from intents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTemplate {
    pub name: String,
    pub action_type: ActionType,
    pub mcp_server: Option<String>,
    pub tool_name: Option<String>,
    pub parameter_mapping: HashMap<String, String>,
    pub conditions: Vec<ActionCondition>,
    pub priority: ActionPriority,
}

/// Conditions for action execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCondition {
    pub condition_type: ConditionType,
    pub parameter: String,
    pub value: serde_json::Value,
    pub operator: ConditionOperator,
}

/// Condition types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionType {
    EntityPresent,
    EntityValue,
    ContextValue,
    Confidence,
}

/// Condition operators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Contains,
    NotContains,
}

/// Workflow state for multi-step processes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub workflow_id: String,
    pub current_step: usize,
    pub total_steps: usize,
    pub step_data: HashMap<String, serde_json::Value>,
    pub completed_actions: Vec<String>,
    pub pending_actions: Vec<String>,
}

impl CommandProcessor {
    pub fn new() -> Self {
        let mut processor = Self {
            action_templates: HashMap::new(),
        };
        
        processor.initialize_action_templates();
        processor
    }

    /// Process an intent and generate executable actions
    pub async fn process_intent(
        &self,
        intent: &Intent,
        context: &ConversationContext,
    ) -> Result<Vec<AgentAction>> {
        let mut actions = Vec::new();
        
        // Get templates for this intent type
        if let Some(templates) = self.action_templates.get(&intent.intent_type) {
            for template in templates {
                if self.check_conditions(template, intent, context).await? {
                    let action = self.create_action_from_template(template, intent, context).await?;
                    actions.push(action);
                }
            }
        }
        
        // If no specific templates matched, try to create a generic action
        if actions.is_empty() {
            if let Some(generic_action) = self.create_generic_action(intent, context).await? {
                actions.push(generic_action);
            }
        }
        
        // Sort actions by priority
        actions.sort_by(|a, b| self.compare_priority(&a.priority, &b.priority));
        
        Ok(actions)
    }

    /// Check if template conditions are met
    async fn check_conditions(
        &self,
        template: &ActionTemplate,
        intent: &Intent,
        context: &ConversationContext,
    ) -> Result<bool> {
        for condition in &template.conditions {
            if !self.evaluate_condition(condition, intent, context).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Evaluate a single condition
    async fn evaluate_condition(
        &self,
        condition: &ActionCondition,
        intent: &Intent,
        context: &ConversationContext,
    ) -> Result<bool> {
        match &condition.condition_type {
            ConditionType::EntityPresent => {
                let entity_type = condition.parameter.as_str();
                Ok(context.extracted_entities.iter().any(|e| 
                    format!("{:?}", e.entity_type).to_lowercase() == entity_type.to_lowercase()
                ))
            }
            ConditionType::EntityValue => {
                let entity_type = condition.parameter.as_str();
                if let Some(entity) = context.extracted_entities.iter().find(|e| 
                    format!("{:?}", e.entity_type).to_lowercase() == entity_type.to_lowercase()
                ) {
                    self.compare_values(&entity.value, &condition.value, &condition.operator)
                } else {
                    Ok(false)
                }
            }
            ConditionType::ContextValue => {
                // Check context values (would need to implement context access)
                Ok(true) // Placeholder
            }
            ConditionType::Confidence => {
                let confidence_value = serde_json::Value::Number(
                    serde_json::Number::from_f64(intent.confidence as f64)
                        .ok_or_else(|| anyhow!("Invalid confidence value"))?
                );
                self.compare_values(&confidence_value.to_string(), &condition.value, &condition.operator)
            }
        }
    }

    /// Compare values using the specified operator
    fn compare_values(
        &self,
        actual: &str,
        expected: &serde_json::Value,
        operator: &ConditionOperator,
    ) -> Result<bool> {
        let expected_str = match expected {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => expected.to_string(),
        };

        match operator {
            ConditionOperator::Equals => Ok(actual == expected_str),
            ConditionOperator::NotEquals => Ok(actual != expected_str),
            ConditionOperator::Contains => Ok(actual.contains(&expected_str)),
            ConditionOperator::NotContains => Ok(!actual.contains(&expected_str)),
            ConditionOperator::GreaterThan => {
                let actual_num: f64 = actual.parse().unwrap_or(0.0);
                let expected_num: f64 = expected_str.parse().unwrap_or(0.0);
                Ok(actual_num > expected_num)
            }
            ConditionOperator::LessThan => {
                let actual_num: f64 = actual.parse().unwrap_or(0.0);
                let expected_num: f64 = expected_str.parse().unwrap_or(0.0);
                Ok(actual_num < expected_num)
            }
        }
    }

    /// Create an action from a template
    async fn create_action_from_template(
        &self,
        template: &ActionTemplate,
        intent: &Intent,
        context: &ConversationContext,
    ) -> Result<AgentAction> {
        let mut parameters = HashMap::new();
        
        // Map intent parameters to action parameters
        for (intent_param, action_param) in &template.parameter_mapping {
            if let Some(value) = intent.parameters.get(intent_param) {
                parameters.insert(action_param.clone(), value.clone());
            }
        }
        
        // Add entity values as parameters
        for entity in &context.extracted_entities {
            let entity_key = format!("{:?}", entity.entity_type).to_lowercase();
            parameters.insert(entity_key, serde_json::Value::String(entity.value.clone()));
        }
        
        let action = AgentAction {
            id: uuid::Uuid::new_v4().to_string(),
            action_type: template.action_type.clone(),
            description: template.name.clone(),
            mcp_server: template.mcp_server.clone(),
            tool_name: template.tool_name.clone(),
            parameters: parameters,
            priority: template.priority.clone(),
            conditions: Vec::new(),
            dependencies: Vec::new(),
            timeout_seconds: Some(30),
        };
        
        Ok(action)
    }

    /// Create a generic action for unmatched intents
    async fn create_generic_action(
        &self,
        intent: &Intent,
        context: &ConversationContext,
    ) -> Result<Option<AgentAction>> {
        match &intent.intent_type {
            IntentType::Question => {
                // Create a generic question-answering action
                Some(AgentAction {
                    id: uuid::Uuid::new_v4().to_string(),
                    action_type: ActionType::McpToolCall,
                    description: "Answer question".to_string(),
                    mcp_server: Some("web-search".to_string()),
                    tool_name: Some("search".to_string()),
                    parameters: [
                        ("query".to_string(), intent.parameters.get("query").unwrap_or(&serde_json::Value::String("".to_string())).clone()),
                    ].into_iter().collect(),
                    priority: ActionPriority::Medium,
                    conditions: Vec::new(),
                    dependencies: Vec::new(),
                    timeout_seconds: Some(30),
                })
            }
            IntentType::Unknown => {
                // For unknown intents, create a clarification action
                Some(AgentAction {
                    id: uuid::Uuid::new_v4().to_string(),
                    action_type: ActionType::BuiltinToolCall,
                    description: "Request clarification".to_string(),
                    mcp_server: None,
                    tool_name: Some("request_clarification".to_string()),
                    parameters: [
                        ("original_text".to_string(), serde_json::Value::String(context.extracted_entities.get(0).map(|e| &e.value).unwrap_or(&"".to_string()).clone())),
                    ].into_iter().collect(),
                    priority: ActionPriority::Low,
                    conditions: Vec::new(),
                    dependencies: Vec::new(),
                    timeout_seconds: Some(10),
                })
            }
            _ => None,
        }.map(Ok).transpose()
    }

    /// Compare action priorities
    fn compare_priority(&self, a: &ActionPriority, b: &ActionPriority) -> std::cmp::Ordering {
        let a_val = match a {
            ActionPriority::Critical => 4,
            ActionPriority::High => 3,
            ActionPriority::Medium => 2,
            ActionPriority::Low => 1,
        };
        let b_val = match b {
            ActionPriority::Critical => 4,
            ActionPriority::High => 3,
            ActionPriority::Medium => 2,
            ActionPriority::Low => 1,
        };
        b_val.cmp(&a_val) // Higher priority first
    }

    /// Initialize action templates
    fn initialize_action_templates(&mut self) {
        // Recording actions
        self.action_templates.insert(IntentType::StartRecording, vec![
            ActionTemplate {
                name: "Start Audio Recording".to_string(),
                action_type: ActionType::BuiltinToolCall,
                mcp_server: None,
                tool_name: Some("record_audio".to_string()),
                parameter_mapping: [
                    ("duration".to_string(), "duration".to_string()),
                ].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::High,
            },
        ]);

        self.action_templates.insert(IntentType::StopRecording, vec![
            ActionTemplate {
                name: "Stop Audio Recording".to_string(),
                action_type: ActionType::BuiltinToolCall,
                mcp_server: None,
                tool_name: Some("record_audio".to_string()),
                parameter_mapping: [].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::High,
            },
        ]);

        // ClickUp actions
        self.action_templates.insert(IntentType::CreateTask, vec![
            ActionTemplate {
                name: "Create ClickUp Task".to_string(),
                action_type: ActionType::McpToolCall,
                mcp_server: Some("clickup".to_string()),
                tool_name: Some("create_task".to_string()),
                parameter_mapping: [
                    ("task_name".to_string(), "name".to_string()),
                    ("project".to_string(), "list_id".to_string()),
                    ("priority".to_string(), "priority".to_string()),
                ].into_iter().collect(),
                conditions: vec![
                    ActionCondition {
                        condition_type: ConditionType::EntityPresent,
                        parameter: "taskname".to_string(),
                        value: serde_json::Value::Bool(true),
                        operator: ConditionOperator::Equals,
                    },
                ],
                priority: ActionPriority::High,
            },
        ]);

        self.action_templates.insert(IntentType::ListTasks, vec![
            ActionTemplate {
                name: "List ClickUp Tasks".to_string(),
                action_type: ActionType::McpToolCall,
                mcp_server: Some("clickup".to_string()),
                tool_name: Some("get_tasks".to_string()),
                parameter_mapping: [
                    ("project".to_string(), "list_id".to_string()),
                ].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::Medium,
            },
        ]);

        // Content generation actions
        self.action_templates.insert(IntentType::GenerateImage, vec![
            ActionTemplate {
                name: "Generate Image with Replicate".to_string(),
                action_type: ActionType::McpToolCall,
                mcp_server: Some("replicate".to_string()),
                tool_name: Some("generate_image".to_string()),
                parameter_mapping: [
                    ("prompt".to_string(), "prompt".to_string()),
                    ("style".to_string(), "style".to_string()),
                ].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::Medium,
            },
        ]);

        self.action_templates.insert(IntentType::GenerateVideo, vec![
            ActionTemplate {
                name: "Generate Video with Replicate".to_string(),
                action_type: ActionType::McpToolCall,
                mcp_server: Some("replicate".to_string()),
                tool_name: Some("generate_video".to_string()),
                parameter_mapping: [
                    ("prompt".to_string(), "prompt".to_string()),
                    ("duration".to_string(), "duration".to_string()),
                ].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::Medium,
            },
        ]);

        // File operations
        self.action_templates.insert(IntentType::OpenFile, vec![
            ActionTemplate {
                name: "Open File".to_string(),
                action_type: ActionType::McpToolCall,
                mcp_server: Some("filesystem".to_string()),
                tool_name: Some("read_file".to_string()),
                parameter_mapping: [
                    ("filename".to_string(), "path".to_string()),
                ].into_iter().collect(),
                conditions: vec![
                    ActionCondition {
                        condition_type: ConditionType::EntityPresent,
                        parameter: "filename".to_string(),
                        value: serde_json::Value::Bool(true),
                        operator: ConditionOperator::Equals,
                    },
                ],
                priority: ActionPriority::Medium,
            },
        ]);

        // Transcription actions
        self.action_templates.insert(IntentType::TranscribeAudio, vec![
            ActionTemplate {
                name: "Transcribe Audio".to_string(),
                action_type: ActionType::BuiltinToolCall,
                mcp_server: None,
                tool_name: Some("transcribe_audio".to_string()),
                parameter_mapping: [
                    ("audio_path".to_string(), "audio_path".to_string()),
                    ("language".to_string(), "language".to_string()),
                ].into_iter().collect(),
                conditions: vec![],
                priority: ActionPriority::High,
            },
        ]);
    }
}

impl Default for CommandProcessor {
    fn default() -> Self {
        Self::new()
    }
} 