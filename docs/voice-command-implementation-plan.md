# Voice Command System Implementation Plan

## Overview

This document outlines the implementation of a voice command system for EchoType using double Shift tap as the trigger. The system will enable users to speak natural language commands that get transcribed, analyzed by OpenAI, and routed to appropriate MCP tools for execution.

**Project Goal**: Transform EchoType from transcription-only to intelligent voice-controlled AI assistant  
**Trigger**: Double Shift tap (similar to existing double Alt for transcription)  
**Flow**: Record → Transcribe → AI Analysis → MCP Tool Execution → Response  

---

## Current System Analysis

### ✅ Existing Infrastructure
- **Double Alt System**: Working transcription trigger in `src-tauri/src/lib.rs`
- **Voice Activation**: VAD-based recording system in `src-tauri/src/voice_activation.rs`
- **Transcription**: OpenAI Whisper API + Candle Whisper local processing
- **MCP Framework**: Client, server registry, and built-in tools
- **AI Agent Core**: Intent processing and command routing in `src-tauri/src/ai_agent/`
- **Settings UI**: Three-page settings system with navigation

### 🔄 Key Differentiators

| Feature | Double Alt (Transcription) | Double Shift (Commands) |
|---------|---------------------------|-------------------------|
| **Purpose** | Text output | Action execution |
| **Processing** | Transcribe only | Transcribe + AI + Execute |
| **Output** | Raw text | Tool results + feedback |
| **Duration** | User controlled | Auto-stop on silence |
| **Feedback** | Recording indicator | Command processing states |
| **Integration** | None | MCP tools + OpenAI |

---

## Architecture Design

### System Flow
```
Double Shift Tap → Voice Recording → Transcription → OpenAI Analysis → MCP Tool Selection → Execution → Response
```

### Core Components

#### 1. **VoiceCommandService**
```rust
pub struct VoiceCommandService {
    transcription_service: Arc<Mutex<TranscriptionService>>,
    ai_agent: Arc<AiAgentCore>,
    mcp_client: Arc<McpClient>,
    openai_client: OpenAiClient,
    recording_state: Arc<Mutex<VoiceCommandState>>,
    settings: VoiceCommandSettings,
}
```

#### 2. **OpenAI Integration**
- **Chat Completion API** for intent understanding
- **Dynamic system prompts** with available MCP tools
- **JSON response parsing** for tool selection
- **Fallback handling** for API failures

#### 3. **Intelligent Routing**
1. **Custom voice commands** (exact phrase matches)
2. **OpenAI intent extraction** (natural language flexibility)
3. **AI agent fallback** (general conversation)

---

## Implementation Tasks

### 🏗️ Phase 1: Core Infrastructure (High Priority)

#### ✅ COMPLETED
*No tasks completed yet*

#### 🔄 IN PROGRESS
- [ ] **Voice Command System Analysis** - Understand current systems and design integration points

#### 📋 PENDING

##### Key Detection System
- [ ] **Implement Double Shift Detection** 
  - Extend existing `monitor_alt_keys()` function in `src-tauri/src/lib.rs`
  - Add support for `LShift` and `RShift` key detection
  - Use same timing constants (200ms press, 500ms between taps)
  - Create separate event handling for voice commands
  - **Dependencies**: None
  - **Priority**: High
  - **Effort**: Low

##### Voice Command Service
- [ ] **Create VoiceCommandService Structure**
  - Create `src-tauri/src/voice_command.rs` module
  - Define `VoiceCommandService` struct and state management
  - Implement basic recording lifecycle
  - Add Tauri command interface
  - **Dependencies**: Double Shift Detection
  - **Priority**: High
  - **Effort**: Medium

- [ ] **Voice Command State Management**
  - Define `VoiceCommandState` enum (Idle, Recording, Processing, etc.)
  - Implement state transitions and validation
  - Add thread-safe state sharing with UI
  - Create status update events for frontend
  - **Dependencies**: VoiceCommandService Structure
  - **Priority**: High
  - **Effort**: Medium

##### OpenAI Integration
- [ ] **OpenAI Client Implementation**
  - Add OpenAI API client to dependencies (reqwest-based)
  - Implement Chat Completion API calls
  - Add retry logic and error handling
  - Create response parsing for JSON tool selection
  - **Dependencies**: None
  - **Priority**: High
  - **Effort**: Medium

