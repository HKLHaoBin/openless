//! [DEBUG-ISSUE-1266] Observations only; retain until the original symptom is verified.
//! Producers never serialize, use the ordinary logger, write files or wait for capacity.
//! `try_send` is bounded, not a promise of lock freedom or zero observer overhead.
#![allow(dead_code)]

use serde::Serialize;

pub(crate) const SESSION: &str = "issue-1266-2e2c7a47";
pub(crate) const LOG_NAME: &str = "openless-issue-1266-2e2c7a47-session.ndjson";

pub(crate) const fn enabled() -> bool {
    cfg!(all(target_os = "windows", feature = "issue-1266-diagnostics"))
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum Probe {
    P1, // H1/H5: real host gate ownership
    P2, // H1/H5: real main-thread dispatch
    P3, // H2: dictation bridge consumption/handler
    P4, // H3: IME blocking admission/worker/ack/terminal
    P5, // H4: recorder shutdown
    P6, // H1/H2/H3/H4/H5: real lifecycle, not heartbeat
}

impl Probe {
    fn hypotheses(self) -> &'static [&'static str] {
        match self {
            Self::P1 | Self::P2 => &["H1", "H5"],
            Self::P3 => &["H2"],
            Self::P4 => &["H3"],
            Self::P5 => &["H4"],
            Self::P6 => &["H1", "H2", "H3", "H4", "H5"],
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    Requested, Acquired, Released, DispatchReturned, Entered, Returned,
    Consumed, HandlerEntered, HandlerReturned, ScopeLeft,
    AckWait, AckReceived, AckTimedOut, WindowFound, SubmitAck, QueryCount,
    ProcessorLock, ProcessorReleased, StopSignalled, JoinLock, JoinEntered,
    JoinReturned, ArchiveFinish, ArchiveSend, ArchiveSent, ArchiveReturned,
    Ready, CloseRequested, Destroyed, ExitRequested, Exit,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome { Observed, Returned, Ok, Error, Unknown, Accepted, Rejected, Skipped }

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    #[default]
    None,
    Settings, MainHotkey, QaHotkey, SelectionHotkey, CodingHotkey, ComboHotkey,
    TranslationHotkey, ActionHotkey, StyleHotkey,
    Pressed, Released, OtherEdge, Recorder, Archive, ActiveRecording,
    MainWindow, QaWindow, CapsuleWindow, LessWindow, OtherWindow,
}

/// Numeric facts are bounded internal status/counts, never resource identities.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct Facts {
    pub kind: Kind,
    pub value: u64,
    pub flag: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Operation { id: u64, parent: u64, probe: Probe }

impl Operation {
    pub(crate) fn new(probe: Probe) -> Self { Self::child(probe, None) }

    pub(crate) fn child(probe: Probe, parent: Option<Self>) -> Self {
        #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
        let id = sink::NEXT_OPERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(all(target_os = "windows", feature = "issue-1266-diagnostics")))]
        let id = 0;
        Self { id, parent: parent.map_or(0, |operation| operation.id), probe }
    }

    pub(crate) fn record(self, event: Event, outcome: Outcome, facts: Facts) {
        #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
        sink::record(self, event, outcome, facts);
        #[cfg(not(all(target_os = "windows", feature = "issue-1266-diagnostics")))]
        let _ = (self, event, outcome, facts);
    }

    pub(crate) fn mark(self, event: Event) {
        self.record(event, Outcome::Observed, Facts::default());
    }

    /// Drop means only scope-left (including unwind/cancellation), never success.
    pub(crate) fn scope(self) -> Scope { Scope(self) }
}

pub(crate) struct Scope(Operation);
impl Drop for Scope {
    fn drop(&mut self) { self.0.mark(Event::ScopeLeft); }
}

#[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
pub(crate) struct HostGuard<'a> {
    guard: Option<parking_lot::MutexGuard<'a, ()>>,
    operation: Operation,
    kind: Kind,
}
#[cfg(not(all(target_os = "windows", feature = "issue-1266-diagnostics")))]
pub(crate) type HostGuard<'a> = parking_lot::MutexGuard<'a, ()>;

