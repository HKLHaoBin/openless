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
) -> Result<Option<SelectionPolishPreviewPayload>, String> {
    let Some(owner) = crate::selection_polish_preview_owner() else {
        return Ok(None);
    };
    if generation.is_some_and(|value| owner.generation != value) {
        return Ok(None);
    }
    let snapshot = core
        .services()
        .selection
        .snapshot()
        .await
        .map_err(|error| error.message)?;
    Ok(map_selection_preview(snapshot, Some(owner)))
}

#[tauri::command]
pub async fn confirm_selection_polish_preview(
    core: CoreState<'_>,
    text: String,
    session_id: Option<openless_core::SessionId>,
    generation: Option<u64>,
) -> Result<(), String> {
    let owner = crate::selection_polish_preview_owner()
        .ok_or_else(|| "selection preview is no longer active".to_string())?;
    if !owner_matches(owner, session_id, generation) {
        return Err("selection preview is stale".to_string());
    }
    let snapshot = core
        .services()
        .selection
        .snapshot()
        .await
        .map_err(|error| error.message)?;
    let session_id = snapshot
        .session_id
        .ok_or_else(|| "selection preview is not active".to_string())?;
    if owner
        .session_id
        .is_some_and(|expected| expected != session_id)
    {
        return Err("selection preview is stale".to_string());
    }
    core.services()
        .selection
        .confirm(session_id, Some(text))
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
pub async fn cancel_selection_polish_preview(
    core: CoreState<'_>,
    session_id: Option<openless_core::SessionId>,
    generation: Option<u64>,
) -> Result<(), String> {
    let owner = crate::selection_polish_preview_owner()
        .ok_or_else(|| "selection preview is no longer active".to_string())?;
    if !owner_matches(owner, session_id, generation) {
        return Err("selection preview is stale".to_string());
    }
    core.services()
        .selection
        .cancel(owner.session_id)
        .await
        .map_err(|error| error.message)
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
}
