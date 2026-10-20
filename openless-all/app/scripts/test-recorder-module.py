#!/usr/bin/env python3
"""Run the native recorder tests without building Tauri/MLX or fetching ASR submodules.

The bridge trait below matches the only core interface used by recorder.rs.
This checks the production recorder module; it does not replace a desktop cargo check.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

recorder = Path(__file__).resolve().parents[1] / "src-tauri/src/recorder.rs"
with tempfile.TemporaryDirectory(prefix="openless-recorder-tests-") as directory:
    root = Path(directory)
    (root / "src").mkdir()
    (root / "Cargo.toml").write_text("""[package]
name = "openless-recorder-tests"
version = "0.1.0"
edition = "2021"
[dependencies]
cpal = "=0.15.3"
parking_lot = "0.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
log = "0.4"
""")
    core_audio = recorder.parents[2] / "crates/openless-core/src/audio.rs"
    debug = recorder.with_name("issue1227_debug.rs")
    audio_debug = recorder.with_name("issue1227_audio_debug.rs")
    (root / "src/lib.rs").write_text(f"""
extern crate self as openless_core;
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BackendErrorCode {{ InvalidArgument }}
#[derive(Debug, Clone)] pub struct BackendError {{ pub code: BackendErrorCode, pub message: String }}
impl BackendError {{ pub fn new(code: BackendErrorCode, message: impl Into<String>) -> Self {{ Self {{ code, message: message.into() }} }} }}
#[path = {json.dumps(str(core_audio), ensure_ascii=False)}]
pub mod audio;
pub use audio::{{AudioProcessingConfig, AudioProcessor}};
pub trait AudioConsumer: Send + Sync {{ fn consume_pcm_chunk(&self, pcm: &[u8]); }}
#[cfg(target_os = "windows")]
#[path = {json.dumps(str(debug), ensure_ascii=False)}]
pub mod issue1227_debug;
#[cfg(target_os = "windows")]
#[path = {json.dumps(str(audio_debug), ensure_ascii=False)}]
pub mod issue1227_audio_debug;
#[path = {json.dumps(str(recorder), ensure_ascii=False)}]
pub mod recorder;
""", encoding="utf-8")
    subprocess.run(["cargo", "test", "--manifest-path", str(root / "Cargo.toml"), *sys.argv[1:]], check=True)
