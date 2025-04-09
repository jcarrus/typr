use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use hound::{WavSpec, WavWriter};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_plugin_store::StoreExt;
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
    pub transcription: String,
    pub openai_response: String,
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
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &_| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            // Convert f32 to i16
                            let sample_i16 = (sample * 32767.0) as i16;
                            writer.write_sample(sample_i16).unwrap_or_else(|e| {
                                error!("Error writing sample: {}", e);
                            });
                        }
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &_| {
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            // Convert u16 to i16
                            let sample_i16 = (sample as i32 - 32768) as i16;
                            writer.write_sample(sample_i16).unwrap_or_else(|e| {
                                error!("Error writing sample: {}", e);
                            });
                        }
                    }
                },
                err_fn,
                None,
            ),
            _ => {
                return Err(format!(
                    "Unsupported sample format: {:?}",
                    config.sample_format()
                ))
            }
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
pub async fn process_audio_with_gpt4o(
    audio_path: &str,
    app_handle: &tauri::AppHandle,
) -> Result<String> {
    info!("Processing audio with GPT-4o-mini: {}", audio_path);

    // Read the audio file
    let audio_data = fs::read(audio_path)?;
    info!("Read audio file: {} bytes", audio_data.len());

    // Get the OpenAI API key from the store
    let api_key = match get_openai_api_key_from_store(app_handle.clone()).await {
        Ok(key) => key,
        Err(e) => {
            error!("Failed to get OpenAI API key: {}", e);
            return Err(anyhow::anyhow!(
                "OpenAI API key not found. Please add your API key in the settings."
            ));
        }
    };
    info!("Retrieved OpenAI API key");

    // Get custom vocabulary and instructions from the store
    let (custom_vocabulary, _custom_instructions) =
        match get_custom_settings_from_store(app_handle.clone()).await {
            Ok(settings) => settings,
            Err(e) => {
                error!("Failed to get custom settings: {}", e);
                (String::new(), String::new())
            }
        };
    info!("Retrieved custom settings");

    // Create a multipart form for the request
    let client = reqwest::Client::new();

    // Create a temporary file for the request
    let temp_file = tempfile::NamedTempFile::new()?;
    fs::write(temp_file.path(), &audio_data)?;
    info!(
        "Created temporary file for API request: {:?}",
        temp_file.path()
    );

    // Build the prompt with custom vocabulary and instructions
    let mut prompt = String::from("Here is a dictation of spoken text.");

    // Add custom vocabulary if available
    if !custom_vocabulary.is_empty() {
        let vocabulary_list: Vec<&str> = custom_vocabulary
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        if !vocabulary_list.is_empty() {
            prompt.push_str("\n\nCommon words and phrases to recognize: ");
            prompt.push_str(&vocabulary_list.join(", "));
        }
    }

    info!("Using whisper prompt: {}", prompt);

    // Create the form
    let file_bytes = fs::read(temp_file.path())?;
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "text")
        .text("language", "en")
        .text("temperature", "0.2")
        .text("prompt", prompt)
        .part(
            "file",
            reqwest::multipart::Part::bytes(file_bytes)
                .file_name("audio.wav")
                .mime_str("audio/wav")?,
        );

    info!("Sending request to OpenAI API...");

    // Send the request
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await?;

    info!("Received response from OpenAI API: {:?}", response.status());

    // Check if the request was successful
    if !response.status().is_success() {
        let error_text = response.text().await?;
        error!("OpenAI API error: {}", error_text);
        return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
    }

    // Get the transcription
    let transcription = response.text().await?;
    info!("Transcription completed: {}", transcription);
    Ok(transcription)
}

