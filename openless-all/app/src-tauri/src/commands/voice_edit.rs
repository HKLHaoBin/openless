use super::CoordinatorState;

/// Opens the desktop Voice Edit panel or asks Android's main WebView to show
/// its embedded equivalent. The session starts only after the panel invokes
/// `start_voice_edit_session`.
#[tauri::command]
pub fn voice_edit_window_open(
    window: tauri::Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Voice Edit can only be opened from the main window".to_string());
    }
    coord.tauri_host().show_voice_edit();
    Ok(())
}

#[tauri::command]
pub fn voice_edit_window_close(
    window: tauri::Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    if window.label() != "voice-edit" && window.label() != "main" {
        return Err("Voice Edit can only be closed from its panel".to_string());
    }
    coord.tauri_host().hide_voice_edit();
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditStartArgs {
    #[serde(default)]
    pub field_context: Option<String>,
    #[serde(default)]
    pub selection: Option<openless_core::TextSelection>,
}

/// Starts the no-insert initial dictation and captures the native target before
/// the first await. Android ignores caller-provided context and uses its
/// generation-bound accessibility capture as the source of truth.
#[tauri::command]
pub async fn start_voice_edit_session(
    coord: CoordinatorState<'_>,
    args: Option<VoiceEditStartArgs>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    let args = args.unwrap_or(VoiceEditStartArgs {
        field_context: None,
        selection: None,
    });
    coord
        .start_voice_edit_session(args.field_context, args.selection)
        .await
}

#[tauri::command]
pub async fn finalize_voice_edit_dictation(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_dictation().await
}

#[tauri::command]
pub async fn start_voice_edit_instruction(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.start_voice_edit_instruction().await
}

#[tauri::command]
pub async fn finalize_voice_edit_instruction(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_instruction().await
}

#[tauri::command]
pub async fn stop_voice_edit_instruction(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.finish_voice_edit_instruction().await
}

#[tauri::command]
pub async fn commit_voice_edit_session(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.commit_voice_edit_session().await
}

#[tauri::command]
pub async fn commit_voice_edit(
    coord: CoordinatorState<'_>,
) -> Result<openless_core::VoiceEditSnapshot, String> {
    coord.commit_voice_edit_session().await
}

#[tauri::command]
pub async fn cancel_voice_edit_session(
    coord: CoordinatorState<'_>,
) -> Result<Option<openless_core::VoiceEditSnapshot>, String> {
    coord.cancel_voice_edit_session().await
}

#[tauri::command]
pub fn get_voice_edit_state(
    coord: CoordinatorState<'_>,
) -> Option<openless_core::VoiceEditSnapshot> {
    coord.voice_edit_session_snapshot()
}

#[cfg(test)]
mod tests {
    use super::VoiceEditStartArgs;

    #[test]
    fn start_args_accept_empty_payload() {
        let args: VoiceEditStartArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(args.field_context.is_none());
        assert!(args.selection.is_none());
    }
}
