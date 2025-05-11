use anyhow::Result;
use chrono::Local;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat};
use hound::{WavSpec, WavWriter};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
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
    writer: Option<Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>>,
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
            writer: None,
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
        let file_path = output_dir.join(format!("recording_{}.wav", timestamp));
        let file = File::create(&file_path).map_err(|e| e.to_string())?;

        self.current_file = Some(file_path.clone());

        // Create WAV writer with proper spec
        let spec = wav_spec_from_config(&config);
        let writer = WavWriter::new(BufWriter::new(file), spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        let writer = Arc::new(Mutex::new(Some(writer)));
        let writer_clone = writer.clone();

        let err_fn = move |err| {
            error!("An error occurred on stream: {}", err);
        };

        let stream = match config.sample_format() {
            SampleFormat::I8 => device.build_input_stream(
                &config.into(),
                move |data, _: &_| write_input_data::<i8, i8>(data, &writer_clone),
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data, _: &_| write_input_data::<i16, i16>(data, &writer_clone),
                err_fn,
                None,
            ),
            SampleFormat::I32 => device.build_input_stream(
                &config.into(),
                move |data, _: &_| write_input_data::<i32, i32>(data, &writer_clone),
                err_fn,
                None,
            ),
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data, _: &_| write_input_data::<f32, f32>(data, &writer_clone),
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

        // Finalize the WAV file
        if let Some(writer) = self.writer.take() {
            if let Ok(mut guard) = writer.lock() {
                if let Some(writer) = guard.take() {
                    writer.finalize().map_err(|e| e.to_string())?;
                }
            }
        }

        Ok(self.current_file.take())
    }
}

fn sample_format(format: SampleFormat) -> hound::SampleFormat {
    if format.is_float() {
        hound::SampleFormat::Float
    } else {
        hound::SampleFormat::Int
    }
}

fn wav_spec_from_config(config: &cpal::SupportedStreamConfig) -> WavSpec {
    WavSpec {
        channels: config.channels() as _,
        sample_rate: config.sample_rate().0 as _,
        bits_per_sample: (config.sample_format().sample_size() * 8) as _,
        sample_format: sample_format(config.sample_format()),
    }
}

type WavWriterHandle = Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>;

fn write_input_data<T, U>(input: &[T], writer: &WavWriterHandle)
where
    T: Sample,
    U: Sample + hound::Sample + FromSample<T>,
{
    if let Ok(mut guard) = writer.try_lock() {
        if let Some(writer) = guard.as_mut() {
            for &sample in input.iter() {
                let sample: U = U::from_sample(sample);
                writer.write_sample(sample).ok();
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Replacement {
    from: String,
    to: String,
}

// Process audio file with Whisper and GPT-4o
pub async fn process_audio_file(
    audio_path: &str,
    api_key: &str,
    whisper_prompt: &str,
    llm_prompt: &str,
) -> Result<AudioProcessingResult> {
    info!("Processing audio with whisper: {}", audio_path);

    // Define replacements
    let replacements = vec![Replacement {
        from: "slap".to_string(),
        to: "\n".to_string(),
    }];

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
    let mut transcription = response.text().await?;
    info!("Transcription completed: {}", transcription);

    // Apply replacements
    let should_perform_substitutions = replacements.len() > 0;
    transcription = if should_perform_substitutions {
        info!("Processing replacements with GPT-4o-mini");

        // Create the request body for GPT processing
        let request_body = serde_json::json!({
            "model": "gpt-4.1-mini",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a careful copyeditor who is copyediting a rough voice transcription output. Your task is to read the transcription and return a copyedited version. Keep the same language and tone as the original transcription. Only make changes to the punctuation, duplicate words, incorrect words, etc. Additionally look out for any of the keyword substitutions and make the necessary replacements in the output. If the keyword appears more than once, then include an equal number of replacements in the output."
                },
                {
                    "role": "user",
                    "content": format!("Keyword substitutions (when the user uses the keyword, they are really meaning to add the replacement text): {}\n\nTranscription: {}", replacements.iter().map(|r| format!("'{}' -> '{}'", r.from, r.to)).collect::<Vec<String>>().join("\n"), transcription)
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

        content.trim().to_string()
    } else {
        info!("Skipping GPT-4o-mini processing as no replacements were found");
        // Return the transcription as is
        transcription.trim().to_string()
    };

    // Clean up whitespace
    transcription = transcription
        .lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n");
    transcription = transcription.replace("  ", " ");

    // Check if "note to the editor" is in the transcription
    let should_process_with_gpt = transcription.to_lowercase().contains("note to the editor");

    let openai_response = if should_process_with_gpt {
        info!("Processing with GPT-4o-mini as 'note to the editor' was found");

        // Create the request body for GPT processing
        let request_body = serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [
                // {
                //     "role": "system",
                //     "content": ""
                // },
                {
                    "role": "user",
                    "content": format!("Task: {}\n\nTranscription: {}", llm_prompt, transcription)
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

        content.trim().to_string()
    } else {
        info!("Skipping GPT-4o-mini processing as 'note to the editor' was not found");
        // Return the transcription as is
        transcription.trim().to_string()
    };

    // Return the result
    Ok(AudioProcessingResult {
        transcription: transcription,
        openai_response: openai_response,
    })
}
