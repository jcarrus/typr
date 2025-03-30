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
- `cliclick` for key monitoring

```bash
brew install sox cliclick
```

## Setup

1. Clone the repository:
```bash
git clone https://github.com/yourusername/typr.git
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

4. Run the program:
```bash
deno task dev
```

## Usage

1. Start the program using `deno task dev`
2. Double-tap and hold the Shift key to start recording
3. Speak your text while holding the Shift key
4. Release the Shift key to stop recording
   - The audio will be transcribed
   - The text will be cleaned up and formatted
   - The result will be typed at your current cursor position

### Voice Commands
- Say "bang" to insert a newline
- Say "ding" to insert a backtick character
- Punctuation marks can be spoken (e.g., "comma", "period")

## Troubleshooting

### Linux
- If `xinput` doesn't detect your keyboard, try running `xinput list` to find your device ID
- If audio recording fails, check if your microphone is set as the default input device

### macOS
- If `cliclick` doesn't work, make sure to grant accessibility permissions in System Preferences > Security & Privacy > Privacy > Accessibility
- If audio recording fails, check your microphone permissions in System Preferences > Security & Privacy > Privacy > Microphone
