//! [DEBUG-HEAP-1261] Diagnostic-only boundaries; remove only after user confirmation.
#![allow(dead_code)]

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SESSION: &str = "issue-1261-9e0ffd34";
const LOG_NAME: &str = "openless-issue-1261-9e0ffd34-session.ndjson";
const QUEUE_CAPACITY: usize = 512;

pub(crate) const fn enabled() -> bool {
    cfg!(all(
        target_os = "windows",
        feature = "issue-1261-diagnostics",
        not(test)
    ))
}

#[derive(Clone, Copy)]
pub(crate) enum Probe {
    Capture,
    Audio,
    Main,
    Ime,
}

impl Probe {
    fn id(self) -> &'static str {
        match self {
            Self::Capture => "P1",
            Self::Audio => "P2",
            Self::Main => "P3",
            Self::Ime => "P4",
        }
    }
    fn hypotheses(self) -> &'static [&'static str] {
        match self {
            Self::Capture => &["H1"],
            Self::Audio => &["H1", "H2"],
            Self::Main => &["H3", "H5"],
            Self::Ime => &["H4"],
        }
    }
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    CaptureConfigured,
    CaptureClosing,
    PauseReturned,
    DropStarted,
    DropReturned,
    ProcessorJoined,
    AudioStarted,
    AudioFirstOutput,
    AudioFlush,
    AudioShutdown,
    DispatchRequested,
    DispatchReturned,
    TaskEntered,
    TaskReturned,
    LayoutObserved,
    EpochSkipped,
    ImeStarted,
    ImeAcknowledged,
    ImeReturned,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Observed,
    Ok,
    Failed,
    Unknown,
}

// Only numbers and booleans can cross this API. Never accept arbitrary text or native IDs.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Key {
    SampleRate,
    Channels,
    FloatFormat,
    EnhancementEnabled,
    Callbacks,
    InFlight,
    CallbacksAfterDrop,
    Skipped,
    QueuedAfterDrop,
    QueueFull,
    QueueDisconnected,
    InputSamples,
    OutputBytes,
    OutputChunks,
    OddOutput,
    EmptyOutput,
    NativeDropped,
    Source,
    EpochCurrent,
    LayoutChanged,
    MirrorTargets,
    MacTypePresent,
    ImeOperation,
    ReplyCode,
    OutcomeUnknown,
    QueryCount,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(untagged)]
pub(crate) enum Fact {
    Count(u64),
    Flag(bool),
}

pub(crate) type Facts<'a> = &'a [(Key, Fact)];

#[derive(Clone, Copy)]
pub(crate) struct Operation {
    id: u64,
    probe: Probe,
    started: Option<Instant>,
}

impl Operation {
    pub(crate) fn new(probe: Probe) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        if !enabled() {
            return Self {
                id: 0,
                probe,
                started: None,
            };
        }
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            probe,
            started: Some(Instant::now()),
        }
    }

    pub(crate) fn record(self, event: Event, outcome: Outcome, facts: Facts<'_>) {
        let Some(started) = self.started else {
            return;
        };
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        static RUN: OnceLock<String> = OnceLock::new();
        let Some(logger) = logger() else {
            return;
        };
        let run = RUN.get_or_init(|| format!("r-{:x}", unix_nanos()));
        let record = make_record(
            self,
            event,
            outcome,
            facts,
            run,
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            started.elapsed().as_micros() as u64,
            &logger.health,
        );
        enqueue(&logger.sender, &logger.health, record);
    }

    pub(crate) fn span(self, entry: Event, exit: Event, facts: Facts<'_>) -> Span {
        self.record(entry, Outcome::Observed, facts);
        Span {
            operation: self,
            exit,
        }
    }
}

pub(crate) struct Span {
    operation: Operation,
    exit: Event,
}
impl Drop for Span {
    fn drop(&mut self) {
        self.operation.record(
            self.exit,
            if std::thread::panicking() {
                Outcome::Failed
            } else {
                Outcome::Observed
            },
            &[],
        );
    }
}

