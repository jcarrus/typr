use anyhow::Result;
use chrono::Local;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use log::{error, info};
use mp3lame_encoder::{Builder, DualPcm, FlushNoGap};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

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
    encoder: Option<Arc<Mutex<mp3lame_encoder::Encoder>>>,
    output_file: Option<Arc<Mutex<File>>>,
    current_file: Option<PathBuf>,
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
            encoder: None,
            output_file: None,
            current_file: None,
        }
    }

    // Start recording audio
    pub fn start_recording(&mut self) -> Result<(), String> {
        let mut is_recording = self.is_recording.lock().unwrap();
        if *is_recording {
            return Err("Already recording".to_string());
        }
        *is_recording = true;
        drop(is_recording);

        // Get default input device
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No input device available".to_string())?;

        info!(
            "Input device: {}",
            device.name().map_err(|e| e.to_string())?
        );

        // Get default input config
        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        info!("Default input config: {:?}", config);

        // Ensure /tmp/typr exists
        let output_dir = PathBuf::from("/tmp/typr");
        fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let file_path = output_dir.join(format!("recording_{}.mp3", timestamp));
        let file = File::create(&file_path).map_err(|e| e.to_string())?;

        self.current_file = Some(file_path.clone());

        // Create MP3 encoder
        let mut mp3_encoder = Builder::new().expect("Create LAME builder");
        mp3_encoder
            .set_num_channels(config.channels() as u8)
            .expect("set channels");
        mp3_encoder
            .set_sample_rate(config.sample_rate().0)
            .expect("set sample rate");
        mp3_encoder
            .set_quality(mp3lame_encoder::Quality::Best)
            .expect("set quality");

        let encoder = mp3_encoder.build().expect("To initialize LAME encoder");
        let encoder = Arc::new(Mutex::new(encoder));
        let output_file = Arc::new(Mutex::new(file));

        let encoder_clone = encoder.clone();
        let output_file_clone = output_file.clone();

        let err_fn = move |err| {
            error!("An error occurred on stream: {}", err);
        };

        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &_| {
                    let mut left_pcm = Vec::with_capacity(data.len() / 2);
                    let mut right_pcm = Vec::with_capacity(data.len() / 2);
                    for chunks in data.chunks_exact(2) {
                        left_pcm.push((chunks[0] * 32767.0) as i16);
                        right_pcm.push((chunks[1] * 32767.0) as i16);
                    }
                    let input = DualPcm {
                        left: &left_pcm[..],
                        right: &right_pcm[..],
                    };

                    let buffer_size = mp3lame_encoder::max_required_buffer_size(input.left.len());
                    let mut mp3_buffer = vec![MaybeUninit::uninit(); buffer_size];

                    if let Ok(mut encoder) = encoder_clone.lock() {
                        if let Ok(mut output_file) = output_file_clone.lock() {
                            if let Ok(encoded_size) = encoder.encode(input, &mut mp3_buffer) {
                                // Safety: mp3lame has initialized the first encoded_size bytes
                                let encoded_data = unsafe {
                                    std::slice::from_raw_parts(
                                        mp3_buffer.as_ptr() as *const u8,
                                        encoded_size,
                                    )
                                };
                                output_file.write_all(encoded_data).ok();
                            }
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
        self.encoder = Some(encoder);
        self.output_file = Some(output_file);

        Ok(())
    }

    // Stop recording and return the path to the recorded file
    pub fn stop_recording(&mut self) -> Result<Option<PathBuf>, String> {
        let mut is_recording = self.is_recording.lock().unwrap();
        if !*is_recording {
            info!("Call to stop_recording when not recording");
            return Ok(None);
        }
        *is_recording = false;
        drop(is_recording);

        // Stop the stream
        self.stream.take();

        // Flush the encoder and close the file
        if let (Some(encoder), Some(output_file)) = (self.encoder.take(), self.output_file.take()) {
            if let (Ok(mut encoder), Ok(mut output_file)) = (encoder.lock(), output_file.lock()) {
                let mut mp3_buffer = vec![MaybeUninit::uninit(); 7200];
                if let Ok(encoded_size) = encoder.flush::<FlushNoGap>(&mut mp3_buffer) {
                    // Safety: mp3lame has initialized the first encoded_size bytes
                    let encoded_data = unsafe {
                        std::slice::from_raw_parts(mp3_buffer.as_ptr() as *const u8, encoded_size)
                    };
                    output_file.write_all(encoded_data).ok();
                }
            }
        }

        Ok(self.current_file.take())
    }
}

// Check if Whisper is available locally
pub fn is_whisper_available() -> bool {
    Command::new("whisper").output().is_ok()
}

// Use local Whisper for transcription - simplified version
async fn transcribe_with_local_whisper(audio_path: &str, whisper_prompt: &str) -> Result<String> {
    info!("Using local Whisper for transcription: {}", audio_path);

    // Simple Whisper command - let Whisper handle the audio format
    let mut cmd = Command::new("whisper");
    cmd.args([
        audio_path,
        "--model",
        "base",
        "--language",
        "en",
        "--output_format",
        "txt",
        "--output_dir",
        "/tmp/typr",
        "--verbose",
        "False",
    ]);

    // Add prompt if provided
    if !whisper_prompt.is_empty() {
        cmd.args(["--initial_prompt", whisper_prompt]);
    }

    let output = cmd.output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Whisper failed: {}", error_msg));
    }

    // Read the output file
    let input_path_buf = PathBuf::from(audio_path);
    let input_filename = input_path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("recording");

    let output_file = format!("/tmp/typr/{}.txt", input_filename);

    let content = fs::read_to_string(&output_file)?;
    let _ = fs::remove_file(&output_file); // Clean up

    Ok(content.trim().to_string())
}

// Use OpenAI API for transcription
async fn transcribe_with_openai_api(
    audio_path: &str,
    api_key: &str,
    whisper_prompt: &str,
) -> Result<String> {
    info!("Using OpenAI API for transcription: {}", audio_path);

    // Read the audio file
    let audio_data = fs::read(audio_path)?;
    info!("Read audio file: {} bytes", audio_data.len());

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
        .text(
            "prompt",
            whisper_prompt.to_string() + " " + "\n\nTranscription:",
        )
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
    info!("OpenAI transcription completed: {}", transcription);

    Ok(transcription)
}

// Process audio file with Whisper and GPT-4o
pub async fn process_audio_file(
    audio_path: &str,
    api_key: &str,
    whisper_prompt: &str,
    llm_prompt: &str,
    use_local_whisper: bool,
) -> Result<AudioProcessingResult> {
    info!("Processing audio with whisper: {}", audio_path);

    // Get transcription - try local first if enabled, otherwise use OpenAI
    let mut transcription = if use_local_whisper && is_whisper_available() {
        info!("Using local Whisper");
        transcribe_with_local_whisper(audio_path, whisper_prompt)
            .await
            .unwrap_or_else(|e| {
                error!("Local Whisper failed, falling back to OpenAI: {}", e);
                // Return empty string to trigger OpenAI fallback
                String::new()
            })
    } else {
        String::new() // Will trigger OpenAI API
    };

    // Use OpenAI if local Whisper wasn't used or failed
    if transcription.is_empty() {
        transcription = transcribe_with_openai_api(audio_path, api_key, whisper_prompt).await?;
    }

    // Apply simple replacements
    transcription = transcription.replace("slap", "\n");

    // Clean up whitespace
    transcription = transcription
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // Process with GPT if "note to the editor" is mentioned
    let openai_response = if transcription.to_lowercase().contains("note to the editor") {
        info!("Processing with GPT-4o-mini");

        let client = reqwest::Client::new();
        let request_body = serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{
                "role": "user",
                "content": format!("Task: {}\n\nTranscription: {}", llm_prompt, transcription)
            }],
            "temperature": 0.2
        });

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if response.status().is_success() {
            let response_json: serde_json::Value = response.json().await?;
            response_json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or(&transcription)
                .trim()
                .to_string()
        } else {
            transcription.clone()
        }
    } else {
        transcription.clone()
    };

    Ok(AudioProcessingResult {
        transcription,
        openai_response,
    })
}
