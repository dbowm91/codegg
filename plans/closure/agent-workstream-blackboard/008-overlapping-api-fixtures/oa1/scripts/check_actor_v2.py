#!/usr/bin/env python3
"""Frozen OA-1 cross-language actor-v2 evaluator."""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "fixtures"


def main() -> int:
    events = (FIXTURES / "actor-v2-events.ndjson").read_bytes()
    expected = json.loads((FIXTURES / "expected-summary-v2.json").read_text())
    producer = subprocess.run(
        ["cargo", "run", "--quiet", "--locked", "--manifest-path", str(ROOT / "rust/Cargo.toml"), "--", "--source-revision", "abc123"],
        cwd=ROOT, input=events, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if producer.returncode:
        sys.stderr.buffer.write(producer.stderr)
        return producer.returncode
    actual = json.loads(producer.stdout)
    if actual != expected:
        raise SystemExit(f"Rust v2 output differs from frozen expected summary: {actual!r}")
    consumer = subprocess.run(
        [sys.executable, "-m", "summary_report", "-"],
        cwd=ROOT, input=producer.stdout, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        env={**__import__("os").environ, "PYTHONPATH": str(ROOT / "python")},
    )
    if consumer.returncode:
        sys.stderr.buffer.write(consumer.stderr)
        return consumer.returncode
    expected_report = (FIXTURES / "expected-report-v2.md").read_bytes()
    if consumer.stdout != expected_report:
        raise SystemExit("Python v2 report differs from frozen expected Markdown bytes")
    print("OA-1 Rust/Python v2 golden passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
