//! [DEBUG-STYLE-ZIP-1320] Dependency-free, append-only diagnostic sink.
//! Also compiled by scripts/test-style-zip-diagnostics.py without Tauri.
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_BYTES: u64 = 1024 * 1024;
const RESERVE_BYTES: u64 = 2048;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Coverage,
    Click,
    Finished,
    Unmounted,
    PluginLoading,
    PluginReady,
    PickerOpening,
    PickerSelected,
    PickerCancelled,
    Failed,
    ImportRequested,
    ContentRead,
    ContentFailed,
    ArchiveSaved,
    ArchiveFailed,
    ExportStarted,
    ExportEncoded,
    ExportWriteStarted,
    ExportSaved,
    ExportFailed,
    ListApplied,
    ListSuperseded,
    ListFailed,
    UiCommitted,
    FeedbackCommitted,
    LimitReached,
}

impl Event {
    pub fn frontend(name: &str) -> Option<Self> {
        Some(match name {
            "click" => Self::Click,
            "finished" => Self::Finished,
            "unmounted" => Self::Unmounted,
            "plugin_loading" => Self::PluginLoading,
            "plugin_ready" => Self::PluginReady,
            "picker_opening" => Self::PickerOpening,
            "picker_selected" => Self::PickerSelected,
            "picker_cancelled" => Self::PickerCancelled,
            "failed" => Self::Failed,
            "list_applied" => Self::ListApplied,
            "list_superseded" => Self::ListSuperseded,
            "list_failed" => Self::ListFailed,
            "ui_committed" => Self::UiCommitted,
            "feedback_committed" => Self::FeedbackCommitted,
            _ => return None,
        })
    }

    fn metadata(self) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Coverage => ("P1", "\"H1\",\"H2\",\"H3\"", "coverage", "partial", "backend"),
            Self::Click => ("P1", "\"H1\",\"H2\",\"H3\"", "click", "started", "frontend"),
            Self::Finished => ("P1", "\"H1\",\"H2\",\"H3\"", "finished", "returned", "frontend"),
            Self::Unmounted => ("P1", "\"H2\",\"H3\"", "unmounted", "unknown", "frontend"),
            Self::PluginLoading => ("P3", "\"H1\",\"H2\",\"H3\"", "plugin_loading", "started", "frontend"),
            Self::PluginReady => ("P3", "\"H1\",\"H2\",\"H3\"", "plugin_ready", "returned", "frontend"),
            Self::PickerOpening => ("P3", "\"H1\",\"H2\",\"H3\"", "picker_opening", "started", "frontend"),
            Self::PickerSelected => ("P3", "\"H1\",\"H2\",\"H3\"", "picker_selected", "selected", "frontend"),
            Self::PickerCancelled => ("P3", "\"H1\",\"H2\",\"H3\"", "picker_cancelled", "cancelled", "frontend"),
            Self::Failed => ("P3", "\"H1\",\"H2\",\"H3\"", "failed", "error", "frontend"),
            Self::ImportRequested => ("P4", "\"H3\"", "import_requested", "started", "backend"),
            Self::ContentRead => ("P4", "\"H3\"", "content_read", "returned", "backend"),
            Self::ContentFailed => ("P4", "\"H3\"", "content_failed", "error", "backend"),
            Self::ArchiveSaved => ("P4", "\"H3\"", "archive_saved", "returned", "backend"),
            Self::ArchiveFailed => ("P4", "\"H3\"", "archive_failed", "error", "backend"),
            Self::ExportStarted => ("P4", "\"H3\"", "export_started", "started", "backend"),
            Self::ExportEncoded => ("P4", "\"H3\"", "export_encoded", "returned", "backend"),
            Self::ExportWriteStarted => ("P4", "\"H3\"", "export_write_started", "started", "backend"),
            Self::ExportSaved => ("P4", "\"H3\"", "export_saved", "returned", "backend"),
            Self::ExportFailed => ("P4", "\"H3\"", "export_failed", "error", "backend"),
            Self::ListApplied => ("P5", "\"H3\"", "list_applied", "applied", "frontend"),
            Self::ListSuperseded => ("P5", "\"H3\"", "list_superseded", "superseded", "frontend"),
            Self::ListFailed => ("P5", "\"H3\"", "list_failed", "error", "frontend"),
            Self::UiCommitted => ("P5", "\"H3\"", "ui_committed", "observed", "frontend"),
            Self::FeedbackCommitted => ("P5", "\"H3\"", "feedback_committed", "observed", "frontend"),
            Self::LimitReached => ("P1", "\"H1\",\"H2\",\"H3\"", "coverage_limit", "disabled", "backend"),
        }
    }
}

