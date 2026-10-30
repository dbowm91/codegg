"""Deterministic eligibility intake for frozen H007 evidence."""
from __future__ import annotations

import json
import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from dev.harness_lab import defect_intake, h007_attribution


ROOT = Path(__file__).resolve().parents[3]


class DefectIntakeTests(unittest.TestCase):
    @staticmethod
    def _synthetic_generic_manifest(root: Path) -> dict:
        artifacts = {
            "source_path": "src/subject.rs",
            "test_path": "tests/red_case.rs",
            "test_result_path": "evidence/red-result.json",
            "verifier_path": "evidence/independent-verifier.json",
            "holdout_path": "evidence/holdout.json",
        }
        for path, contents in {
            artifacts["source_path"]: "pub fn subject() -> bool { false }\n",
            artifacts["test_path"]: "#[test] fn reproduces_red() { assert!(subject()); }\n",
            artifacts["holdout_path"]: json.dumps({
                "schema": "protected_holdout_v1", "mutable_by_candidate": False,
            }, sort_keys=True),
        }.items():
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(contents, encoding="utf-8")
        digest = lambda path: hashlib.sha256((root / path).read_bytes()).hexdigest()
        result = {
            "schema": "red_test_result_v1", "test_sha256": digest(artifacts["test_path"]),
            "failure_signature": "assertion:subject_true", "terminal": "red",
        }
        (root / artifacts["test_result_path"]).write_text(json.dumps(result, sort_keys=True), encoding="utf-8")
        verifier = {
            "schema": "independent_verifier_result_v1", "verifier_owner": "independent",
            "source_sha256": digest(artifacts["source_path"]),
            "test_sha256": digest(artifacts["test_path"]),
            "holdout_sha256": digest(artifacts["holdout_path"]), "verdict": "red_confirmed",
        }
        (root / artifacts["verifier_path"]).write_text(json.dumps(verifier, sort_keys=True), encoding="utf-8")
        return {
            "schema": "defect_reproduction_manifest_v1", "evidence_kind": "synthetic_fixture",
            "defect_id": "synthetic-index-contract", "owner": "codegg_harness",
            "source_revision": "a" * 40, **artifacts,
            "source_sha256": digest(artifacts["source_path"]), "candidate_scope": ["src/"],
            "failure_signature": "assertion:subject_true",
            "reproductions": [
                {"lane_id": "private-lane-a", "failure_signature": "assertion:subject_true", "terminal": "red"},
                {"lane_id": "private-lane-b", "failure_signature": "assertion:subject_true", "terminal": "red"},
            ],
        }

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

    def test_generic_positive_synthetic_reproduction_is_stable_but_never_campaign_eligible(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self._synthetic_generic_manifest(root)
            first = defect_intake.build_generic_dossier(root, manifest)
            second = defect_intake.build_generic_dossier(root, manifest)
        self.assertEqual(first, second)
        self.assertTrue(first["reproduction_validated"])
        self.assertEqual(first["disposition"], "synthetic_reproduction_validated")
        self.assertFalse(first["campaign_eligible"])
        self.assertEqual(len(first["reproductions"]), 2)

    def test_generic_positive_intake_is_available_through_the_cli(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self._synthetic_generic_manifest(root)
            manifest_path = root / "evidence" / "generic-manifest.json"
            manifest_path.write_text(json.dumps(manifest, sort_keys=True), encoding="utf-8")
            output_path = root / "evidence" / "generic-dossier.json"
            result = defect_intake.main([
                "--repo-root", str(root), "--generic-manifest", "evidence/generic-manifest.json",
                "--output", str(output_path),
            ])
            dossier = json.loads(output_path.read_text(encoding="utf-8"))
        self.assertEqual(result, 0)
        self.assertEqual(dossier["disposition"], "synthetic_reproduction_validated")
        self.assertFalse(dossier["campaign_eligible"])

    def test_generic_intake_rejects_duplicate_runs_unbound_verifier_and_live_evidence_label(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self._synthetic_generic_manifest(root)
            duplicated = dict(manifest)
            duplicated["reproductions"] = [manifest["reproductions"][0]] * 2
            with self.assertRaisesRegex(defect_intake.IntakeError, "distinct"):
                defect_intake.build_generic_dossier(root, duplicated)
            verifier_path = root / manifest["verifier_path"]
            verifier_path.write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(defect_intake.IntakeError, "verifier"):
                defect_intake.build_generic_dossier(root, manifest)
            manifest["evidence_kind"] = "current_codegg"
            with self.assertRaisesRegex(defect_intake.IntakeError, "synthetic fixtures only"):
                defect_intake.build_generic_dossier(root, manifest)

    def test_generic_intake_refuses_candidate_scope_over_harness_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self._synthetic_generic_manifest(root)
            manifest["candidate_scope"] = ["dev/harness_lab/"]
            with self.assertRaisesRegex(defect_intake.IntakeError, "scope"):
                defect_intake.build_generic_dossier(root, manifest)


if __name__ == "__main__":
    unittest.main()