#[derive(Default)]
struct Health {
    dropped: AtomicU64,
    io_failed: AtomicBool,
}
struct Logger {
    sender: SyncSender<serde_json::Value>,
    health: Arc<Health>,
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn make_record(
    operation: Operation,
    event: Event,
    outcome: Outcome,
    facts: Facts<'_>,
    run: &str,
    sequence: u64,
    elapsed_us: u64,
    health: &Health,
) -> serde_json::Value {
    let mut safe = serde_json::Map::new();
    for (key, value) in facts {
        if let Ok(serde_json::Value::String(key)) = serde_json::to_value(key) {
            if let Ok(value) = serde_json::to_value(value) {
                safe.insert(key, value);
            }
        }
    }
    safe.insert("duration_us".into(), elapsed_us.into());
    serde_json::json!({
        "schema": "diagnosing-bugs.v1", "session_id": SESSION, "run_id": run,
        "probe_id": operation.probe.id(), "hypothesis_ids": operation.probe.hypotheses(),
        "operation_id": format!("op-{}", operation.id), "event_seq": sequence,
        "event": event, "outcome": outcome, "safe": safe,
        "coverage": { "dropped": health.dropped.load(Ordering::Relaxed),
            "io_failed": health.io_failed.load(Ordering::Relaxed) },
        "ts": (unix_nanos() / 1_000_000).to_string(),
    })
}

fn enqueue(sender: &SyncSender<serde_json::Value>, health: &Health, record: serde_json::Value) {
    match sender.try_send(record) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            health.dropped.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(_)) => {
            health.dropped.fetch_add(1, Ordering::Relaxed);
            health.io_failed.store(true, Ordering::Relaxed);
        }
    }
}

fn write_records(mut output: impl Write, receiver: Receiver<serde_json::Value>, health: &Health) {
    for record in receiver {
        let Ok(mut line) = serde_json::to_vec(&record) else {
            health.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        line.push(b'\n');
        if output.write_all(&line).is_err() {
            health.io_failed.store(true, Ordering::Relaxed);
            health.dropped.fetch_add(1, Ordering::Relaxed);
            break;
        }
    }
}

fn logger() -> &'static Option<Logger> {
    static LOGGER: OnceLock<Option<Logger>> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let (sender, receiver) = sync_channel(QUEUE_CAPACITY);
        let health = Arc::new(Health::default());
        let writer_health = Arc::clone(&health);
        std::thread::Builder::new()
            .name("heap-1261-diagnostics".into())
            .spawn(move || {
                let path = std::env::temp_dir().join(LOG_NAME);
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    Ok(file) => write_records(file, receiver, &writer_health),
                    Err(_) => {
                        writer_health.io_failed.store(true, Ordering::Relaxed);
                    }
                }
            })
            .ok()?;
        Some(Logger { sender, health })
    })
}

pub(crate) fn mac_type_present() -> bool {
    #[cfg(all(target_os = "windows", feature = "issue-1261-diagnostics", not(test)))]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
        }
        let name: Vec<u16> = "MacType64.dll\0".encode_utf16().collect();
        // Query only: no DLL is loaded, retained, called or unloaded.
        return !unsafe { GetModuleHandleW(name.as_ptr()) }.is_null();
    }
    #[cfg(not(all(target_os = "windows", feature = "issue-1261-diagnostics", not(test))))]
    false
}

pub(crate) struct CaptureProbe {
    operation: Operation,
    callbacks: AtomicU64,
    in_flight: AtomicU64,
    after_drop: AtomicU64,
    skipped: AtomicU64,
    queued_after_drop: AtomicU64,
    queue_full: AtomicU64,
    queue_disconnected: AtomicU64,
    input_samples: AtomicU64,
    output_bytes: AtomicU64,
    output_chunks: AtomicU64,
    odd_output: AtomicU64,
    empty_output: AtomicU64,
    native_dropped: AtomicBool,
}

