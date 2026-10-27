from __future__ import annotations

import unittest

from dev.harness_lab import long_horizon_report


class LongHorizonReportTests(unittest.TestCase):
    case = {"schema_version": 2, "case_id": "fixture", "case_manifest_sha256": "a" * 64,
            "fixture_source_sha256": "b" * 64, "hidden_verifier_sha256": "c" * 64,
            "public_contract_sha256": "d" * 64}

    def _run(self):
        return {"event_observation_state": "measured_no_gap", "event_gap_count": 0,
                "verification": {"returncode": 0},
                "tool_calls": [{"tool": "read", "arguments_sha256": "e" * 64,
                                "status": "completed", "state_revision": "f" * 64,
                                "rawOutput": "private content must not be copied"}],
                "fixture_calls": [{"name": "test", "job_id": "job-a"},
                                  {"name": "job", "job_id": "job-a"}],
                "source_sha256": "1" * 64, "session_id": "session-a",
                "stop_reason": "end_turn", "stop_owner": "agent_completed",
                "tool_failures": 0, "provider_calls": 3, "codegg_usage": None,
                "wall_time_ms": 5000}

    def test_receipt_records_job_correlation_without_raw_output(self):
        receipt = long_horizon_report.build_receipt(
            self.case, self._run(), attempt_id="attempt-a")
        self.assertEqual(receipt["job_ids"], ["job-a"])
        self.assertEqual(receipt["job_wait_count"], 1)
        self.assertNotIn("rawOutput", repr(receipt))
        self.assertEqual(receipt["provider_cost_usd"], None)

    def test_event_gap_is_not_scored_as_a_completed_attempt(self):
        run = self._run()
        run["event_gap_count"] = 1
        with self.assertRaisesRegex(ValueError, "event gap"):
            long_horizon_report.build_receipt(self.case, run, attempt_id="attempt-a")

    def test_job_id_mismatch_is_named_and_rejected(self):
        run = self._run()
        run["fixture_calls"][1]["job_id"] = "job-b"
        with self.assertRaisesRegex(ValueError, "do not match"):
            long_horizon_report.build_receipt(self.case, run, attempt_id="attempt-a")


if __name__ == "__main__":
    unittest.main()
