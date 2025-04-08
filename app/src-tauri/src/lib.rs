// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// Dummy method to activate the app
fn activate_app(app: &tauri::AppHandle) {
    println!("Activating app via global shortcut");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(
            |app_handle, argv, cwd| {
                println!("app already running: {argv:?}, {cwd}");
                // Try to get any existing window and bring it to front
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_focus();
                } else {
                    // If the main window isn't found, try to find any window
                    // This is a simple approach since Tauri 2.0 doesn't have direct access to all windows
                    println!("Main window not found");
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
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        println!("quit menu item was clicked");
                        app.exit(0);
                    }
                    _ => {
                        println!("menu item {:?} not handled", event.id);
                    }
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        println!("left click pressed and released");
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
                        println!("right click pressed and released");
                        // Right click will show the menu (this is the default behavior)
                    }
                    _ => {
                        println!("unhandled event {event:?}");
                    }
                })
                .build(app)?;

            // Register global shortcut for Cmd+Shift+Space
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{
                    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState, ShortcutWrapper,
                };

                let app_handle = app.handle();
                let cmd_shift_space_shortcut =
                    Shortcut::new(Some(Modifiers::SHIFT | Modifiers::SUPER), Code::Space);
                let shortcut_wrapper = ShortcutWrapper::from(cmd_shift_space_shortcut.clone());

                // Initialize the global shortcut plugin
                app_handle.plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |app, shortcut, event| {
                            println!("in handler");
                            if shortcut == &cmd_shift_space_shortcut {
                                match event.state() {
                                    ShortcutState::Pressed => {
                                        println!("Cmd+Shift+Space Pressed!");
                                        activate_app(app);
                                    }
                                    ShortcutState::Released => {
                                        println!("Cmd+Shift+Space Released!");
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
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
