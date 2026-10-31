//! [DEBUG-STYLE-ZIP-1320] Four boundaries: P1 input/termination, P3 picker promise,
//! P4 import/read/archive, P5 list application and React commit. No native callback coverage.
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Correlation {
    pub run_id: String,
    pub operation_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendFacts {
    pub event_seq: Option<u32>,
    pub present: Option<bool>,
    pub busy: Option<bool>,
    pub count: Option<u32>,
    pub duration_ms: Option<u32>,
    pub feedback: Option<u8>,
    pub stage: Option<u8>,
    pub error_code: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendDelivery {
    instance_id: Option<String>,
    skipped: u32,
    failed: u32,
    pending: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    enabled: bool,
    run_id: Option<String>,
    status: &'static str,
    dropped: u64,
    native_activity_covered: bool,
}

#[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
mod enabled {
    use super::*;
    use crate::issue1320_sink::{opaque_id, Event, Facts, Sink};
    use std::sync::{atomic::{AtomicU32, Ordering}, Mutex, OnceLock};

    // One writer per process. No logging worker, timer, retry, ordinary app logger,
    // or unbounded queue. Filesystem latency is deliberate disclosed observer overhead.
    static SINK: OnceLock<Result<Mutex<Sink>, &'static str>> = OnceLock::new();
    static EVENT_SEQ: AtomicU32 = AtomicU32::new(0);

    fn sink() -> Result<&'static Mutex<Sink>, &'static str> {
        SINK.get_or_init(|| {
            let files = crate::android::jni::android::app_files_dir().map_err(|_| "init_failure")?;
            let directory = std::path::PathBuf::from(files).join("OpenLess");
            std::fs::create_dir_all(&directory).map_err(|_| "init_failure")?;
            let mut sink = Sink::open(
                &directory.join("issue-1320-style-zip.ndjson"),
                &uuid::Uuid::new_v4().simple().to_string(),
                option_env!("OPENLESS_BUILD_ID").unwrap_or("unknown"),
            ).map_err(|_| "init_failure")?;
            sink.record(Event::Coverage, None, Facts::default());
            Ok(Mutex::new(sink))
        }).as_ref().map_err(|status| *status)
    }

    pub fn coverage() -> Coverage {
        match sink().and_then(|sink| sink.lock().map_err(|_| "lock_failure")) {
            Ok(sink) => Coverage {
                enabled: true,
                run_id: Some(sink.run_id().to_owned()),
                status: sink.status,
                dropped: sink.dropped,
                native_activity_covered: false,
            },
            Err(status) => Coverage {
                enabled: true, run_id: None, status, dropped: 0, native_activity_covered: false,
            },
        }
    }

    pub fn observe(event: Event, correlation: Option<&Correlation>, mut facts: Facts) {
        facts.event_seq = Some(EVENT_SEQ.fetch_add(1, Ordering::Relaxed));
        // A backend-only build must never initialize a diagnostic file on a product import.
        let Some(Ok(sink)) = SINK.get() else { return; };
        let Ok(mut sink) = sink.lock() else { return; };
        let operation = correlation.filter(|context| {
            context.run_id == sink.run_id() && opaque_id(&context.operation_id)
        }).map(|context| context.operation_id.as_str());
        // No guessed association with the latest click. Uncorrelated imports remain uncorrelated.
        sink.record(event, operation, facts);
    }

    pub fn backend_context() -> Option<Correlation> {
        let Some(Ok(sink)) = SINK.get() else { return None; };
        let sink = sink.lock().ok()?;
        Some(Correlation {
            run_id: sink.run_id().to_owned(),
            operation_id: uuid::Uuid::new_v4().simple().to_string(),
        })
    }

    pub fn frontend(event: &str, correlation: &Correlation, facts: FrontendFacts) -> Result<(), &'static str> {
        let event = Event::frontend(event).ok_or("invalid_event")?;
        let sink = sink()?;
        let mut sink = sink.lock().map_err(|_| "lock_failure")?;
        if correlation.run_id != sink.run_id() || !opaque_id(&correlation.operation_id) {
            sink.dropped = sink.dropped.saturating_add(1);
            return Err("context_mismatch");
        }
        sink.record(event, Some(&correlation.operation_id), Facts {
            event_seq: facts.event_seq, present: facts.present, busy: facts.busy,
            count: facts.count, duration_ms: facts.duration_ms, feedback: facts.feedback,
            stage: facts.stage,
            error_code: facts.error_code,
            ..Facts::default()
        });
        if sink.status != "ready" { return Err(sink.status); }
        Ok(())
    }

    pub fn snapshot(delivery: Option<FrontendDelivery>) -> Result<Vec<u8>, &'static str> {
        let bytes = {
            let mut sink = sink()?.lock().map_err(|_| "lock_failure")?;
            let facts = if let Some(delivery) = delivery {
                Facts {
                    skipped: Some(delivery.skipped), failed: Some(delivery.failed),
                    pending: Some(delivery.pending), frontend_instance_id: delivery.instance_id,
                    ..Facts::default()
                }
            } else {
                Facts::default()
            };
            sink.snapshot_for_export(facts).map_err(|_| "snapshot_or_coverage_failure")?
        };
        Ok(bytes)
    }
}

#[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
pub use enabled::{observe, backend_context};

#[tauri::command]
pub fn style_zip_diagnostic_context() -> Coverage {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    { enabled::coverage() }
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    { Coverage { enabled: false, run_id: None, status: "disabled", dropped: 0, native_activity_covered: false } }
}

#[tauri::command]
pub fn record_style_zip_diagnostic(event: String, correlation: Correlation, facts: FrontendFacts) -> Result<(), String> {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    { enabled::frontend(&event, &correlation, facts).map_err(str::to_owned) }
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    { let _ = (event, correlation, facts); Err("disabled".to_owned()) }
}

/// Bundle only in the exported copy; never append to the ordinary application log.
pub(crate) fn include_in_error_log(bytes: Vec<u8>, frontend_coverage: Option<FrontendDelivery>) -> Result<Vec<u8>, String> {
    #[cfg(all(target_os = "android", feature = "issue-1320-diagnostics"))]
    {
        let snapshot = enabled::snapshot(frontend_coverage).map_err(str::to_owned)?;
        Ok(crate::issue1320_sink::bundle_export(&bytes, &snapshot))
    }
    #[cfg(not(all(target_os = "android", feature = "issue-1320-diagnostics")))]
    { let _ = frontend_coverage; Ok(bytes) }
}
