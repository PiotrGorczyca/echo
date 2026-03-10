use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
// use unicode_segmentation::UnicodeSegmentation;

/// NLP Processor for understanding voice commands and text input
pub struct NlpProcessor {
    intent_patterns: HashMap<IntentType, Vec<Regex>>,
    entity_extractors: HashMap<EntityType, Regex>,
    command_templates: HashMap<String, CommandTemplate>,
}

/// NLP processing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NlpResult {
    pub intent: Intent,
    pub entities: Vec<Entity>,
    pub confidence: f32,
    pub processed_text: String,
}

/// Intent classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_type: IntentType,
    pub confidence: f32,
    pub parameters: HashMap<String, serde_json::Value>,
    pub context_required: bool,
}

/// Intent types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IntentType {
    // Recording and transcription
    StartRecording,
    StopRecording,
    TranscribeAudio,
    
    // ClickUp operations
    CreateTask,
    UpdateTask,
    ListTasks,
    CreateProject,
    UpdateProject,
    
    // Content generation
    GenerateImage,
    GenerateVideo,
    GenerateCode,
    GenerateDiagram,
    
    // File operations
    OpenFile,
    SaveFile,
    SearchFiles,
    
    // System operations
    GetSettings,
    UpdateSettings,
    
    // Conversation
    Question,
    Command,
    Clarification,
    
    // MCP Tool Call
    McpToolCall,
    
    // Unknown
    Unknown,
}

/// Entity extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_type: EntityType,
    pub value: String,
    pub confidence: f32,
    pub start_pos: usize,
    pub end_pos: usize,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Entity types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EntityType {
    TaskName,
    ProjectName,
    FileName,
    Duration,
    Date,
    Time,
    Person,
    Priority,
    Status,
    Description,
    ImageStyle,
    VideoLength,
    CodeLanguage,
    DiagramType,
    Number,
    Url,
    Email,
}

/// Command template for structured commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandTemplate {
    pub name: String,
    pub pattern: String,
    pub intent_type: IntentType,
    pub required_entities: Vec<EntityType>,
    pub optional_entities: Vec<EntityType>,
    pub examples: Vec<String>,
}

impl NlpProcessor {
    pub fn new() -> Self {
        let mut processor = Self {
            intent_patterns: HashMap::new(),
            entity_extractors: HashMap::new(),
            command_templates: HashMap::new(),
        };
        
        processor.initialize_patterns();
        processor.initialize_entity_extractors();
        processor.initialize_command_templates();
        
        processor
    }

    /// Process text input and extract intent and entities
    pub async fn process_text(&self, text: &str) -> Result<NlpResult> {
        let cleaned_text = self.clean_text(text);
        
        // Extract entities first
        let entities = self.extract_entities(&cleaned_text)?;
        
        // Determine intent
        let intent = self.determine_intent(&cleaned_text, &entities)?;
        
        // Calculate overall confidence
        let confidence = self.calculate_confidence(&intent, &entities);
        
        Ok(NlpResult {
            intent,
            entities,
            confidence,
            processed_text: cleaned_text,
        })
    }

