use anyhow::{Context, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(all(target_os = "windows", not(test)))]
use crate::issue1227_debug;

/// A timed-out native call cannot be forcibly stopped. Its worker owns the
/// permit and model until it actually exits; later calls wait asynchronously.
pub(super) async fn run_serialized_decode<T: Send + 'static>(
    gate: Arc<tokio::sync::Semaphore>,
    timeout: std::time::Duration,
    decode: impl FnOnce(Arc<AtomicBool>) -> Result<T> + Send + 'static,
) -> Result<T> {
    #[cfg(all(target_os = "windows", not(test)))]
    issue1227_debug::record(
        "P4",
        &["H4"],
        "decode_enter",
        "entered",
        serde_json::json!({"gate_present": true}),
    );
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancelled));
    let timed = tokio::time::timeout(timeout, async move {
        let permit = gate
            .acquire_owned()
            .await
            .context("sherpa decode gate closed")?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if cancelled.load(Ordering::Acquire) {
                anyhow::bail!("sherpa-onnx transcribe cancelled");
            }
            let result = decode(cancelled);
            #[cfg(all(target_os = "windows", not(test)))]
            issue1227_debug::record(
                "P4",
                &["H4"],
                "decode_worker",
                if result.is_ok() {
                    "succeeded"
                } else {
                    "failed"
                },
                serde_json::json!({"worker_returned": true}),
            );
            result
        })
        .await
        .context("sherpa-onnx transcribe worker failed")?
    })
    .await;
    if timed.is_err() {
        #[cfg(all(target_os = "windows", not(test)))]
        issue1227_debug::record(
            "P4",
            &["H4"],
            "decode_timeout",
            "timed_out",
            serde_json::json!({"worker_returned": false}),
        );
    }
    timed.context("sherpa-onnx transcribe timeout")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn timed_out_or_dropped_decode_keeps_exclusive_model_ownership_until_worker_exits() {
        use std::time::Duration;
        for abort_caller in [false, true] {
            let gate = Arc::new(tokio::sync::Semaphore::new(1));
            let model = Arc::new(());
            let model_alive = Arc::downgrade(&model);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let worker_gate = Arc::clone(&gate);
            let caller = tokio::spawn(run_serialized_decode(
                worker_gate,
                Duration::from_millis(200),
                move |cancelled| {
                    let _model = model;
                    let _ = started_tx.send(Arc::clone(&cancelled));
                    release_rx.recv_timeout(Duration::from_secs(3))?;
                    Ok(())
                },
            ));
            let cancelled = tokio::time::timeout(Duration::from_secs(2), started_rx)
                .await
                .unwrap()
                .unwrap();
            if abort_caller {
                caller.abort();
                assert!(caller.await.unwrap_err().is_cancelled());
            } else {
                assert!(caller
                    .await
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains("timeout"));
            }
            assert!(cancelled.load(Ordering::Acquire));
            assert!(
                model_alive.upgrade().is_some(),
                "in-flight native model must stay alive"
            );
            assert_eq!(gate.available_permits(), 0);
            let queued: Result<()> =
                run_serialized_decode(Arc::clone(&gate), Duration::from_millis(10), |_| {
                    panic!("a second decode must not overlap the timed-out native call")
                })
                .await;
            assert!(queued.is_err());
            release_tx.send(()).unwrap();
            let permit = tokio::time::timeout(Duration::from_secs(2), gate.acquire_owned())
                .await
                .unwrap()
                .unwrap();
            assert!(model_alive.upgrade().is_none());
            drop(permit);
        }
    }
}
