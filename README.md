# Typr

Elegant dictation with press-and-hold recording powered by OpenAI's Whisper and GPT-4o-mini.

## Features

- 🎙️ **Press & Hold Recording**: Natural keyboard shortcut workflow
- 🗣️ **Smart Transcription**: Local Whisper or OpenAI API transcription  
- 🤖 **Intelligent Processing**: GPT-4o-mini processes "note to the editor" commands
- ⌨️ **Seamless Typing**: Cross-platform text insertion
- ⚙️ **Simple Setup**: JSON-based configuration
- 🔄 **Process-Based**: Elegant inter-process communication

## Quick Start

### 1. Install

```bash
# Make sure Deno is installed
curl -fsSL https://deno.land/install.sh | sh

# Build and install
./build.sh
```

### 2. Configure

```bash
typr config  # Check current configuration
```

Add your OpenAI API key to `~/.typr-settings.json` (auto-created on first run).

### 3. Setup Keyboard Shortcut

```bash
typr shortcuts
```

Follow the instructions to bind a keyboard shortcut to `typr toggle`.

### 4. Start Recording!

- **Press** your shortcut: Starts recording (you'll hear a beep)
- **Release** your shortcut: Stops, transcribes, and types the result

## Commands

- `typr config` - Show current configuration and status
- `typr shortcuts` - Show keyboard shortcut setup instructions  
- `typr record` - One-time recording and transcription
- `typr toggle` - Toggle recording (used by shortcuts)

## System Requirements

### For Audio Recording
- **macOS**: Built-in (uses AVFoundation)
- **Linux**: `ffmpeg` with PulseAudio support
- **Windows**: `ffmpeg` with DirectShow support

### For Text Typing
- **macOS**: Built-in (uses AppleScript)
- **Linux**: `xdotool` package
- **Windows**: Built-in (uses PowerShell)

### For Local Whisper (Optional)
```bash
pip install openai-whisper
```

## Installation

### Install Dependencies

**Linux (Ubuntu/Debian):**
```bash
sudo apt update
sudo apt install ffmpeg xdotool
```

**Linux (Arch/Manjaro):**
```bash
sudo pacman -S ffmpeg xdotool
```

**macOS:**
```bash
brew install ffmpeg
```

**Windows:**
```bash
# Install ffmpeg via chocolatey or download from https://ffmpeg.org/
choco install ffmpeg
```

### Install Typr Globally

```bash
# After building
sudo mv ./dist/typr /usr/local/bin/
# or on Windows, add to PATH
```

## Configuration

Settings are stored in `~/.typr-settings.json`:

```json
{
  "openAIKey": "your-api-key",
  "whisperPrompt": "Custom vocabulary context...",
  "llmPrompt": "Custom processing instructions...",
  "useLocalWhisper": false
}
```

## Usage Examples

### Basic Recording
```bash
typr record
# Speak into microphone, press Enter to stop
# Text will be transcribed and optionally typed
```

### With Custom Instructions
Say "note to the editor" followed by instructions during recording:

> "Please write a professional email about the quarterly results. Note to the editor: make it formal and include bullet points."

The GPT model will process your transcription according to your custom LLM prompt.

### Interactive Mode
```bash
typr start
# Press 'r' to record, 'q' to quit
```

## Differences from Desktop Version

The CLI version maintains all core functionality but with some changes:

**✅ Preserved:**
- OpenAI Whisper/GPT integration
- Local Whisper support
- Custom prompts and vocabulary
- Cross-platform text typing
- Same transcription quality

**🔄 Changed:**
- Process-based architecture instead of daemon mode
- Keyboard shortcut triggers process spawning
- Elegant signal-based inter-process communication
- Simplified setup with clear instructions

**📦 Improved:**
- No background processes or daemons
- Robust process coordination via PID files
- Natural press-and-hold workflow
- Better resource usage (only runs when needed)

## Development

The CLI is implemented in a single TypeScript file (`typr.ts`) using Deno:

```bash
# Run directly with Deno
deno run --allow-all typr.ts setup

# Build binary
deno compile --allow-all --output dist/typr typr.ts
```

## Troubleshooting

### Audio Recording Issues
- Ensure ffmpeg is installed and in PATH
- Check microphone permissions
- Test with: `ffmpeg -f [format] -i [input] -t 5 test.wav`

### Transcription Issues
- Verify OpenAI API key is valid
- For local Whisper: `pip install openai-whisper`
- Check internet connection for API calls

### Text Typing Issues
- **Linux**: Install `xdotool`
- **macOS**: Grant accessibility permissions if prompted
- **Windows**: Run as administrator if needed

## License

Same as original project.
