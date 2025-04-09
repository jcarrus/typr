use anyhow::{Context, Result};
use log::{error, info};
use std::process::Command;

#[cfg(target_os = "macos")]
fn type_text_impl(text: &str) -> Result<()> {
    info!("Typing text on macOS");

    // First, copy the text to clipboard
    let mut copy_cmd = Command::new("pbcopy");
    copy_cmd.stdin(std::process::Stdio::piped());
    let mut copy_process = copy_cmd.spawn().context("Failed to start pbcopy process")?;

    if let Some(mut stdin) = copy_process.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .context("Failed to write to pbcopy stdin")?;
    }

    copy_process
        .wait()
        .context("Failed to wait for pbcopy process")?;

    // Then paste the text using AppleScript
    let paste_script = r#"tell application "System Events" to keystroke "v" using command down"#;
    Command::new("osascript")
        .args(["-e", paste_script])
        .output()
        .context("Failed to execute paste command")?;

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
