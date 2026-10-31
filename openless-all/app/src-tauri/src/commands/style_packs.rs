use super::*;

fn refresh_tray_menu_async(app: &AppHandle) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = crate::refresh_tray_microphone_menu(&app_for_main) {
            log::warn!("[tray] refresh after style change failed: {err}");
        }
    });
}

pub(crate) fn activate_style_pack_by_id(
    coord: &Coordinator,
    app: &AppHandle,
    id: &str,
) -> Result<StylePack, String> {
    let backend = coord.backend();
    let pack = backend.get_style_pack(id).map_err(|e| e.to_string())?;
    log::info!(
        "[style-pack] activate helper requested id={} kind={:?} base_mode={:?} enabled={}",
        pack.id,
        pack.kind,
        pack.base_mode,
        pack.enabled
    );
    let pack = backend.activate_style_pack(id).map_err(|e| e.to_string())?;
    refresh_tray_menu_async(app);
    log::info!("[style-pack] activate helper applied id={id}");
    Ok(pack)
}

pub(crate) fn activate_builtin_style_mode(
    coord: &Coordinator,
    app: &AppHandle,
    mode: PolishMode,
) -> Result<(), String> {
    let pack_id = builtin_style_pack_id(mode).to_string();
    log::info!(
        "[style-pack] activate builtin mode helper mode={:?} pack_id={}",
        mode,
        pack_id
    );
    let _ = activate_style_pack_by_id(coord, app, &pack_id)?;
    Ok(())
}

// ─────────────────────────── style packs ───────────────────────────