// Function to process transcription with GPT-4o-mini
pub async fn process_transcription_with_gpt4o(
    transcription: &str,
    app_handle: &tauri::AppHandle,
) -> Result<String> {
    info!("Processing transcription with GPT-4o-mini");

    // Get the OpenAI API key from the store
    let api_key = match get_openai_api_key_from_store(app_handle.clone()).await {
        Ok(key) => key,
        Err(e) => {
            error!("Failed to get OpenAI API key: {}", e);
            return Err(anyhow::anyhow!(
                "OpenAI API key not found. Please add your API key in the settings."
            ));
        }
    };
    info!("Retrieved OpenAI API key");

    // Get custom vocabulary and instructions from the store
    let (_custom_vocabulary, custom_instructions) =
        match get_custom_settings_from_store(app_handle.clone()).await {
            Ok(settings) => settings,
            Err(e) => {
                error!("Failed to get custom settings: {}", e);
                (String::new(), String::new())
            }
        };
    info!("Retrieved custom settings");

    // Build the prompt with custom vocabulary and instructions
    let mut prompt = String::from("You are a helpful assistant that processes dictation transcriptions. Respond with a copyedited version of the transcription. If there is a 'note to the editor' in the transcription, follow it. Otherwise, just fix any grammatical errors.");

    // Add custom instructions if available
    if !custom_instructions.is_empty() {
        prompt.push_str("\n\nAdditional notes to consider while editing: ");
        prompt.push_str(&custom_instructions);
    }

    info!("Using prompt for GPT-4o-mini: {}", prompt);

    // Create the client
    let client = reqwest::Client::new();

    // Create the request body
    let request_body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {
                "role": "system",
                "content": prompt
            },
            {
                "role": "user",
                "content": transcription
            }
        ],
        "temperature": 0.2
    });

    info!("Sending request to OpenAI API for GPT-4o-mini...");

    // Send the request
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    info!(
        "Received response from OpenAI API for GPT-4o-mini: {:?}",
        response.status()
    );

    // Check if the request was successful
    if !response.status().is_success() {
        let error_text = response.text().await?;
        error!("OpenAI API error: {}", error_text);
        return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
    }

    // Parse the response
    let response_json: serde_json::Value = response.json().await?;

    // Extract the content from the response
    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to extract content from response"))?;

    info!("GPT-4o-mini processing completed: {}", content);
    Ok(content.to_string())
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

    // Process the audio with Whisper
    let transcription = match process_audio_with_gpt4o(&audio_path_str, &app_handle).await {
        Ok(text) => text,
        Err(e) => {
            // Check if the error is related to the API key
            if e.to_string().contains("API key not found") {
                return Err(
                    "OpenAI API key not found. Please add your API key in the settings."
                        .to_string(),
                );
            }
            return Err(format!("Failed to process audio: {}", e));
        }
    };

    // Process the transcription with GPT-4o-mini
    let processed_text = match process_transcription_with_gpt4o(&transcription, &app_handle).await {
        Ok(text) => text,
        Err(e) => {
            error!("Failed to process transcription with GPT-4o-mini: {}", e);
            // If GPT-4o-mini processing fails, use the original transcription
            transcription.clone()
        }
    };

    // Return the result
    Ok(AudioProcessingResult {
        transcription: transcription,
        openai_response: processed_text,
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

// Tauri command to get the OpenAI API key from the store
#[tauri::command]
pub async fn get_openai_api_key_from_store(app_handle: tauri::AppHandle) -> Result<String, String> {
    // Get the store from the app handle using the StoreExt trait
    let store = app_handle
        .store(".settings.dat")
        .map_err(|e| format!("Failed to load store: {}", e))?;

    // Get the API key from the store
    let api_key = store
        .get("openAIKey")
        .ok_or_else(|| "OpenAI API key not found in store".to_string())?;

    // Convert the serde_json::Value to a String
    let api_key_str = api_key
        .as_str()
        .ok_or_else(|| "API key is not a string".to_string())?
        .to_string();

    if api_key_str.is_empty() {
        return Err("OpenAI API key is empty".to_string());
    }

    Ok(api_key_str)
}

// Tauri command to get custom vocabulary and instructions from the store
#[tauri::command]
pub async fn get_custom_settings_from_store(
    app_handle: tauri::AppHandle,
) -> Result<(String, String), String> {
    // Get the store from the app handle using the StoreExt trait
    let store = app_handle
        .store(".settings.dat")
        .map_err(|e| format!("Failed to load store: {}", e))?;

    // Get the custom vocabulary from the store
    let custom_vocabulary = store
        .get("customVocabulary")
        .map(|v| v.as_str().unwrap_or("").to_string())
        .unwrap_or_default();

    // Get the custom instructions from the store
    let custom_instructions = store
        .get("customInstructions")
        .map(|v| v.as_str().unwrap_or("").to_string())
        .unwrap_or_default();

    Ok((custom_vocabulary, custom_instructions))
}
