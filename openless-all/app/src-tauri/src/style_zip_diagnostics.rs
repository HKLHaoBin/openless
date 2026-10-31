//! [DEBUG-ISSUE-1320-RESULT] Four low-volume boundary probes, no product retries.
//! Only the Android diagnostic feature writes; no ordinary application logger is used.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub(crate) const SESSION: &str = "issue-1320-result-a1";
pub(crate) const LOG_NAME: &str = "issue-1320-result-a1.ndjson";
pub(crate) const ENABLED: bool = cfg!(all(target_os = "android", feature = "issue-1320-diagnostics"));

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    ImportStarted, PickerSelected, PickerCancelled, IpcStarted, IpcOk, ImportError, ImportSettled,
    ReadStarted, ReadOk, ReadError, CoreStarted, CoreOk, CoreError,
    ListStarted, ListApplied, ListSuperseded, ListError, RowObserved, CoverageSnapshot,
}

impl Event {
    fn probe(self) -> &'static str {
        match self {
            Self::ReadStarted | Self::ReadOk | Self::ReadError => "P2",
            Self::CoreStarted | Self::CoreOk | Self::CoreError => "P3",
            Self::ListStarted | Self::ListApplied | Self::ListSuperseded
                | Self::ListError | Self::RowObserved | Self::CoverageSnapshot => "P4",
            _ => "P1",
        }
    }
    pub(crate) fn frontend(self) -> bool {
        !matches!(self, Self::ReadStarted | Self::ReadOk | Self::ReadError
            | Self::CoreStarted | Self::CoreOk | Self::CoreError | Self::CoverageSnapshot)
    }
    fn outcome(self) -> &'static str {
        match self {
            Self::ReadError | Self::CoreError | Self::ImportError | Self::ListError => "error",
            Self::IpcOk | Self::ReadOk | Self::CoreOk | Self::ListApplied => "ok",
            Self::PickerCancelled => "cancelled",
            Self::ListSuperseded => "superseded",
            _ => "observed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Facts {
    pub bytes: Option<u64>,
    pub count: Option<u64>,
    pub present: Option<bool>,
    pub visible: Option<bool>,
    pub content_uri: Option<bool>,
    pub duration_ms: Option<u64>,
    pub source_seq: Option<u64>,
    pub error_code: Option<ErrorCode>,
    pub frontend_failures: Option<u64>,
    pub frontend_pending: Option<u64>,
    pub frontend_history_unknown: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorCode { UriRead, InvalidArgument, Persistence, PermissionDenied, OutcomeUnknown, Core, Frontend }

/// Use Core's stable enum, never infer a stage from user-controlled exception text.
pub(crate) fn core_error_code(error: &openless_core::BackendError) -> ErrorCode {
    use openless_core::BackendErrorCode;
    match error.code {
        BackendErrorCode::InvalidArgument => ErrorCode::InvalidArgument,
        BackendErrorCode::Persistence => ErrorCode::Persistence,
        BackendErrorCode::PermissionDenied => ErrorCode::PermissionDenied,
        BackendErrorCode::OutcomeUnknown => ErrorCode::OutcomeUnknown,
        _ => ErrorCode::Core,
    }
}

pub(crate) fn operation(supplied: Option<&str>) -> String {
    if !ENABLED && !cfg!(test) { return String::new(); }
    supplied.and_then(|value| uuid::Uuid::parse_str(value).ok())
        .filter(|value| value.get_version_num() == 4)
        .unwrap_or_else(uuid::Uuid::new_v4).to_string()
}

#[derive(Serialize)]
struct Record<'a> {
    schema: &'static str,
    session_id: &'static str,
    run_id: &'a str,
    operation_id: &'a str,
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event_seq: u64,
    event: Event,
    outcome: &'static str,
    safe: Facts,
    ts: u64,
}

