use super::*;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPolishPreviewPayload {
    pub text: String,
    pub source_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<openless_core::SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPolishPreviewClientEvent {
    pub event: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<openless_core::SessionId>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub attempt: Option<u32>,
    #[serde(default)]
    pub epoch: Option<u64>,
    #[serde(default)]
    pub text_chars: Option<usize>,
    #[serde(default)]
    pub source_chars: Option<usize>,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
}

fn session_label(session_id: Option<openless_core::SessionId>) -> String {
    session_id
        .map(|session| session.as_uuid().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn request_label(request_id: Option<&str>) -> String {
    let label: String = request_id
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(*character, '-' | '_'))
        .take(96)
        .collect();
    if label.is_empty() {
        "-".to_string()
    } else {
        label
    }
}

fn safe_event_label(value: &str) -> &'static str {
    match value {
        "shown" => "shown",
        "hydrate_start" => "hydrate_start",
        "hydrate_retry" => "hydrate_retry",
        "hydrate_result" => "hydrate_result",
        "hydrate_discard" => "hydrate_discard",
        "confirm_click" => "confirm_click",
        "confirm_result" => "confirm_result",
        "confirm_error" => "confirm_error",
        "confirm_finally" => "confirm_finally",
        "cancel_click" => "cancel_click",
        "cancel_result" => "cancel_result",
        "cancel_error" => "cancel_error",
        "cancel_finally" => "cancel_finally",
        "hide_event" => "hide_event",
        "close_result" => "close_result",
        _ => "unknown",
    }
}

fn safe_reason_label(value: Option<&str>) -> &'static str {
    match value {
        Some("stale_epoch") => "stale_epoch",
        Some("owner_mismatch") => "owner_mismatch",
        Some("null_payload") => "null_payload",
        Some("already_dismissed") => "already_dismissed",
        Some("confirm_success") => "confirm_success",
        Some("cancel") => "cancel",
        Some("confirm_dead_session") => "confirm_dead_session",
        Some("begin_failure") => "begin_failure",
        Some("qa_window_dismiss") => "qa_window_dismiss",
        Some("ipc_error") => "ipc_error",
        Some("inactive") => "inactive",
        _ => "-",
    }
}

fn safe_outcome_label(value: Option<&str>) -> &'static str {
    match value {
        Some("ok") => "ok",
        Some("error") => "error",
        Some("ignored") => "ignored",
        Some("pending") => "pending",
        _ => "-",
    }
}

fn error_class(message: &str) -> &'static str {
    match message {
        "selection preview is no longer active" | "selection preview is not active" => "inactive",
        "selection preview is stale" => "stale_owner",
        "selectionPolishTargetUnavailable" => "target_unavailable",
        "selectionPolishSelectionChanged" => "selection_changed",
        "selectionPolishTargetChanged" => "target_changed",
        "selectionPolishInsertFailed" => "insertion_error",
        "selection session is stale" => "cancelled",
        "selection session is not awaiting confirmation" => "invalid_state",
        _ => "other",
    }
}

#[tauri::command]
pub fn log_selection_polish_preview_event(event: SelectionPolishPreviewClientEvent) {
    log::info!(
        "[selection-polish] event=client_{} request_id={} session_id={} generation={} attempt={} epoch={} text_chars={} source_chars={} elapsed_ms={} reason={} outcome={}",
        safe_event_label(&event.event),
        request_label(event.request_id.as_deref()),
        session_label(event.session_id),
        event.generation.map_or_else(|| "-".to_string(), |value| value.to_string()),
        event.attempt.map_or_else(|| "-".to_string(), |value| value.to_string()),
        event.epoch.map_or_else(|| "-".to_string(), |value| value.to_string()),
        event.text_chars.map_or_else(|| "-".to_string(), |value| value.to_string()),
        event.source_chars.map_or_else(|| "-".to_string(), |value| value.to_string()),
        event.elapsed_ms.map_or_else(|| "-".to_string(), |value| value.to_string()),
        safe_reason_label(event.reason.as_deref()),
        safe_outcome_label(event.outcome.as_deref()),
    );
}

fn map_selection_preview(
    snapshot: openless_core::SelectionSnapshot,
    owner: Option<crate::SelectionPolishPreviewOwner>,
) -> Option<SelectionPolishPreviewPayload> {
    let (text, source_text) = match (snapshot.preview_text, snapshot.source_text) {
        (Some(text), Some(source_text)) => (text, source_text),
        _ => return None,
    };
    if let Some(owner) = owner {
        if owner
            .session_id
            .is_some_and(|session_id| snapshot.session_id != Some(session_id))
        {
            return None;
        }
        Some(SelectionPolishPreviewPayload {
            text,
            source_text,
            session_id: owner.session_id,
            generation: Some(owner.generation),
        })
    } else {
        Some(SelectionPolishPreviewPayload {
            text,
            source_text,
            session_id: None,
            generation: None,
        })
    }
}