pub(crate) fn lock_host(mutex: &parking_lot::Mutex<()>, kind: Kind) -> HostGuard<'_> {
    #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
    {
        let operation = Operation::new(Probe::P1);
        operation.record(Event::Requested, Outcome::Observed, Facts { kind, ..Facts::default() });
        let guard = mutex.lock();
        operation.record(Event::Acquired, Outcome::Observed, Facts { kind, ..Facts::default() });
        HostGuard { guard: Some(guard), operation, kind }
    }
    #[cfg(not(all(target_os = "windows", feature = "issue-1266-diagnostics")))]
    { let _ = kind; mutex.lock() }
}

#[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
impl Drop for HostGuard<'_> {
    fn drop(&mut self) {
        drop(self.guard.take());
        self.operation.record(Event::Released, Outcome::Observed,
            Facts { kind: self.kind, ..Facts::default() });
    }
}

pub(crate) fn initialize() {
    #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics", not(test)))]
    sink::initialize();
}

pub(crate) fn build_id() -> &'static str {
    let value = option_env!("OPENLESS_BUILD_ID").unwrap_or("unbound");
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) { value }
    else { "unbound" }
}

/// Old voice-1003 identities do not cover any of this session's six probes.
/// Only closed statuses are counted; arbitrary legacy JSON is never forwarded.
fn legacy_is_mapped(event: &str, outcome: &str) -> bool {
    matches!((event, outcome), ("cancel", "entered") | ("cancel", "returned"))
}

pub(crate) fn legacy(event: &str, outcome: &str) {
    #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
    if let Some(sink) = sink::SINK.get() {
        use std::sync::atomic::Ordering::Relaxed;
        let counter = if legacy_is_mapped(event, outcome) { &sink.health.legacy_mapped }
            else { &sink.health.legacy_unmapped };
        counter.fetch_add(1, Relaxed);
    }
    #[cfg(not(all(target_os = "windows", feature = "issue-1266-diagnostics")))]
    let _ = (event, outcome);
}

#[derive(Default, Serialize)]
pub(crate) struct Health {
    enabled: bool,
    build_id: &'static str,
    run_id: Option<String>,
    initialized: bool,
    ready: bool,
    dropped: u64,
    queue_closed: u64,
    legacy_mapped: u64,
    legacy_unmapped: u64,
    spawn_failed: bool,
    io_failed: bool,
    written: u64,
    flushed: u64,
}

/// Independent counters, not a record through the possibly failed channel.
/// A snapshot is not transactional; flush is an OS write, not durable fsync.
pub(crate) fn health() -> Health {
    #[allow(unused_mut)]
    let mut health = Health { enabled: enabled(), build_id: build_id(), ..Health::default() };
    #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics"))]
    if let Some(sink) = sink::SINK.get() {
        sink.health.snapshot(&mut health);
        health.run_id = Some(format!("r{:x}", sink.run));
    }
    health
}