    /// Clean and normalize text
    fn clean_text(&self, text: &str) -> String {
        // Remove extra whitespace and normalize
        let cleaned = text.trim().to_lowercase();
        
        // Replace common contractions
        let cleaned = cleaned
            .replace("don't", "do not")
            .replace("can't", "cannot")
            .replace("won't", "will not")
            .replace("i'm", "i am")
            .replace("you're", "you are")
            .replace("it's", "it is")
            .replace("let's", "let us");
        
        // Normalize whitespace
        cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Extract entities from text
    fn extract_entities(&self, text: &str) -> Result<Vec<Entity>> {
        let mut entities = Vec::new();
        
        for (entity_type, regex) in &self.entity_extractors {
            for mat in regex.find_iter(text) {
                let entity = Entity {
                    entity_type: entity_type.clone(),
                    value: mat.as_str().to_string(),
                    confidence: 0.8, // Base confidence
                    start_pos: mat.start(),
                    end_pos: mat.end(),
                    metadata: HashMap::new(),
                };
                entities.push(entity);
            }
        }
        
        // Sort by position
        entities.sort_by_key(|e| e.start_pos);
        
        Ok(entities)
    }

    /// Determine intent from text and entities
    fn determine_intent(&self, text: &str, entities: &[Entity]) -> Result<Intent> {
        let mut best_intent = Intent {
            intent_type: IntentType::Unknown,
            confidence: 0.0,
            parameters: HashMap::new(),
            context_required: false,
        };

        // Check against intent patterns
        for (intent_type, patterns) in &self.intent_patterns {
            for pattern in patterns {
                if pattern.is_match(text) {
                    let confidence = self.calculate_pattern_confidence(pattern, text, entities);
                    if confidence > best_intent.confidence {
                        best_intent = Intent {
                            intent_type: intent_type.clone(),
                            confidence,
                            parameters: self.extract_intent_parameters(text, entities, intent_type),
                            context_required: self.requires_context(intent_type),
                        };
                    }
                }
            }
        }

        // Check command templates
        for template in self.command_templates.values() {
            if let Ok(template_regex) = Regex::new(&template.pattern) {
                if template_regex.is_match(text) {
                    let confidence = self.calculate_template_confidence(template, text, entities);
                    if confidence > best_intent.confidence {
                        best_intent = Intent {
                            intent_type: template.intent_type.clone(),
                            confidence,
                            parameters: self.extract_template_parameters(template, text, entities),
                            context_required: self.requires_context(&template.intent_type),
                        };
                    }
                }
            }
        }

        Ok(best_intent)
    }

    /// Calculate pattern confidence
    fn calculate_pattern_confidence(&self, pattern: &Regex, text: &str, entities: &[Entity]) -> f32 {
        let mut confidence = 0.5; // Base confidence
        
        // Boost confidence based on pattern specificity
        if pattern.as_str().len() > 10 {
            confidence += 0.2;
        }
        
        // Boost confidence based on relevant entities
        let relevant_entities = self.count_relevant_entities(entities);
        confidence += relevant_entities as f32 * 0.1;
        
        // Boost confidence for exact matches
        if pattern.find(text).map_or(false, |m| m.as_str() == text) {
            confidence += 0.3;
        }
        
        confidence.min(1.0)
    }

    /// Calculate template confidence
    fn calculate_template_confidence(&self, template: &CommandTemplate, _text: &str, entities: &[Entity]) -> f32 {
        let mut confidence = 0.6; // Base confidence for templates
        
        // Check required entities
        let required_found = template.required_entities.iter()
            .filter(|&entity_type| entities.iter().any(|e| &e.entity_type == entity_type))
            .count();
        
        if required_found == template.required_entities.len() {
            confidence += 0.3;
        } else {
            confidence -= 0.2 * (template.required_entities.len() - required_found) as f32;
        }
        
        // Check optional entities
        let optional_found = template.optional_entities.iter()
            .filter(|&entity_type| entities.iter().any(|e| &e.entity_type == entity_type))
            .count();
        
        confidence += optional_found as f32 * 0.05;
        
        confidence.max(0.0).min(1.0)
    }

    /// Count relevant entities
    fn count_relevant_entities(&self, entities: &[Entity]) -> usize {
        entities.iter().filter(|e| e.confidence > 0.5).count()
    }

    /// Extract intent parameters
    fn extract_intent_parameters(&self, text: &str, entities: &[Entity], intent_type: &IntentType) -> HashMap<String, serde_json::Value> {
        let mut parameters = HashMap::new();
        
        match intent_type {
            IntentType::CreateTask => {
                if let Some(task_name) = entities.iter().find(|e| e.entity_type == EntityType::TaskName) {
                    parameters.insert("task_name".to_string(), serde_json::Value::String(task_name.value.clone()));
                }
                if let Some(project) = entities.iter().find(|e| e.entity_type == EntityType::ProjectName) {
                    parameters.insert("project".to_string(), serde_json::Value::String(project.value.clone()));
                }
                if let Some(priority) = entities.iter().find(|e| e.entity_type == EntityType::Priority) {
                    parameters.insert("priority".to_string(), serde_json::Value::String(priority.value.clone()));
                }
            }
            IntentType::GenerateImage => {
                if let Some(style) = entities.iter().find(|e| e.entity_type == EntityType::ImageStyle) {
                    parameters.insert("style".to_string(), serde_json::Value::String(style.value.clone()));
                }
                parameters.insert("prompt".to_string(), serde_json::Value::String(text.to_string()));
            }
            IntentType::StartRecording => {
                if let Some(duration) = entities.iter().find(|e| e.entity_type == EntityType::Duration) {
                    parameters.insert("duration".to_string(), serde_json::Value::String(duration.value.clone()));
                }
            }
            _ => {
                // Generic parameter extraction
                for entity in entities {
                    let key = format!("{:?}", entity.entity_type).to_lowercase();
                    parameters.insert(key, serde_json::Value::String(entity.value.clone()));
                }
            }
        }
        
        parameters
    }

    /// Extract template parameters
    fn extract_template_parameters(&self, template: &CommandTemplate, _text: &str, entities: &[Entity]) -> HashMap<String, serde_json::Value> {
        let mut parameters = HashMap::new();
        
        // Extract based on template requirements
        for entity_type in &template.required_entities {
            if let Some(entity) = entities.iter().find(|e| &e.entity_type == entity_type) {
                let key = format!("{:?}", entity_type).to_lowercase();
                parameters.insert(key, serde_json::Value::String(entity.value.clone()));
            }
        }
        
        for entity_type in &template.optional_entities {
            if let Some(entity) = entities.iter().find(|e| &e.entity_type == entity_type) {
                let key = format!("{:?}", entity_type).to_lowercase();
                parameters.insert(key, serde_json::Value::String(entity.value.clone()));
            }
        }
        
        parameters
    }

    /// Check if intent requires context
    fn requires_context(&self, intent_type: &IntentType) -> bool {
        matches!(intent_type, 
            IntentType::UpdateTask | 
            IntentType::UpdateProject | 
            IntentType::Clarification |
            IntentType::Question
        )
    }

    /// Calculate overall confidence
    fn calculate_confidence(&self, intent: &Intent, entities: &[Entity]) -> f32 {
        let mut confidence = intent.confidence;
        
        // Boost confidence based on entity quality
        let avg_entity_confidence = if entities.is_empty() {
            0.5
        } else {
            entities.iter().map(|e| e.confidence).sum::<f32>() / entities.len() as f32
        };
        
        confidence = (confidence + avg_entity_confidence) / 2.0;
        
        confidence.min(1.0)
    }

    /// Initialize intent patterns
    fn initialize_patterns(&mut self) {
        // Recording patterns
        self.intent_patterns.insert(IntentType::StartRecording, vec![
            Regex::new(r"start recording|begin recording|record audio|start capture").unwrap(),
            Regex::new(r"record for \d+ (seconds?|minutes?|hours?)").unwrap(),
        ]);
        
        self.intent_patterns.insert(IntentType::StopRecording, vec![
            Regex::new(r"stop recording|end recording|finish recording|stop capture").unwrap(),
        ]);
        
        // ClickUp patterns
        self.intent_patterns.insert(IntentType::CreateTask, vec![
            Regex::new(r"create (a )?task|new task|add task").unwrap(),
            Regex::new(r"create (a )?todo|new todo|add todo").unwrap(),
        ]);
        
        self.intent_patterns.insert(IntentType::ListTasks, vec![
            Regex::new(r"list tasks|show tasks|get tasks|what tasks").unwrap(),
            Regex::new(r"what do i need to do|what's on my todo").unwrap(),
        ]);
        
        // Content generation patterns
        self.intent_patterns.insert(IntentType::GenerateImage, vec![
            Regex::new(r"generate (an? )?image|create (an? )?image|make (an? )?picture").unwrap(),
            Regex::new(r"draw|sketch|illustrate").unwrap(),
        ]);
        
        self.intent_patterns.insert(IntentType::GenerateVideo, vec![
            Regex::new(r"generate (a )?video|create (a )?video|make (a )?video").unwrap(),
        ]);
        
        // File operations
        self.intent_patterns.insert(IntentType::OpenFile, vec![
            Regex::new(r"open (the )?file|open document").unwrap(),
        ]);
        
        // Questions
        self.intent_patterns.insert(IntentType::Question, vec![
            Regex::new(r"^(what|how|when|where|why|who|which)").unwrap(),
            Regex::new(r"\?$").unwrap(),
        ]);
    }

    /// Initialize entity extractors
    fn initialize_entity_extractors(&mut self) {
        // Duration patterns
        self.entity_extractors.insert(EntityType::Duration, 
            Regex::new(r"\d+\s*(seconds?|minutes?|hours?|secs?|mins?)").unwrap());
        
        // Date patterns
        self.entity_extractors.insert(EntityType::Date, 
            Regex::new(r"\d{1,2}[-/]\d{1,2}[-/]\d{2,4}|today|tomorrow|yesterday").unwrap());
        
        // Time patterns
        self.entity_extractors.insert(EntityType::Time, 
            Regex::new(r"\d{1,2}:\d{2}(\s*(am|pm))?").unwrap());
        
        // Priority patterns
        self.entity_extractors.insert(EntityType::Priority, 
            Regex::new(r"(high|medium|low|urgent|normal)\s*priority").unwrap());
        
        // Status patterns
        self.entity_extractors.insert(EntityType::Status, 
            Regex::new(r"(todo|in progress|done|completed|pending|blocked)").unwrap());
        
        // Number patterns
        self.entity_extractors.insert(EntityType::Number, 
            Regex::new(r"\b\d+\b").unwrap());
        
        // Email patterns
        self.entity_extractors.insert(EntityType::Email, 
            Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
        
        // URL patterns
        self.entity_extractors.insert(EntityType::Url, 
            Regex::new(r"https?://[^\s]+").unwrap());
    }

    /// Initialize command templates
    fn initialize_command_templates(&mut self) {
        // Task creation template
        self.command_templates.insert("create_task".to_string(), CommandTemplate {
            name: "Create Task".to_string(),
            pattern: r"create (a )?task (called |named )?(.+?)( in (.+?))?( with (high|medium|low) priority)?".to_string(),
            intent_type: IntentType::CreateTask,
            required_entities: vec![EntityType::TaskName],
            optional_entities: vec![EntityType::ProjectName, EntityType::Priority],
            examples: vec![
                "Create a task called Review documentation".to_string(),
                "Create task Update website in Marketing project".to_string(),
                "Create a task named Fix bug with high priority".to_string(),
            ],
        });
        
        // Image generation template
        self.command_templates.insert("generate_image".to_string(), CommandTemplate {
            name: "Generate Image".to_string(),
            pattern: r"generate (an? )?image of (.+?)( in (.+?) style)?".to_string(),
            intent_type: IntentType::GenerateImage,
            required_entities: vec![EntityType::Description],
            optional_entities: vec![EntityType::ImageStyle],
            examples: vec![
                "Generate an image of a sunset over mountains".to_string(),
                "Generate image of a cat in cartoon style".to_string(),
            ],
        });
        
        // Recording template
        self.command_templates.insert("start_recording".to_string(), CommandTemplate {
            name: "Start Recording".to_string(),
            pattern: r"(start|begin) recording( for (\d+) (seconds?|minutes?))?".to_string(),
            intent_type: IntentType::StartRecording,
            required_entities: vec![],
            optional_entities: vec![EntityType::Duration],
            examples: vec![
                "Start recording".to_string(),
                "Begin recording for 30 seconds".to_string(),
            ],
        });
    }
}

impl Default for NlpProcessor {
    fn default() -> Self {
        Self::new()
    }
} 