use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};

/// Context manager for maintaining conversation state and context
pub struct ContextManager {
    contexts: Arc<RwLock<HashMap<String, SessionContext>>>,
    global_context: Arc<RwLock<GlobalContext>>,
    cleanup_interval: Duration,
}

/// Session-specific context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub conversation_count: usize,
    pub user_preferences: UserPreferences,
    pub active_workflows: Vec<ActiveWorkflow>,
    pub context_variables: HashMap<String, ContextVariable>,
    pub recent_entities: Vec<RecentEntity>,
    pub interaction_history: Vec<InteractionSummary>,
}

/// Global application context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalContext {
    pub application_state: ApplicationState,
    pub user_profile: UserProfile,
    pub system_capabilities: SystemCapabilities,
    pub integration_status: IntegrationStatus,
    pub performance_metrics: PerformanceMetrics,
}

/// User preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub language: String,
    pub timezone: String,
    pub preferred_response_style: ResponseStyle,
    pub voice_settings: VoiceSettings,
    pub notification_preferences: NotificationPreferences,
    pub privacy_settings: PrivacySettings,
}

/// Response style preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseStyle {
    Concise,
    Detailed,
    Conversational,
    Technical,
}

/// Voice settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub wake_words: Vec<String>,
    pub sensitivity: f32,
    pub auto_transcribe: bool,
    pub preferred_voice_speed: f32,
}

/// Notification preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub enable_desktop_notifications: bool,
    pub enable_sound_notifications: bool,
    pub notification_level: NotificationLevel,
}

/// Notification levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationLevel {
    All,
    Important,
    Critical,
    None,
}

/// Privacy settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    pub store_conversations: bool,
    pub share_analytics: bool,
    pub local_processing_only: bool,
    pub data_retention_days: Option<u32>,
}

/// Active workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWorkflow {
    pub workflow_id: String,
    pub workflow_type: WorkflowType,
    pub current_step: usize,
    pub total_steps: usize,
    pub started_at: DateTime<Utc>,
    pub context_data: HashMap<String, serde_json::Value>,
    pub pending_actions: Vec<String>,
}

/// Workflow types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowType {
    TaskCreation,
    ContentGeneration,
    DataAnalysis,
    FileManagement,
    ProjectSetup,
    Custom(String),
}

/// Context variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextVariable {
    pub name: String,
    pub value: serde_json::Value,
    pub variable_type: ContextVariableType,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub access_count: usize,
}

/// Context variable types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextVariableType {
    Temporary,
    Session,
    Persistent,
    Shared,
}

/// Recent entity for context awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntity {
    pub entity_type: String,
    pub entity_value: String,
    pub confidence: f32,
    pub first_mentioned: DateTime<Utc>,
    pub last_mentioned: DateTime<Utc>,
    pub mention_count: usize,
    pub context: String,
}

/// Interaction summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionSummary {
    pub timestamp: DateTime<Utc>,
    pub interaction_type: InteractionType,
    pub intent: String,
    pub success: bool,
    pub response_time_ms: u64,
    pub entities_extracted: usize,
    pub actions_executed: usize,
}

/// Interaction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionType {
    VoiceCommand,
    TextInput,
    FileOperation,
    TaskManagement,
    ContentGeneration,
    SystemQuery,
}

/// Application state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationState {
    pub current_mode: ApplicationMode,
    pub active_features: Vec<String>,
    pub recent_errors: Vec<ErrorRecord>,
    pub system_health: SystemHealth,
}

/// Application modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApplicationMode {
    Normal,
    VoiceOnly,
    TextOnly,
    Offline,
    Maintenance,
}

/// System health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
    pub network_status: NetworkStatus,
    pub service_status: HashMap<String, ServiceStatus>,
}

/// Network status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkStatus {
    Online,
    Offline,
    Limited,
    Unknown,
}

/// Service status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Error,
    Unknown,
}

/// Error record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub timestamp: DateTime<Utc>,
    pub error_type: String,
    pub message: String,
    pub severity: ErrorSeverity,
    pub context: HashMap<String, serde_json::Value>,
}

/// Error severity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// User profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub total_interactions: usize,
    pub favorite_features: Vec<String>,
    pub skill_level: SkillLevel,
    pub usage_patterns: UsagePatterns,
}

/// Skill levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

/// Usage patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePatterns {
    pub most_active_hours: Vec<u8>,
    pub preferred_interaction_types: Vec<InteractionType>,
    pub average_session_duration: Duration,
    pub common_workflows: Vec<String>,
}

/// System capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCapabilities {
    pub available_models: Vec<String>,
    pub supported_languages: Vec<String>,
    pub max_file_size: u64,
    pub concurrent_operations: usize,
    pub feature_flags: HashMap<String, bool>,
}

