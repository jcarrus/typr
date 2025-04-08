use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use hound::{WavSpec, WavWriter};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tempfile::NamedTempFile;

// Struct to hold the state of our audio recording
pub struct AudioRecorder {
    is_recording: Arc<Mutex<bool>>,
    temp_file: Arc<Mutex<Option<NamedTempFile>>>,
    wav_writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>,
    stream: Arc<Mutex<Option<cpal::Stream>>>,
}

// Implement Send and Sync for AudioRecorder
unsafe impl Send for AudioRecorder {}
unsafe impl Sync for AudioRecorder {}

impl AudioRecorder {
    // Create a new AudioRecorder instance
    pub fn new() -> Self {
        AudioRecorder {
            is_recording: Arc::new(Mutex::new(false)),
            temp_file: Arc::new(Mutex::new(None)),
            wav_writer: Arc::new(Mutex::new(None)),
            stream: Arc::new(Mutex::new(None)),
        }
    }

    // Start recording audio
    pub fn start_recording(&self) -> Result<()> {
        {
            let is_recording = self.is_recording.lock().unwrap();
            if *is_recording {
                return Ok(());
            }
        }

        // Create a temporary file for the WAV data
        let temp_file = NamedTempFile::new().context("Failed to create temporary file")?;
        let temp_file_path = temp_file.path().to_path_buf();

        // Create a WAV writer with the appropriate format
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let writer =
            WavWriter::create(&temp_file_path, spec).context("Failed to create WAV writer")?;

        // Store the temp file and writer in our state
        *self.temp_file.lock().unwrap() = Some(temp_file);
        *self.wav_writer.lock().unwrap() = Some(writer);

        // Get the default audio input device
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device available"))?;

        // Get the default input config
        let config = device.default_input_config()?;

        // Create a clone of the wav_writer for the callback
        let wav_writer = Arc::clone(&self.wav_writer);
        let is_recording = Arc::clone(&self.is_recording);

        // Create the audio stream based on the sample format
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut writer = wav_writer.lock().unwrap();
                        if let Some(writer) = writer.as_mut() {
                            for &sample in data {
                                // Convert f32 to i16 and write to WAV
                                let sample_i16 = (sample * 32767.0) as i16;
                                writer.write_sample(sample_i16).unwrap();
                            }
                        }
                    },
                    move |err| {
                        error!("Error in audio stream: {}", err);
                        *is_recording.lock().unwrap() = false;
                    },
                    None,
                )?
            }
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut writer = wav_writer.lock().unwrap();
                    if let Some(writer) = writer.as_mut() {
                        for &sample in data {
                            writer.write_sample(sample).unwrap();
                        }
                    }
                },
                move |err| {
                    error!("Error in audio stream: {}", err);
                    *is_recording.lock().unwrap() = false;
                },
                None,
            )?,
            SampleFormat::U16 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let mut writer = wav_writer.lock().unwrap();
                        if let Some(writer) = writer.as_mut() {
                            for &sample in data {
                                // Convert u16 to i16 and write to WAV
                                let sample_i16 = (sample as i32 - 32768) as i16;
                                writer.write_sample(sample_i16).unwrap();
                            }
                        }
                    },
                    move |err| {
                        error!("Error in audio stream: {}", err);
                        *is_recording.lock().unwrap() = false;
                    },
                    None,
                )?
            }
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        // Play the stream
        stream.play()?;

        // Store the stream in our state
        *self.stream.lock().unwrap() = Some(stream);

        // Start recording
        {
            let mut is_recording = self.is_recording.lock().unwrap();
            *is_recording = true;
        }

        info!("Started recording audio");
        Ok(())
    }

    // Stop recording and return the path to the recorded file
    pub fn stop_recording(&self) -> Result<Option<PathBuf>> {
        let mut is_recording = self.is_recording.lock().unwrap();
        if !*is_recording {
            return Ok(None);
        }

        // Stop recording
        *is_recording = false;

        // Drop the stream to stop recording
        *self.stream.lock().unwrap() = None;

        // Finalize the WAV writer
        let mut wav_writer_guard = self.wav_writer.lock().unwrap();
        if let Some(writer) = wav_writer_guard.take() {
            writer.finalize()?;
        }

        // Get the temp file
        let mut temp_file_guard = self.temp_file.lock().unwrap();
        let temp_file = temp_file_guard.take();

        if let Some(temp_file) = temp_file {
            // Get the path to the temp file
            let path = temp_file.path().to_path_buf();
            info!("Stopped recording audio, saved to {:?}", path);
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    // Check if we're currently recording
    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}

// Struct to represent the result of processing audio
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioProcessingResult {
    pub transcription: String,
    pub openai_response: String,
}

// Struct to represent an audio device
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

// Function to process audio with Whisper (placeholder for now)
pub fn process_audio_with_whisper(audio_path: &PathBuf) -> Result<String> {
    // This is a placeholder - in a real implementation, we would use whisper-rs or similar
    info!("Processing audio with Whisper: {:?}", audio_path);

    // For now, just return a dummy transcription
    Ok("This is a dummy transcription of the audio.".to_string())
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
pub fn start_recording(app: tauri::AppHandle) -> Result<(), String> {
    let recorder = app.state::<AudioRecorder>();
    recorder.start_recording().map_err(|e| e.to_string())
}

// Tauri command to stop recording and process the audio
#[tauri::command]
pub fn stop_recording_and_process(app: tauri::AppHandle) -> Result<AudioProcessingResult, String> {
    let recorder = app.state::<AudioRecorder>();
    // Stop recording and get the audio file path
    let audio_path = recorder
        .stop_recording()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No audio was recorded".to_string())?;

    // Process the audio with Whisper
    let transcription = process_audio_with_whisper(&audio_path).map_err(|e| e.to_string())?;

    // Process the transcription with OpenAI
    let openai_response = process_with_openai(&transcription).map_err(|e| e.to_string())?;

    // Return the result
    Ok(AudioProcessingResult {
        transcription,
        openai_response,
    })
}

// Tauri command to check if we're currently recording
#[tauri::command]
pub fn is_recording(app: tauri::AppHandle) -> bool {
    let recorder = app.state::<AudioRecorder>();
    recorder.is_recording()
}

// Tauri command to list audio input devices
#[tauri::command]
pub fn get_audio_input_devices() -> Result<Vec<AudioDevice>, String> {
    list_audio_input_devices().map_err(|e| e.to_string())
}
