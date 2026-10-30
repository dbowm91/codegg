"""Deterministic eligibility intake for frozen H007 evidence."""
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from dev.harness_lab import defect_intake, h007_attribution


ROOT = Path(__file__).resolve().parents[3]


class DefectIntakeTests(unittest.TestCase):
    def test_same_frozen_inputs_produce_same_dossier_identity(self) -> None:
        first = defect_intake.build_dossier(ROOT)
        second = defect_intake.build_dossier(ROOT)
        self.assertEqual(first, second)
        self.assertEqual(first["schema"], "defect_dossier_v1")
        self.assertEqual(first["disposition"], "no_reproducible_defect")
        self.assertFalse(first["campaign_eligible"])

    def test_all_h007_trials_are_typed_and_ineligible(self) -> None:
        dossier = defect_intake.build_dossier(ROOT)
        self.assertEqual(len(dossier["items"]), 7)
        self.assertEqual([item["evidence_id"] for item in dossier["items"]],
                         list(h007_attribution.EXPECTED_TRIALS))
        self.assertTrue(all(not item["campaign_eligible"] for item in dossier["items"]))
        by_id = {item["evidence_id"]: item for item in dossier["items"]}
        self.assertEqual(by_id["H007-1"]["disposition"], "route_failure")
        self.assertEqual(by_id["H007-3"]["disposition"], "route_failure")
        self.assertEqual(by_id["H007-4"]["disposition"], "fixture_or_evaluator_issue")
        self.assertEqual(by_id["H007-6"]["disposition"], "fixture_or_evaluator_issue")
        self.assertEqual(by_id["H007-7"]["disposition"], "observed_failure")

    def test_historical_c003_reference_is_not_current_eligibility(self) -> None:
        dossier = defect_intake.build_dossier(ROOT)
        self.assertEqual(dossier["historical_reference"]["class"], "historical_fixture")
        self.assertIn("not current eligibility", dossier["historical_reference"]["status"])
        self.assertIsNone(dossier["current_red_reproduction"])
        self.assertEqual(dossier["candidate_allow_prefixes"], [])

    def test_changed_manifest_makes_the_s001_report_stale(self) -> None:
        original = defect_intake._repo_read
        changed_manifest = original(
            ROOT, h007_attribution.DEFAULT_MANIFEST.as_posix()
        ) + b" "
        with mock.patch.object(
            defect_intake, "_repo_read",
            side_effect=lambda root, path: changed_manifest
            if path == h007_attribution.DEFAULT_MANIFEST.as_posix()
            else original(root, path),
        ):
            with self.assertRaisesRegex(defect_intake.IntakeError, "stale"):
                defect_intake.build_dossier(ROOT)

    def test_incomplete_or_duplicated_report_rows_are_refused(self) -> None:
        original = defect_intake._repo_read
        report_path = defect_intake.DEFAULT_REPORT.as_posix()
        report = json.loads(original(ROOT, report_path))
        report["trials"] = report["trials"][:-1]
        changed_report = json.dumps(report).encode()
        with mock.patch.object(
            defect_intake, "_repo_read",
            side_effect=lambda root, path: changed_report if path == report_path
            else original(root, path),
        ):
            with self.assertRaisesRegex(defect_intake.IntakeError, "incomplete"):
                defect_intake.build_dossier(ROOT)

    def test_symlink_escape_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as root_tmp, tempfile.TemporaryDirectory() as outside_tmp:
            root = Path(root_tmp)
            outside = Path(outside_tmp)
            (outside / "evidence.json").write_text("{}", encoding="utf-8")
            (root / "external").symlink_to(outside, target_is_directory=True)
            with self.assertRaisesRegex(defect_intake.IntakeError, "outside repository"):
                defect_intake._repo_read(root, "external/evidence.json")


if __name__ == "__main__":
    unittest.main()
