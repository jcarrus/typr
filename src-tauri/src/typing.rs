use anyhow::Result;
use enigo::{Enigo, Keyboard, Settings};
use log::info;

// Type text using the enigo crate
pub async fn type_text(text: String) -> Result<(), String> {
    info!("Typing text: {} characters", text.len());
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let _ = enigo.text(text.as_str());
    Ok(())
}