- [ ] **Dynamic System Prompt Generation**
  - Query available MCP tools from registry
  - Generate context-aware system prompts
  - Include tool schemas and examples
  - Implement prompt caching for performance
  - **Dependencies**: OpenAI Client Implementation
  - **Priority**: High
  - **Effort**: Medium

### 🤖 Phase 2: AI Integration (High Priority)

#### Intent Processing
- [ ] **Voice Command Intent Extraction**
  - Design prompts for tool selection and parameter extraction
  - Implement confidence scoring for tool matches
  - Add parameter validation and type conversion
  - Create fallback logic for low confidence scores
  - **Dependencies**: Dynamic System Prompt Generation
  - **Priority**: High
  - **Effort**: High

- [ ] **Natural Language Parameter Mapping**
  - Extract parameters from natural speech patterns
  - Map informal language to formal tool parameters
  - Handle missing required parameters gracefully
  - Implement parameter confirmation for critical operations
  - **Dependencies**: Voice Command Intent Extraction
  - **Priority**: Medium
  - **Effort**: High

#### Tool Routing
- [ ] **Intelligent MCP Tool Selection**
  - Route commands to appropriate MCP servers and tools
  - Handle multiple potential tool matches
  - Implement tool priority and preference system
  - Add user confirmation for ambiguous commands
  - **Dependencies**: Voice Command Intent Extraction
  - **Priority**: High
  - **Effort**: Medium

- [ ] **Command Execution Pipeline**
  - Execute selected MCP tools with extracted parameters
  - Handle tool execution errors gracefully
  - Format tool responses for user presentation
  - Implement execution timeout and cancellation
  - **Dependencies**: Intelligent MCP Tool Selection
  - **Priority**: High
  - **Effort**: Medium

### 🎨 Phase 3: User Interface (Medium Priority)

#### Settings Integration
- [ ] **Voice Command Settings Page**
  - Add voice command configuration to settings UI
  - Include OpenAI API key configuration
  - Add confidence threshold and timeout settings
  - Create custom voice command management interface
  - **Dependencies**: VoiceCommandService Structure
  - **Priority**: Medium
  - **Effort**: Medium

- [ ] **Custom Voice Commands UI**
  - Interface for adding/editing custom command phrases
  - Tool and parameter mapping configuration
  - Import/export custom command sets
  - Command testing and validation interface
  - **Dependencies**: Voice Command Settings Page
  - **Priority**: Medium
  - **Effort**: High

#### Visual Feedback
- [ ] **Voice Command Status Overlay**
  - Extend existing overlay to show command processing states
  - Add visual indicators for each processing stage
  - Display tool execution progress and results
  - Implement error state visualization
  - **Dependencies**: Voice Command State Management
  - **Priority**: Medium
  - **Effort**: Medium

- [ ] **Command History and Results**
  - Show recent voice commands and their outcomes
  - Allow re-execution of previous commands
  - Display tool execution logs and debugging info
  - Implement command favorites and shortcuts
  - **Dependencies**: Voice Command Status Overlay
  - **Priority**: Low
  - **Effort**: Medium

### ⚙️ Phase 4: Advanced Features (Low Priority)

#### Enhanced Functionality
- [ ] **Multi-Step Command Workflows**
  - Support for commands that require multiple tool calls
  - Workflow state persistence across command sessions
  - Conditional execution based on previous results
  - User confirmation for workflow continuation
  - **Dependencies**: Command Execution Pipeline
  - **Priority**: Low
  - **Effort**: High

- [ ] **Context-Aware Commands**
  - Maintain conversation context across commands
  - Reference previous command results in new commands
  - Session-based context management
  - Context expiration and cleanup
  - **Dependencies**: Multi-Step Command Workflows
  - **Priority**: Low
  - **Effort**: High

#### Learning and Optimization
- [ ] **Command Learning System**
  - Track command success rates and user corrections
  - Improve tool selection based on usage patterns
  - Personalized command shortcuts and aliases
  - Adaptive confidence thresholds
  - **Dependencies**: Context-Aware Commands
  - **Priority**: Low
  - **Effort**: High

