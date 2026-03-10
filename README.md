# Echo - Voice-to-Text Desktop Application

Echo is a desktop application that captures your voice, transcribes it in real-time, and instantly inserts the text into whatever application you're focused on. Built with Tauri 2, Rust, and SvelteKit.

It supports multiple transcription backends (cloud and local), works across Linux desktop environments (X11 and Wayland), and includes advanced features like voice commands, meeting transcription, and AI-powered task management.

## Features

### Core: Voice-to-Text with Auto-Paste

- **Press-to-talk recording** via global hotkeys (Alt+Alt to toggle)
- **Instant auto-paste** into the focused window after transcription
- **Smart paste detection** on Linux Wayland: detects whether the focused window is a terminal (via `xprop`) and uses the correct shortcut (`Ctrl+V` for GUI apps, `Ctrl+Shift+V` for terminals)
- **Clipboard safety net** via `wl-copy`: text is always available for manual paste even if auto-paste fails
- **Full Unicode support** including accented and non-Latin characters
- **Overlay status indicator**: non-intrusive, always-on-top mini bar showing recording/transcribing/success state without stealing focus

### Transcription Backends

| Backend | Type | Speed | Setup | Best For |
|---------|------|-------|-------|----------|
| **OpenAI Whisper API** | Cloud | Fast | API key | Highest accuracy, any language |
| **Local Whisper** (whisper.cpp) | Local | Medium | Automatic model download | Privacy-focused, offline use |
| **Faster Whisper** | Local (Python) | Fast | Python venv + model | Best local speed, persistent server |
| **Candle Whisper** | Local (Python) | Medium | Python venv | Experimental |

**Supported models**: Tiny, Base, Small, Medium, Large, LargeTurbo, quantized variants (Q5/Q8), Distil-Whisper (HuggingFace), and Moonshine (ultra-fast CPU inference).

### Voice Commands

- Record voice commands that are analyzed for intent using OpenAI
- Automatic routing to MCP tools, AI agent actions, or direct LLM responses
- Fallback to local AI agent when cloud is unavailable

### Meeting Transcription

- Long-form meeting recording with configurable audio chunking
- AI-powered extraction of action items, decisions, follow-ups, and questions
- Meeting metadata: participants, timestamps, priorities
- Configurable audio quality (16kHz mono to 44.1kHz stereo)

### Transcription History

- Searchable history of all transcriptions (up to 1000 entries)
- Pin important entries, re-paste from history
- Metadata: duration, model used, source type

### Task Management

- Markdown-based task storage in `.echo/tasks.md` per repository
- Auto-detects git repositories
- Checklist support, priorities, assignees, due dates
- Integration with Cursor/Claude Code IDEs

### Orchestration Mode (Experimental)