fn owner_matches(
    owner: crate::SelectionPolishPreviewOwner,
    session_id: Option<openless_core::SessionId>,
    generation: Option<u64>,
) -> bool {
    session_id.map_or(true, |session| owner.session_id == Some(session))
        && generation.map_or(true, |value| owner.generation == value)
}

#[tauri::command]
pub async fn get_selection_polish_preview(
    core: CoreState<'_>,
    generation: Option<u64>,
    request_id: Option<String>,
) -> Result<Option<SelectionPolishPreviewPayload>, String> {
    let started = std::time::Instant::now();
    let request_id = request_label(request_id.as_deref());
    log::info!(
        "[selection-polish] event=ipc_get_start request_id={} generation={}",
        request_id,
        generation.map_or_else(|| "-".to_string(), |value| value.to_string()),
    );
    let Some(owner) = crate::selection_polish_preview_owner() else {
        log::info!(
            "[selection-polish] event=ipc_get_result request_id={} outcome=owner_missing elapsed_ms={}",
            request_id,
            started.elapsed().as_millis(),
        );
        return Ok(None);
    };
    if generation.is_some_and(|value| owner.generation != value) {
        log::info!(
            "[selection-polish] event=ipc_get_result request_id={} outcome=generation_mismatch generation={} owner_generation={} elapsed_ms={}",
            request_id,
            generation.map_or_else(|| "-".to_string(), |value| value.to_string()),
            owner.generation,
            started.elapsed().as_millis(),
        );
        return Ok(None);
    }
    let snapshot = match core.services().selection.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::info!(
                "[selection-polish] event=ipc_get_error request_id={} failure=other elapsed_ms={}",
                request_id,
                started.elapsed().as_millis(),
            );
            return Err(error.message);
        }
    };
    let payload = map_selection_preview(snapshot, Some(owner));
    log::info!(
        "[selection-polish] event=ipc_get_result request_id={} outcome={} session_id={} generation={} elapsed_ms={}",
        request_id,
        if payload.is_some() { "payload" } else { "null_payload" },
        session_label(owner.session_id),
        owner.generation,
        started.elapsed().as_millis(),
    );
    Ok(payload)
}

#[tauri::command]
pub async fn confirm_selection_polish_preview(
    core: CoreState<'_>,
    text: String,
    session_id: Option<openless_core::SessionId>,
    generation: Option<u64>,
    request_id: Option<String>,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let request_id = request_label(request_id.as_deref());
    log::info!(
        "[selection-polish] event=ipc_confirm_start request_id={} session_id={} generation={} text_chars={}",
        request_id,
        session_label(session_id),
        generation.map_or_else(|| "-".to_string(), |value| value.to_string()),
        text.chars().count(),
    );
    let owner = match crate::selection_polish_preview_owner() {
        Some(owner) => owner,
        None => {
            log::info!(
                "[selection-polish] event=ipc_confirm_error request_id={} failure=inactive elapsed_ms={}",
                request_id,
                started.elapsed().as_millis(),
            );
            return Err("selection preview is no longer active".to_string());
        }
    };
    if !owner_matches(owner, session_id, generation) {
        log::info!(
            "[selection-polish] event=ipc_confirm_error request_id={} failure=stale_owner owner_session_id={} owner_generation={} elapsed_ms={}",
            request_id,
            session_label(owner.session_id),
            owner.generation,
            started.elapsed().as_millis(),
        );
        return Err("selection preview is stale".to_string());
    }
    let snapshot = match core.services().selection.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::info!(
                "[selection-polish] event=ipc_confirm_error request_id={} failure=other elapsed_ms={}",
                request_id,
                started.elapsed().as_millis(),
            );
            return Err(error.message);
        }
    };
    let session_id = match snapshot.session_id {
        Some(session_id) => session_id,
        None => {
            log::info!(
                "[selection-polish] event=ipc_confirm_error request_id={} failure=inactive elapsed_ms={}",
                request_id,
                started.elapsed().as_millis(),
            );
            return Err("selection preview is not active".to_string());
        }
    };
    if owner
        .session_id
        .is_some_and(|expected| expected != session_id)
    {
        log::info!(
            "[selection-polish] event=ipc_confirm_error request_id={} failure=stale_owner snapshot_session_id={} owner_session_id={} elapsed_ms={}",
            request_id,
            session_label(Some(session_id)),
            session_label(owner.session_id),
            started.elapsed().as_millis(),
        );
        return Err("selection preview is stale".to_string());
    }
    let result = core
        .services()
        .selection
        .confirm(session_id, Some(text))
        .await
        .map_err(|error| error.message);
    match &result {
        Ok(()) => log::info!(
            "[selection-polish] event=ipc_confirm_result request_id={} outcome=ok session_id={} generation={} elapsed_ms={}",
            request_id,
            session_label(Some(session_id)),
            owner.generation,
            started.elapsed().as_millis(),
        ),
        Err(error) => log::info!(
            "[selection-polish] event=ipc_confirm_error request_id={} failure={} session_id={} generation={} elapsed_ms={}",
            request_id,
            error_class(error),
            session_label(Some(session_id)),
            owner.generation,
            started.elapsed().as_millis(),
        ),
    }
    result
}

