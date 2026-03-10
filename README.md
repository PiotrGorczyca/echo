# 🎙️ Echo - Voice-to-Text with Auto-Paste

Echo is a cross-platform Tauri application that records audio from your selected device, transcribes it using OpenAI's Whisper API, and automatically pastes the text into whatever application you're currently focused on.

## ✨ Features

- **Audio Recording**: Record from any available input device (microphone, line-in, etc.)
- **Device Selection**: Choose from all available audio input devices
- **OpenAI Whisper Integration**: High-quality speech-to-text transcription
- **Automatic Pasting**: Automatically pastes transcribed text using Ctrl+V simulation
- **Modern UI**: Beautiful, responsive interface with real-time status updates
- **Cross-Platform**: Works on Linux, Windows, and macOS
- **Secure**: API key stored locally in browser's localStorage

## 🚀 Getting Started

### Prerequisites

- [Bun](https://bun.sh/) for frontend dependencies
- [Rust](https://rustup.rs/) for Tauri backend
- [OpenAI API Key](https://platform.openai.com/api-keys)

### Installation

1. **Clone the repository:**
   ```bash
   git clone <your-repo-url>
   cd echo-tauri
   ```

2. **Install frontend dependencies:**
   ```bash
   bun install
   ```

3. **Run the development server:**
   ```bash
   bun run tauri dev
   ```

4. **Build for production:**
   ```bash
   bun run tauri build
   ```

## 🎯 How to Use

1. **Setup OpenAI API Key:**
   - Get your API key from [OpenAI Platform](https://platform.openai.com/api-keys)
   - Enter it in the "OpenAI Configuration" section
   - Click "Save" to store it locally

2. **Select Audio Device:**
   - Choose your preferred microphone from the dropdown
   - Click "Refresh" to reload available devices

3. **Record and Transcribe:**
   - Click "🎤 Start Recording" to begin
   - Speak clearly into your microphone
   - Click "⏹️ Stop Recording" when finished
   - The app will automatically transcribe and paste the text

4. **Manual Operations:**
   - View transcribed text in the results section
   - Click "📋 Paste Again" to re-paste the text
   - Click "Clear" to remove the transcription

## 🔧 Technical Details

### Backend (Rust)

- **Audio Processing**: Uses `cpal` for cross-platform audio capture
- **File Handling**: Generates WAV files with `hound`
- **HTTP Client**: `reqwest` for OpenAI API communication
- **Clipboard/Keyboard**: `arboard` and `enigo` for text insertion
- **Threading**: Safe state management with `Arc<Mutex>`

### Frontend (Svelte)

- **Framework**: SvelteKit with modern reactive patterns
- **Styling**: Custom CSS with gradient themes and animations
- **State Management**: Svelte 5 reactive state (`$state`)
- **API Integration**: Tauri's `invoke` for backend communication

### Key Libraries

| Purpose | Library | Version |
|---------|---------|---------|
| Audio I/O | cpal | 0.15 |
| WAV Files | hound | 3.5 |
| HTTP Client | reqwest | 0.12 |
| Clipboard | arboard | 3.4 |
| Keyboard Sim | enigo | 0.2 |
| Frontend | SvelteKit | 2.x |
| Build Tool | Tauri | 2.x |

## 🐧 Linux-Specific Features

- **Audio Systems**: Supports ALSA, PulseAudio, and JACK
- **Desktop Integration**: Works with X11 and Wayland
- **Permissions**: May require microphone permissions

### Linux Setup Tips

1. **Microphone Permissions:**
   ```bash
   # Add user to audio group (may require logout/login)
   sudo usermod -a -G audio $USER
   ```

2. **PulseAudio/ALSA Issues:**
   ```bash
   # Check available devices
   arecord -l
   pactl list sources
   ```

## 🔒 Privacy & Security

- **Local Storage**: API keys stored in browser's localStorage
- **No Data Collection**: All processing happens locally or via OpenAI API
- **Temporary Files**: Audio files automatically cleaned up after transcription
- **Network**: Only communicates with OpenAI's Whisper API

## 🚨 Troubleshooting

### Common Issues

1. **No Audio Devices Found:**
   - Check microphone connections
   - Restart the application
   - Verify system audio permissions

2. **API Key Issues:**
   - Ensure valid OpenAI API key
   - Check internet connectivity
   - Verify API key has Whisper access

3. **Paste Not Working:**
   - Ensure target application is focused
   - Check clipboard permissions
   - Try manual paste with Ctrl+V

4. **Build Errors:**
   ```bash
   # Clean and rebuild
   bun run tauri build --no-bundle
   ```

## 🎛️ Configuration

### Audio Settings
- Sample Rate: Automatic (device default)
- Bit Depth: 16-bit
- Format: WAV (uncompressed)

### API Settings
- Model: whisper-1 (OpenAI's latest)
- Language: Auto-detect
- Response Format: JSON

## 🌍 Cross-Platform Status

| Platform | Audio | Clipboard | Keyboard | Status |
|----------|-------|-----------|----------|---------|
| Linux | ✅ | ✅ | ✅ | Fully Supported |
| Windows | ✅ | ✅ | ✅ | Should Work* |
| macOS | ✅ | ✅ | ✅ | Should Work* |

*\*Windows and macOS support expected but not fully tested*

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes
4. Test thoroughly on your platform
5. Submit a pull request

## 📄 License

MIT License - feel free to use and modify as needed.

## 🔮 Future Features

- [ ] Hotkey support for quick recording
- [ ] Multiple language selection
- [ ] Custom whisper model support
- [ ] Recording history
- [ ] Export transcriptions
- [ ] Batch processing

---

**Built with ❤️ using Tauri, Rust, and SvelteKit**
