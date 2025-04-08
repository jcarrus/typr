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

### Phase 4: OpenAI Integration (Completed)
- [x] Implement OpenAI API communication
- [x] Process transcriptions with OpenAI
- [x] Display OpenAI responses in the UI
- [x] Add error handling for API key management

### Phase 5: Streaming Audio Implementation (Planned)
- [ ] Implement real-time audio streaming buffer
- [ ] Add chunked audio processing
- [ ] Create streaming transcription pipeline
- [ ] Implement progressive UI updates
- [ ] Add configurable chunk size and buffer settings
- [ ] Optimize streaming performance

### Phase 6: Polish and Refinement (Planned)
- [ ] Improve error handling
- [ ] Add user feedback for recording status
- [ ] Optimize performance
- [ ] Add additional settings and customization options
- [ ] Implement proper cleanup of temporary files
- [ ] Add support for different audio formats
- [ ] Improve logging and debugging capabilities

## Technical Details

### Backend (Rust)
- **Framework**: Tauri 2.0
- **Audio Recording**: cpal library
- **File Handling**: tempfile for temporary files, hound for WAV file handling
- **Error Handling**: anyhow for error propagation
- **Logging**: log crate with env_logger
- **Transcription**: OpenAI API for transcription
- **HTTP Client**: reqwest with multipart support

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
- OpenAI API integration for transcription
- Local model management
- Error handling for API key management
- Multipart form handling for file uploads

### Known Issues
- Need to implement proper cleanup of temporary files
- Need to add retry mechanism for failed API requests
- Need to implement streaming audio processing
- Need to add support for different OpenAI models

## Next Steps
1. Implement retry mechanism for failed API requests
2. Add rate limiting and quota management for OpenAI API
3. Implement proper cleanup of temporary files
4. Add support for different OpenAI models
5. Begin work on streaming audio implementation
6. Improve error handling and user feedback
7. Add additional settings and customization options

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
- reqwest (with multipart feature)
- bytes
- tokio
- base64
- futures-util

### Frontend (package.json)
- @tauri-apps/api
- react
- react-dom
- tailwindcss
- daisyui

## Notes
- The application uses Tauri 2.0, which has different state access patterns compared to Tauri 1.x
- Audio recording is implemented using the cpal library, which provides cross-platform audio capture
- OpenAI API integration is now working with proper multipart form handling
- Streaming audio processing will enable real-time transcription 