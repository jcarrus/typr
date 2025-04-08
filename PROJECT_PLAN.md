# Typr - Voice-to-Text Application

## Project Overview
Typr is a desktop application that allows users to record audio, transcribe it using Whisper, and process the transcription with OpenAI. The application is built using Tauri (Rust backend) and React (TypeScript frontend).

## Notes
- use cargo build -q to suppress unuseful output.
- always build in the app/src-tauri directory.

## Implementation Phases

### Phase 1: Basic Setup and UI (Completed)
- [x] Set up Tauri application with React frontend
- [x] Implement tray icon with menu
- [x] Add global shortcut (Cmd+Shift+Space)
- [x] Create basic UI for settings and audio recording
- [x] Implement settings storage for OpenAI API key, custom vocabulary, and instructions

### Phase 2: Audio Recording Implementation (Completed)
- [x] Implement audio recording using cpal library
- [x] Save recorded audio to temporary WAV files
- [x] Add functionality to list available audio input devices
- [x] Create UI for starting/stopping recording and displaying results

### Phase 3: Whisper Integration (Completed)
- [x] Implement Whisper transcription functionality
- [x] Add Whisper model loading and management
- [x] Integrate audio file processing with Whisper
- [x] Handle transcription results in the UI

### Phase 4: Streaming Audio Implementation (Planned)
- [ ] Implement real-time audio streaming buffer
- [ ] Add chunked audio processing
- [ ] Create streaming transcription pipeline
- [ ] Implement progressive UI updates
- [ ] Add configurable chunk size and buffer settings
- [ ] Optimize streaming performance

### Phase 5: OpenAI Integration (Planned)
- [ ] Implement OpenAI API communication
- [ ] Process transcriptions with OpenAI
- [ ] Display OpenAI responses in the UI

### Phase 6: Polish and Refinement (Planned)
- [ ] Improve error handling
- [ ] Add user feedback for recording status
- [ ] Optimize performance
- [ ] Add additional settings and customization options

## Technical Details

### Backend (Rust)
- **Framework**: Tauri 2.0
- **Audio Recording**: cpal library
- **File Handling**: tempfile for temporary files, hound for WAV file handling
- **Error Handling**: anyhow for error propagation
- **Logging**: log crate with env_logger
- **Transcription**: openai api for transcription

### Frontend (React/TypeScript)
- **Framework**: React with TypeScript
- **UI Library**: Tailwind CSS with DaisyUI
- **State Management**: React hooks (useState, useEffect)
- **Tauri Integration**: @tauri-apps/api

## File Structure

### Backend
- `app/src-tauri/src/lib.rs`: Main application entry point, tray icon, and global shortcut setup
- `app/src-tauri/src/audio_processing.rs`: Audio recording and processing functionality

### Frontend
- `app/src/App.tsx`: Main React component with UI for settings and audio recording
- `app/src/main.tsx`: React entry point
- `app/src/App.css`: Styling

## Current Status

### Completed Features
- Tray icon with menu (including quit option)
- Global shortcut (Cmd+Shift+Space) to activate the app
- Basic UI for settings (OpenAI API key, custom vocabulary, instructions)
- Audio recording functionality using cpal
- Listing available audio input devices
- UI for starting/stopping recording and displaying results
- Whisper transcription integration
- Local model management

### Known Issues
- Fixed compilation errors related to Tauri 2.0 state access patterns
- Need to implement streaming audio processing
- Need to implement OpenAI API integration

## Next Steps
1. Implement real-time audio streaming buffer
2. Add chunked audio processing
3. Create streaming transcription pipeline
4. Integrate with OpenAI API
5. Improve error handling and user feedback
6. Add additional settings and customization options

## Dependencies

### Backend (Cargo.toml)
- tauri
- cpal
- hound
- tempfile
- anyhow
- log
- env_logger
- serde
- serde_json
- whisper-rs

### Frontend (package.json)
- @tauri-apps/api
- react
- react-dom
- tailwindcss
- daisyui

## Notes
- The application uses Tauri 2.0, which has different state access patterns compared to Tauri 1.x
- Audio recording is implemented using the cpal library, which provides cross-platform audio capture
- Whisper transcription is handled locally using whisper-rs
- OpenAI API integration is planned for future phases
- Streaming audio processing will enable real-time transcription 