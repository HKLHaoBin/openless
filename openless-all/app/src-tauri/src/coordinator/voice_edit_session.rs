//! Host adapter for the core Voice Edit Session state machine (Issue #900).
//!
//! Core owns the draft, turns and deterministic EditPlan application. This
//! module only owns asynchronous dictation IDs and the platform target that
//! was captured before model work began.

use std::sync::Arc;

use openless_core::{
    DictationOutputTarget, DictationResult, DictationSession, DictationStartOptions,
    HistoryInsertStatus, HistorySource, InsertStatus, PolishMode, SessionId, TextSelection,
    VoiceEditError, VoiceEditPhase, VoiceEditSession, VoiceEditSnapshot,
};

use super::Inner;
use crate::selection::{
    reactivate_selection_insertion_target, resolve_selection_workspace_capture,
    selection_insertion_target_is_captured, validate_selection_insertion_target,
    SelectionInsertionTarget, SelectionInsertionTargetValidation,
};

#[derive(Default)]
pub(crate) struct VoiceEditHostState {
    pub(crate) session: Option<VoiceEditSession>,
    pub(crate) dictation_session_id: Option<SessionId>,
    pub(crate) target: Option<VoiceEditNativeTarget>,
    pub(crate) initial_raw_text: String,
    pub(crate) duration_ms: u64,
}

#[derive(Clone)]
pub(crate) enum VoiceEditNativeTarget {
    Android {
        generation: i64,
    },
    Desktop {
        target: SelectionInsertionTarget,
        expected_selection: String,
    },
}

pub(crate) async fn start(
    inner: &Arc<Inner>,
    field_context: Option<String>,
    selection: Option<TextSelection>,
) -> Result<VoiceEditSnapshot, String> {
    if !inner.backend.get_preferences().voice_edit_enabled {
        return Err("voiceEditDisabled".to_string());
    }
    {
        let host = inner.voice_edit_host.lock();
        if host.session.as_ref().is_some_and(|session| {
            !matches!(
                session.snapshot().phase,
                VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
            )
        }) {
            return Err("voiceEditSessionBusy".to_string());
        }
    }

    let (field_text, selection, target) = capture_target(field_context, selection)?;
    let session =
        VoiceEditSession::start(field_text, selection).map_err(|error| error.to_string())?;
    let session_id = session.session_id();
    inner
        .backend
        .start()
        .await
        .map_err(|error| error.to_string())?;
    let dictation_session_id = inner
        .backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            output_target: DictationOutputTarget::ForegroundApp,
            ..DictationStartOptions::default()
        })
        .await
        .map_err(|error| error.to_string())?;

    let mut host = inner.voice_edit_host.lock();
    let conflict = host.session.as_ref().is_some_and(|current| {
        !matches!(
            current.snapshot().phase,
            VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
        )
    });
    if conflict {
        drop(host);
        let _ = inner
            .backend
            .cancel_dictation(Some(dictation_session_id))
            .await;
        return Err("voiceEditSessionBusy".to_string());
    }
    host.session = Some(session);
    host.dictation_session_id = Some(dictation_session_id);
    host.target = Some(target);
    host.initial_raw_text.clear();
    host.duration_ms = 0;
    Ok(host
        .session
        .as_ref()
        .expect("voice edit session inserted")
        .snapshot())
}

pub(crate) async fn finish_dictation(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    let dictation_session_id = {
        let host = inner.voice_edit_host.lock();
        let session = host
            .session
            .as_ref()
            .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
        if session.snapshot().phase != VoiceEditPhase::Dictating {
            return Err("voiceEditInitialDictationUnavailable".to_string());
        }
        host.dictation_session_id
            .ok_or_else(|| "voiceEditDictationUnavailable".to_string())?
    };
    let result = inner
        .backend
        .stop_dictation_session(dictation_session_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut host = inner.voice_edit_host.lock();
    let session = host
        .session
        .as_mut()
        .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
    let dictated = non_empty_or_fallback(&result.polished_text, &result.raw_text);
    session
        .finish_dictation(dictated)
        .map_err(|error| error.to_string())?;
    host.dictation_session_id = None;
    host.initial_raw_text = result.raw_text;
    host.duration_ms = result.duration_ms;
    Ok(session.snapshot())
}

pub(crate) async fn start_instruction(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    let session_id = {
        let mut host = inner.voice_edit_host.lock();
        let session = host
            .session
            .as_mut()
            .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
        session.enter_editing().map_err(|error| error.to_string())?;
        session.session_id()
    };
    let dictation_session_id = inner
        .backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            output_target: DictationOutputTarget::ForegroundApp,
            ..DictationStartOptions::default()
        })
        .await
        .map_err(|error| error.to_string())?;
    let mut host = inner.voice_edit_host.lock();
    let conflict = host.session.as_ref().map(VoiceEditSession::session_id) != Some(session_id)
        || host.dictation_session_id.is_some();
    if conflict {
        drop(host);
        let _ = inner
            .backend
            .cancel_dictation(Some(dictation_session_id))
            .await;
        return Err("voiceEditSessionChanged".to_string());
    }
    host.dictation_session_id = Some(dictation_session_id);
    Ok(host
        .session
        .as_ref()
        .expect("voice edit session exists")
        .snapshot())
}

