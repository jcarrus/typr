use anyhow::Result;
use tauri_plugin_store::StoreExt;

// Get the OpenAI API key from the store
pub async fn get_openai_api_key_from_store(app_handle: tauri::AppHandle) -> Result<String, String> {
    // Get the store from the app handle
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

// Get custom vocabulary and instructions from the store
pub async fn get_custom_settings_from_store(
    app_handle: tauri::AppHandle,
) -> Result<(String, String), String> {
    // Get the store from the app handle
    let store = app_handle
        .store(".settings.dat")
        .map_err(|e| format!("Failed to load store: {}", e))?;

    // Get the custom vocabulary from the store
    let whisper_prompt = store
        .get("whisperPrompt")
        .map(|v| v.as_str().unwrap_or("").to_string())
        .unwrap_or_default();

    // Get the custom instructions from the store
    let llm_prompt = store
        .get("llmPrompt")
        .map(|v| v.as_str().unwrap_or("").to_string())
        .unwrap_or_default();

    Ok((whisper_prompt, llm_prompt))
}
