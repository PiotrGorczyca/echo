use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use reqwest::Client;
use std::time::Duration;

#[derive(Debug)]
pub struct OpenAiClient {
    api_key: String,
    client: Client,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntentAnalysis {
    pub tool_name: Option<String>,
    pub server_name: Option<String>,
    pub parameters: HashMap<String, serde_json::Value>,
    pub confidence: f32,
    pub reasoning: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub server: String,
    pub description: String,
    pub parameters: HashMap<String, serde_json::Value>,
}

impl OpenAiClient {
    pub fn get_api_key(&self) -> &str {
        &self.api_key
    }
    
    pub fn new(api_key: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("OpenAI API key is empty"));
        }

        if api_key.len() < 20 {
            eprintln!("Warning: OpenAI API key seems too short, might be invalid");
        }
        
        if !api_key.starts_with("sk-") {
            println!("⚠️ OpenAI API key doesn't start with 'sk-', might be invalid");
        }
        
        println!("🌐 Building HTTP client with 30s timeout...");
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                println!("❌ Failed to build HTTP client: {}", e);
                e
            })?;
        
        println!("✅ OpenAI client initialized successfully");
        Ok(Self {
            api_key,
            client,
        })
    }

    /// Answer a direct question using OpenAI when no specific tools are available
    pub async fn answer_question_directly(&self, question: &str) -> Result<String> {
        println!("🤔 OpenAI Direct Question Answering:");
        println!("   Question: '{}'", question);
        
        let system_prompt = "You are a helpful AI assistant. Answer the user's question concisely and accurately. \
                            If you're not certain about current or real-time information, mention that your knowledge has a cutoff date \
                            and suggest the user might want to search for the most up-to-date information.";
        
        let request = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: question.to_string(),
                },
            ],
            temperature: 0.7,
            max_tokens: 500,
            response_format: None, // No JSON format for direct questions
        };

        println!("📡 Sending direct question to OpenAI API...");
        let response = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                println!("❌ OpenAI API request failed: {}", e);
                anyhow!("OpenAI request failed: {}", e)
            })?;

        let status = response.status();
        println!("📊 OpenAI API response status: {}", status);
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            println!("❌ OpenAI API Error Response: {}", error_text);
            return Err(anyhow!("OpenAI API Error {}: {}", status, error_text));
        }

        let chat_response: ChatResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse OpenAI response: {}", e))?;
        
        if let Some(choice) = chat_response.choices.first() {
            let answer = &choice.message.content;
            println!("✅ OpenAI direct answer received: {}", 
                    if answer.len() > 100 { 
                        format!("{}...", answer.chars().take(100).collect::<String>()) 
                    } else { 
                        answer.clone() 
                    });
            Ok(answer.clone())
        } else {
            Err(anyhow!("No response from OpenAI"))
        }
    }

    pub async fn analyze_intent(&self, text: &str, available_tools: &[McpTool]) -> Result<IntentAnalysis> {
        println!("🧠 OpenAI Intent Analysis Debug:");
        println!("   Input text: '{}'", text);
        println!("   Available tools count: {}", available_tools.len());
        
        // Check if this looks like an informational question first
        let is_informational_query = self.is_informational_query(text);
        println!("   Is informational query: {}", is_informational_query);
        
        // Filter tools to only the most relevant ones to avoid rate limits
        let filtered_tools = self.filter_relevant_tools(text, available_tools);
        println!("   Filtered to {} most relevant tools", filtered_tools.len());
        
        // Look for web search tools specifically for informational queries
        let has_web_search = filtered_tools.iter().any(|tool| {
            tool.name.contains("search") || tool.name.contains("web") || 
            tool.description.to_lowercase().contains("search") || 
            tool.description.to_lowercase().contains("internet") ||
            tool.description.to_lowercase().contains("web")
        });
        
        if filtered_tools.is_empty() && !is_informational_query {
            println!("⚠️ No relevant MCP tools found for analysis!");
        } else {
            println!("   Selected tools:");
            for tool in &filtered_tools {
                println!("     - {}.{}: {}", tool.server, tool.name, tool.description.chars().take(100).collect::<String>());
            }
            if has_web_search {
                println!("   ✅ Web search tools available for informational queries");
            }
        }
        
        let system_prompt = self.generate_system_prompt(&filtered_tools, is_informational_query, has_web_search);
        println!("   System prompt length: {} characters", system_prompt.len());
        
        let request = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                },
            ],
            temperature: 0.3,
            max_tokens: 1000,
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
        };

        println!("📡 Sending request to OpenAI API...");
        let response = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                println!("❌ OpenAI API request failed: {}", e);
                if e.is_timeout() {
                    println!("   Error type: Request timeout");
                } else if e.is_connect() {
                    println!("   Error type: Connection error");
                } else {
                    println!("   Error type: {}", e);
                }
                e
            })?;

        let status = response.status();
        println!("📊 OpenAI API response status: {}", status);
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            println!("❌ OpenAI API Error Response:");
            println!("   Status: {}", status);
            println!("   Error: {}", error_text);
            
            let error_msg = match status.as_u16() {
                401 => "Invalid API key or authentication failed",
                429 => "Rate limit exceeded or quota exhausted",
                500..=599 => "OpenAI server error",
                _ => "Unknown OpenAI API error",
            };
            
            return Err(anyhow!("OpenAI API Error {}: {} ({})", status, error_msg, error_text));
        }

        let chat_response: ChatResponse = response.json().await
            .map_err(|e| {
                println!("❌ Failed to parse OpenAI response JSON: {}", e);
                e
            })?;
        
        println!("✅ OpenAI API response received successfully");
        println!("   Choices count: {}", chat_response.choices.len());
        
        if let Some(choice) = chat_response.choices.first() {
            let content = &choice.message.content;
            println!("   Response content length: {} characters", content.len());
            println!("   Response content preview: {}", if content.len() > 200 { 
                content.chars().take(200).collect::<String>() 
            } else { 
                content.clone() 
            });
            
            // Parse the JSON response
            let intent_analysis: IntentAnalysis = serde_json::from_str(content)
                .map_err(|e| {
                    println!("❌ Failed to parse OpenAI response JSON:");
                    println!("   Error: {}", e);
                    println!("   Raw content: {}", content);
                    anyhow!("Failed to parse OpenAI response: {}", e)
                })?;
            
            println!("✅ Intent analysis parsed successfully:");
            println!("   Tool: {:?}", intent_analysis.tool_name);
            println!("   Server: {:?}", intent_analysis.server_name);
            println!("   Confidence: {}", intent_analysis.confidence);
            
            Ok(intent_analysis)
        } else {
            println!("❌ No choices in OpenAI response");
            Err(anyhow!("No response from OpenAI"))
        }
    }

    /// Check if the query is informational (asking for knowledge, not actions)
    fn is_informational_query(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        
        // Question patterns
        let question_starters = [
            "what", "how", "when", "where", "why", "who", "which", "can you tell me",
            "do you know", "explain", "describe", "tell me about", "jaka jest", "jakie są"
        ];
        
        // Informational keywords
        let info_keywords = [
            "list", "status", "statuses", "options", "types", "kinds", "available",
            "possible", "information", "details", "about", "definition", "meaning"
        ];
        
        // Check for question starters
        let has_question_starter = question_starters.iter()
            .any(|starter| text_lower.starts_with(starter) || text_lower.contains(&format!(" {}", starter)));
        
        // Check for question mark
        let has_question_mark = text.contains('?');
        
        // Check for informational keywords
        let has_info_keywords = info_keywords.iter()
            .any(|keyword| text_lower.contains(keyword));
        
        // Action keywords that suggest this is NOT informational
        let action_keywords = [
            "create", "add", "delete", "remove", "update", "change", "set", "move", 
            "assign", "complete", "start", "stop", "send", "save", "open", "close"
        ];
        
        let has_action_keywords = action_keywords.iter()
            .any(|keyword| text_lower.contains(keyword));
        
        (has_question_starter || has_question_mark || has_info_keywords) && !has_action_keywords
    }

    fn generate_system_prompt(&self, available_tools: &[McpTool], is_informational: bool, has_web_search: bool) -> String {
        let mut prompt = String::from(
            "You are a voice command assistant that analyzes user speech and determines the appropriate tool to execute.\n\n"
        );

        prompt.push_str("Available tools:\n");
        for tool in available_tools {
            // Truncate very long descriptions to stay under token limits
            let description = if tool.description.len() > 150 {
                format!("{}...", tool.description.chars().take(147).collect::<String>())
            } else {
                tool.description.clone()
            };
            
            prompt.push_str(&format!(
                "- {}.{}: {}\n",
                tool.server, tool.name, description
            ));
            
            // Show detailed parameter information
            if !tool.parameters.is_empty() {
                prompt.push_str("  Parameters:\n");
                for (param_name, param_info) in &tool.parameters {
                    let mut param_desc = format!("    - {}", param_name);
                    
                    if let Some(info_obj) = param_info.as_object() {
                        if let Some(required) = info_obj.get("required").and_then(|r| r.as_bool()) {
                            if required {
                                param_desc.push_str(" (REQUIRED)");
                            }
                        }
                        if let Some(param_type) = info_obj.get("type").and_then(|t| t.as_str()) {
                            param_desc.push_str(&format!(" [{}]", param_type));
                        }
                        if let Some(description) = info_obj.get("description").and_then(|d| d.as_str()) {
                            if description.len() <= 50 {
                                param_desc.push_str(&format!(" - {}", description));
                            }
                        }
                    }
                    param_desc.push('\n');
                    prompt.push_str(&param_desc);
                }
            }
        }

        prompt.push_str("\nYour task is to:\n");
        prompt.push_str("1. Analyze the user's voice command\n");
        prompt.push_str("2. Determine which tool (if any) should be executed\n");
        prompt.push_str("3. Extract parameters from the user's speech\n");
        prompt.push_str("4. Assess confidence level (0.0-1.0)\n");
        prompt.push_str("5. Determine if user confirmation is needed\n\n");

        // Add special handling for informational queries
        if is_informational {
            prompt.push_str("SPECIAL NOTE: This appears to be an informational query (asking for knowledge/details).\n");
            if has_web_search {
                prompt.push_str("- Prefer web search tools for current information, definitions, or general knowledge questions\n");
                prompt.push_str("- Use search terms that will find the most relevant and current information\n");
            } else {
                prompt.push_str("- Since no web search tools are available, set tool_name to null for direct LLM response\n");
                prompt.push_str("- Use reasoning to explain that this is an informational query requiring direct assistance\n");
            }
            prompt.push_str("\n");
        }

        prompt.push_str("Respond with JSON in this exact format:\n");
        prompt.push_str("{\n");
        prompt.push_str("  \"tool_name\": \"exact_tool_name_or_null\",\n");
        prompt.push_str("  \"server_name\": \"exact_server_name_or_null\",\n");
        prompt.push_str("  \"parameters\": {\"param1\": \"value1\", \"param2\": \"value2\"},\n");
        prompt.push_str("  \"confidence\": 0.85,\n");
        prompt.push_str("  \"reasoning\": \"Why this tool was selected or why direct LLM response is needed\",\n");
        prompt.push_str("  \"requires_confirmation\": false\n");
        prompt.push_str("}\n\n");

        prompt.push_str("Guidelines:\n");
        prompt.push_str("- Be conservative with confidence scores\n");
        prompt.push_str("- If unsure, set tool_name to null\n");
        prompt.push_str("- IMPORTANT: When selecting a tool, you MUST provide BOTH tool_name AND server_name\n");
        prompt.push_str("- server_name is the part before the dot (e.g., 'clickup' for 'clickup.get_workspace_tasks')\n");
        prompt.push_str("- tool_name is the part after the dot (e.g., 'get_workspace_tasks' for 'clickup.get_workspace_tasks')\n");
        prompt.push_str("- CRITICAL: Check parameters marked as REQUIRED and provide reasonable defaults\n");
        prompt.push_str("- For workspace/team operations, use empty string \"\" for team_id to get user's default workspace\n");
        prompt.push_str("- For get_workspace_tasks: When user wants 'all tasks' or 'entire task list', provide filters:\n");
        prompt.push_str("  Example: {\"include_closed\": false, \"statuses\": [\"Open\", \"in progress\", \"to do\", \"review\"]}\n");
        prompt.push_str("- ClickUp requires at least one filter parameter (tags, list_ids, folder_ids, space_ids, statuses, assignees, or dates)\n");
        prompt.push_str("- Extract parameters naturally from speech or provide sensible defaults\n");
        prompt.push_str("- Use requires_confirmation for destructive actions\n");
        prompt.push_str("- Map informal language to formal parameters\n");
        prompt.push_str("- For informational queries without appropriate tools, set tool_name to null for direct LLM response\n");

        prompt
    }

    /// Filter tools to most relevant ones based on user input to avoid rate limits
    fn filter_relevant_tools(&self, text: &str, available_tools: &[McpTool]) -> Vec<McpTool> {
        let text_lower = text.to_lowercase();
        let mut scored_tools = Vec::new();

        for tool in available_tools {
            let mut score = 0;
            
            // Direct keyword matching
            if text_lower.contains("task") && tool.name.contains("task") { score += 10; }
            if text_lower.contains("create") && tool.name.contains("create") { score += 10; }
            if text_lower.contains("get") && tool.name.contains("get") { score += 5; }
            if text_lower.contains("update") && tool.name.contains("update") { score += 8; }
            if text_lower.contains("delete") && tool.name.contains("delete") { score += 8; }
            if text_lower.contains("list") && tool.name.contains("list") { score += 7; }
            if text_lower.contains("time") && tool.name.contains("time") { score += 8; }
            if text_lower.contains("comment") && tool.name.contains("comment") { score += 8; }
            
            // Boost web search tools for informational queries
            let is_informational = self.is_informational_query(text);
            if is_informational {
                if tool.name.contains("search") || tool.name.contains("web") || 
                   tool.description.to_lowercase().contains("search") || 
                   tool.description.to_lowercase().contains("internet") ||
                   tool.description.to_lowercase().contains("web") {
                    score += 15; // High priority for search tools on informational queries
                }
            }
            
            // Always include essential tools
            if tool.name == "get_workspace_hierarchy" { score += 5; }
            if tool.name == "create_task" { score += 3; }
            
            scored_tools.push((score, tool));
        }

        // Sort by score and take top 15 tools to stay under token limits
        scored_tools.sort_by(|a, b| b.0.cmp(&a.0));
        scored_tools.into_iter()
            .take(15)
            .map(|(_, tool)| tool.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_generation() {
        let client = OpenAiClient::new("test_key".to_string()).unwrap();
        let tools = vec![
            McpTool {
                name: "create_task".to_string(),
                server: "todo".to_string(),
                description: "Create a new task".to_string(),
                parameters: {
                    let mut map = HashMap::new();
                    map.insert("title".to_string(), serde_json::json!("string"));
                    map.insert("description".to_string(), serde_json::json!("string"));
                    map
                },
            }
        ];

        let prompt = client.generate_system_prompt(&tools, false, false);
        assert!(prompt.contains("create_task"));
        assert!(prompt.contains("todo"));
        assert!(prompt.contains("title"));
    }
}