use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::openai_client::OpenAiClient;
use super::{ActionItem, ActionItemType, Priority, ActionItemStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItemExtraction {
    pub action_items: Vec<ExtractedActionItem>,
    pub meeting_summary: String,
    pub key_decisions: Vec<String>,
    pub next_meeting_suggestions: Vec<String>,
    pub confidence_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedActionItem {
    pub text: String,
    pub assignee: Option<String>,
    pub due_date: Option<String>, // Will be parsed later
    pub priority: String, // "low", "medium", "high", "critical"
    pub category: String, // "task", "decision", "followup", "question", "note"
    pub context: String, // Surrounding context from the meeting
    pub timestamp_in_meeting: Option<f64>, // Estimated seconds from meeting start
    pub confidence: f32,
}

#[derive(Debug)]
pub struct MeetingAiProcessor {
    pub openai_client: Option<OpenAiClient>,
}

impl MeetingAiProcessor {
    pub fn new(api_key: Option<String>) -> Result<Self> {
        let openai_client = match api_key {
            Some(key) if !key.is_empty() => {
                Some(OpenAiClient::new(key)?)
            }
            _ => None,
        };
        
        Ok(Self {
            openai_client,
        })
    }
    
    pub fn set_api_key(&mut self, api_key: String) -> Result<()> {
        self.openai_client = Some(OpenAiClient::new(api_key)?);
        Ok(())
    }
    
    pub fn has_api_key(&self) -> bool {
        self.openai_client.is_some()
    }
    
    /// Extract action items and meeting insights from a transcript
    pub async fn process_meeting_transcript(
        &self,
        _meeting_id: &str,
        transcript: &str,
        participants: &[String],
        meeting_title: &str,
        meeting_duration_minutes: f64,
    ) -> Result<ActionItemExtraction> {
        println!("🤖 Starting AI processing for meeting: {}", meeting_title);
        
        let client = self.openai_client.as_ref()
            .ok_or_else(|| anyhow!("OpenAI API key not configured"))?;
        
        if transcript.trim().is_empty() {
            return Err(anyhow!("Empty transcript provided"));
        }
        
        let prompt = self.generate_extraction_prompt(
            transcript,
            participants,
            meeting_title,
            meeting_duration_minutes,
        );
        
        println!("📝 Sending transcript to OpenAI for action item extraction...");
        println!("   Transcript length: {} characters", transcript.len());
        println!("   Participants: {:?}", participants);
        
        let response = self.call_openai_for_extraction(&client, &prompt).await?;
        
        // Parse the JSON response
        let extraction: ActionItemExtraction = serde_json::from_str(&response)
            .map_err(|e| {
                println!("❌ Failed to parse AI extraction response:");
                println!("   Error: {}", e);
                println!("   Raw response: {}", response);
                anyhow!("Failed to parse AI response: {}", e)
            })?;
        
        println!("✅ AI extraction completed:");
        println!("   Action items found: {}", extraction.action_items.len());
        println!("   Key decisions: {}", extraction.key_decisions.len());
        println!("   Confidence score: {:.2}", extraction.confidence_score);
        
        Ok(extraction)
    }
    
    /// Convert extracted action items to internal ActionItem format
    pub fn convert_to_action_items(
        &self,
        meeting_id: &str,
        extraction: &ActionItemExtraction,
    ) -> Vec<ActionItem> {
        extraction.action_items.iter().map(|item| {
            ActionItem {
                id: uuid::Uuid::new_v4().to_string(),
                meeting_id: meeting_id.to_string(),
                text: item.text.clone(),
                assignee: item.assignee.clone(),
                due_date: self.parse_due_date(&item.due_date),
                priority: self.parse_priority(&item.priority),
                category: self.parse_category(&item.category),
                context: item.context.clone(),
                status: ActionItemStatus::Pending,
                timestamp_in_meeting: item.timestamp_in_meeting,
            }
        }).collect()
    }
    
    fn generate_extraction_prompt(
        &self,
        transcript: &str,
        participants: &[String],
        meeting_title: &str,
        duration_minutes: f64,
    ) -> String {
        let participants_str = participants.join(", ");
        
        format!(r#"
You are an AI assistant specialized in analyzing meeting transcripts to extract actionable items and insights.

Meeting Details:
- Title: {meeting_title}
- Duration: {duration_minutes:.1} minutes
- Participants: {participants_str}

Please analyze the following meeting transcript and extract:

1. **Action Items**: Tasks, assignments, commitments, and follow-ups
2. **Key Decisions**: Important decisions made during the meeting
3. **Meeting Summary**: A brief summary of what was discussed
4. **Next Meeting Suggestions**: Any mentioned follow-up meetings or scheduling needs

For each action item, provide:
- **text**: Clear, actionable description
- **assignee**: Person responsible (if mentioned, otherwise null)
- **due_date**: Deadline if mentioned (format: "YYYY-MM-DD" or descriptive like "next week", or null)
- **priority**: "low", "medium", "high", or "critical" based on urgency/importance
- **category**: "task" (actionable item), "decision" (decision made), "followup" (follow-up required), "question" (question to answer), or "note" (important note)
- **context**: 1-2 sentences of surrounding context from the meeting
- **timestamp_in_meeting**: Estimated seconds from start of meeting (0-{duration_seconds}) or null
- **confidence**: 0.0-1.0 confidence that this is truly an action item

Guidelines:
- Only include items that are truly actionable or important decisions
- Be specific and clear in descriptions
- Estimate timestamps based on flow of conversation
- Assign realistic priorities based on language used ("urgent", "ASAP", "when you can", etc.)
- For assignee detection, look for phrases like "John will", "can you", "your responsibility", etc.
- Context should help understand why this action item exists

Respond with valid JSON in this exact format:

{{
  "action_items": [
    {{
      "text": "Complete the quarterly report",
      "assignee": "John",
      "due_date": "2024-01-15",
      "priority": "high",
      "category": "task",
      "context": "Discussion about Q4 results led to agreement that quarterly report needs to be completed before board meeting.",
      "timestamp_in_meeting": 450.0,
      "confidence": 0.85
    }}
  ],
  "meeting_summary": "Brief 2-3 sentence summary of the meeting",
  "key_decisions": [
    "Decision 1: We will proceed with the new project timeline",
    "Decision 2: Budget approved for Q1 marketing campaign"
  ],
  "next_meeting_suggestions": [
    "Follow-up meeting needed in 2 weeks to review progress",
    "Schedule 1:1 with Sarah to discuss project details"
  ],
  "confidence_score": 0.78
}}

Meeting Transcript:
{transcript}
"#, 
            meeting_title = meeting_title,
            duration_minutes = duration_minutes,
            participants_str = participants_str,
            duration_seconds = (duration_minutes * 60.0) as u32,
            transcript = transcript
        )
    }
    
    async fn call_openai_for_extraction(
        &self,
        client: &OpenAiClient,
        prompt: &str,
    ) -> Result<String> {
        // Use the OpenAI client's answer_question_directly method with JSON format
        // We'll need to modify this to support JSON format extraction
        
        // For now, let's implement a direct API call similar to the existing client
        use reqwest::Client as HttpClient;
        use serde_json::json;
        
        let http_client = HttpClient::new();
        
        let request_body = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "system",
                    "content": "You are an expert meeting analyst. Always respond with valid JSON in the exact format requested."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.3,
            "max_tokens": 2000,
            "response_format": {
                "type": "json_object"
            }
        });
        
        // Get API key from client (we'll need to expose this)
        let api_key = self.get_api_key_from_client(client)?;
        
        let response = http_client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("OpenAI request failed: {}", e))?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("OpenAI API Error {}: {}", status, error_text));
        }
        
        let response_json: serde_json::Value = response.json().await
            .map_err(|e| anyhow!("Failed to parse OpenAI response: {}", e))?;
        
        let content = response_json
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
            .ok_or_else(|| anyhow!("Invalid OpenAI response format"))?;
        
        Ok(content.to_string())
    }
    
    fn get_api_key_from_client(&self, client: &OpenAiClient) -> Result<String> {
        Ok(client.get_api_key().to_string())
    }
    
    fn parse_due_date(&self, due_date_str: &Option<String>) -> Option<DateTime<Utc>> {
        if let Some(date_str) = due_date_str {
            // Try to parse common date formats
            if let Ok(parsed) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                return Some(parsed.and_hms_opt(23, 59, 59)?.and_utc());
            }
            
            // Handle relative dates like "next week", "tomorrow", etc.
            let now = Utc::now();
            match date_str.to_lowercase().as_str() {
                "today" => Some(now.date_naive().and_hms_opt(23, 59, 59)?.and_utc()),
                "tomorrow" => Some((now + chrono::Duration::days(1)).date_naive().and_hms_opt(23, 59, 59)?.and_utc()),
                "next week" => Some((now + chrono::Duration::weeks(1)).date_naive().and_hms_opt(23, 59, 59)?.and_utc()),
                "next month" => Some((now + chrono::Duration::days(30)).date_naive().and_hms_opt(23, 59, 59)?.and_utc()),
                _ => None,
            }
        } else {
            None
        }
    }
    
    fn parse_priority(&self, priority_str: &str) -> Priority {
        match priority_str.to_lowercase().as_str() {
            "critical" => Priority::Critical,
            "high" => Priority::High,
            "medium" => Priority::Medium,
            "low" | _ => Priority::Low,
        }
    }
    
    fn parse_category(&self, category_str: &str) -> ActionItemType {
        match category_str.to_lowercase().as_str() {
            "task" => ActionItemType::Task,
            "decision" => ActionItemType::Decision,
            "followup" | "follow_up" => ActionItemType::FollowUp,
            "question" => ActionItemType::Question,
            "note" | _ => ActionItemType::Note,
        }
    }
    
    /// Generate a summary of extracted action items for logging/debugging
    pub fn summarize_extraction(&self, extraction: &ActionItemExtraction) -> String {
        format!(
            "Meeting Analysis Summary:\n\
            - {} action items extracted\n\
            - {} key decisions recorded\n\
            - {} next meeting suggestions\n\
            - Overall confidence: {:.1}%\n\
            - Summary: {}",
            extraction.action_items.len(),
            extraction.key_decisions.len(),
            extraction.next_meeting_suggestions.len(),
            extraction.confidence_score * 100.0,
            extraction.meeting_summary
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_priority() {
        let processor = MeetingAiProcessor::new(None).unwrap();
        
        assert!(matches!(processor.parse_priority("critical"), Priority::Critical));
        assert!(matches!(processor.parse_priority("high"), Priority::High));
        assert!(matches!(processor.parse_priority("medium"), Priority::Medium));
        assert!(matches!(processor.parse_priority("low"), Priority::Low));
        assert!(matches!(processor.parse_priority("unknown"), Priority::Low));
    }
    
    #[test]
    fn test_parse_category() {
        let processor = MeetingAiProcessor::new(None).unwrap();
        
        assert!(matches!(processor.parse_category("task"), ActionItemType::Task));
        assert!(matches!(processor.parse_category("decision"), ActionItemType::Decision));
        assert!(matches!(processor.parse_category("followup"), ActionItemType::FollowUp));
        assert!(matches!(processor.parse_category("question"), ActionItemType::Question));
        assert!(matches!(processor.parse_category("note"), ActionItemType::Note));
    }
    
    #[test]
    fn test_parse_due_date() {
        let processor = MeetingAiProcessor::new(None).unwrap();
        
        // Test valid date
        let due_date = processor.parse_due_date(&Some("2024-12-25".to_string()));
        assert!(due_date.is_some());
        
        // Test relative dates
        let today = processor.parse_due_date(&Some("today".to_string()));
        assert!(today.is_some());
        
        let tomorrow = processor.parse_due_date(&Some("tomorrow".to_string()));
        assert!(tomorrow.is_some());
        
        // Test None
        let none_date = processor.parse_due_date(&None);
        assert!(none_date.is_none());
    }
}