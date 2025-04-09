use anyhow::{Context, Result};
use log::{error, info};
use std::process::Command;

#[cfg(target_os = "macos")]
fn type_text_impl(text: &str) -> Result<()> {
    info!("Typing text on macOS");

    // Escape special characters for AppleScript
    let escaped_text = text
        .replace('"', r#"\""#)
        .replace('\\', r#"\\"#)
        .replace('$', r#"\$"#)
        .replace('`', r#"\`"#);

    // Use a more reliable approach with AppleScript
    // This approach uses a single AppleScript command that handles the entire text
    // rather than splitting by newlines or using clipboard operations
    let script = format!(
        r#"tell application "System Events"
            set theText to "{}"
            repeat with i from 1 to (count of characters in theText)
                set theChar to character i of theText
                keystroke theChar
            end repeat
        end tell"#,
        escaped_text
    );

    Command::new("osascript")
        .args(["-e", &script])
        .output()
        .context("Failed to execute osascript command")?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn type_text_impl(text: &str) -> Result<()> {
    info!("Typing text on Linux");
    Command::new("xdotool")
        .args(["type", text])
        .output()
        .context("Failed to execute xdotool command")?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn type_text_impl(text: &str) -> Result<()> {
    error!("Typing not supported on this platform");
    Err(anyhow::anyhow!("Typing not supported on this platform"))
}

#[tauri::command]
pub async fn type_text(text: String) -> Result<(), String> {
    info!("Received text to type: {} characters", text.len());

    type_text_impl(&text).map_err(|e| {
        error!("Failed to type text: {}", e);
        e.to_string()
    })
}