impl CaptureProbe {
    pub(crate) fn new() -> Option<Arc<Self>> {
        if !enabled() {
            return None;
        }
        Some(Arc::new(Self::with_operation(Operation::new(
            Probe::Capture,
        ))))
    }
    fn with_operation(operation: Operation) -> Self {
        Self {
            operation,
            callbacks: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            after_drop: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
            queued_after_drop: AtomicU64::new(0),
            queue_full: AtomicU64::new(0),
            queue_disconnected: AtomicU64::new(0),
            input_samples: AtomicU64::new(0),
            output_bytes: AtomicU64::new(0),
            output_chunks: AtomicU64::new(0),
            odd_output: AtomicU64::new(0),
            empty_output: AtomicU64::new(0),
            native_dropped: AtomicBool::new(false),
        }
    }
    pub(crate) fn callback(&self, samples: usize) -> CallbackGuard<'_> {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        self.input_samples
            .fetch_add(samples as u64, Ordering::Relaxed);
        if self.native_dropped.load(Ordering::Acquire) {
            self.after_drop.fetch_add(1, Ordering::Relaxed);
        }
        CallbackGuard(self)
    }
    pub(crate) fn skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn queued(&self) {
        if self.native_dropped.load(Ordering::Acquire) {
            self.queued_after_drop.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub(crate) fn queue_failed(&self, full: bool) {
        if full {
            self.queue_full.fetch_add(1, Ordering::Relaxed);
        } else {
            self.queue_disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub(crate) fn output(&self, bytes: usize) {
        let first = self.output_chunks.fetch_add(1, Ordering::Relaxed) == 0;
        self.output_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        if bytes == 0 {
            self.empty_output.fetch_add(1, Ordering::Relaxed);
        }
        if bytes % 2 != 0 {
            self.odd_output.fetch_add(1, Ordering::Relaxed);
        }
        if first {
            self.audio(Event::AudioFirstOutput);
        }
    }
    pub(crate) fn configured(&self, rate: u32, channels: usize, float: bool) {
        self.operation.record(
            Event::CaptureConfigured,
            Outcome::Observed,
            &[
                (Key::SampleRate, Fact::Count(rate as u64)),
                (Key::Channels, Fact::Count(channels as u64)),
                (Key::FloatFormat, Fact::Flag(float)),
            ],
        );
    }
    pub(crate) fn boundary(&self, event: Event, outcome: Outcome) {
        let load = |counter: &AtomicU64| Fact::Count(counter.load(Ordering::Relaxed));
        self.operation.record(
            event,
            outcome,
            &[
                (Key::Callbacks, load(&self.callbacks)),
                (Key::InFlight, load(&self.in_flight)),
                (Key::CallbacksAfterDrop, load(&self.after_drop)),
                (Key::Skipped, load(&self.skipped)),
                (Key::QueuedAfterDrop, load(&self.queued_after_drop)),
                (Key::QueueFull, load(&self.queue_full)),
                (Key::QueueDisconnected, load(&self.queue_disconnected)),
                (Key::InputSamples, load(&self.input_samples)),
                (
                    Key::NativeDropped,
                    Fact::Flag(self.native_dropped.load(Ordering::Acquire)),
                ),
            ],
        );
    }
    pub(crate) fn dropped(&self) {
        self.native_dropped.store(true, Ordering::Release);
        self.boundary(Event::DropReturned, Outcome::Observed);
    }
    pub(crate) fn audio(&self, event: Event) {
        let mut operation = self.operation;
        operation.probe = Probe::Audio;
        operation.record(
            event,
            Outcome::Observed,
            &[
                (
                    Key::OutputBytes,
                    Fact::Count(self.output_bytes.load(Ordering::Relaxed)),
                ),
                (
                    Key::OutputChunks,
                    Fact::Count(self.output_chunks.load(Ordering::Relaxed)),
                ),
                (
                    Key::OddOutput,
                    Fact::Count(self.odd_output.load(Ordering::Relaxed)),
                ),
                (
                    Key::EmptyOutput,
                    Fact::Count(self.empty_output.load(Ordering::Relaxed)),
                ),
            ],
        );
    }
    pub(crate) fn enhancement(&self, enabled: bool) {
        let mut operation = self.operation;
        operation.probe = Probe::Audio;
        operation.record(
            Event::AudioStarted,
            Outcome::Observed,
            &[(Key::EnhancementEnabled, Fact::Flag(enabled))],
        );
    }
}

pub(crate) struct CallbackGuard<'a>(&'a CaptureProbe);
impl Drop for CallbackGuard<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn disabled_probes_do_not_initialize_the_writer() {
        assert!(!enabled());
        assert!(CaptureProbe::new().is_none());
        let operation = Operation::new(Probe::Main);
        assert!(operation.started.is_none());
        operation.record(Event::TaskEntered, Outcome::Observed, &[]);
    }

    #[test]
    fn records_have_closed_safe_fields_and_opaque_labels() {
        let record = make_record(
            Operation {
                id: 17,
                probe: Probe::Ime,
                started: None,
            },
            Event::ImeReturned,
            Outcome::Unknown,
            &[
                (Key::OutcomeUnknown, Fact::Flag(true)),
                (Key::QueryCount, Fact::Count(2)),
            ],
            "r-synthetic",
            3,
            10,
            &Health::default(),
        );
        assert_eq!(record["operation_id"], "op-17");
        assert_eq!(record["probe_id"], "P4");
        assert_eq!(record["schema"], "diagnosing-bugs.v1");
        assert_eq!(record["hypothesis_ids"], serde_json::json!(["H4"]));
        assert!(record["safe"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| value.is_boolean() || value.is_u64()));
        assert!(record.get("text").is_none());
        assert!(record.get("token").is_none());
    }

    #[test]
    fn full_and_disconnected_queues_are_observable_not_blocking() {
        let (sender, receiver) = sync_channel(1);
        let health = Health::default();
        enqueue(&sender, &health, serde_json::json!(1));
        enqueue(&sender, &health, serde_json::json!(2));
        assert_eq!(health.dropped.load(Ordering::Relaxed), 1);
        assert!(!health.io_failed.load(Ordering::Relaxed));
        drop(receiver);
        enqueue(&sender, &health, serde_json::json!(3));
        assert!(health.io_failed.load(Ordering::Relaxed));
        assert_eq!(health.dropped.load(Ordering::Relaxed), 2);
    }

    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn concurrent_producers_have_one_complete_line_per_record() {
        let (sender, receiver) = sync_channel(512);
        let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
        let output = buffer.clone();
        let writer =
            std::thread::spawn(move || write_records(output, receiver, &Health::default()));
        let producers: Vec<_> = (0..4)
            .map(|producer| {
                let sender = sender.clone();
                std::thread::spawn(move || {
                    for seq in 0..32 {
                        sender
                            .send(serde_json::json!({"producer":producer,"seq":seq}))
                            .unwrap();
                    }
                })
            })
            .collect();
        for producer in producers {
            producer.join().unwrap();
        }
        drop(sender);
        writer.join().unwrap();
        let bytes = buffer.0.lock().unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(text.lines().count(), 128);
        for line in text.lines() {
            assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
        }
    }

    #[test]
    fn writer_reports_io_failure_without_panicking() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (sender, receiver) = sync_channel(1);
        sender
            .send(serde_json::json!({"event":"synthetic"}))
            .unwrap();
        drop(sender);
        let health = Health::default();
        write_records(Broken, receiver, &health);
        assert!(health.io_failed.load(Ordering::Relaxed));
        assert_eq!(health.dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn callbacks_and_output_counters_do_not_mistake_drain_for_a_late_callback() {
        let probe = CaptureProbe::with_operation(Operation::new(Probe::Capture));
        {
            let _guard = probe.callback(480);
            assert_eq!(probe.in_flight.load(Ordering::Relaxed), 1);
            probe.queued();
        }
        assert_eq!(probe.in_flight.load(Ordering::Relaxed), 0);
        probe.dropped();
        probe.output(320);
        assert_eq!(probe.after_drop.load(Ordering::Relaxed), 0);
        {
            let _guard = probe.callback(0);
            probe.skipped();
        }
        assert_eq!(probe.after_drop.load(Ordering::Relaxed), 1);
        assert_eq!(probe.queued_after_drop.load(Ordering::Relaxed), 0);
        assert_eq!(probe.output_bytes.load(Ordering::Relaxed), 320);
    }
}
