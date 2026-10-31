#!/usr/bin/env python3
"""Actions-only entry: exercise the real std-only sink without linking Tauri.

Run from openless-all/app: python scripts/test-style-zip-diagnostics.py
No dependencies are downloaded; the temporary Cargo crate uses only Rust std.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def main():
    app = Path(__file__).resolve().parent.parent
    sink = app / "src-tauri/src/issue1320_sink.rs"
    with tempfile.TemporaryDirectory(
        prefix="openless-style-zip-", dir=os.environ.get("RUNNER_TEMP") or os.environ.get("TMPDIR")
    ) as temporary:
        root = Path(temporary)
        evidence = root / "evidence"
        evidence.mkdir()
        # Absolute [lib] path compiles exactly the production sink, not a copy or mock.
        manifest = (
            '[package]\nname = "style-zip-sink-tests"\nversion = "0.0.0"\nedition = "2021"\n'
            '[lib]\npath = ' + json.dumps(sink.as_posix()) + '\n[workspace]\n'
        )
        (root / "Cargo.toml").write_text(manifest, encoding="utf-8")
        environment = dict(os.environ, STYLE_ZIP_TEST_DIR=str(evidence), CARGO_TARGET_DIR=str(root / "target"))
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(root / "Cargo.toml")],
            cwd=root, env=environment, check=True,
        )
        records = 0
        required = {"schema", "session_id", "run_id", "probe_id", "hypothesis_ids", "event", "outcome", "safe", "ts"}
        safe_keys = {
            "source", "event_seq", "present", "busy", "count", "duration_ms", "feedback", "stage",
            "error_code",
            "bytes",
            "native_activity_covered", "plugin_callback_covered", "frontend_delivery", "build_id",
            "dropped", "subsequent_records_discarded",
            "frontend_skipped", "frontend_failed", "frontend_pending",
            "frontend_accounting_scope", "frontend_instance_id", "previous_frontend_instances_covered",
        }
        files = sorted(evidence.glob("*.ndjson"))
        assert files, "Rust tests must produce real sink records"
        for path in files:
            data = path.read_bytes()
            assert len(data) <= 1024 * 1024
            assert not data or data.endswith(b"\n"), "complete NDJSON lines"
            for line in data.splitlines():
                record = json.loads(line)
                assert required <= record.keys()
                assert record["schema"] == "diagnosing-bugs.v1"
                assert record["session_id"] == "sz-a73c"
                assert re.fullmatch(r"[0-9a-f]{32}", record["run_id"])
                assert record["probe_id"] in {"P1", "P3", "P4", "P5"}
                assert set(record["hypothesis_ids"]) <= {"H1", "H2", "H3"}
                assert record["safe"].keys() <= safe_keys
                if "operation_id" in record:
                    assert re.fullmatch(r"[0-9a-f]{32}", record["operation_id"])
                assert b"content://" not in line and b"password" not in line
                records += 1
        assert records > 100, "exercise serialized output, not only empty files"
        print(f"style ZIP sink: real Rust tests and {records} NDJSON records validated")


if __name__ == "__main__":
    main()