pub(crate) async fn finish_instruction(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    let dictation_session_id = {
        let host = inner.voice_edit_host.lock();
        host.dictation_session_id
            .ok_or_else(|| "voiceEditInstructionUnavailable".to_string())?
    };
    let result = inner
        .backend
        .stop_dictation_session(dictation_session_id)
        .await
        .map_err(|error| error.to_string())?;
    let (session_id, field_context, draft) = {
        let mut host = inner.voice_edit_host.lock();
        let session = host
            .session
            .as_mut()
            .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
        session
            .begin_applying()
            .map_err(|error| error.to_string())?;
        let snapshot = session.snapshot();
        let context = snapshot
            .context
            .ok_or_else(|| "voiceEditDraftUnavailable".to_string())?;
        host.dictation_session_id = None;
        (snapshot.session_id, context.field_text, context.preview)
    };
    let raw = non_empty_or_fallback(&result.raw_text, &result.polished_text);
    let polished = non_empty_or_fallback(&result.polished_text, &result.raw_text);
    let generated = match inner
        .backend
        .services()
        .selection_voice
        .voice_edit_plan(openless_core::domains::VoiceEditPlanRequest {
            session_id,
            field_context,
            draft,
            instruction_raw: raw.clone(),
            instruction_polished: polished,
        })
        .await
    {
        Ok(generated) => generated,
        Err(error) => {
            recover_after_instruction_error(inner, session_id);
            return Err(error.to_string());
        }
    };
    let mut host = inner.voice_edit_host.lock();
    let session = host
        .session
        .as_mut()
        .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
    if session.session_id() != session_id {
        return Err("voiceEditSessionChanged".to_string());
    }
    if let Err(error) = session.apply_instruction(
        raw,
        generated.instruction_polished,
        generated.plan,
    ) {
        let message = error.to_string();
        let _ = session.recover_applying();
        return Err(message);
    }
    Ok(session.snapshot())
}

fn recover_after_instruction_error(inner: &Arc<Inner>, session_id: SessionId) {
    let mut host = inner.voice_edit_host.lock();
    if let Some(session) = host.session.as_mut() {
        if session.session_id() == session_id {
            let _ = session.recover_applying();
        }
    }
}

pub(crate) async fn commit(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    let (text, target) = {
        let host = inner.voice_edit_host.lock();
        let session = host
            .session
            .as_ref()
            .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
        let snapshot = session.snapshot();
        let text = snapshot
            .context
            .as_ref()
            .map(|context| context.preview.clone())
            .ok_or_else(|| "voiceEditDraftUnavailable".to_string())?;
        (
            text,
            host.target
                .clone()
                .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?,
        )
    };
    apply_native_target(inner, target, &text)?;

    let mut host = inner.voice_edit_host.lock();
    let session = host
        .session
        .as_mut()
        .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
    let commit = session.commit().map_err(|error| error.to_string())?;
    persist_history(&inner.backend, &host, &commit);
    Ok(session.snapshot())
}

pub(crate) async fn cancel(inner: &Arc<Inner>) -> Result<Option<VoiceEditSnapshot>, String> {
    let dictation_session_id = inner.voice_edit_host.lock().dictation_session_id.take();
    if let Some(session_id) = dictation_session_id {
        let _ = inner.backend.cancel_dictation(Some(session_id)).await;
    }
    let mut host = inner.voice_edit_host.lock();
    let Some(session) = host.session.as_mut() else {
        return Ok(None);
    };
    session.cancel().map_err(|error| error.to_string())?;
    host.target = None;
    Ok(Some(session.snapshot()))
}

pub(crate) fn snapshot(inner: &Arc<Inner>) -> Option<VoiceEditSnapshot> {
    inner
        .voice_edit_host
        .lock()
        .session
        .as_ref()
        .map(VoiceEditSession::snapshot)
}

fn capture_target(
    field_context: Option<String>,
    selection: Option<TextSelection>,
) -> Result<(String, Option<TextSelection>, VoiceEditNativeTarget), String> {
    #[cfg(target_os = "android")]
    {
        let raw = crate::android::capture_voice_edit_target()
            .map_err(|error| format!("voiceEditTargetCaptureFailed:{error}"))?
            .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
        let (generation, text, start, end) = parse_android_target(&raw)?;
        let selection = (start != end).then_some(TextSelection { start, end });
        return Ok((
            text,
            selection,
            VoiceEditNativeTarget::Android { generation },
        ));
    }

    #[cfg(not(target_os = "android"))]
    {
        let (captured_selection, target) = resolve_selection_workspace_capture();
        let (text, selection, expected_selection) = match field_context {
            Some(text) => {
                let expected = selection
                    .and_then(|range| slice_chars(&text, range).map(str::to_string))
                    .unwrap_or_else(|| text.clone());
                (text, selection, expected)
            }
            None => {
                let selection = captured_selection
                    .ok_or_else(|| "voiceEditSelectionUnavailable".to_string())?;
                let chars = selection.text.chars().count() as u32;
                (
                    selection.text.clone(),
                    Some(TextSelection {
                        start: 0,
                        end: chars,
                    }),
                    selection.text,
                )
            }
        };
        if !selection_insertion_target_is_captured(&target) {
            return Err("voiceEditTargetUnavailable".to_string());
        }
        return Ok((
            text,
            selection,
            VoiceEditNativeTarget::Desktop {
                target,
                expected_selection,
            },
        ));
    }
}

