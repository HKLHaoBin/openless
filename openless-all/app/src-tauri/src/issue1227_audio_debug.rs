//! [DEBUG-MIGRATION-1192] P5/P6: capture lifetime and callback/worker accounting.
//! The native callback only updates atomics. Snapshots are taken off that callback;
//! concurrent snapshots are not transactions and must not be compared as final totals.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::issue1227_debug;

static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct CaptureProbe {
    operation: u64,
    sequence: AtomicU64,
    native_dropped: AtomicBool,
    callbacks: AtomicU64,
    callbacks_in_flight: AtomicU64,
    callbacks_after_drop: AtomicU64,
    callbacks_skipped: AtomicU64,
    unaligned_blocks: AtomicU64,
    invalid_geometry: AtomicU64,
    queued: AtomicU64,
    queued_after_drop: AtomicU64,
    queue_full: AtomicU64,
    queue_disconnected: AtomicU64,
    processing_started: AtomicU64,
    processing_completed: AtomicU64,
    empty_output: AtomicU64,
    odd_pcm_bytes: AtomicU64,
    stream_errors: AtomicU64,
}

impl CaptureProbe {
    pub(crate) fn new() -> Self {
        Self {
            operation: NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed),
            ..Self::default()
        }
    }

    pub(crate) fn lifecycle(&self, event: &'static str, outcome: &'static str) {
        issue1227_debug::record_correlated(
            "P5",
            &["H5", "H7"],
            event,
            outcome,
            (
                self.operation,
                self.sequence.fetch_add(1, Ordering::Relaxed),
            ),
            serde_json::json!({"native_dropped": self.native_dropped.load(Ordering::Acquire)}),
        );
    }

    pub(crate) fn configuration(&self, sample_rate: u32, channels: usize, format: &'static str) {
        issue1227_debug::record_correlated(
            "P6",
            &["H6"],
            "input_geometry",
            "observed",
            (
                self.operation,
                self.sequence.fetch_add(1, Ordering::Relaxed),
            ),
            serde_json::json!({"sample_rate": sample_rate, "channels": channels, "format": format}),
        );
    }

    pub(crate) fn callback(
        &self,
        samples: usize,
        channels: usize,
        sample_rate: u32,
    ) -> Callback<'_> {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.callbacks_in_flight.fetch_add(1, Ordering::Relaxed);
        if self.native_dropped.load(Ordering::Acquire) {
            self.callbacks_after_drop.fetch_add(1, Ordering::Relaxed);
        }
        if channels == 0 || sample_rate == 0 {
            self.invalid_geometry.fetch_add(1, Ordering::Relaxed);
        } else if samples % channels != 0 {
            self.unaligned_blocks.fetch_add(1, Ordering::Relaxed);
        }
        Callback(self)
    }

    pub(crate) fn skipped(&self) {
        self.callbacks_skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn queued(&self) {
        self.queued.fetch_add(1, Ordering::Relaxed);
        if self.native_dropped.load(Ordering::Acquire) {
            self.queued_after_drop.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn queue_full(&self) {
        self.queue_full.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn queue_disconnected(&self) {
        self.queue_disconnected.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn processing_started(&self) {
        self.processing_started.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn processing_completed(&self, pcm_bytes: Option<usize>) {
        match pcm_bytes {
            None => {
                self.empty_output.fetch_add(1, Ordering::Relaxed);
            }
            Some(bytes) if bytes % 2 != 0 => {
                self.odd_pcm_bytes.fetch_add(1, Ordering::Relaxed);
            }
            Some(_) => {}
        }
        self.processing_completed.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn stream_error(&self) {
        self.stream_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn native_dropped(&self) {
        self.native_dropped.store(true, Ordering::Release);
    }

    pub(crate) fn snapshot(&self, event: &'static str) {
        issue1227_debug::record_correlated(
            "P6",
            &["H5", "H6", "H7"],
            event,
            "observed",
            (
                self.operation,
                self.sequence.fetch_add(1, Ordering::Relaxed),
            ),
            self.counts(),
        );
    }

    fn counts(&self) -> serde_json::Value {
        serde_json::json!({
            "callbacks": self.callbacks.load(Ordering::Relaxed),
            "callbacks_in_flight": self.callbacks_in_flight.load(Ordering::Relaxed),
            "callbacks_after_drop": self.callbacks_after_drop.load(Ordering::Relaxed),
            "callbacks_skipped": self.callbacks_skipped.load(Ordering::Relaxed),
            "unaligned_blocks": self.unaligned_blocks.load(Ordering::Relaxed),
            "invalid_geometry": self.invalid_geometry.load(Ordering::Relaxed),
            "queued": self.queued.load(Ordering::Relaxed),
            "queued_after_drop": self.queued_after_drop.load(Ordering::Relaxed),
            "queue_full": self.queue_full.load(Ordering::Relaxed),
            "queue_disconnected": self.queue_disconnected.load(Ordering::Relaxed),
            "processing_started": self.processing_started.load(Ordering::Relaxed),
            "processing_completed": self.processing_completed.load(Ordering::Relaxed),
            "empty_output": self.empty_output.load(Ordering::Relaxed),
            "odd_pcm_bytes": self.odd_pcm_bytes.load(Ordering::Relaxed),
            "stream_errors": self.stream_errors.load(Ordering::Relaxed),
            "native_dropped": self.native_dropped.load(Ordering::Acquire),
        })
    }
}

pub(crate) struct Callback<'a>(&'a CaptureProbe);

impl Drop for Callback<'_> {
    fn drop(&mut self) {
        self.0.callbacks_in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_accounting_is_balanced_without_logging_samples() {
        let probe = CaptureProbe::new();
        {
            let _callback = probe.callback(960, 2, 48_000);
            probe.queued();
            assert_eq!(probe.counts()["callbacks_in_flight"], 1);
        }
        probe.processing_started();
        probe.processing_completed(Some(320));
        let counts = probe.counts();
        assert_eq!(counts["callbacks_in_flight"], 0);
        assert_eq!(counts["queued"], counts["processing_completed"]);
        assert_eq!(counts["unaligned_blocks"], 0);
        assert_eq!(counts["odd_pcm_bytes"], 0);
    }

    #[test]
    fn boundary_anomalies_are_counted_without_changing_delivery() {
        let probe = CaptureProbe::new();
        probe.native_dropped();
        drop(probe.callback(3, 2, 48_000));
        probe.queued();
        drop(probe.callback(0, 0, 0));
        probe.skipped();
        probe.queue_full();
        probe.queue_disconnected();
        probe.processing_started();
        probe.processing_completed(Some(3));
        probe.processing_started();
        probe.processing_completed(None);
        probe.stream_error();
        let counts = probe.counts();
        for key in [
            "queued_after_drop",
            "unaligned_blocks",
            "invalid_geometry",
            "callbacks_skipped",
            "queue_full",
            "queue_disconnected",
            "odd_pcm_bytes",
            "empty_output",
            "stream_errors",
        ] {
            assert_eq!(counts[key], 1, "{key}");
        }
        assert_eq!(counts["callbacks_after_drop"], 2);
        assert_eq!(counts["callbacks_in_flight"], 0);
    }
}
