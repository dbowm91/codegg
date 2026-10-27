from __future__ import annotations

import hashlib
import subprocess
import unittest
from pathlib import Path

from dev.harness_lab import recovery_controls_v2 as controls


def _call(revision: str, status: str = "failed") -> dict:
    return {"tool": "read", "arguments_sha256": "a" * 64,
            "status": status, "state_revision": revision,
            "rawOutput": "untrusted output is deliberately ignored"}


class RecoveryControlManifestTests(unittest.TestCase):
    def test_v2_manifest_is_versioned_and_digest_pinned(self):
        suite = controls.load_suite()
        self.assertEqual(suite.version, 2)
        self.assertEqual(len(suite.digest), 64)
        self.assertEqual({row["case_id"] for row in suite.cases}, {
            "repeated-identical-failure-bounded", "changed-state-read-is-progress",
            "event-gap-is-unknown", "turn-budget-owner-is-explicit"})

    def test_mixed_schema_or_tampered_digest_is_rejected(self):
        row = {"suite_version": 2, "suite_sha256": "a" * 64,
               "control_state": "exercised", "control_held": True}
        with self.assertRaises(controls.RecoveryControlError):
            controls.compare_pair(row, {**row, "suite_sha256": "b" * 64})
        with self.assertRaises(controls.RecoveryControlError):
            controls.compare_pair({**row, "suite_version": 1}, row)

    def test_v1_manifest_is_separate_from_v2(self):
        root = Path(__file__).resolve().parents[3]
        historical = root / "dev" / "harness_lab" / "fixtures" / "agent_autonomy" / "cases.jsonl"
        self.assertNotEqual(controls.MANIFEST, historical)
        original = subprocess.run(
            ["git", "show", f"HEAD:{historical.relative_to(root)}"], cwd=root,
            capture_output=True, check=True).stdout
        self.assertEqual(hashlib.sha256(historical.read_bytes()).digest(),
                         hashlib.sha256(original).digest())


class RecoveryControlEvaluationTests(unittest.TestCase):
    def setUp(self):
        self.suite = controls.load_suite()
        self.cases = {row["case_id"]: row for row in self.suite.cases}

    def test_repeated_identical_failed_call_enters_bounded_control(self):
        revision = "1" * 64
        result = controls.evaluate_control(
            self.cases["repeated-identical-failure-bounded"],
            [_call(revision), _call(revision), _call(revision)],
            suite_sha256=self.suite.digest,
            terminal="completed", stop_owner="agent_completed")
        self.assertTrue(result["control_exercised"])
        self.assertTrue(result["control_held"])
        self.assertEqual(result["identical_failed_call_count"], 3)
        self.assertEqual(result["recovery_rung_evidence"], "unavailable_not_exposed")
        self.assertEqual(result["progress_evidence_class"],
                         "identical_call_without_revision_change")

    def test_changed_workspace_revision_breaks_failure_identity(self):
        result = controls.evaluate_control(
            self.cases["changed-state-read-is-progress"],
            [_call("1" * 64), _call("2" * 64)],
            suite_sha256=self.suite.digest,
            terminal="completed", stop_owner="agent_completed")
        self.assertTrue(result["control_exercised"])
        self.assertTrue(result["control_held"])
        self.assertEqual(result["identical_failed_call_count"], 1)

    def test_missing_terminal_or_event_gap_is_unknown_not_failure(self):
        result = controls.evaluate_control(
            self.cases["repeated-identical-failure-bounded"],
            [_call("1" * 64, "unknown"), _call("1" * 64, "unknown")],
            suite_sha256=self.suite.digest,
            terminal="completed", stop_owner="unknown", event_gap=True)
        self.assertEqual(result["control_state"], "unknown")
        self.assertIsNone(result["control_held"])

    def test_hard_budget_control_names_the_actual_stop_owner(self):
        result = controls.evaluate_control(
            self.cases["turn-budget-owner-is-explicit"],
            [_call("1" * 64), _call("1" * 64)],
            suite_sha256=self.suite.digest,
            terminal="completed", stop_owner="agent_turn_budget")
        self.assertTrue(result["control_exercised"])
        self.assertEqual(result["actual_stop_owner"], "agent_turn_budget")

    def test_unexercised_control_cannot_be_scored(self):
        baseline = {"suite_version": 2, "suite_sha256": "a" * 64,
                    "control_state": "not_exercised", "control_held": None}
        with self.assertRaises(controls.RecoveryControlError):
            controls.compare_pair(baseline, baseline)

    def test_false_stall_is_a_protected_regression_in_same_digest_pair(self):
        case = self.cases["repeated-identical-failure-bounded"]
        calls = [_call("1" * 64), _call("1" * 64)]
        baseline = controls.evaluate_control(
            case, calls, suite_sha256=self.suite.digest,
            terminal="completed", stop_owner="agent_completed")
        candidate = controls.evaluate_control(
            case, calls, suite_sha256=self.suite.digest,
            terminal="stalled", stop_owner="agent_completed")
        result = controls.compare_pair(baseline, candidate)
        self.assertTrue(result["regression"])
        self.assertFalse(result["candidate_held"])


if __name__ == "__main__":
    unittest.main()