Orchestration is an experimental feature that turns voice commands into automated multi-step development workflows. It connects Echo to **Claude Code** (Anthropic's CLI agent) to manage tasks and write code hands-free.

**How it works:**

1. **Double-Shift** triggers orchestration recording
2. You describe what you want done (e.g. "Create a REST API endpoint for user registration with validation")
3. Echo transcribes your voice and saves the prompt
4. A local HTTP API server starts (port 17832) so Claude Code can report progress
5. Claude Code is invoked in **headless streaming mode** with full workspace context:
   - Git status, recent commits, modified files
   - All tracked repositories and their tasks (`.echo/tasks.md`)
   - Project type detection (Rust, JS, Python, etc.)
6. Claude Code executes autonomously — creating files, updating tasks, writing code
7. Optionally writes `.echo/CURSOR_INSTRUCTIONS.md` which Echo auto-pastes into Cursor IDE

**Dependencies:**
- **Claude Code CLI** (`claude`) must be installed and authenticated — see [Claude Code docs](https://docs.anthropic.com/en/docs/claude-code)
- Uses `--dangerously-skip-permissions` flag for autonomous execution
- Repositories must be registered in Echo's task management UI

**Current limitations:**
- Requires `claude` CLI binary in PATH with valid authentication
- Workspace detection can be unreliable — falls back through multiple strategies (configured default → Cursor state → most recent project → CWD)
- Task file content is truncated to 1500 chars per repo in the prompt context
- Orchestration logs are in-memory only (max 200 entries, lost on restart)
- The file watcher for Cursor handoff has a 2-second debounce and only watches one directory
- Port for the local API server is not persistent across sessions
- MCP tools are intentionally not used during orchestration (Claude Code has native file/shell tools)
- The feature is under active development and may produce unexpected results

### MCP Integration (Model Context Protocol)

- Connect to MCP servers via WebSocket
- Tool discovery and execution from voice commands
- JSON-RPC protocol support

## Getting Started

### Prerequisites

- **[Rust](https://rustup.rs/)** (stable toolchain)
- **[Bun](https://bun.sh/)** (JavaScript runtime and package manager)
- **[OpenAI API Key](https://platform.openai.com/api-keys)** (for cloud transcription and voice commands)

### Linux System Dependencies

Echo relies on a few system tools for clipboard and keyboard simulation. Install them for your distribution:

**Arch Linux / CachyOS:**
```bash
sudo pacman -S xdotool xprop wl-clipboard
```

**Ubuntu / Debian:**
```bash
sudo apt install xdotool x11-utils wl-clipboard
```

**Fedora:**
```bash
sudo dnf install xdotool xprop wl-clipboard
```

| Tool | Purpose | Required |
|------|---------|----------|
| `wl-clipboard` (`wl-copy`) | Reliable clipboard on Wayland | Yes (Wayland) |
| `xdotool` | Keyboard simulation (paste shortcut) via XWayland | Yes |
| `xprop` | Window class detection (terminal vs GUI app) | Yes |
| `xclip` | Clipboard fallback for X11 | Optional (X11 only) |

**For local transcription backends** (optional):
```bash
# Python venv for Faster Whisper / Candle Whisper
sudo pacman -S python python-pip    # Arch
sudo apt install python3 python3-venv  # Ubuntu
```

### Build & Run

```bash
# Clone the repository
git clone https://github.com/piogor/echo.git
cd echo

# Install frontend dependencies
bun install

# Run in development mode
bun run tauri dev

# Build for production
bun run tauri build
```

Production builds output `.deb` and `.rpm` packages in `src-tauri/target/release/bundle/`.

### First Launch

1. Open Echo - the Welcome page guides you through setup
2. Enter your OpenAI API key in Core Settings
3. Select your microphone from the audio device dropdown
4. Press **Alt+Alt** (double-tap Alt) to start recording, speak, and press **Alt+Alt** again to stop
5. The transcribed text is automatically pasted into the focused window

## Usage

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| **Alt + Alt** (double-tap) | Toggle transcription recording |
| **Shift + Shift** (double-tap) | Toggle orchestration/task recording |
| **Esc** | Cancel active recording |

### Recording Modes

- **Transcription**: Standard voice-to-text. Records audio, transcribes, pastes text into focused window.
- **Voice Command**: Records voice, transcribes, analyzes intent with AI, executes tools/actions.
- **Orchestration**: For complex multi-step tasks. Transcribed prompt is routed to task management system.
- **Meeting**: Long-form recording with chunked transcription, action item extraction, and meeting summaries.

### Auto-Paste Behavior

Echo determines the best paste strategy based on the focused window:

1. Text is copied to clipboard via `wl-copy` (Wayland) or `xclip` (X11)
2. `xprop` reads the `WM_CLASS` of the active window (~4ms, no user interaction)
3. If the window is a known terminal emulator or a native Wayland app (no WM_CLASS) → `Ctrl+Shift+V`
4. Otherwise → `Ctrl+V`
5. If auto-paste fails for any reason, the text remains in clipboard for manual paste

### Settings

Settings are stored in `~/.config/echo/settings.json` and include:

- **Transcription mode**: OpenAI API, Local Whisper, Faster Whisper, Candle Whisper
- **Whisper model**: From Tiny to Large, quantized, Distil, and Moonshine variants
- **Audio device**: Selected input device
- **Auto-paste**: Enable/disable automatic text insertion
- **Language**: Transcription language hint (or auto-detect)
- **Voice activation**: Wake word detection with configurable sensitivity
- **Meeting settings**: Chunk duration, audio quality, auto-save interval

## Architecture

```
echo/
  src/                          # SvelteKit frontend (Svelte 5)
    routes/
      +page.svelte              # Main settings UI
      overlay/+page.svelte      # Status overlay (always-on-top)
    components/
      Settings/                 # Settings pages and navigation
      ui/                       # Shared UI components
    styles/                     # Dark theme, animations
  src-tauri/                    # Rust backend (Tauri 2)
    src/
      lib.rs                    # Main orchestration: recording, transcription, paste, hotkeys
      state.rs                  # AppState (Arc<Mutex<>>)
      settings.rs               # Settings persistence
      recording_manager.rs      # Recording mode and state machine
      voice_activation.rs       # Wake word detection, VAD
      voice_command.rs           # Voice command intent analysis
      history.rs                # Transcription history (JSON)
      transcription/
        mod.rs                  # TranscriptionService abstraction
        openai.rs               # OpenAI Whisper API client
        whisper_local.rs        # whisper.cpp via whisper-rs (with model cache)
        faster_whisper.rs       # Python persistent server (JSON-line protocol)
        whisper_candle.rs       # Candle-based inference
        python_env.rs           # Python venv management
      meeting/                  # Meeting recording, chunking, AI processing
      ai_agent/                 # Local AI agent with NLP
      mcp/                      # MCP client, server, registry
      claude/                   # Claude Code / Cursor IDE integration
      tasks/                    # Markdown-based task management
      commands/                 # Tauri command handlers
```

### Key Design Decisions

- **`TranscriptionService`** uses `Arc<TranscriptionService>` (not `Arc<Mutex<>>`) since `transcribe(&self)` only needs a shared reference
- **Faster Whisper** runs as a persistent Python child process communicating via JSON-line protocol over stdin/stdout to avoid model reload on each transcription
- **Local Whisper** maintains a global `MODEL_CACHE` (static HashMap) so models are loaded once and reused
- **Audio device** is pre-warmed at startup in a background thread to avoid 3-5 second ALSA enumeration delays on first recording
- **Hotkey detection** uses `device_query` polling (not global shortcuts) for reliable Alt+Alt and Shift+Shift detection; polling is suppressed during auto-paste via `IS_TYPING` atomic flag to prevent synthetic key events from triggering hotkeys
- **Overlay window** is transparent and always-on-top but never calls `setFocus`, so it displays status without stealing focus from the target application

## Technology Stack

### Backend (Rust)

| Library | Purpose |
|---------|---------|
| tauri 2.0 | Application framework |
| cpal | Cross-platform audio capture |
| hound | WAV file encoding |
| whisper-rs | Local Whisper (whisper.cpp bindings) |
| reqwest | HTTP client (OpenAI API) |
| arboard | Clipboard access |
| enigo 0.6 | Input simulation (x11rb backend) |
| sqlx | SQLite for task storage |
| tokio | Async runtime |
| warp | Local HTTP server (Claude Code callbacks) |
| tokio-tungstenite | WebSocket (MCP) |

### Frontend (SvelteKit)

| Library | Purpose |
|---------|---------|
| Svelte 5 | UI framework with runes reactivity |
| SvelteKit | Routing and build system |
| Vite | Build tooling |
| @tauri-apps/api | Tauri IPC bridge |
| marked | Markdown rendering |

## Platform Support

| Platform | Audio | Transcription | Auto-Paste | Status |
|----------|-------|---------------|------------|--------|
| **Linux (Wayland/KDE)** | ALSA, PulseAudio, JACK | All backends | wl-copy + xdotool | Primary target, fully tested |
| **Linux (X11)** | ALSA, PulseAudio, JACK | All backends | xdotool / enigo | Supported |
| **Windows** | WASAPI | OpenAI API, Local Whisper | enigo | Builds, limited testing |
| **macOS** | CoreAudio | OpenAI API, Local Whisper | enigo | Builds, limited testing |

## Troubleshooting

### Auto-paste not working

- **Wayland**: Ensure `wl-clipboard` and `xdotool` are installed
- **Terminal apps**: Echo auto-detects terminals and uses `Ctrl+Shift+V`. If detection fails, the text is always in clipboard for manual paste
- **Native Wayland windows** (no XWayland): Detected via missing `WM_CLASS`, automatically uses `Ctrl+Shift+V`

### No audio devices found

```bash
# Check available recording devices
arecord -l            # ALSA
pactl list sources    # PulseAudio
```

Ensure your user is in the `audio` group:
```bash
sudo usermod -a -G audio $USER
# Log out and back in
```

### Local Whisper model download fails

Models are downloaded to `~/.cache/whisper/` on first use. Ensure you have internet access and sufficient disk space. Model sizes range from ~75MB (Tiny) to ~3GB (Large).

### Faster Whisper Python errors

```bash
# Check if Python venv exists and has correct packages
ls ~/.config/echo/venv/
# Recreate if needed — Echo auto-creates the venv on first use
```

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes and test on your platform
4. Submit a pull request

## License

MIT License - see [LICENSE](LICENSE) for details.

## Session Changes Summary

### Paste System Overhaul (Latest)

The text insertion mechanism was completely redesigned for reliability and speed on Linux Wayland:

**Before**: `xdotool type` character-by-character (~2s per sentence, triggered IDE autocomplete, no terminal support)

**After**: Clipboard-based instant paste with smart window detection:
1. `wl-copy` sets clipboard reliably (forks daemon, survives process exit)
2. `xprop` detects active window type in ~4ms without user interaction
3. `xdotool key ctrl+v` for GUI apps, `ctrl+shift+v` for terminals and native Wayland windows
4. Hotkey detection (`IS_TYPING` flag) suppressed during paste to prevent synthetic key events from triggering shortcuts
5. Clipboard always available as manual fallback

**Dependencies added**: `wl-clipboard` (system package), enigo upgraded 0.2 → 0.6 (x11rb backend)
