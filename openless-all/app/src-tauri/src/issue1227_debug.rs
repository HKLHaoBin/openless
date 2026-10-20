//! Diagnostic-branch issue #1227 runtime probes. The records contain only bounded state facts.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION_ID: &str = "3e857688-76d6-4d76-8747-7ff1e05ee657";
const DEFAULT_LOG_NAME: &str = "openless-issue-1227-3e857688-session.ndjson";
const DEBUG_PREFIX: &str = "[DEBUG-MIGRATION-1192]";

fn run_id() -> &'static str {
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| format!("app-{}-{}", std::process::id(), unix_millis()))
}

fn log_path() -> PathBuf {
    std::env::var_os("OPENLESS_ISSUE_1227_DEBUG_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(DEFAULT_LOG_NAME))
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// Write one complete NDJSON record and never affect the product path on I/O failure.
pub(crate) fn record(
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event: &'static str,
    outcome: &'static str,
    safe: serde_json::Value,
) {
    record_inner(probe_id, hypothesis_ids, event, outcome, None, safe);
}

pub(crate) fn record_correlated(
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event: &'static str,
    outcome: &'static str,
    correlation: (u64, u64),
    safe: serde_json::Value,
) {
    record_inner(
        probe_id,
        hypothesis_ids,
        event,
        outcome,
        Some(correlation),
        safe,
    );
}

fn record_inner(
    probe_id: &'static str,
    hypothesis_ids: &'static [&'static str],
    event: &'static str,
    outcome: &'static str,
    correlation: Option<(u64, u64)>,
    safe: serde_json::Value,
) {
    let mut record = serde_json::json!({
        "schema": "diagnosing-bugs.v1",
        "session_id": SESSION_ID,
        "run_id": run_id(),
        "probe_id": probe_id,
        "hypothesis_ids": hypothesis_ids,
        "event": event,
        "outcome": outcome,
        "safe": safe,
        "ts": unix_millis(),
    });
    if let Some((operation, sequence)) = correlation {
        record["operation_id"] = serde_json::json!(format!("capture-{operation}"));
        record["event_seq"] = serde_json::json!(sequence);
    }
    let Ok(mut line) = serde_json::to_vec(&record) else {
        return;
    };
    line.push(b'\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .and_then(|mut file| file.write_all(&line));

    log::trace!("{DEBUG_PREFIX} probe={probe_id} event={event} outcome={outcome}");
}
