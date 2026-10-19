#!/usr/bin/env python3
"""Frozen OA-2 source-revision cross-language evaluator."""
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REVISION = "rev`|<x>"


def main() -> int:
    producer = subprocess.run(
        ["cargo", "run", "--quiet", "--locked", "--manifest-path", str(ROOT / "rust/Cargo.toml"), "--", "--source-revision", REVISION],
        cwd=ROOT, input=b"", stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if producer.returncode:
        sys.stderr.buffer.write(producer.stderr)
        return producer.returncode
    summary = json.loads(producer.stdout)
    if summary["source_revision"] != REVISION or summary["schema"] != "codegg.project-summary.v1":
        raise SystemExit("Rust producer changed the accepted source revision")
    consumer = subprocess.run(
        [sys.executable, "-m", "summary_report", "-"], cwd=ROOT, input=producer.stdout,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        env={**os.environ, "PYTHONPATH": str(ROOT / "python")},
    )
    if consumer.returncode:
        sys.stderr.buffer.write(consumer.stderr)
        return consumer.returncode
    expected = (ROOT / "fixtures/expected-report-source-revision.md").read_bytes()
    if consumer.stdout != expected:
        raise SystemExit("Markdown report did not preserve the safe source-revision code span")
    print("OA-2 source-revision golden passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
