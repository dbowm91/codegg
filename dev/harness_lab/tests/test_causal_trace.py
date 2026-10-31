from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from io import StringIO

from dev.harness_lab import causal_trace
from dev.harness_lab import __main__ as harness_cli


def notification(kind: str, **fields):
    return {"method": "session/update", "_received_elapsed_ms": 14,
            "params": {"update": {"sessionUpdate": kind, **fields}}}


class CausalTraceTests(unittest.TestCase):
    def test_append_only_trace_redacts_payloads_correlates_exact_jobs_and_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "private" / "trace.jsonl"
            recorder = causal_trace.Recorder(path, run_id="run-1", case_id="case-1",
                                             source_sha256="a" * 40, binary_sha256="b" * 64)
            recorder.bind_daemon_generation("generation-1")
            recorder.observe(notification("tool_call", toolCallId="call-secret", title="test",
                                          codeggAction="submit", rawInput="secret prompt"))
            recorder.observe(notification("tool_call_update", toolCallId="call-secret", status="completed",
                                          rawOutput="job running 12345678-1234-1234-1234-123456789abc"))
            recorder.observe(notification("tool_call", toolCallId="call-next", title="write",
                                          rawInput="credential=never-store"))
            recorder.observe(notification("tool_call_update", toolCallId="call-next", status="completed",
                                          rawOutput="ordinary private file contents"))
            summary = recorder.finish()
            rows = causal_trace.read_trace(path)
            joined = path.read_text(encoding="utf-8")

        self.assertEqual(summary["state"], "complete")
        self.assertEqual(len(rows), 6)
        self.assertEqual(rows[2]["event_type"], "job_observation")
        self.assertEqual(rows[2]["job_status"], "running")
        self.assertEqual(len(rows[2]["job_correlation"]), 24)
        self.assertNotIn("call-secret", joined)
        self.assertNotIn("12345678-1234-1234-1234-123456789abc", joined)
        self.assertNotIn("secret prompt", joined)
        self.assertNotIn("credential=never-store", joined)
        self.assertNotIn("ordinary private file contents", joined)

    def test_trace_caps_record_incomplete_state_without_growing_past_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "trace.jsonl"
            with mock.patch.object(causal_trace, "MAX_EVENTS", 1):
                recorder = causal_trace.Recorder(path, run_id="run", case_id="case",
                                                 source_sha256=None, binary_sha256=None)
                recorder.observe(notification("tool_call", toolCallId="one", title="read"))
                recorder.observe(notification("tool_call", toolCallId="two", title="read"))
                summary = recorder.finish()
            rows = causal_trace.read_trace(path)
        self.assertEqual(summary["state"], "truncated")
        self.assertEqual(summary["dropped_events"], 2)
        self.assertLessEqual(summary["bytes"], causal_trace.MAX_BYTES)
        self.assertEqual(len(rows), 1)
        report = causal_trace.attribution_report(
            rows, {"causal_trace": {"state": "truncated"}})
        self.assertEqual(report["trace_state"], "truncated")
        self.assertEqual(report["observed_failure_class"], "observation_incomplete")

    def test_tool_title_is_allowlisted_before_persistence(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "trace.jsonl"
            recorder = causal_trace.Recorder(path, run_id="run", case_id="case",
                                             source_sha256=None, binary_sha256=None)
            recorder.observe(notification("tool_call", toolCallId="call",
                                          title="write credential=do-not-store"))
            recorder.finish()
            rows = causal_trace.read_trace(path)
            joined = path.read_text(encoding="utf-8")
        self.assertEqual(rows[0]["tool"], "unknown")
        self.assertNotIn("credential=do-not-store", joined)

    def test_cli_rejects_trace_receipt_identity_or_digest_mismatch(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "trace.jsonl"
            recorder = causal_trace.Recorder(path, run_id="run-1", case_id="case-1",
                                             source_sha256="a" * 40, binary_sha256="b" * 64)
            recorder.finish()
            good = {"case_id": "case-1", "source_revision": "a" * 40,
                    "binary_sha256": "b" * 64, "causal_trace": {
                        "sha256": __import__("hashlib").sha256(path.read_bytes()).hexdigest()}}
            with mock.patch("sys.stderr", new_callable=StringIO) as errors:
                code = harness_cli.main(["attribution", "--trace", str(path), "--receipt", str(
                    self._write_receipt(Path(temporary), {**good, "source_revision": "c" * 40}))])
                self.assertEqual(code, 2)
                self.assertIn("source identity", errors.getvalue())
                code = harness_cli.main(["attribution", "--trace", str(path), "--receipt", str(
                    self._write_receipt(Path(temporary), {**good, "causal_trace": {"sha256": "d" * 64}}))])
                self.assertEqual(code, 2)
                self.assertIn("digest", errors.getvalue())
                valid = {"task_id": "case-1", "source_sha256": "b" * 64,
                         "ledger_manifest": {"subject": {"revision": "a" * 40}},
                         "causal_trace": good["causal_trace"]}
                with mock.patch("sys.stdout", new_callable=StringIO) as output:
                    code = harness_cli.main(["attribution", "--trace", str(path), "--receipt", str(
                        self._write_receipt(Path(temporary), valid))])
                self.assertEqual(code, 0)
                self.assertEqual(json.loads(output.getvalue())["observed_failure_class"], "unknown")

    @staticmethod
    def _write_receipt(root: Path, receipt: dict) -> Path:
        path = root / "receipt.json"
        path.write_text(json.dumps(receipt), encoding="utf-8")
        return path

    def test_report_keeps_provider_verifier_task_and_observation_outcomes_distinct(self):
        empty = [{"event_type": "trace_terminal", "state": "complete"}]
        cases = [
            ({"terminal_failure": {"category": "provider_timeout"}}, "provider_timeout"),
            ({"independent_verifier": {"tests_run": 0, "exit_code": 2}}, "verifier_setup_error"),
            ({"task_outcome": "failed"}, "task_failure"),
            ({}, "unknown"),
        ]
        for receipt, expected in cases:
            with self.subTest(expected=expected):
                report = causal_trace.attribution_report(empty, receipt)
                self.assertEqual(report["observed_failure_class"], expected)
                self.assertFalse(report["codegg_defect_reproduced"])
        incomplete = causal_trace.attribution_report([], {"task_outcome": "failed"})
        self.assertEqual(incomplete["observed_failure_class"], "task_failure")
        missing = causal_trace.attribution_report([], {})
        self.assertEqual(missing["observed_failure_class"], "observation_incomplete")

    def test_verified_success_preserves_repaired_tool_failures_without_blame(self):
        trace = [
            {"event_type": "tool_terminal", "status": "failed"},
            {"event_type": "repair_cycle", "detail": "failed"},
            {"event_type": "repair_cycle", "detail": "passed"},
            {"event_type": "continuation"},
            {"event_type": "trace_terminal", "state": "complete"},
        ]
        report = causal_trace.attribution_report(trace, {
            "task_outcome": "passed",
            "independent_verifier": {"tests_run": 1, "exit_code": 0},
        })
        self.assertEqual(report["observed_failure_class"], "task_success")
        self.assertEqual(report["observed_tool_failure_count"], 1)
        self.assertEqual(report["repair_cycles_observed"], 1)
        self.assertTrue(report["continuation_observed"])
        self.assertEqual(report["plausible_owners"], ["none"])

    def test_tampered_trace_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "trace.jsonl"
            recorder = causal_trace.Recorder(path, run_id="run", case_id="case",
                                             source_sha256=None, binary_sha256=None)
            recorder.observe(notification("tool_call", toolCallId="call", title="read"))
            recorder.finish()
            payload = json.loads(path.read_text(encoding="utf-8").splitlines()[0])
            payload["tool"] = "write"
            path.write_text(json.dumps(payload) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "digest"):
                causal_trace.read_trace(path)


if __name__ == "__main__":
    unittest.main()