#[cfg(any(test, all(target_os = "windows", feature = "issue-1266-diagnostics")))]
mod sink {
    use super::*;
    use std::io::{self, Write};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
    use std::sync::{Arc, OnceLock};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    pub(super) static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);
    // One source sequence per probe, not a global clock or cross-probe causality.
    static SEQUENCES: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
    pub(super) static SINK: OnceLock<Sink> = OnceLock::new();

    #[derive(Default)]
    pub(super) struct HealthState {
        initialized: AtomicBool,
        ready: AtomicBool,
        dropped: AtomicU64,
        queue_closed: AtomicU64,
        pub(super) legacy_mapped: AtomicU64,
        pub(super) legacy_unmapped: AtomicU64,
        spawn_failed: AtomicBool,
        io_failed: AtomicBool,
        written: AtomicU64,
        flushed: AtomicU64,
    }
    impl HealthState {
        pub(super) fn snapshot(&self, health: &mut Health) {
            health.initialized = self.initialized.load(Relaxed);
            health.ready = self.ready.load(Relaxed);
            health.dropped = self.dropped.load(Relaxed);
            health.queue_closed = self.queue_closed.load(Relaxed);
            health.legacy_mapped = self.legacy_mapped.load(Relaxed);
            health.legacy_unmapped = self.legacy_unmapped.load(Relaxed);
            health.spawn_failed = self.spawn_failed.load(Relaxed);
            health.io_failed = self.io_failed.load(Relaxed);
            health.written = self.written.load(Relaxed);
            health.flushed = self.flushed.load(Relaxed);
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct Record {
        operation: Operation,
        event: Event,
        outcome: Outcome,
        facts: Facts,
        seq: u64,
        elapsed_us: u64,
    }

    pub(super) struct Sink {
        sender: SyncSender<Record>,
        pub(super) health: Arc<HealthState>,
        started: Instant,
        run: u128,
    }

    impl Sink {
        fn submit(&self, record: Record) {
            match self.sender.try_send(record) {
                Ok(()) => {},
                Err(TrySendError::Full(_)) => { self.health.dropped.fetch_add(1, Relaxed); },
                Err(TrySendError::Disconnected(_)) => {
                    self.health.dropped.fetch_add(1, Relaxed);
                    self.health.queue_closed.fetch_add(1, Relaxed);
                },
            }
        }
    }

    pub(super) fn record(operation: Operation, event: Event, outcome: Outcome, facts: Facts) {
        // No lazy spawn, channel allocation or file initialization in a probe.
        if let Some(sink) = SINK.get() {
            sink.submit(Record {
                operation, event, outcome, facts,
                seq: SEQUENCES[operation.probe as usize].fetch_add(1, Relaxed),
                elapsed_us: sink.started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            });
        }
    }

    fn encode(record: Record, run: u128, started_ms: u128) -> io::Result<Vec<u8>> {
        let operation = record.operation;
        let mut line = serde_json::to_vec(&serde_json::json!({
            "schema": "diagnosing-bugs.v1", "session_id": SESSION,
            "run_id": format!("r{run:x}"), "build_id": build_id(),
            "attempt_id": "A0-observation", "change_id": "C0-no-behavior-change",
            "probe_id": operation.probe, "hypothesis_ids": operation.probe.hypotheses(),
            "operation_id": operation.id, "parent_operation_id": (operation.parent != 0).then_some(operation.parent),
            "event_seq": record.seq, "event": record.event, "outcome": record.outcome,
            "safe": record.facts, "monotonic_us": record.elapsed_us,
            "ts": (started_ms + u128::from(record.elapsed_us / 1000)).to_string(),
        }))?;
        line.push(b'\n');
        Ok(line)
    }

    fn write_record(writer: &mut impl Write, record: Record, run: u128, start: u128,
                    health: &HealthState) -> io::Result<()> {
        writer.write_all(&encode(record, run, start)?)?;
        health.written.fetch_add(1, Relaxed);
        writer.flush()?;
        health.flushed.fetch_add(1, Relaxed);
        Ok(())
    }

    fn drain(mut writer: impl Write, rx: Receiver<Record>, run: u128, start: u128,
             health: &HealthState) {
        health.ready.store(true, Relaxed);
        for record in rx {
            if write_record(&mut writer, record, run, start, health).is_err() {
                health.io_failed.store(true, Relaxed);
                break;
            }
        }
        health.ready.store(false, Relaxed);
    }

    #[cfg(all(target_os = "windows", feature = "issue-1266-diagnostics", not(test)))]
    pub(super) fn initialize() {
        SINK.get_or_init(|| {
            let (sender, receiver) = sync_channel(512);
            let health = Arc::new(HealthState::default());
            health.initialized.store(true, Relaxed);
            let writer_health = Arc::clone(&health);
            let started = Instant::now();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            if std::thread::Builder::new().name("issue-1266-writer".into()).spawn(move || {
                // Only the writer touches the session-owned append-only file.
                match std::fs::OpenOptions::new().create(true).append(true)
                    .open(std::env::temp_dir().join(LOG_NAME)) {
                    Ok(file) => drain(file, receiver, now.as_nanos(), now.as_millis(), &writer_health),
                    Err(_) => { writer_health.io_failed.store(true, Relaxed); },
                }
            }).is_err() { health.spawn_failed.store(true, Relaxed); }
            Sink { sender, health, started, run: now.as_nanos() }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn sample() -> Record {
            Record { operation: Operation { id: 2, parent: 1, probe: Probe::P5 },
                event: Event::ScopeLeft, outcome: Outcome::Observed,
                facts: Facts { kind: Kind::Recorder, value: 0, flag: false }, seq: 4, elapsed_us: 1234 }
        }
        #[test]
        fn schema_is_typed_single_line_and_scope_left_is_not_success() {
            let line = encode(sample(), 123, 456).unwrap();
            assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
            let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(value["schema"], "diagnosing-bugs.v1");
            assert_eq!(value["probe_id"], "P5");
            assert_eq!(value["hypothesis_ids"], serde_json::json!(["H4"]));
            assert_eq!(value["event"], "scope_left");
            assert_eq!(value["outcome"], "observed");
            assert_eq!(value["parent_operation_id"], 1);
            assert_eq!(value["event_seq"], 4);
            let safe = value["safe"].as_object().unwrap();
            assert_eq!(safe.keys().map(String::as_str).collect::<Vec<_>>(), ["flag", "kind", "value"]);
            assert!(safe.values().all(|v| v.is_boolean() || v.is_number() || v == "recorder"));
        }
        #[test]
        fn full_and_disconnected_are_counted_without_waiting() {
            let (sender, receiver) = sync_channel(1);
            let sink = Sink { sender, health: Arc::default(), started: Instant::now(), run: 1 };
            sink.submit(sample());
            sink.submit(sample());
            assert_eq!(sink.health.dropped.load(Relaxed), 1);
            assert_eq!(sink.health.queue_closed.load(Relaxed), 0);
            receiver.try_recv().unwrap();
            drop(receiver);
            sink.submit(sample());
            assert_eq!(sink.health.dropped.load(Relaxed), 2);
            assert_eq!(sink.health.queue_closed.load(Relaxed), 1);
        }
        struct FailFlush(Vec<u8>);
        impl Write for FailFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { self.0.write(bytes) }
            fn flush(&mut self) -> io::Result<()> { Err(io::Error::other("injected")) }
        }
        #[test]
        fn io_failure_retains_independent_write_and_flush_watermarks() {
            let (tx, rx) = sync_channel(1);
            tx.send(sample()).unwrap(); drop(tx);
            let health = HealthState::default();
            drain(FailFlush(Vec::new()), rx, 1, 2, &health);
            assert!(health.io_failed.load(Relaxed));
            assert!(!health.ready.load(Relaxed));
            assert_eq!(health.written.load(Relaxed), 1);
            assert_eq!(health.flushed.load(Relaxed), 0);
        }
        #[test]
        fn successful_writer_records_every_complete_line() {
            let (tx, rx) = sync_channel(2);
            tx.send(sample()).unwrap(); tx.send(sample()).unwrap(); drop(tx);
            let health = HealthState::default();
            let mut bytes = Vec::new();
            drain(&mut bytes, rx, 1, 2, &health);
            assert_eq!(health.flushed.load(Relaxed), 2);
            assert_eq!(bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()).count(), 2);
        }
        #[test]
        fn probe_sources_have_independent_sequences() {
            let first = SEQUENCES[0].fetch_add(1, Relaxed);
            SEQUENCES[1].fetch_add(1, Relaxed);
            assert_eq!(SEQUENCES[0].fetch_add(1, Relaxed), first + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_during_tests_never_opens_a_session_file() {
        initialize();
        assert!(sink::SINK.get().is_none());
        let mutex = parking_lot::Mutex::new(());
        {
            let _guard = lock_host(&mutex, Kind::Settings);
            assert!(mutex.try_lock().is_none());
        }
        assert!(mutex.try_lock().is_some());
    }
    #[test]
    fn original_if_let_temporary_guard_stays_held_through_join_body() {
        let mutex = parking_lot::Mutex::new(Some(()));
        if let Some(()) = mutex.lock().take() {
            assert!(mutex.try_lock().is_none());
        }
        assert!(mutex.try_lock().is_some());
    }
    #[test]
    fn legacy_whitelist_does_not_accept_free_text_or_old_probe_ids() {
        assert!(legacy_is_mapped("cancel", "entered"));
        assert!(legacy_is_mapped("cancel", "returned"));
        assert!(!legacy_is_mapped("P5", "returned"));
        assert!(!legacy_is_mapped("native_result", "personal text"));
        assert!(!legacy_is_mapped("cancel", "successful secret"));
    }
    #[test]
    fn disabled_facade_has_no_initialized_writer() {
        if !enabled() {
            initialize();
            assert!(!health().initialized);
            assert!(!health().ready);
            assert!(health().run_id.is_none());
        }
    }
    #[test]
    fn build_identity_is_fixed_or_unbound() {
        assert!(build_id() == "unbound" || build_id().len() == 40);
    }
}
