# typr

A voice-to-text tool that lets you dictate text anywhere in your system. Double-tap and hold the Shift key to start recording, release it to stop and insert the transcribed text.

## Prerequisites

- [Deno](https://deno.land/) installed
- [OpenAI API key](https://platform.openai.com/api-keys)
- [OpenAI Whisper](https://github.com/openai/whisper) installed

### Linux Requirements
- `arecord` (usually comes with ALSA utils)
- `pactl` (PulseAudio command-line interface)
- `xdotool` for typing simulation
- `xinput` for key monitoring

```bash
# Ubuntu/Debian
sudo apt-get install alsa-utils pulseaudio-utils xdotool x11-utils

# Arch/Manjaro
sudo pacman -S alsa-utils pulseaudio-utils xdotool xorg-xinput
```

### macOS Requirements
- `sox` for audio recording
- [Hammerspoon](https://www.hammerspoon.org/) for key monitoring

```bash
# Install sox
brew install sox

# Install Hammerspoon
brew install --cask hammerspoon
```

## Setup

1. Clone the repository:
```bash
git clone https://github.com/jcarrus/typr.git
cd typr
```

2. Create a `.env` file with your OpenAI API key:
```bash
echo "OPENAI_API_KEY=your_api_key_here" > .env
```

3. (Optional) Create a `config.user.json` file to override default settings:
```bash
cp config.json config.user.json
```

4. Set up Hammerspoon (macOS only):
```bash
# Create Hammerspoon config directory if it doesn't exist
mkdir -p ~/.hammerspoon

# Create symbolic link to the init.lua script
ln -sf "$(pwd)/init.lua" ~/.hammerspoon/init.lua

# Start Hammerspoon
open -a Hammerspoon
```

5. Run the program:
```bash
deno task dev
```

## Usage Linux

1. Start the program using `deno task dev`
2. Activate the extension with either a double-tap and hold of the shift key.
3. Speak your text
4. Release the Shift key to stop recording
   - The audio will be transcribed
   - The text will be cleaned up and formatted
   - The result will be typed at your current cursor position

## Usage macOS

1. Start the program using `deno task dev`
2. Press Command + Shift + Space to start recording
3. Speak your text
4. Press Command + Shift + Space again to stop recording
   - The audio will be transcribed
   - The text will be cleaned up and formatted
   - The result will be typed at your current cursor position
5. Press Escape at any time to cancel the current operation

### Voice Commands
- Say "bang" to insert a newline
- Say "ding" to insert a backtick character
- Punctuation marks can be spoken (e.g., "comma", "period")

## Troubleshooting

### Linux
- If `xinput` doesn't detect your keyboard, try running `xinput list` to find your device ID
- If audio recording fails, check if your microphone is set as the default input device

### macOS
- If Hammerspoon doesn't work, make sure to:
  1. Grant accessibility permissions in System Preferences > Security & Privacy > Privacy > Accessibility
  2. Check that Hammerspoon is running (look for the hammer icon in your menu bar)
  3. Reload the Hammerspoon configuration by clicking the hammer icon and selecting "Reload Config"
- If audio recording fails, check your microphone permissions in System Preferences > Security & Privacy > Privacy > Microphone