#[tauri::command]
pub async fn cancel_selection_polish_preview(
    core: CoreState<'_>,
    session_id: Option<openless_core::SessionId>,
    generation: Option<u64>,
    request_id: Option<String>,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let request_id = request_label(request_id.as_deref());
    log::info!(
        "[selection-polish] event=ipc_cancel_start request_id={} session_id={} generation={}",
        request_id,
        session_label(session_id),
        generation.map_or_else(|| "-".to_string(), |value| value.to_string()),
    );
    let owner = match crate::selection_polish_preview_owner() {
        Some(owner) => owner,
        None => {
            log::info!(
                "[selection-polish] event=ipc_cancel_error request_id={} failure=inactive elapsed_ms={}",
                request_id,
                started.elapsed().as_millis(),
            );
            return Err("selection preview is no longer active".to_string());
        }
    };
    if !owner_matches(owner, session_id, generation) {
        log::info!(
            "[selection-polish] event=ipc_cancel_error request_id={} failure=stale_owner owner_session_id={} owner_generation={} elapsed_ms={}",
            request_id,
            session_label(owner.session_id),
            owner.generation,
            started.elapsed().as_millis(),
        );
        return Err("selection preview is stale".to_string());
    }
    let result = core
        .services()
        .selection
        .cancel(owner.session_id)
        .await
        .map_err(|error| error.message);
    match &result {
        Ok(()) => log::info!(
            "[selection-polish] event=ipc_cancel_result request_id={} outcome=ok session_id={} generation={} elapsed_ms={}",
            request_id,
            session_label(owner.session_id),
            owner.generation,
            started.elapsed().as_millis(),
        ),
        Err(error) => log::info!(
            "[selection-polish] event=ipc_cancel_error request_id={} failure={} session_id={} generation={} elapsed_ms={}",
            request_id,
            error_class(error),
            session_label(owner.session_id),
            owner.generation,
            started.elapsed().as_millis(),
        ),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_preview_wire_keeps_legacy_fields_without_owner() {
        let payload = map_selection_preview(
            openless_core::SelectionSnapshot {
                phase: openless_core::SelectionPhase::Preview,
                session_id: Some(openless_core::SessionId::new()),
                source_text: Some("source".to_string()),
                preview_text: Some("preview".to_string()),
                instruction: None,
                insert_outcome: None,
                revert_outcome: None,
            },
            None,
        )
        .expect("preview snapshot should map to the legacy payload");

        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({ "text": "preview", "sourceText": "source" })
        );
    }

    #[test]
    fn selection_preview_wire_includes_owner_when_present() {
        let session_id = openless_core::SessionId::new();
        let payload = map_selection_preview(
            openless_core::SelectionSnapshot {
                phase: openless_core::SelectionPhase::Preview,
                session_id: Some(session_id),
                source_text: Some("source".to_string()),
                preview_text: Some("preview".to_string()),
                instruction: None,
                insert_outcome: None,
                revert_outcome: None,
            },
            Some(crate::SelectionPolishPreviewOwner {
                session_id: Some(session_id),
                generation: 7,
            }),
        )
        .expect("preview snapshot should map to an owner-aware payload");

        assert_eq!(
            serde_json::to_value(payload).unwrap(),
            serde_json::json!({
                "text": "preview",
                "sourceText": "source",
                "sessionId": session_id,
                "generation": 7,
            })
        );
    }

    #[test]
    fn owner_matching_rejects_stale_generation_or_session() {
        let session_id = openless_core::SessionId::new();
        let owner = crate::SelectionPolishPreviewOwner {
            session_id: Some(session_id),
            generation: 7,
        };
        assert!(owner_matches(owner, Some(session_id), Some(7)));
        assert!(!owner_matches(owner, Some(session_id), Some(8)));
        assert!(!owner_matches(
            owner,
            Some(openless_core::SessionId::new()),
            Some(7)
        ));
    }

    #[test]
    fn diagnostic_labels_reject_untrusted_values() {
        assert_eq!(request_label(Some("preview-123_abc")), "preview-123_abc");
        assert_eq!(request_label(Some("preview\nsecret\t🙂")), "previewsecret");
        assert_eq!(request_label(Some("\n\t")), "-");
        assert_eq!(request_label(Some(&"x".repeat(200))).len(), 96);
        assert_eq!(safe_event_label("source_text"), "unknown");
        assert_eq!(safe_reason_label(Some("clipboard")), "-");
        assert_eq!(safe_outcome_label(Some("inserted")), "-");
    }
}