#[tauri::command]
pub fn list_style_packs(core: CoreState<'_>) -> Result<Vec<StylePack>, String> {
    let prefs = core.get_preferences();
    core.list_style_packs(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_style_pack_from_template(
    core: CoreState<'_>,
    app: AppHandle,
    template: StylePack,
) -> Result<StylePack, String> {
    log::info!(
        "[style-pack] command create_from_template name={} base_mode={:?}",
        template.name,
        template.base_mode
    );
    let created = core
        .create_style_pack(template)
        .map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    Ok(created)
}

#[tauri::command]
pub fn save_style_pack(
    core: CoreState<'_>,
    app: AppHandle,
    style_pack: StylePack,
) -> Result<StylePack, String> {
    log::info!(
        "[style-pack] command save id={} kind={:?} base_mode={:?}",
        style_pack.id,
        style_pack.kind,
        style_pack.base_mode
    );
    let saved = core
        .update_style_pack(style_pack)
        .map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    Ok(saved)
}

#[tauri::command]
pub fn preview_style_pack_runtime(
    core: CoreState<'_>,
    style_pack: StylePack,
) -> Result<StylePackRuntimeDiagnostics, String> {
    log::info!(
        "[style-pack] command preview_runtime id={} base_mode={:?} prompt_chars={}",
        style_pack.id,
        style_pack.base_mode,
        style_pack.prompt.chars().count()
    );
    Ok(core.preview_style_pack_runtime(&style_pack))
}

#[tauri::command]
pub fn set_style_pack_icon(
    core: CoreState<'_>,
    id: String,
    png: Option<Vec<u8>>,
) -> Result<StylePack, String> {
    core.set_style_pack_icon(&id, png.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_style_pack_icon(core: CoreState<'_>, id: String) -> Result<Option<String>, String> {
    core.read_style_pack_icon(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_active_style_pack(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    id: String,
) -> Result<StylePack, String> {
    activate_style_pack_by_id(&coord, &app, &id)
}

#[tauri::command]
pub fn set_style_pack_enabled(
    core: CoreState<'_>,
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<Vec<StylePack>, String> {
    log::info!(
        "[style-pack] command set_enabled requested id={} enabled={}",
        id,
        enabled
    );
    core.set_style_pack_enabled(&id, enabled)
        .map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    let prefs = core.get_preferences();
    core.list_style_packs(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_builtin_style_pack(
    core: CoreState<'_>,
    app: AppHandle,
    id: String,
) -> Result<StylePack, String> {
    log::info!("[style-pack] command reset_builtin requested id={id}");
    let saved = core
        .reset_builtin_style_pack(&id)
        .map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    Ok(saved)
}

#[tauri::command]
pub fn delete_style_pack(
    coord: CoordinatorState<'_>,
    core: CoreState<'_>,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    let _host_guard = coord.lock_settings_host();
    log::info!("[style-pack] command delete requested id={id}");
    let outcome = core.remove_style_pack(&id).map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    if let Some(change) = &outcome.effects.hotkeys {
        if let Err(error) = coord.apply_hotkey_runtime_change(change) {
            // The Core delete transaction is already committed; keep the new explicit target so
            // the resident supervisor keeps converging instead of inferring the delete intent
            // from the preferences document.
            log::warn!("[style-pack] refresh hotkeys after delete failed: {error}");
        }
    }
    Ok(())
}

#[tauri::command]
pub fn import_style_pack_from_zip(
    core: CoreState<'_>,
    zip_path: String,
    diagnostic_operation: Option<String>,
) -> Result<StylePack, String> {
    use crate::style_zip_diagnostics::{self as diagnostic, Event, Facts};
    let operation = diagnostic::operation(diagnostic_operation.as_deref());
    log::info!(
        "[style-pack] command import requested source_kind={}",
        if zip_path.starts_with("content://") { "content-uri" } else { "file-path" }
    );
    #[cfg(target_os = "android")]
    if zip_path.starts_with("content://") {
        observe_style_zip_document(&operation, &zip_path, false);
        let started = std::time::Instant::now();
        diagnostic::record(&operation, Event::ReadStarted,
            Facts { content_uri: Some(true), ..Default::default() });
        let bytes = crate::android::jni::android::read_content_uri(
            &zip_path,
            crate::persistence::STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES,
        ).map_err(|error| {
            diagnostic::record(&operation, Event::ReadError, Facts {
                error_code: Some(diagnostic::ErrorCode::UriRead),
                duration_ms: Some(started.elapsed().as_millis() as u64), ..Default::default()
            });
            error
        })?;
        diagnostic::record(&operation, Event::ReadOk, Facts {
            bytes: Some(bytes.len() as u64),
            duration_ms: Some(started.elapsed().as_millis() as u64), ..Default::default()
        });
        diagnostic::record(&operation, Event::CoreStarted, Facts::default());
        let result = core.import_style_pack_bytes(&bytes);
        record_style_zip_core_result(&operation, &result);
        return result.map_err(|error| error.to_string());
    }
    diagnostic::record(&operation, Event::CoreStarted, Facts { content_uri: Some(false), ..Default::default() });
    let result = core.import_style_pack_path(std::path::Path::new(&zip_path));
    record_style_zip_core_result(&operation, &result);
    result.map_err(|error| error.to_string())
}

#[cfg(target_os = "android")]
fn observe_style_zip_document(operation: &str, uri: &str, exporting: bool) {
    #[cfg(feature = "issue-1320-diagnostics")]
    if uri.starts_with("content://") {
        use crate::style_zip_diagnostics::{self as diagnostic, Event, Facts};
        let result = crate::android::jni::android::content_uri_declared_size(uri);
        diagnostic::record(operation, Event::DocumentMetadata, Facts {
            declared_bytes: result.as_ref().ok().copied().flatten(),
            metadata_available: Some(matches!(&result, Ok(Some(_)))),
            metadata_error: Some(result.is_err()), exporting: Some(exporting), ..Default::default()
        });
    }
    #[cfg(not(feature = "issue-1320-diagnostics"))]
    let _ = (operation, uri, exporting);
}

fn record_style_zip_core_result(operation: &str, result: &Result<StylePack, openless_core::BackendError>) {
    use crate::style_zip_diagnostics::{self as diagnostic, Event, Facts};
    match result {
        Ok(_) => diagnostic::record(operation, Event::CoreOk, Facts::default()),
        Err(error) => diagnostic::record(operation, Event::CoreError, Facts {
            error_code: Some(diagnostic::core_error_code(error)), ..Default::default()
        }),
    }
}

/// [DEBUG-ISSUE-1320-RESULT] Strict enums/numeric facts only; raw client errors are rejected.
#[tauri::command]
pub fn style_zip_diagnostic_event(
    operation_id: String,
    producer_instance: Option<uuid::Uuid>,
    event: crate::style_zip_diagnostics::Event,
    facts: crate::style_zip_diagnostics::Facts,
) {
    if event.frontend() {
        let operation = crate::style_zip_diagnostics::operation(Some(&operation_id));
        let producer = producer_instance.filter(|value| value.get_version_num() == 4);
        crate::style_zip_diagnostics::record_from(&operation, producer, event, facts);
    }
}

#[tauri::command]
pub fn export_style_pack_to_zip(
    core: CoreState<'_>,
    id: String,
    target_path: String,
) -> Result<String, String> {
    use crate::style_zip_diagnostics::{self as diagnostic, Event, Facts};
    let operation = diagnostic::operation(None);
    diagnostic::record(&operation, Event::ExportStarted, Facts {
        content_uri: Some(target_path.starts_with("content://")), ..Default::default()
    });
    log::info!("[style-pack] command export requested target_kind={}",
        if target_path.starts_with("content://") { "content-uri" } else { "file-path" });
    #[cfg(target_os = "android")]
    {
        let result = crate::style_zip_transfer::export_zip(
            &target_path,
            || {
                let bytes = core.export_style_pack_bytes(&id).map_err(|error| error.to_string())?;
                diagnostic::record(&operation, Event::ExportEncoded, Facts {
                    bytes: Some(bytes.len() as u64), ..Default::default()
                });
                Ok(bytes)
            },
            |uri, bytes| {
                diagnostic::record(&operation, Event::ExportWriteStarted, Facts {
                    bytes: Some(bytes.len() as u64), ..Default::default()
                });
                crate::android::jni::android::write_content_uri(uri, bytes)
            },
            || core.export_style_pack_path(&id, std::path::Path::new(&target_path))
                .map_err(|error| error.to_string()),
        );
        match result {
            Ok(bytes) => {
                diagnostic::record(&operation, Event::ExportOk, Facts {
                    bytes: bytes.map(|value| value as u64), ..Default::default()
                });
                observe_style_zip_document(&operation, &target_path, true);
                return Ok(target_path);
            }
            Err(error) => {
                diagnostic::record(&operation, Event::ExportError, Facts::default());
                return Err(error);
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        core.export_style_pack_path(&id, std::path::Path::new(&target_path))
            .map_err(|e| e.to_string())?;
        Ok(target_path)
    }
}

// ─────────────────────────── style toggles (compat) ───────────────────────────

#[tauri::command]
pub fn set_default_polish_mode(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    mode: PolishMode,
) -> Result<(), String> {
    activate_builtin_style_mode(&coord, &app, mode)
}

#[tauri::command]
pub fn set_style_enabled(
    core: CoreState<'_>,
    app: AppHandle,
    mode: PolishMode,
    enabled: bool,
) -> Result<(), String> {
    let pack_id = builtin_style_pack_id(mode).to_string();
    log::info!(
        "[style-pack] compat set_style_enabled mode={:?} pack_id={} enabled={}",
        mode,
        pack_id,
        enabled
    );
    core.set_style_pack_enabled(&pack_id, enabled)
        .map_err(|e| e.to_string())?;
    refresh_tray_menu_async(&app);
    Ok(())
}