- [ ] **Performance Optimization**
  - Cache frequent tool responses
  - Optimize transcription for command processing
  - Background tool discovery and prompt preparation
  - Reduce latency for common commands
  - **Dependencies**: Command Learning System
  - **Priority**: Low
  - **Effort**: Medium

### 🧪 Phase 5: Testing and Validation (Medium Priority)

#### Testing Infrastructure
- [ ] **Voice Command Test Suite**
  - Unit tests for command parsing and tool selection
  - Integration tests for MCP tool execution
  - Mock OpenAI responses for consistent testing
  - Performance benchmarks for command processing
  - **Dependencies**: Command Execution Pipeline
  - **Priority**: Medium
  - **Effort**: Medium

- [ ] **User Acceptance Testing**
  - Real-world command scenarios and test cases
  - Voice command accuracy validation
  - User experience testing and feedback collection
  - Cross-platform compatibility testing
  - **Dependencies**: Voice Command Test Suite
  - **Priority**: Medium
  - **Effort**: High

---

## Configuration Design

### Voice Command Settings
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandSettings {
    pub enabled: bool,
    pub openai_api_key: String,
    pub auto_stop_silence_ms: u64,         // Default: 2000ms
    pub confidence_threshold: f32,          // Default: 0.7
    pub fallback_to_agent: bool,           // Default: true
    pub max_recording_duration_ms: u64,    // Default: 30000ms
    pub custom_commands: Vec<CustomVoiceCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomVoiceCommand {
    pub id: String,
    pub name: String,
    pub trigger_phrases: Vec<String>,
    pub mcp_server: String,
    pub tool_name: String,
    pub parameter_mapping: HashMap<String, String>,
    pub enabled: bool,
}
```

---

## Test Cases and Examples

### Priority Voice Commands
```
High Priority:
- "Create a task called 'Review project documentation'"
- "List my tasks for today"
- "Search for files containing 'budget'"
- "Show me my recent conversations"

Medium Priority:
- "Generate an image of a sunset over mountains"
- "Open the file called readme.md"
- "What's the weather like today?"
- "Send a message to the development team"

Low Priority:
- "Start a workflow to process customer feedback"
- "Analyze the sentiment of recent support tickets"
- "Create a project timeline for Q1 goals"
```

### Error Handling Scenarios
- OpenAI API failures → Fallback to local AI agent
- Unknown commands → Request clarification
- Missing parameters → Prompt for required information
- Tool execution errors → User-friendly error messages
- Network issues → Offline mode with reduced functionality

---

## Success Metrics

### Phase 1 Success Criteria
- [ ] Double Shift detection working reliably
- [ ] Voice recording and transcription functional
- [ ] Basic OpenAI integration operational
- [ ] Simple tool routing implemented

### Phase 2 Success Criteria
- [ ] Natural language commands correctly parsed
- [ ] 80%+ accuracy for common tool selection
- [ ] Parameter extraction working for basic scenarios
- [ ] Error handling prevents system crashes

### Phase 3 Success Criteria
- [ ] Settings UI allows full voice command configuration
- [ ] Visual feedback provides clear command status
- [ ] Custom commands can be created and edited
- [ ] User experience feels responsive and intuitive

### Final Success Criteria
- [ ] Voice commands work as naturally as the existing transcription
- [ ] Users can accomplish complex tasks through voice alone
- [ ] System handles edge cases and errors gracefully
- [ ] Performance is acceptable for real-world usage

---

## Technical Notes

### Dependencies to Add
```toml
# Cargo.toml additions
openai-api-rust = "0.1"  # or similar OpenAI client
serde_json = "1.0"       # Enhanced JSON handling
tokio-retry = "0.3"      # API retry logic
```

### Key Files to Modify
- `src-tauri/src/lib.rs` - Add double shift detection
- `src-tauri/src/state.rs` - Add voice command state
- `src-tauri/Cargo.toml` - Add new dependencies
- `src/components/Settings/` - Add voice command settings
- `src/routes/overlay/+page.svelte` - Add command status display

### Memory Management
- Ensure voice command recording doesn't conflict with transcription recording
- Implement proper cleanup for OpenAI client resources
- Handle concurrent command execution gracefully
- Cache frequently used tool schemas and prompts

---

*Last Updated: December 2024*  
*Next Review: After Phase 1 completion*  
*Status: Phase 1 - Planning Complete, Ready for Implementation* 