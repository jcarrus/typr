use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use dirs::data_dir;
use futures_util::StreamExt;
use hound::{WavSpec, WavWriter};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tempfile::NamedTempFile;

// Struct to hold audio device information
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    name: String,
    id: String,
}

// Struct to hold the result of audio processing
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioProcessingResult {
    transcription: String,
    openai_response: String,
}

// Struct to hold the state of our audio recording
pub struct AudioRecorder {
    is_recording: Arc<Mutex<bool>>,
    stream: Option<cpal::Stream>,
    writer: Option<Arc<Mutex<WavWriter<BufWriter<File>>>>>,
    temp_file: Option<NamedTempFile>,
}

// Implement Send and Sync for AudioRecorder
unsafe impl Send for AudioRecorder {}
unsafe impl Sync for AudioRecorder {}

impl AudioRecorder {
    // Create a new AudioRecorder instance
    pub fn new() -> Self {
        AudioRecorder {
            is_recording: Arc::new(Mutex::new(false)),
            stream: None,
            writer: None,
            temp_file: None,
        }
    }

    // Start recording audio
    pub fn start_recording(&mut self) -> Result<(), String> {
        // Check if already recording
        let mut is_recording = self.is_recording.lock().unwrap();
        if *is_recording {
            return Err("Already recording".to_string());
        }
        *is_recording = true;
        drop(is_recording); // Release the lock

        // Create a temporary file for recording
        let temp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
        let file = File::create(temp_file.path()).map_err(|e| e.to_string())?;
        let writer = WavWriter::new(
            BufWriter::new(file),
            WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .map_err(|e| e.to_string())?;

        let writer = Arc::new(Mutex::new(writer));
        let writer_clone = Arc::clone(&writer);

        // Get default input device
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No input device available".to_string())?;

        // Create stream config
        let config = device.default_input_config().map_err(|e| e.to_string())?;

        // Create the stream
        let err_fn = move |err| {
            error!("An error occurred on stream: {}", err);
        };

        let stream = match config.sample_format() {
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &_| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            writer.write_sample(sample).unwrap_or_else(|e| {
                                error!("Error writing sample: {}", e);
                            });
                        }
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported sample format".to_string()),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;

        self.stream = Some(stream);
        self.writer = Some(writer);
        self.temp_file = Some(temp_file);

        Ok(())
    }

    // Stop recording and return the path to the recorded file
    pub fn stop_recording(&mut self) -> Result<Option<PathBuf>, String> {
        // Check if recording
        let mut is_recording = self.is_recording.lock().unwrap();
        if !*is_recording {
            return Ok(None);
        }
        *is_recording = false;
        drop(is_recording); // Release the lock

        // Stop the stream
        self.stream.take();

        // Finalize the WAV file
        if let Some(writer) = self.writer.take() {
            // Take ownership of the writer by dropping the mutex guard
            let writer = Arc::try_unwrap(writer)
                .map_err(|_| "Failed to get exclusive ownership of writer".to_string())?
                .into_inner()
                .map_err(|_| "Failed to acquire mutex lock".to_string())?;

            // Now we can call finalize
            writer.finalize().map_err(|e| e.to_string())?;
        }

        // Get the path of the temporary file
        let path = self
            .temp_file
            .take()
            .map(|file| file.into_temp_path().keep().unwrap());

        Ok(path)
    }

    // Check if we're currently recording
    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}

// Function to process audio with GPT-4o-mini
pub async fn process_audio_with_gpt4o(audio_path: &str) -> Result<String> {
    info!("Processing audio with GPT-4o-mini: {}", audio_path);

    // Read the audio file
    let audio_data = fs::read(audio_path)?;

    // Encode the audio data as base64
    let audio_base64 = BASE64.encode(&audio_data);

    // Get the OpenAI API key from the app state
    let api_key = get_openai_api_key()?;

    // Create the request to OpenAI
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "file": audio_base64,
            "response_format": "text",
            "language": "en",
            "temperature": 0.2,
            "prompt": "You are a helpful assistant that transcribes speech to text. If you detect a command in the speech (like 'rewrite this' or 'make this more formal'), interpret it and apply it to the transcription. Otherwise, just transcribe the speech accurately."
        }))
        .send()
        .await?;

    // Check if the request was successful
    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
    }

    // Get the transcription
    let transcription = response.text().await?;

    info!("Transcription completed: {}", transcription);
    Ok(transcription)
}

// Helper function to get the OpenAI API key
fn get_openai_api_key() -> Result<String> {
    // In a real application, you would get this from your app's state or configuration
    // For now, we'll use an environment variable
    std::env::var("OPENAI_API_KEY").map_err(|e| anyhow::anyhow!("OpenAI API key not found: {}", e))
}

// Function to process transcription with OpenAI (placeholder for now)
pub fn process_with_openai(transcription: &str) -> Result<String> {
    // This is a placeholder - in a real implementation, we would use the OpenAI API
    info!("Processing transcription with OpenAI: {}", transcription);

    // For now, just return a dummy response
    Ok("This is a dummy response from OpenAI.".to_string())
}

// Function to list available audio input devices
pub fn list_audio_input_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let devices = host.input_devices()?;

    let mut result = Vec::new();

    for (device_index, device) in devices.enumerate() {
        let device_name = device.name()?;
        let device_id = format!("{}", device_index);

        result.push(AudioDevice {
            name: device_name,
            id: device_id,
        });
    }

    Ok(result)
}

// Tauri command to start recording
#[tauri::command]
pub fn start_recording(app_handle: tauri::AppHandle) -> Result<(), String> {
    let mut recorder = app_handle
        .state::<Arc<Mutex<AudioRecorder>>>()
        .inner()
        .lock()
        .unwrap();
    recorder.start_recording()
}

// Tauri command to stop recording and process the audio
#[tauri::command]
pub async fn stop_recording_and_process(
    app_handle: tauri::AppHandle,
) -> Result<AudioProcessingResult, String> {
    // Get the audio path before dropping the mutex guard
    let audio_path = {
        let mut audio_recorder = app_handle
            .state::<Arc<Mutex<AudioRecorder>>>()
            .inner()
            .lock()
            .unwrap();
        audio_recorder.stop_recording().map_err(|e| e.to_string())?
    };

    // Convert the path to a string, handling the Option
    let audio_path_str = audio_path
        .ok_or_else(|| "No audio was recorded".to_string())?
        .to_string_lossy()
        .to_string();

    // Process the audio with GPT-4o-mini
    let transcription = process_audio_with_gpt4o(&audio_path_str)
        .await
        .map_err(|e| e.to_string())?;

    // Return the result
    Ok(AudioProcessingResult {
        transcription: transcription.clone(),
        openai_response: transcription, // For now, we're using the same text for both
    })
}

// Tauri command to check if we're currently recording
#[tauri::command]
pub fn is_recording(app_handle: tauri::AppHandle) -> bool {
    let recorder = app_handle
        .state::<Arc<Mutex<AudioRecorder>>>()
        .inner()
        .lock()
        .unwrap();
    recorder.is_recording()
}

// Tauri command to list audio input devices
#[tauri::command]
pub fn get_audio_input_devices() -> Result<Vec<AudioDevice>, String> {
    list_audio_input_devices().map_err(|e| e.to_string())
}
