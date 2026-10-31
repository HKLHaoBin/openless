//! [DEBUG-ISSUE-1320-RESULT] Six low-volume boundary probes, no product retries.
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
    ListStarted, ListApplied, ListSuperseded, ListError, RowObserved, RowSuperseded, CoverageSnapshot,
    DocumentMetadata, ExportStarted, ExportEncoded, ExportWriteStarted, ExportOk, ExportError,
}

impl Event {
    fn probe(self) -> &'static str {
        match self {
            Self::ReadStarted | Self::ReadOk | Self::ReadError => "P2",
            Self::CoreStarted | Self::CoreOk | Self::CoreError => "P3",
            Self::DocumentMetadata => "P5",
            Self::ExportStarted | Self::ExportEncoded | Self::ExportWriteStarted
                | Self::ExportOk | Self::ExportError => "P6",
            Self::ListStarted | Self::ListApplied | Self::ListSuperseded
                | Self::ListError | Self::RowObserved | Self::RowSuperseded | Self::CoverageSnapshot => "P4",
            _ => "P1",
        }
    }
    pub(crate) fn frontend(self) -> bool {
        matches!(self, Self::ImportStarted | Self::PickerSelected | Self::PickerCancelled
            | Self::IpcStarted | Self::IpcOk | Self::ImportError | Self::ImportSettled
            | Self::ListStarted | Self::ListApplied | Self::ListSuperseded
            | Self::ListError | Self::RowObserved | Self::RowSuperseded)
    }
    fn outcome(self) -> &'static str {
        match self {
            Self::ReadError | Self::CoreError | Self::ImportError | Self::ListError | Self::ExportError => "error",
            Self::IpcOk | Self::ReadOk | Self::CoreOk | Self::ListApplied | Self::ExportOk => "ok",
            Self::PickerCancelled => "cancelled",
            Self::ListSuperseded | Self::RowSuperseded => "superseded",
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
    pub accounting_current_instance: Option<bool>,
    pub backend_dropped: Option<u64>,
    pub backend_write_failed: Option<bool>,
    pub declared_bytes: Option<u64>,
    pub metadata_available: Option<bool>,
    pub metadata_error: Option<bool>,
    pub exporting: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FrontendCoverage {
    pub producer_instance: Option<uuid::Uuid>,
    pub frontend_failures: u64,
    pub frontend_pending: u64,
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
    producer_instance: Option<uuid::Uuid>,
    attempt_id: &'static str,
    change_id: &'static str,
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event_seq: u64,
    event: Event,
    outcome: &'static str,
    safe: Facts,
    ts: u64,
}

fn encode(run: &str, op: &str, producer: Option<uuid::Uuid>, seq: u64, event: Event, facts: Facts) -> Vec<u8> {
    let record = Record {
        schema: "diagnosing-bugs.v1", session_id: SESSION, run_id: run,
        operation_id: op, probe_id: event.probe(), hypothesis_ids: &["H1", "H2", "H3"],
        producer_instance: producer, attempt_id: "A2-content-export", change_id: "C2-resolver-export",
        event_seq: seq, event, outcome: event.outcome(), safe: facts,
        ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |value| value.as_millis() as u64),
    };
    let mut bytes = serde_json::to_vec(&record).expect("fixed diagnostic schema serializes");
    bytes.push(b'\n');
    bytes
}

pub(crate) fn record(op: &str, event: Event, facts: Facts) {
    record_from(op, None, event, facts);
}

pub(crate) fn record_from(op: &str, producer: Option<uuid::Uuid>, event: Event, facts: Facts) {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    sink::record(op, producer, event, facts);
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    let _ = (op, producer, event, facts);
}

/// Export a copy through the existing error-log entry, never mutate openless.log.
pub(crate) fn append_to_error_log(bytes: &mut Vec<u8>, coverage: Option<FrontendCoverage>) -> Result<(), String> {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    {
        let snapshot = sink::snapshot(coverage)?;
        bytes.extend_from_slice(b"\n\n=== ISSUE-1320 NDJSON (six boundary probes; coverage is instance-scoped) ===\n");
        bytes.extend_from_slice(&snapshot);
    }
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    let _ = (bytes, coverage);
    Ok(())
}

// A readable older file must not mask failure to persist this export's coverage.
fn snapshot_after_append(
    record: &[u8], append: impl FnOnce(&[u8]) -> Result<(), String>,
    read: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    append(record)?;
    read()
}