/// Integration status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub clickup_connected: bool,
    pub replicate_connected: bool,
    pub mcp_servers: HashMap<String, bool>,
    pub last_sync: HashMap<String, DateTime<Utc>>,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub average_response_time: f32,
    pub success_rate: f32,
    pub total_requests: usize,
    pub cache_hit_rate: f32,
    pub error_rate: f32,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            global_context: Arc::new(RwLock::new(GlobalContext::default())),
            cleanup_interval: Duration::hours(1),
        }
    }

    /// Get or create session context
    pub async fn get_session_context(&self, session_id: &str) -> Result<SessionContext> {
        let mut contexts = self.contexts.write().await;
        
        if let Some(context) = contexts.get(session_id) {
            Ok(context.clone())
        } else {
            let new_context = SessionContext::new(session_id);
            contexts.insert(session_id.to_string(), new_context.clone());
            Ok(new_context)
        }
    }

    /// Update session context
    pub async fn update_session_context(&self, context: SessionContext) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        contexts.insert(context.session_id.clone(), context);
        Ok(())
    }

    /// Add context variable
    pub async fn set_context_variable(
        &self,
        session_id: &str,
        name: String,
        value: serde_json::Value,
        variable_type: ContextVariableType,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        
        if let Some(context) = contexts.get_mut(session_id) {
            let variable = ContextVariable {
                name: name.clone(),
                value,
                variable_type,
                created_at: Utc::now(),
                expires_at,
                access_count: 0,
            };
            
            context.context_variables.insert(name, variable);
            context.last_activity = Utc::now();
        }
        
        Ok(())
    }

    /// Get context variable
    pub async fn get_context_variable(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<Option<serde_json::Value>> {
        let mut contexts = self.contexts.write().await;
        
        if let Some(context) = contexts.get_mut(session_id) {
            if let Some(variable) = context.context_variables.get_mut(name) {
                // Check if variable has expired
                if let Some(expires_at) = variable.expires_at {
                    if Utc::now() > expires_at {
                        context.context_variables.remove(name);
                        return Ok(None);
                    }
                }
                
                variable.access_count += 1;
                return Ok(Some(variable.value.clone()));
            }
        }
        
        Ok(None)
    }

    /// Add recent entity
    pub async fn add_recent_entity(
        &self,
        session_id: &str,
        entity_type: String,
        entity_value: String,
        confidence: f32,
        context: String,
    ) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        
        if let Some(session_context) = contexts.get_mut(session_id) {
            // Check if entity already exists
            if let Some(existing) = session_context.recent_entities.iter_mut()
                .find(|e| e.entity_type == entity_type && e.entity_value == entity_value) {
                existing.last_mentioned = Utc::now();
                existing.mention_count += 1;
                existing.confidence = (existing.confidence + confidence) / 2.0;
            } else {
                let entity = RecentEntity {
                    entity_type,
                    entity_value,
                    confidence,
                    first_mentioned: Utc::now(),
                    last_mentioned: Utc::now(),
                    mention_count: 1,
                    context,
                };
                
                session_context.recent_entities.push(entity);
                
                // Keep only last 50 entities
                if session_context.recent_entities.len() > 50 {
                    session_context.recent_entities.drain(0..session_context.recent_entities.len() - 50);
                }
            }
            
            session_context.last_activity = Utc::now();
        }
        
        Ok(())
    }

    /// Get recent entities by type
    pub async fn get_recent_entities(
        &self,
        session_id: &str,
        entity_type: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<RecentEntity>> {
        let contexts = self.contexts.read().await;
        
        if let Some(context) = contexts.get(session_id) {
            let mut entities = context.recent_entities.clone();
            
            // Filter by type if specified
            if let Some(filter_type) = entity_type {
                entities.retain(|e| e.entity_type == filter_type);
            }
            
            // Sort by last mentioned (most recent first)
            entities.sort_by(|a, b| b.last_mentioned.cmp(&a.last_mentioned));
            
            // Apply limit
            if let Some(limit) = limit {
                entities.truncate(limit);
            }
            
            Ok(entities)
        } else {
            Ok(Vec::new())
        }
    }

    /// Record interaction
    pub async fn record_interaction(
        &self,
        session_id: &str,
        interaction_type: InteractionType,
        intent: String,
        success: bool,
        response_time_ms: u64,
        entities_extracted: usize,
        actions_executed: usize,
    ) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        
        if let Some(context) = contexts.get_mut(session_id) {
            let summary = InteractionSummary {
                timestamp: Utc::now(),
                interaction_type,
                intent,
                success,
                response_time_ms,
                entities_extracted,
                actions_executed,
            };
            
            context.interaction_history.push(summary);
            context.conversation_count += 1;
            context.last_activity = Utc::now();
            
            // Keep only last 100 interactions
            if context.interaction_history.len() > 100 {
                context.interaction_history.drain(0..context.interaction_history.len() - 100);
            }
        }
        
        Ok(())
    }

    /// Get global context
    pub async fn get_global_context(&self) -> GlobalContext {
        let context = self.global_context.read().await;
        context.clone()
    }

    /// Update global context
    pub async fn update_global_context<F>(&self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut GlobalContext),
    {
        let mut context = self.global_context.write().await;
        updater(&mut *context);
        Ok(())
    }

    /// Cleanup expired contexts and variables
    pub async fn cleanup_expired(&self) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        let now = Utc::now();
        
        // Remove expired sessions (inactive for more than 24 hours)
        let session_timeout = Duration::hours(24);
        contexts.retain(|_, context| {
            now.signed_duration_since(context.last_activity) < session_timeout
        });
        
        // Remove expired variables from remaining sessions
        for context in contexts.values_mut() {
            context.context_variables.retain(|_, variable| {
                if let Some(expires_at) = variable.expires_at {
                    now < expires_at
                } else {
                    true
                }
            });
        }
        
        Ok(())
    }

    /// Get context statistics
    pub async fn get_context_stats(&self) -> Result<ContextStats> {
        let contexts = self.contexts.read().await;
        
        let active_sessions = contexts.len();
        let total_variables = contexts.values()
            .map(|c| c.context_variables.len())
            .sum();
        let total_entities = contexts.values()
            .map(|c| c.recent_entities.len())
            .sum();
        let total_interactions = contexts.values()
            .map(|c| c.interaction_history.len())
            .sum();
        
        Ok(ContextStats {
            active_sessions,
            total_variables,
            total_entities,
            total_interactions,
        })
    }
}

