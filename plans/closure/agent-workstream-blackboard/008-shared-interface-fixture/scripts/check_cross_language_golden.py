#!/usr/bin/env python3
"""Frozen Rust producer -> Python consumer golden acceptance check."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "fixtures"


def main() -> int:
    producer = subprocess.run(
        ["cargo", "run", "--quiet", "--locked", "-p", "summary-producer", "--", "--source-revision", "abc123"],
        cwd=ROOT,
        input=(FIXTURES / "events.ndjson").read_text(encoding="utf-8"),
        capture_output=True,
        text=True,
        check=False,
    )
    if producer.returncode:
        sys.stderr.write(producer.stderr)
        return producer.returncode
    actual_json = json.loads(producer.stdout)
    expected_json = json.loads((FIXTURES / "expected-summary.json").read_text(encoding="utf-8"))
    if actual_json != expected_json:
        print("Rust producer output differs from frozen v1 JSON fixture", file=sys.stderr)
        return 1

    env = dict(os.environ)
    env["PYTHONPATH"] = str(ROOT / "python")
    consumer = subprocess.run(
        [sys.executable, "-m", "summary_report", "-"],
        cwd=ROOT,
        env=env,
        input=producer.stdout,
        capture_output=True,
        text=True,
        check=False,
    )
    if consumer.returncode:
        sys.stderr.write(consumer.stderr)
        return consumer.returncode
    expected_report = (FIXTURES / "expected-report.md").read_text(encoding="utf-8")
    if consumer.stdout != expected_report:
        print("Python report differs from frozen Markdown fixture", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
