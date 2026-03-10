# Voice Command Fallback Mechanism

## Overview

A new fallback mechanism has been implemented to handle informational queries (like "What are the available task statuses in ClickUp?") when no specific MCP tools are available to handle them.

## How It Works

### 1. Query Classification

The system now automatically classifies incoming voice commands to determine if they are:
- **Informational queries**: Questions asking for knowledge, definitions, or general information
- **Action commands**: Instructions to perform specific tasks

### Informational Query Detection

The system looks for:
- Question starters: "what", "how", "when", "where", "why", "who", "which", "can you tell me", "explain", "describe"
- Question marks: "?"
- Information keywords: "list", "status", "statuses", "options", "types", "available", "information"
- Multilingual support: Polish question starters like "jaka jest", "jakie są"

### 2. Fallback Hierarchy

When processing a voice command, the system follows this hierarchy:

1. **MCP Tool Execution**: First priority - use available MCP tools if they match the query
2. **Web Search Tools**: For informational queries, prioritize any available web search MCP tools
3. **Direct OpenAI Response**: If no appropriate tools exist, use OpenAI directly for informational queries
4. **AI Agent Fallback**: Traditional fallback to local AI agent for other cases

### 3. Enhanced Intent Analysis

The OpenAI intent analysis now:
- Detects informational queries automatically
- Prioritizes web search tools for such queries (score boost of +15)
- Provides clear reasoning when direct LLM response is needed
- Uses enhanced system prompts that guide tool selection

## Implementation Details

### New OpenAI Client Methods

```rust
// Direct question answering for informational queries
pub async fn answer_question_directly(&self, question: &str) -> Result<String>

// Enhanced informational query detection
fn is_informational_query(&self, text: &str) -> bool

// Updated system prompt generation with fallback guidance
fn generate_system_prompt(&self, available_tools: &[McpTool], is_informational: bool, has_web_search: bool) -> String
```

### Enhanced Voice Command Processing

```rust
// New fallback logic in process_text_command
if analysis.reasoning.to_lowercase().contains("informational") || 
   analysis.reasoning.to_lowercase().contains("direct llm") ||
   analysis.reasoning.to_lowercase().contains("direct response") {
    
    // Try direct OpenAI response
    match self.answer_question_directly(command_text).await {
        Ok(answer) => {
            // Show direct AI response with 🤖 prefix
        }
        Err(_) => {
            // Fallback to reasoning explanation
        }
    }
}
```

## Examples

### Supported Informational Queries

✅ **"What are the available task statuses in ClickUp?"**
- Detected as informational query
- Will use web search tools if available, or provide direct AI response

✅ **"How do I create a new project in ClickUp?"**
- Informational query about procedures
- Falls back to direct AI knowledge

✅ **"What does 'in progress' status mean?"**
- Definition/explanation request
- Direct AI response

### Action Commands (Handled Normally)

⚙️ **"Create a task called 'Review documentation'"**
- Action command, uses create_task tool

⚙️ **"Update my current task status to completed"**
- Action command, uses update_task tool

## User Experience

### UI Indicators

- **"🧠 Analyzing command intent..."** - Initial processing
- **"🤔 Getting direct AI response..."** - Fallback to direct OpenAI
- **"🤖 [Answer]"** - Direct AI response prefix
- **"💭 [Reasoning]"** - When no appropriate action found

### Metadata Tracking

Each response includes metadata indicating:
- `intent`: Type of processing used ("direct_llm_response", "tool_execution", etc.)
- `server`: Source of response ("openai_direct", specific MCP server, etc.)
- `confidence`: Confidence level of the response
- `processing_time`: Time taken to process the query

## Configuration

### Requirements

- OpenAI API key configured for direct responses
- Works with existing MCP server configurations
- Automatically detects and prioritizes web search tools

### Fallback Behavior

1. If OpenAI is unavailable → Falls back to AI agent
2. If direct response fails → Shows reasoning + error note
3. If no tools match → Provides explanation of capabilities

## Future Enhancements

### Planned Improvements

1. **Web Search MCP Integration**: Automatic setup of web search servers
2. **Caching**: Cache common informational responses
3. **Context Awareness**: Remember recent queries for follow-up questions
4. **Multi-language Support**: Enhanced detection for other languages
5. **Confidence Thresholds**: Configurable confidence levels for fallback triggers

### Potential MCP Tools

Consider adding these MCP servers for better informational query support:
- Web search tools (Google, Bing, DuckDuckGo)
- Wikipedia integration
- Documentation search tools
- API reference tools

## Testing

### Test Cases

```bash
# Test informational queries
"What are the available options?"
"How does authentication work?"
"Can you explain what this means?"
"Jaka jest lista dostępnych statusów?" # Polish

# Test action commands (should not trigger fallback)
"Create a new task"
"Update my settings"
"Delete the file"
```

### Expected Behavior

- Informational queries → Direct AI response or web search
- Action commands → Tool execution as normal
- Graceful degradation when services unavailable
- Clear user feedback about processing method used 