#[cfg(target_os = "android")]
fn parse_android_target(raw: &str) -> Result<(i64, String, u32, u32), String> {
    let mut parts = raw.splitn(6, '|');
    let generation = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let _package = parts.next().filter(|value| !value.is_empty());
    let _window = parts.next().and_then(|value| value.parse::<i32>().ok());
    let start = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let end = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let text = parts
        .next()
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?
        .to_string();
    let start = openless_core::host_document::utf16_offset_to_char_offset(&text, start) as u32;
    let end = openless_core::host_document::utf16_offset_to_char_offset(&text, end) as u32;
    Ok((generation, text, start, end))
}

fn apply_native_target(
    inner: &Arc<Inner>,
    target: &VoiceEditNativeTarget,
    text: &str,
) -> Result<(), String> {
    match target {
        VoiceEditNativeTarget::Android { generation } => {
            let result = crate::android::replace_voice_edit_target(*generation, text)
                .map_err(|error| format!("voiceEditReplaceFailed:{error}"))?;
            if result == crate::android::accessibility::PASTE_RESULT_SUCCESS {
                Ok(())
            } else {
                Err(format!("voiceEditReplaceFailed:{result}"))
            }
        }
        VoiceEditNativeTarget::Desktop {
            target,
            expected_selection,
        } => {
            if !reactivate_selection_insertion_target(target) {
                return Err("voiceEditTargetChanged".to_string());
            }
            match validate_selection_insertion_target(target, expected_selection) {
                SelectionInsertionTargetValidation::Valid => {}
                invalid => {
                    return Err(invalid
                        .error_code()
                        .unwrap_or("voiceEditTargetChanged")
                        .to_string())
                }
            }
            let preferences = inner.backend.get_preferences();
            match inner.inserter.insert(
                text,
                preferences.restore_clipboard_after_paste,
                preferences.paste_shortcut,
            ) {
                InsertStatus::Inserted | InsertStatus::PasteSent => Ok(()),
                InsertStatus::CopiedFallback => Err("voiceEditInsertFallback".to_string()),
                InsertStatus::Failed => Err("voiceEditInsertFailed".to_string()),
            }
        }
    }
}

fn persist_history(
    backend: &openless_core::OpenLessBackend,
    host: &VoiceEditHostState,
    commit: &openless_core::VoiceEditCommit,
) {
    let preferences = backend.get_preferences();
    let turns = commit
        .turns
        .iter()
        .map(|turn| turn.instruction_polished.as_str())
        .collect::<Vec<_>>()
        .join("；");
    let raw = if turns.is_empty() {
        host.initial_raw_text.clone()
    } else if host.initial_raw_text.is_empty() {
        turns
    } else {
        format!("{}；{}", host.initial_raw_text, turns)
    };
    let session = DictationSession {
        id: commit.session_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source: HistorySource::VoiceEdit,
        raw_transcript: raw,
        asr_transcript: None,
        final_text: commit.text.clone(),
        mode: PolishMode::Light,
        style_pack_id: None,
        translation_active: false,
        polish_source: Some("voice_edit_session".to_string()),
        app_bundle_id: None,
        app_name: None,
        insert_status: HistoryInsertStatus::Inserted,
        error_code: None,
        duration_ms: Some(host.duration_ms),
        dictionary_entry_count: None,
        has_audio_recording: None,
        asr_provider: None,
        asr_model: None,
        llm_provider: None,
        llm_model: None,
        pipeline_mode: None,
        asr_ms: None,
        polish_ms: None,
    };
    if let Err(error) = backend.append_history(
        session,
        preferences.history_retention_days,
        preferences.history_max_entries,
    ) {
        log::warn!("voice edit history persistence failed: {error}");
    }
}

fn non_empty_or_fallback(primary: &str, fallback: &str) -> String {
    if primary.trim().is_empty() {
        fallback.trim().to_string()
    } else {
        primary.trim().to_string()
    }
}

fn slice_chars(text: &str, selection: TextSelection) -> Option<&str> {
    let (start, end) = selection.normalized();
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    let mut offsets = text
        .char_indices()
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    offsets.push(text.len());
    Some(text.get(*offsets.get(start)?, *offsets.get(end)?)?)
}

#[cfg(test)]
mod tests {
    use super::non_empty_or_fallback;

    #[test]
    fn dictation_text_falls_back_to_raw_when_polish_is_empty() {
        assert_eq!(non_empty_or_fallback("  ", " raw "), "raw");
        assert_eq!(non_empty_or_fallback(" polished ", "raw"), "polished");
    }
}
