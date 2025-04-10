use tauri::{
    image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

mod audio_processing;
mod store;
mod typing;

use audio_processing::{AudioProcessingResult, AudioRecorder};
use log::{error, info};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use typing::type_text;

// Structure to track the recording state
struct RecordingState {
    is_recording: bool,
    recorder: Option<AudioRecorder>,
}

impl RecordingState {
    fn new() -> Self {
        Self {
            is_recording: false,
            recorder: None,
        }
    }
}

// Function to start recording
async fn start_recording_audio(app_handle: &tauri::AppHandle) -> Result<(), String> {
    info!("Starting recording");

    // Get the recording state
    let mut state = app_handle
        .state::<Arc<Mutex<RecordingState>>>()
        .inner()
        .lock()
        .unwrap();

    // Check if already recording
    if state.is_recording {
        return Err("Already recording".to_string());
    }

    // Create a new recorder
    let mut recorder = AudioRecorder::new();

    // Start recording
    if let Err(e) = recorder.start_recording() {
        return Err(format!("Failed to start recording: {}", e));
    }

    // Update state
    state.is_recording = true;
    state.recorder = Some(recorder);

    Ok(())
}

// Function to stop recording
async fn stop_recording_audio(app_handle: &tauri::AppHandle) -> Result<Option<PathBuf>, String> {
    info!("Stopping recording");

    // Get the recording state
    let mut state = app_handle
        .state::<Arc<Mutex<RecordingState>>>()
        .inner()
        .lock()
        .unwrap();

    // Check if recording
    if !state.is_recording {
        return Ok(None);
    }

    // Get the recorder
    let mut recorder = match state.recorder.take() {
        Some(recorder) => recorder,
        None => return Err("No recorder found".to_string()),
    };

    // Update state
    state.is_recording = false;

    // Stop recording
    recorder.stop_recording()
}

// Function to process audio
async fn process_audio_file(
    app_handle: &tauri::AppHandle,
    audio_path: &str,
) -> Result<AudioProcessingResult, String> {
    info!("Processing audio file: {}", audio_path);

    // Get the OpenAI API key
    let api_key = match store::get_openai_api_key_from_store(app_handle.clone()).await {
        Ok(key) => key,
        Err(e) => {
            error!("Failed to get OpenAI API key: {}", e);
            return Err(
                "OpenAI API key not found. Please add your API key in the settings.".to_string(),
            );
        }
    };

    // Get custom settings
    let (custom_vocabulary, custom_instructions) =
        match store::get_custom_settings_from_store(app_handle.clone()).await {
            Ok(settings) => settings,
            Err(e) => {
                error!("Failed to get custom settings: {}", e);
                (String::new(), String::new())
            }
        };

    // Process the audio file
    audio_processing::process_audio_file(
        audio_path,
        &api_key,
        &custom_vocabulary,
        &custom_instructions,
    )
    .await
    .map_err(|e| format!("Failed to process audio: {}", e))
}

// Check if recording is active
#[tauri::command]
fn is_recording(app_handle: tauri::AppHandle) -> bool {
    let state = app_handle
        .state::<Arc<Mutex<RecordingState>>>()
        .inner()
        .lock()
        .unwrap();
    state.is_recording
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(
            |app_handle, argv, cwd| {
                info!("app already running: {argv:?}, {cwd}");
                // Try to get any existing window and bring it to front
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_focus();
                } else {
                    // If the main window isn't found, try to find any window
                    // This is a simple approach since Tauri 2.0 doesn't have direct access to all windows
                    info!("Main window not found");
                }
            },
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]), // Optional command line arguments when app starts
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Create a quit menu item
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_i])?;

            // Create the tray icon with menu
            let tray = TrayIconBuilder::new()
                .icon(image::Image::from_bytes(include_bytes!(
                    "../icons/icon-Template.png"
                ))?)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(false) // Don't show menu on left click
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        info!("Quitting...");
                        app.exit(0);
                    }
                    _ => {
                        info!("menu item {:?} not handled", event.id);
                    }
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        info!("left click pressed and released");
                        // Show and focus the main window when the tray is left-clicked
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    TrayIconEvent::Click {
                        button: MouseButton::Right,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        info!("right click pressed and released");
                        // Right click will show the menu (this is the default behavior)
                    }
                    _ => {
                        // info!("unhandled event {event:?}");
                    }
                })
                .build(app)?;

            // Register global shortcut for Cmd+Shift+Space
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{
                    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
                };

                let app_handle = app.handle();
                let cmd_shift_space_shortcut =
                    Shortcut::new(Some(Modifiers::SHIFT | Modifiers::SUPER), Code::Space);

                // Initialize the global shortcut plugin
                app_handle.plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler({
                            let tray = tray.clone();
                            move |app, shortcut, event| {
                                if shortcut == &cmd_shift_space_shortcut {
                                    match event.state() {
                                        ShortcutState::Pressed => {
                                            info!("Cmd+Shift+Space Pressed!");
                                            
                                            // Start recording audio
                                            let app_clone = app.clone();
                                            let tray = tray.clone();
                                            tauri::async_runtime::spawn(async move {
                                                if let Err(e) = start_recording_audio(&app_clone).await {
                                                    info!("Failed to start recording: {}", e);
                                                    return;
                                                }
                                                
                                                // Update tray icon to show recording
                                                let _ = tray.set_icon(
                                                    image::Image::from_bytes(include_bytes!(
                                                        "../icons/icon-recording.png"
                                                    ))
                                                    .ok(),
                                                );
                                                let _ = tray.set_icon_as_template(false);
                                            });
                                        }
                                        ShortcutState::Released => {
                                            info!("Cmd+Shift+Space Released!");
                                            
                                            // Clone for use in async task
                                            let app_clone = app.clone();
                                            let tray_clone = tray.clone();
                                            
                                            // Set to processing icon
                                            let _ = tray_clone.set_icon(
                                                image::Image::from_bytes(include_bytes!(
                                                    "../icons/icon-processing.png"
                                                ))
                                                .ok(),
                                            );
                                            let _ = tray_clone.set_icon_as_template(false);
                                            
                                            tauri::async_runtime::spawn(async move {
                                                // First stop the recording
                                                match stop_recording_audio(&app_clone).await {
                                                    Ok(Some(path)) => {
                                                        info!("Recording stopped, processing audio...");
                                                        let audio_path =
                                                            path.to_string_lossy().to_string();
                                                        
                                                        // Process the audio
                                                        match process_audio_file(
                                                            &app_clone,
                                                            &audio_path,
                                                        )
                                                        .await
                                                        {
                                                            Ok(result) => {
                                                                info!("Audio processing successful");
                                                                // Type the text
                                                                if let Err(e) =
                                                                    type_text(result.openai_response)
                                                                        .await
                                                                {
                                                                    info!("Failed to type text: {}", e);
                                                                }
                                                            }
                                                            Err(e) => {
                                                                info!("Failed to process audio: {}", e);
                                                            }
                                                        }
                                                    }
                                                    Ok(None) => {
                                                        info!("No audio was recorded");
                                                    }
                                                    Err(e) => {
                                                        info!("Failed to stop recording: {}", e);
                                                    }
                                                }
                                                
                                                // Set the icon back to normal when done
                                                let _ = tray_clone.set_icon(
                                                    image::Image::from_bytes(include_bytes!(
                                                        "../icons/icon-Template.png"
                                                    ))
                                                    .ok(),
                                                );
                                                let _ = tray_clone.set_icon_as_template(true);
                                            });
                                        }
                                    }
                                }
                            }
                        })
                        .build(),
                )?;

                // Register the shortcut
                app_handle
                    .global_shortcut()
                    .register(cmd_shift_space_shortcut)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Prevent the window from actually closing
                    api.prevent_close();
                    // Just hide the window instead
                    window.hide().unwrap();
                }
                _ => {}
            }
        })
        // Register the RecordingState
        .manage(Arc::new(Mutex::new(RecordingState::new())))
        .invoke_handler(tauri::generate_handler![is_recording,])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
