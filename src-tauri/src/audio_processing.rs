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
use tempfile::NamedTempFile;

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
}

// Process audio file with Whisper and GPT-4o
pub async fn process_audio_file(
    audio_path: &str,
    api_key: &str,
    custom_vocabulary: &str,
    custom_instructions: &str,
) -> Result<AudioProcessingResult> {
    info!("Processing audio with GPT-4o-mini: {}", audio_path);

    // Read the audio file
    let audio_data = fs::read(audio_path)?;
    info!("Read audio file: {} bytes", audio_data.len());

    // Build the prompt with custom vocabulary
    let mut prompt = String::from("A user is dictating text to be typed into a computer program. Transcribe the text as accurately as possible.");

    // Add custom vocabulary if available
    if !custom_vocabulary.is_empty() {
        let vocabulary_list: Vec<&str> = custom_vocabulary
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        if !vocabulary_list.is_empty() {
            prompt.push_str("\n\nThe user may use the following uncommon words and phrases: ");
            prompt.push_str(&vocabulary_list.join(", "));
            prompt.push_str("\n\nTranscription:\n");
        }
    }

    info!("Using whisper prompt: {}", prompt);

    // Create the form for transcription
    let client = reqwest::Client::new();
    let temp_file = tempfile::NamedTempFile::new()?;
    fs::write(temp_file.path(), &audio_data)?;

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

    // Send the transcription request
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await?;

    // Check if the request was successful
    if !response.status().is_success() {
        let error_text = response.text().await?;
        error!("OpenAI API error: {}", error_text);
        return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
    }

    // Get the transcription
    let transcription = response.text().await?;
    info!("Transcription completed: {}", transcription);

    // Check if "note to the editor" is in the transcription
    let should_process_with_gpt = transcription.to_lowercase().contains("note to the editor");

    let openai_response = if should_process_with_gpt {
        info!("Processing with GPT-4o-mini as 'note to the editor' was found");

        // Process with GPT-4o-mini
        let mut prompt = String::from("You are a helpful assistant that processes dictation transcriptions. Respond with a copyedited version of the transcription. If there is a 'note to the editor' in the transcription, follow it. Otherwise, just fix any grammatical errors. If the transcription is asking a question, but does not EXPLICITLY ask 'the editor' to respond, then do not respond and just transcribe the question.");

        // Add custom instructions if available
        if !custom_instructions.is_empty() {
            prompt.push_str("\n\nAdditional notes to consider while editing: ");
            prompt.push_str(custom_instructions);
        }

        // Create the request body for GPT processing
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

        // Send the GPT request
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

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

        content.to_string()
    } else {
        info!("Skipping GPT-4o-mini processing as 'note to the editor' was not found");
        // Return the transcription as is
        transcription.clone()
    };

    // Return the result
    Ok(AudioProcessingResult {
        transcription: transcription,
        openai_response: openai_response,
    })
}