fn encode(run: &str, op: &str, seq: u64, event: Event, facts: Facts) -> Vec<u8> {
    let record = Record {
        schema: "diagnosing-bugs.v1", session_id: SESSION, run_id: run,
        operation_id: op, probe_id: event.probe(), hypothesis_ids: &["H1", "H2", "H3"],
        event_seq: seq, event, outcome: event.outcome(), safe: facts,
        ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |value| value.as_millis() as u64),
    };
    let mut bytes = serde_json::to_vec(&record).expect("fixed diagnostic schema serializes");
    bytes.push(b'\n');
    bytes
}

pub(crate) fn record(op: &str, event: Event, facts: Facts) {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    sink::record(op, event, facts);
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    let _ = (op, event, facts);
}

#[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
pub(crate) mod sink {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock, atomic::{AtomicU64, Ordering}};
    const LIMIT: u64 = 2 * 1024 * 1024;
    static RUN: OnceLock<String> = OnceLock::new();
    static STATE: Mutex<(u64, bool)> = Mutex::new((0, false));
    static DROPPED: AtomicU64 = AtomicU64::new(0);

    fn path() -> Result<std::path::PathBuf, String> {
        let root = crate::persistence::data_dir().map_err(|_| "diagnostic_directory_unavailable")?;
        if root.as_os_str().is_empty() { return Err("diagnostic_directory_unavailable".into()); }
        Ok(root.join(LOG_NAME))
    }

    pub(super) fn record(op: &str, event: Event, facts: Facts) {
        // Never wait for another producer; contention is disclosed by export health.
        let Ok(mut state) = STATE.try_lock() else { DROPPED.fetch_add(1, Ordering::Relaxed); return; };
        state.0 += 1;
        let result = (|| -> std::io::Result<()> {
            let path = path().map_err(std::io::Error::other)?;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
            let bytes = encode(RUN.get_or_init(|| uuid::Uuid::new_v4().to_string()), op, state.0, event, facts);
            if file.metadata()?.len().saturating_add(bytes.len() as u64) > LIMIT {
                return Err(std::io::Error::other("diagnostic_capacity"));
            }
            file.write_all(&bytes)
        })();
        if result.is_err() { state.1 = true; DROPPED.fetch_add(1, Ordering::Relaxed); }
    }

    #[derive(Serialize)]
    pub(crate) struct ExportReport { pub path: String, pub dropped: u64, pub write_failed: bool }

    pub(crate) fn export() -> Result<ExportReport, String> {
        // User-triggered export only. Hold the lock for a consistent snapshot; runtime
        // producers use try_lock and disclose any dropped record, never block behind it.
        let state = STATE.lock().map_err(|_| "diagnostic_lock_failed")?;
        let bytes = std::fs::read(path()?).map_err(|_| "diagnostic_log_unavailable")?;
        let target = crate::android::jni::android::write_public_download(LOG_NAME, &bytes)
            .map_err(|_| "diagnostic_export_failed")?;
        Ok(ExportReport { path: target, dropped: DROPPED.load(Ordering::Relaxed), write_failed: state.1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_are_one_line_allowlisted_and_operation_ids_are_generated() {
        let id = operation(Some("content://private-document-identifier"));
        assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 4);
        assert_eq!(operation(Some(&id)), id);
        let bytes = encode("opaque-run", &id, 1, Event::ReadError,
            Facts { error_code: Some(ErrorCode::UriRead), ..Default::default() });
        assert_eq!(bytes.iter().filter(|&&c| c == b'\n').count(), 1);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["probe_id"], "P2");
        assert_eq!(value["schema"], "diagnosing-bugs.v1");
        assert!(!String::from_utf8(bytes).unwrap().contains("private-document"));
        assert!(serde_json::from_str::<Facts>(r#"{"uri":"content://secret"}"#).is_err());
        assert!(serde_json::from_str::<Facts>(r#"{"error_code":"raw secret"}"#).is_err());
    }
    #[test]
    fn backend_events_cannot_be_spoofed_by_frontend_command() {
        assert!(!Event::CoreOk.frontend());
        assert!(!Event::ReadOk.frontend());
        assert!(Event::RowObserved.frontend());
        assert!(!Event::CoverageSnapshot.frontend());
        assert_eq!(Event::ListSuperseded.probe(), "P4");
    }
}