#[derive(Default, Clone)]
pub struct Facts {
    pub bytes: Option<u64>,
    pub event_seq: Option<u32>,
    pub present: Option<bool>,
    pub busy: Option<bool>,
    pub count: Option<u32>,
    pub duration_ms: Option<u32>,
    // 0=idle, 1=saved, 2=failed, 3=saving; other values become unknown.
    pub feedback: Option<u8>,
    pub stage: Option<u8>,
    pub error_code: Option<u8>,
    pub skipped: Option<u32>,
    pub failed: Option<u32>,
    pub pending: Option<u32>,
    pub frontend_instance_id: Option<String>,
}

pub fn opaque_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub struct Sink {
    path: PathBuf,
    run_id: String,
    build_id: String,
    bytes: u64,
    pub dropped: u64,
    pub status: &'static str,
    file: Option<File>,
}

impl Sink {
    pub fn open(path: &Path, run_id: &str, build_id: &str) -> std::io::Result<Self> {
        if !opaque_id(run_id) {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        let mut file = OpenOptions::new().create(true).read(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        // A killed/failed append may leave a partial line. Preserve it as evidence,
        // disable new writes, and expose the gap rather than joining two JSON objects.
        let mut complete_tail = true;
        if bytes > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            complete_tail = last[0] == b'\n';
        }
        Ok(Self {
            path: path.to_owned(),
            run_id: run_id.to_owned(),
            build_id: if build_id.len() == 40 && build_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                build_id.to_ascii_lowercase()
            } else {
                "unknown".to_owned()
            },
            bytes,
            dropped: 0,
            status: if !complete_tail { "incomplete" } else if bytes >= MAX_BYTES - RESERVE_BYTES { "limit" } else { "ready" },
            file: Some(file),
        })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    fn line(&self, event: Event, operation: Option<&str>, facts: Facts) -> String {
        let (probe, hypotheses, name, outcome, source) = event.metadata();
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let mut safe = format!("\"source\":\"{source}\"");
        if let Some(bytes) = facts.bytes {
            safe.push_str(&format!(",\"bytes\":{bytes}"));
        }
        if let Some(seq) = facts.event_seq {
            safe.push_str(&format!(",\"event_seq\":{seq}"));
        }
        if let Some(present) = facts.present {
            safe.push_str(&format!(",\"present\":{present}"));
        }
        if let Some(busy) = facts.busy {
            safe.push_str(&format!(",\"busy\":{busy}"));
        }
        if let Some(count) = facts.count {
            safe.push_str(&format!(",\"count\":{}", count.min(1_000_000)));
        }
        if let Some(duration) = facts.duration_ms {
            safe.push_str(&format!(",\"duration_ms\":{duration}"));
        }
        if let Some(feedback) = facts.feedback {
            let state = match feedback { 0 => "idle", 1 => "saved", 2 => "failed", 3 => "saving", _ => "unknown" };
            safe.push_str(&format!(",\"feedback\":\"{state}\""));
        }
        if matches!(event, Event::Coverage | Event::LimitReached) {
            // Coverage includes observer limits; absence never proves native non-execution.
            safe.push_str(&format!(
                ",\"native_activity_covered\":false,\"plugin_callback_covered\":false,\"frontend_delivery\":\"best_effort\",\"build_id\":\"{}\",\"dropped\":{},\"subsequent_records_discarded\":{}",
                self.build_id, self.dropped, matches!(event, Event::LimitReached)
            ));
        }
        let operation = operation.filter(|id| opaque_id(id)).map(|id| format!(",\"operation_id\":\"{id}\"" )).unwrap_or_default();
        if let Some(stage) = facts.stage {
            let stage = match stage { 0 => "plugin", 1 => "picker", 2 => "import", 3 => "refresh", _ => "unknown" };
            safe.push_str(&format!(",\"stage\":\"{stage}\""));
        }
        if let Some(code) = facts.error_code {
            let code = match code {
                1 => "activity_not_found",
                2 => "picker_cancelled",
                3 => "permission_denied",
                4 => "dialog_unavailable",
                _ => "unknown",
            };
            safe.push_str(&format!(",\"error_code\":\"{code}\""));
        }
        if matches!(event, Event::Coverage) {
            let instance = facts.frontend_instance_id.as_deref().filter(|id| opaque_id(id));
            let scope = if instance.is_some() { "instance" } else { "unknown" };
            safe.push_str(&format!(",\"frontend_accounting_scope\":\"{scope}\",\"previous_frontend_instances_covered\":false"));
            if let Some(instance) = instance {
                safe.push_str(&format!(",\"frontend_instance_id\":\"{instance}\""));
            }
            for (key, value) in [("frontend_skipped", facts.skipped), ("frontend_failed", facts.failed), ("frontend_pending", facts.pending)] {
                if let Some(value) = value { safe.push_str(&format!(",\"{key}\":{value}")); }
            }
        }
        format!("{{\"schema\":\"diagnosing-bugs.v1\",\"session_id\":\"sz-a73c\",\"run_id\":\"{}\",\"probe_id\":\"{probe}\",\"hypothesis_ids\":[{hypotheses}],\"event\":\"{name}\",\"outcome\":\"{outcome}\",\"safe\":{{{safe}}},\"ts\":{ts}{operation}}}\n", self.run_id)
    }

    fn write_line(&mut self, line: &str) {
        if self.file.as_mut().is_none_or(|file| file.write_all(line.as_bytes()).is_err()) {
            self.status = "io_failure";
            self.file = None;
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.bytes += line.len() as u64;
        }
    }

    pub fn record(&mut self, event: Event, operation: Option<&str>, facts: Facts) {
        if self.status != "ready" {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let line = self.line(event, operation, facts);
        if self.bytes + line.len() as u64 > MAX_BYTES - RESERVE_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            let marker = self.line(Event::LimitReached, None, Facts::default());
            self.write_line(&marker);
            if self.status == "ready" {
                self.status = "limit";
            }
            return;
        }
        self.write_line(&line);
    }

    /// Export must include the actual current coverage, not a readable but stale log
    /// whose last record predates an I/O failure or a newly dropped frontend record.
    /// Refuse export if that record cannot be appended; preserve the original file.
    pub fn snapshot_for_export(&mut self, delivery: Facts) -> std::io::Result<Vec<u8>> {
        self.record(Event::Coverage, None, delivery);
        if self.status != "ready" {
            return Err(std::io::ErrorKind::Other.into());
        }
        self.snapshot()
    }

    /// The caller holds the same mutex as record(): no mixed/partial in-process writes.
    /// Never append a fabricated footer, merge app logs, or truncate the session log.
    pub fn snapshot(&self) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(&self.path)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        Ok(bytes)
    }
}