fn coverage_facts(coverage: Option<&FrontendCoverage>, dropped: u64, failed: bool) -> (Option<uuid::Uuid>, Facts) {
    let producer = coverage.and_then(|value| value.producer_instance)
        .filter(|value| value.get_version_num() == 4);
    (producer, Facts {
        frontend_failures: coverage.map(|value| value.frontend_failures),
        frontend_pending: coverage.map(|value| value.frontend_pending),
        frontend_history_unknown: Some(true), accounting_current_instance: Some(producer.is_some()),
        backend_dropped: Some(dropped), backend_write_failed: Some(failed), ..Default::default()
    })
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

    fn append(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)
            .map_err(|_| "diagnostic_append_failed")?;
        if file.metadata().map_err(|_| "diagnostic_metadata_failed")?.len()
            .saturating_add(bytes.len() as u64) > LIMIT {
            return Err("diagnostic_capacity_exceeded".into());
        }
        file.write_all(bytes).map_err(|_| "diagnostic_append_failed".into())
    }

    pub(super) fn record(op: &str, producer: Option<uuid::Uuid>, event: Event, facts: Facts) {
        // Never wait for another producer; contention is disclosed by export health.
        let Ok(mut state) = STATE.try_lock() else { DROPPED.fetch_add(1, Ordering::Relaxed); return; };
        state.0 += 1;
        let result = path().and_then(|path| append(&path, &encode(
            RUN.get_or_init(|| uuid::Uuid::new_v4().to_string()), op, producer, state.0, event, facts)));
        if result.is_err() { state.1 = true; DROPPED.fetch_add(1, Ordering::Relaxed); }
    }

    pub(crate) fn snapshot(coverage: Option<FrontendCoverage>) -> Result<Vec<u8>, String> {
        // Export is the only blocking observer; product producers always try_lock.
        // Persist THIS snapshot before reading, otherwise fail instead of exporting
        // a stale older coverage record as a seemingly complete log.
        let mut state = STATE.lock().map_err(|_| "diagnostic_lock_failed")?;
        let path = path()?;
        state.0 += 1;
        let (producer, facts) = coverage_facts(coverage.as_ref(), DROPPED.load(Ordering::Relaxed), state.1);
        let bytes = encode(RUN.get_or_init(|| uuid::Uuid::new_v4().to_string()),
            &uuid::Uuid::new_v4().to_string(), producer, state.0, Event::CoverageSnapshot, facts);
        let result = snapshot_after_append(&bytes, |record| append(&path, record),
            || std::fs::read(&path).map_err(|_| "diagnostic_snapshot_unavailable".into()));
        if result.is_err() { state.1 = true; DROPPED.fetch_add(1, Ordering::Relaxed); }
        result
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
        let bytes = encode("opaque-run", &id, None, 1, Event::ReadError,
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
        assert!(!Event::DocumentMetadata.frontend());
        assert!(!Event::ExportOk.frontend());
        assert_eq!(Event::DocumentMetadata.probe(), "P5");
        assert_eq!(Event::ExportWriteStarted.probe(), "P6");
        assert_eq!(Event::ListSuperseded.probe(), "P4");
    }

    #[test]
    fn snapshot_cannot_hide_failed_current_append_behind_readable_stale_data() {
        let result = snapshot_after_append(b"new coverage\n", |_| Err("append_failed".into()),
            || -> Result<Vec<u8>, String> { panic!("must not export old readable snapshot") });
        assert_eq!(result, Err("append_failed".into()));
        let result = snapshot_after_append(b"new coverage\n", |_| Ok(()), || Ok(b"new coverage\n".to_vec()));
        assert_eq!(result.unwrap(), b"new coverage\n");
    }

    #[test]
    fn exported_coverage_is_instance_scoped_and_never_claims_prior_instances_complete() {
        let producer = uuid::Uuid::new_v4();
        let coverage = FrontendCoverage { producer_instance: Some(producer), frontend_failures: 0, frontend_pending: 0 };
        let (_, facts) = coverage_facts(Some(&coverage), 2, true);
        let bytes = encode("generated-run", "generated-operation", Some(producer), 1, Event::CoverageSnapshot, facts);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["producer_instance"], producer.to_string());
        assert_eq!(value["safe"]["frontend_history_unknown"], true);
        assert_eq!(value["safe"]["accounting_current_instance"], true);
        assert_eq!(value["safe"]["backend_dropped"], 2);
        assert_eq!(value["safe"]["backend_write_failed"], true);
        let (unknown_producer, unknown_facts) = coverage_facts(None, 0, false);
        assert!(unknown_producer.is_none());
        assert_eq!(unknown_facts.frontend_failures, None);
        assert_eq!(unknown_facts.frontend_history_unknown, Some(true));
        assert_eq!(unknown_facts.accounting_current_instance, Some(false));
    }
}