/// Context statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStats {
    pub active_sessions: usize,
    pub total_variables: usize,
    pub total_entities: usize,
    pub total_interactions: usize,
}

impl SessionContext {
    fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            created_at: Utc::now(),
            last_activity: Utc::now(),
            conversation_count: 0,
            user_preferences: UserPreferences::default(),
            active_workflows: Vec::new(),
            context_variables: HashMap::new(),
            recent_entities: Vec::new(),
            interaction_history: Vec::new(),
        }
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            timezone: "UTC".to_string(),
            preferred_response_style: ResponseStyle::Conversational,
            voice_settings: VoiceSettings {
                wake_words: vec!["hey echo".to_string()],
                sensitivity: 0.5,
                auto_transcribe: true,
                preferred_voice_speed: 1.0,
            },
            notification_preferences: NotificationPreferences {
                enable_desktop_notifications: true,
                enable_sound_notifications: false,
                notification_level: NotificationLevel::Important,
            },
            privacy_settings: PrivacySettings {
                store_conversations: true,
                share_analytics: false,
                local_processing_only: false,
                data_retention_days: Some(30),
            },
        }
    }
}

impl Default for GlobalContext {
    fn default() -> Self {
        Self {
            application_state: ApplicationState {
                current_mode: ApplicationMode::Normal,
                active_features: Vec::new(),
                recent_errors: Vec::new(),
                system_health: SystemHealth {
                    cpu_usage: 0.0,
                    memory_usage: 0.0,
                    disk_usage: 0.0,
                    network_status: NetworkStatus::Unknown,
                    service_status: HashMap::new(),
                },
            },
            user_profile: UserProfile {
                user_id: uuid::Uuid::new_v4().to_string(),
                created_at: Utc::now(),
                total_interactions: 0,
                favorite_features: Vec::new(),
                skill_level: SkillLevel::Beginner,
                usage_patterns: UsagePatterns {
                    most_active_hours: Vec::new(),
                    preferred_interaction_types: Vec::new(),
                    average_session_duration: Duration::minutes(10),
                    common_workflows: Vec::new(),
                },
            },
            system_capabilities: SystemCapabilities {
                available_models: Vec::new(),
                supported_languages: vec!["en".to_string()],
                max_file_size: 100 * 1024 * 1024, // 100MB
                concurrent_operations: 5,
                feature_flags: HashMap::new(),
            },
            integration_status: IntegrationStatus {
                clickup_connected: false,
                replicate_connected: false,
                mcp_servers: HashMap::new(),
                last_sync: HashMap::new(),
            },
            performance_metrics: PerformanceMetrics {
                average_response_time: 0.0,
                success_rate: 0.0,
                total_requests: 0,
                cache_hit_rate: 0.0,
                error_rate: 0.0,
            },
        }
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
} 