/// Copies both sources into one returned artifact; the source files are untouched.
pub fn bundle_export(ordinary: &[u8], snapshot: &[u8]) -> Vec<u8> {
    let mut bytes = ordinary.to_vec();
    if !bytes.ends_with(b"\n") { bytes.push(b'\n'); }
    bytes.extend_from_slice(b"\n=== OpenLess ZIP diagnostics (NDJSON; partial coverage) ===\n");
    bytes.extend_from_slice(snapshot);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn sink(name: &str) -> Sink {
        let root = PathBuf::from(std::env::var_os("STYLE_ZIP_TEST_DIR").expect("isolated test directory"));
        Sink::open(&root.join(name), "0123456789abcdef0123456789abcdef", "not-a-build-id").unwrap()
    }

    #[test]
    fn append_snapshot_and_allowlist() {
        let mut sink = sink("append.ndjson");
        sink.record(Event::Coverage, None, Facts::default());
        let before = sink.snapshot().unwrap();
        sink.record(Event::Failed, Some("content://private/password.zip\n"), Facts { count: Some(u32::MAX), ..Facts::default() });
        let bytes = sink.snapshot().unwrap();
        assert!(bytes.starts_with(&before));
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(!text.contains("private"));
        assert!(text.contains("\"count\":1000000"));
        assert!(text.contains("\"native_activity_covered\":false"));
        assert!(Event::frontend("secret error\n").is_none());
        assert!(!opaque_id("../../not-an-operation"));
    }

    #[test]
    fn bounded_append_never_rotates_or_truncates() {
        let mut sink = sink("limit.ndjson");
        for _ in 0..10_000 {
            sink.record(Event::Click, None, Facts::default());
        }
        let before = sink.snapshot().unwrap();
        assert!(before.len() as u64 <= MAX_BYTES);
        assert_eq!(sink.status, "limit");
        assert!(String::from_utf8_lossy(&before).contains("coverage_limit"));
        sink.record(Event::Finished, None, Facts::default());
        assert_eq!(before, sink.snapshot().unwrap());
        assert!(sink.dropped > 1);
        let reopened = Sink::open(&sink.path, "abcdef0123456789abcdef0123456789", "").unwrap();
        assert_eq!(reopened.status, "limit");
        assert_eq!(before, reopened.snapshot().unwrap());
    }

    #[test]
    fn concurrent_writes_keep_complete_lines() {
        let sink = Arc::new(Mutex::new(sink("concurrent.ndjson")));
        let threads: Vec<_> = (0..4).map(|_| {
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || {
                for seq in 0..25 {
                    sink.lock().unwrap().record(Event::Click, Some("abcdef0123456789abcdef0123456789"), Facts { event_seq: Some(seq), ..Facts::default() });
                }
            })
        }).collect();
        for thread in threads { thread.join().unwrap(); }
        let text = String::from_utf8(sink.lock().unwrap().snapshot().unwrap()).unwrap();
        assert_eq!(text.lines().count(), 100);
        assert!(text.lines().all(|line| line.starts_with('{') && line.ends_with('}')));
    }

    #[test]
    fn write_failure_is_disabled_without_panic() {
        let mut sink = sink("failure.ndjson");
        sink.file = None;
        sink.record(Event::Click, None, Facts::default());
        assert_eq!(sink.status, "io_failure");
        assert_eq!(sink.dropped, 1);
        sink.record(Event::Finished, None, Facts::default());
        assert_eq!(sink.dropped, 2);
    }

    #[test]
    fn incomplete_tail_is_preserved_without_new_append() {
        let root = PathBuf::from(std::env::var_os("STYLE_ZIP_TEST_DIR").unwrap());
        let path = root.join("incomplete.fixture");
        std::fs::write(&path, b"{\"schema\":").unwrap();
        let mut sink = Sink::open(&path, "abcdef0123456789abcdef0123456789", "").unwrap();
        assert_eq!(sink.status, "incomplete");
        sink.record(Event::Coverage, None, Facts::default());
        assert_eq!(sink.snapshot().unwrap().as_slice(), b"{\"schema\":");
        assert_eq!(sink.dropped, 1);
        assert!(Sink::open(&root.join("missing/parent.ndjson"), "abcdef0123456789abcdef0123456789", "").is_err());
    }

    #[test]
    fn every_event_and_safe_enum_serializes() {
        let mut sink = sink("events.ndjson");
        for event in [
            Event::Coverage, Event::Click, Event::Finished, Event::Unmounted,
            Event::PluginLoading, Event::PluginReady, Event::PickerOpening,
            Event::PickerSelected, Event::PickerCancelled, Event::Failed,
            Event::ImportRequested, Event::ContentRead, Event::ContentFailed,
            Event::ArchiveSaved, Event::ArchiveFailed, Event::ListApplied,
            Event::ExportStarted, Event::ExportEncoded, Event::ExportWriteStarted,
            Event::ExportSaved, Event::ExportFailed,
            Event::ListSuperseded, Event::ListFailed, Event::UiCommitted,
            Event::FeedbackCommitted, Event::LimitReached,
        ] {
            sink.record(event, None, Facts {
                event_seq: Some(1), present: Some(false), busy: Some(true),
                bytes: Some(5696),
                stage: Some(u8::MAX), feedback: Some(u8::MAX),
                error_code: Some(u8::MAX),
                skipped: Some(1), failed: Some(2), pending: Some(3),
                ..Facts::default()
            });
        }
        let text = String::from_utf8(sink.snapshot().unwrap()).unwrap();
        assert!(text.contains("\"stage\":\"unknown\""));
        assert!(text.contains("\"feedback\":\"unknown\""));
        assert!(text.contains("\"frontend_pending\":3"));
        assert!(text.contains("\"error_code\":\"unknown\""));
        sink.record(Event::Failed, None, Facts { error_code: Some(1), ..Facts::default() });
        let text = String::from_utf8(sink.snapshot().unwrap()).unwrap();
        assert!(text.contains("\"error_code\":\"activity_not_found\""));
    }

    #[test]
    fn delivery_accounting_is_scoped_to_the_frontend_instance() {
        let mut sink = sink("instance.ndjson");
        let first = "0123456789abcdef0123456789abcdef";
        let second = "abcdef0123456789abcdef0123456789";
        sink.record(Event::Coverage, None, Facts {
            frontend_instance_id: Some(first.to_owned()), failed: Some(1), ..Facts::default()
        });
        sink.record(Event::Coverage, None, Facts {
            frontend_instance_id: Some(second.to_owned()), failed: Some(0), ..Facts::default()
        });
        sink.record(Event::Coverage, None, Facts {
            frontend_instance_id: Some("content://private".to_owned()), ..Facts::default()
        });
        let text = String::from_utf8(sink.snapshot().unwrap()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert!(lines[0].contains(first) && lines[0].contains("\"frontend_failed\":1"));
        assert!(lines[1].contains(second) && lines[1].contains("\"frontend_failed\":0"));
        assert!(lines.iter().all(|line| line.contains("\"previous_frontend_instances_covered\":false")));
        assert!(lines[2].contains("\"frontend_accounting_scope\":\"unknown\""));
        assert!(!text.contains("content://"));
    }

    #[test]
    fn export_refuses_stale_evidence_when_coverage_cannot_be_written() {
        let mut sink = sink("export-failure.ndjson");
        sink.record(Event::Coverage, None, Facts::default());
        let before = sink.snapshot().unwrap();
        // Real write path with an unavailable descriptor: preserve readable old bytes.
        sink.file = None;
        assert!(sink.snapshot_for_export(Facts { failed: Some(1), pending: Some(2), ..Facts::default() }).is_err());
        assert_eq!(sink.status, "io_failure");
        assert_eq!(sink.snapshot().unwrap(), before);
        assert!(sink.snapshot_for_export(Facts::default()).is_err());
        assert_eq!(sink.snapshot().unwrap(), before);
        let mut ready = self::sink("export-ready.ndjson");
        let exported = ready.snapshot_for_export(Facts { failed: Some(1), pending: Some(2), ..Facts::default() }).unwrap();
        let text = String::from_utf8(exported).unwrap();
        assert!(text.contains("\"frontend_failed\":1"));
        assert!(text.contains("\"frontend_pending\":2"));
    }

    #[test]
    fn centralized_export_preserves_sources_and_contains_qualified_snapshot() {
        let ordinary = b"ordinary application log without newline";
        let mut sink = sink("bundle.ndjson");
        let snapshot = sink.snapshot_for_export(Facts { failed: Some(2), pending: Some(1), ..Facts::default() }).unwrap();
        let bundled = bundle_export(ordinary, &snapshot);
        assert!(bundled.starts_with(ordinary));
        assert!(bundled.ends_with(&snapshot));
        assert_eq!(sink.snapshot().unwrap(), snapshot);
        assert_eq!(ordinary, b"ordinary application log without newline");
        assert!(String::from_utf8(bundled).unwrap().contains("\"frontend_failed\":2"));
    }
}
