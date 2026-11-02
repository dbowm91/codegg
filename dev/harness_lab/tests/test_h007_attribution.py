"""Evidence-limited attribution checks for H007's immutable receipts."""
from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from dev.harness_lab import h007_attribution as attribution


ROOT = Path(__file__).resolve().parents[3]
MANIFEST = ROOT / attribution.DEFAULT_MANIFEST


class H007AttributionTests(unittest.TestCase):
    def test_shallow_checkout_uses_digest_checked_frozen_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frozen = root / "fixture.json"
            frozen.write_bytes(b'{"frozen":true}\n')
            with mock.patch.object(
                attribution.subprocess, "run", return_value=mock.Mock(returncode=128),
            ):
                self.assertEqual(
                    attribution._source_blob(root, "a" * 40, "fixture.json"),
                    b'{"frozen":true}\n',
                )

    def test_shallow_checkout_uses_the_pinned_h007_fixture_snapshot(self) -> None:
        changed_fixture = ROOT / "dev/harness_lab/fixtures/build_provenance_catalog/fixture/README.md"
        snapshot = ROOT / attribution.S001_FIXTURE_SNAPSHOT / "README.md"
        self.assertNotEqual(changed_fixture.read_bytes(), snapshot.read_bytes())
        with mock.patch.object(
            attribution.subprocess, "run", return_value=mock.Mock(returncode=128),
        ):
            report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        self.assertEqual(report["attempt_count"], 7)

    def test_frozen_report_is_deterministic_and_has_seven_unique_receipts(self) -> None:
        first = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        second = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        self.assertEqual(first, second)
        self.assertEqual(first["attempt_count"], 7)
        self.assertEqual([row["trial_id"] for row in first["trials"]],
                         list(attribution.EXPECTED_TRIALS))
        self.assertEqual(len({row["receipt_sha256"] for row in first["trials"]}), 7)

    def test_provider_timeout_is_not_a_task_failure(self) -> None:
        report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        timed_out = [row for row in report["trials"]
                     if row["classification"] == "provider_boundary_timeout"]
        self.assertEqual([row["trial_id"] for row in timed_out], ["H007-1", "H007-3"])
        self.assertTrue(all(row["suspected_owner"] == "provider route" for row in timed_out))
        self.assertTrue(all(row["observed_outcome"]["task_outcome"] == "inconclusive"
                            for row in timed_out))
        self.assertTrue(all(row["cause_assertion"]["established"] is False
                            for row in timed_out))

    def test_verifier_setup_errors_are_not_completed_test_failures(self) -> None:
        report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        setup = {row["trial_id"]: row for row in report["trials"]
                 if row["classification"] == "candidate_or_verifier_setup_failure"}
        self.assertEqual(set(setup), {"H007-4", "H007-6"})
        self.assertTrue(all(row["observed_outcome"]["verifier"]["tests_run"] == 0
                            for row in setup.values()))
        self.assertTrue(all(row["suspected_owner"] == "fixture" for row in setup.values()))

    def test_one_of_two_repairs_is_a_task_observation_not_recovery_cause(self) -> None:
        report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        row = next(row for row in report["trials"] if row["trial_id"] == "H007-7")
        self.assertEqual(row["observed_outcome"]["repair_cycles"], 1)
        self.assertEqual(row["observed_outcome"]["task_outcome"], "failed")
        self.assertIsNone(row["cause_assertion"]["assertion"])
        self.assertFalse(row["codegg_defect_reproduced"])

    def test_zero_observed_event_gaps_is_bounded_language(self) -> None:
        report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        self.assertEqual(report["summary"]["event_gap_counts"], [0] * 7)
        self.assertIn("only the recorded observation window", report["causal_limit"])
        self.assertNotIn("reliable event stream", report["causal_limit"])
        self.assertEqual(report["summary"]["verified_codegg_defects"], 0)

    def test_h007_2_historical_horizon_reason_contradiction_is_visible(self) -> None:
        report = attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        row = next(row for row in report["trials"] if row["trial_id"] == "H007-2")
        self.assertFalse(row["recorded_horizon_reason_consistent"])
        self.assertEqual(row["reason_evidence_state"], "contradicted")
        self.assertIn("repair_cycles_insufficient", row["operator_note"])

    def test_mismatched_job_identity_is_not_qualified(self) -> None:
        receipt = {
            "job_lifecycle": {
                "same_id_running_and_terminal_observed": False,
                "productive_work_during_job": True,
                "observations": [
                    {"action": "submit", "job_id": "job-a"},
                    {"action": "status", "job_id": "job-b"},
                ],
            }
        }
        summary = attribution._job_summary(receipt)
        self.assertFalse(summary["same_id_running_and_terminal_observed"])
        self.assertTrue(summary["productive_overlap"])
        self.assertEqual(summary["submit_count"], 1)
        self.assertEqual(summary["status_observation_count"], 1)

    def test_tampered_receipt_is_refused(self) -> None:
        raw_manifest = MANIFEST.read_bytes()
        manifest = json.loads(raw_manifest)
        receipt_path = manifest["receipts"][0]["path"]
        original = attribution._repo_bytes(ROOT, receipt_path)
        read_original = attribution._repo_bytes
        changed = original + b" "
        with mock.patch.object(
            attribution, "_repo_bytes",
            side_effect=lambda _root, path: changed if path == receipt_path else read_original(
                _root, path),
        ):
            with self.assertRaisesRegex(attribution.AttributionInputError, "digest changed"):
                attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)

    def test_duplicate_trial_ids_are_refused(self) -> None:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        manifest["receipts"][1]["trial_id"] = manifest["receipts"][0]["trial_id"]
        changed = json.dumps(manifest).encode()
        read_original = attribution._repo_bytes
        with mock.patch.object(
            attribution, "_repo_bytes",
            side_effect=lambda _root, path: changed if path == attribution.DEFAULT_MANIFEST.as_posix()
            else read_original(_root, path),
        ):
            with self.assertRaisesRegex(attribution.AttributionInputError, "unique"):
                attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)

    def test_changed_case_or_verifier_identity_is_refused(self) -> None:
        with mock.patch.object(
            attribution, "_verify_case_identity",
            side_effect=attribution.AttributionInputError("pinned verifier digest does not match"),
        ):
            with self.assertRaisesRegex(attribution.AttributionInputError, "verifier digest"):
                attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)

    def test_receipt_sources_remain_byte_identical(self) -> None:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        before = {
            row["trial_id"]: hashlib.sha256((ROOT / row["path"]).read_bytes()).hexdigest()
            for row in manifest["receipts"]
        }
        attribution.build_report(ROOT, attribution.DEFAULT_MANIFEST)
        after = {
            row["trial_id"]: hashlib.sha256((ROOT / row["path"]).read_bytes()).hexdigest()
            for row in manifest["receipts"]
        }
        self.assertEqual(before, after)

if __name__ == "__main__":
    unittest.main()
