use anyhow::Result;
use tauri_plugin_store::StoreExt;

// Get all settings from the store in one call
pub async fn get_all_settings_from_store(
    app_handle: tauri::AppHandle,
) -> Result<(String, String, String, bool), String> {
    let store = app_handle
        .store(".settings.dat")
        .map_err(|e| format!("Failed to load store: {}", e))?;

    let api_key = store
        .get("openAIKey")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "".to_string());

    if api_key.is_empty() {
        return Err("OpenAI API key not found or empty".to_string());
    }

    let whisper_prompt = store
        .get("whisperPrompt")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "".to_string());

    let llm_prompt = store
        .get("llmPrompt")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "".to_string());

    let use_local_whisper = store
        .get("useLocalWhisper")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok((api_key, whisper_prompt, llm_prompt, use_local_whisper))
}
