# Issue #1227: diagnostic checkpoint, not a crash fix

## Scope and symbol audit (2026-10-21)

- Original report: Windows 10, OpenLess 2.0.1, silent exit with `0xc0000374`.
- Additional report: Windows 11 Build 26200, 2.0.2-Beta.1+build.20261017,
  cloud ASR/polish, paste insertion, 48 kHz stereo F32 input. Exits have occurred
  both during recording and around normal stop. There is no deterministic trigger.
- Evidence source: https://github.com/Open-Less/openless/issues/1227#issuecomment-6284976066
  (reporter's event/log/dump-analysis summary, not raw dumps or complete logs).
- The reported EXE SHA-256 is
  `0B22A30876369839A1248C25FCA211932FE39C8A033911523D2F2C23BB3AE4D3`.
  The same exception class does not establish the same corruption path.
- Release source: `5ab6ed438a3d8b3a7420b705e7b4cec5fcdf67c4`.
  https://github.com/Open-Less/openless/actions/runs/39039176653
- That run's artifact inventory has Windows installer/updater artifacts but no PDB
  or raw EXE/PDB pair. The release assets likewise have no symbols. The workflow
  uploads installers, not the runner's target tree. The release profile does not
  enable debug information and uses `strip = "symbols"` on Windows.
- No matching historical PDB has been located in those retained artifacts. No
  local faulting EXE or minidump was supplied to this session, so its RSDS GUID/Age
  has not been independently extracted. The quoted `openless+offset` frames
  remain unresolved: rebuilding that commit cannot establish a symbol match.
- Heap failure in `RtlFreeHeap` identifies detection, not the original writer.
  `CallNextSubclassProc` is a UI callback lead, not proof of UI causation. Neither
  the audio device nor TSF is established as the cause.

## Normal behavior and semantic pass

| Contract | Expected behavior and source | Existing evidence | Status / next check |
| --- | --- | --- | --- |
| N1 | Process survives recording, cancellation, and normal stop; user expectation | Reported repeated heap-corruption exits, including during recording | VIOLATION in reported runs; collect source-correlated evidence |
| N2 | Native capture stops before the processing FIFO shutdown barrier; accepted frames drain before stop returns (`run_audio_thread`, `AudioProcessingWorker::finish`, `Recorder::stop`) | Only last application-log messages and release stack; no callback/worker accounting | INSUFFICIENT_EVIDENCE; compare settled P6 counts and P5 lifecycle |
| N3 | Input is interleaved in its declared channels/rate; normalized output is 16 kHz mono i16 little-endian (recorder module and core audio contracts) | Only 48000 Hz / 2 channels / F32 is known | INSUFFICIENT_EVIDENCE; count geometry/byte-alignment anomalies, then inspect the specific conversion |

N2's Windows backend expectation is also grounded in CPAL 0.15.3 WASAPI
`Stream::drop`: when `Command::Terminate` is delivered, it joins the callback
thread. A failed termination delivery is a separate path, not verified here.
Cancellation may legally produce no recognition or inserted text. Absence of
terminal application messages is not proof of an unexecuted path: original
capture/flush coverage is unknown. Removing ERROR/WARN lines does not remove the
reported N1 failure. No comparable performance baseline exists, so no new time
threshold is introduced. A 20-round start/cancel run with page heap enabled is
not a comparable successful normal-stop baseline.

## Hypotheses before the added audio probes

The original P1-P4 probes/hypotheses remain for the original report. This is a
separate recording-scope hypothesis set, not a rejection of those hypotheses.

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| H5 | Callback enqueue races the worker shutdown boundary (N2) | After successful native/worker teardown, accepted frames and completed frame processing differ | Repair the enqueue/shutdown ownership barrier, preserving normal draining | P5, P6 |
| H6 | Input geometry or normalized byte units violate N3 | Invalid channels/rate, partial channel frames, or odd PCM byte counts are observed; lawful filtering/tails must be ruled out | Correct the proven conversion boundary's length/unit calculation | P6 |
| H7 | Native callback resources outlive the release boundary (N2) | Callback activity or enqueue observations persist beyond native drop; rule out non-atomic snapshot skew and legal draining | Repair the proven callback-resource lifetime boundary | P5, P6 |

All three are INCONCLUSIVE and none is ACTIONABLE yet. The current artifact is
instrumentation only; an anomaly must still be causally connected to N1 before
any behavioral fix. H6 anomalies alone need not corrupt memory. A symbolized
future stack may require replacing these hypotheses, not forcing the evidence
into them.

## Observation contract

- Keep the existing session log: `%TEMP%\openless-issue-1227-3e857688-session.ndjson`.
  `OPENLESS_ISSUE_1227_DEBUG_LOG` only overrides this path. Never clear or rotate it.
- Exactly six conceptual probe boundaries are retained: P1 transport, P2 TSF
  lifecycle, P3 edit/submit ownership, P4 local-ASR lifetime, P5 native capture
  lifecycle, P6 input/worker accounting. P5/P6 are the only newly added boundaries.
- P5 events: `capture_start` (entered/succeeded/failed/thread_spawn_failed/
  startup_disconnected), `capture_stop` (entered/returned), `native_play`
  (succeeded), `native_shutdown` (entered), `native_pause` (failed), `native_drop`
  (entered/returned), `processor_drain` (entered/returned).
- P6 events: `input_geometry`, `capture_progress`, `native_drop_counts`,
  `processor_drained_counts`, `capture_stop_counts`; outcome `observed`.
- P5/P6 share an opaque `capture-N` operation ID and source-local `event_seq` within
  the Rust run. Wall timestamps and physical file-line order are not a global
  causal order. Original native DLL records have a different run ID and do not
  acquire capture correlation retroactively.
- Native callbacks only update atomics; no added callback file I/O, formatting,
  content inspection, delays, retries, product locks, or changes to error handling.
  PCM content, device names, provider config, paths, and exception strings are
  not added to the diagnostic log.
- Counts are snapshots, not atomic transactions. While capture is active,
  completion can briefly appear ahead of enqueue accounting. Compare final
  totals only after verified callback and worker termination; `returned` alone
  is not proof that a join succeeded. An in-flight frame can legitimately finish
  after the native stream drops. Skipped pre-play/shutdown callbacks are legal.
- Existing watchdog wakeups emit progress snapshots; no timer or wait is added.
  File I/O is synchronous off the native callback. This still adds observer
  overhead and can change timing. A materially perturbed run is INCONCLUSIVE.
  Existing TSF/native probes also allocate/write and are not observer-free.
- An abrupt crash can lose the last observations. Missing events are coverage
  gaps unless the emission and collection boundaries are verified.

## New build symbol contract

For a manual Windows **branch** build, set `windows_symbols=true` on
`release-tauri.yml`. It preserves release optimization while enabling limited
Windows debug information and disabling strip for this build only. Tag builds
and draft-release repairs do not enable this option.

The build validates the PE's CodeView RSDS GUID/Age against the PDB info stream,
then archives `openless.exe`, `openless.pdb`, and `windows-symbols.json` separately
from the installer as `openless-windows-x64-symbols` (14-day retention). The
manifest records both hashes, symbol identity, exact source revision and run URL.
These symbols are for that newly built EXE, **not** the old 2.0.2-Beta.1 dumps.

CI runs synthetic matching/mismatching GUID/Age tests and the production recorder
module tests. These establish diagnostic/tool contracts, not a reproduction or
fix for heap corruption. All tests and builds run in GitHub Actions, not locally.

After a successful build, ask the reporter to reproduce ordinary recording,
normal stop, and the already-reported mid-recording scenario without changing
ASR, paste mode, or device to conceal a path. Return the unchanged NDJSON plus a
safe observed outcome and approximate test duration. Do not publicly upload
memory dumps: they may contain audio, text, or credentials.
