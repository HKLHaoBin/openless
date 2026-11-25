//! Issue #1460 Windows voice-crash observations.
//!
//! This module is compile-time opt-in for the diagnostic Windows package. It emits
//! one-line NDJSON through the existing application logger; it never opens files,
//! waits, retries, or records user audio/text/configuration.

use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const SESSION_ID: &str = "issue-1460-windows-voice-crash";
const ATTEMPT_ID: &str = "A1-instrumented-audio-lifecycle";
const CHANGE_ID: &str = "C1-voice-crash-probes";
static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RUN_ID: OnceLock<String> = OnceLock::new();

pub(crate) fn initialize() {
    record(
        "P1",
        &["H1", "H2", "H3", "H4"],
        "process_start",
        "observed",
        serde_json::json!({
            "diagnostics_enabled": enabled(),
            "build_id": build_id(),
        }),
    );
}

pub(crate) fn record(
    probe_id: &'static str,
    hypotheses: &'static [&'static str],
    event: &'static str,
    outcome: &'static str,
    safe: Value,
) {
    #[cfg(all(target_os = "windows", feature = "issue-1460-diagnostics"))]
    {
        let record = serde_json::json!({
            "schema": "diagnosing-bugs.v1",
            "session_id": SESSION_ID,
            "attempt_id": ATTEMPT_ID,
            "change_id": CHANGE_ID,
            "run_id": run_id(),
            "probe_id": probe_id,
            "hypothesis_ids": hypotheses,
            "event": event,
            "outcome": outcome,
            "safe": safe,
            "event_seq": EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            "ts": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        });
        log::info!("[DEBUG-ISSUE1460] {}", record);
    }

    #[cfg(not(all(target_os = "windows", feature = "issue-1460-diagnostics")))]
    let _ = (probe_id, hypotheses, event, outcome, safe);
}

pub(crate) const fn enabled() -> bool {
    cfg!(all(target_os = "windows", feature = "issue-1460-diagnostics"))
}

fn run_id() -> &'static str {
    RUN_ID.get_or_init(|| {
        let started_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("pid-{}-start-{}", std::process::id(), started_at_ms)
    })
}

fn build_id() -> &'static str {
    let value = option_env!("OPENLESS_BUILD_ID").unwrap_or("unbound");
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        value
    } else {
        "unbound"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_id_is_redacted_to_a_full_revision_or_unbound() {
        assert!(build_id() == "unbound" || build_id().len() == 40);
    }

    #[test]
    fn diagnostic_constants_are_issue_scoped() {
        assert_eq!(SESSION_ID, "issue-1460-windows-voice-crash");
        assert_eq!(ATTEMPT_ID, "A1-instrumented-audio-lifecycle");
        assert_eq!(CHANGE_ID, "C1-voice-crash-probes");
    }
}
