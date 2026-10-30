from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from io import StringIO

from dev.harness_lab import current_defect_intake as intake
from dev.harness_lab import __main__ as harness_cli


class CurrentDefectIntakeTests(unittest.TestCase):
    def test_controller_owned_h007_holdouts_exist_in_the_checked_in_fixture(self):
        root = Path(__file__).resolve().parents[3]
        self.assertGreater(len(intake.HOLDOUT_PATHS), 1)
        for relative in intake.HOLDOUT_PATHS:
            with self.subTest(path=relative.as_posix()):
                self.assertTrue((root / relative).is_file())

    def test_production_shaped_synthetic_red_is_never_eligible(self):
        dossier = intake.build_synthetic_receipt(
            defect_id="fixture-write-contract", source_sha256="a" * 64,
            failure_signature="synthetic-assertion")
        report = intake.report_dossier(dossier)
        self.assertTrue(dossier["reproduction_validated"])
        self.assertFalse(dossier["defect_eligible"])
        self.assertFalse(dossier["live_authorized"])
        self.assertEqual(dossier["evidence_kind"], "synthetic_fixture")
        self.assertEqual(report["eligible_real_defects"], 0)

    def test_controller_runs_two_independent_lanes_and_binds_exact_source_and_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            subprocess = __import__("subprocess")
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "harness@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Harness Test"], check=True)
            for relative in (intake.TEST_PATH, *intake.HOLDOUT_PATHS):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("frozen fixture\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "freeze intake subject"], check=True)
            binary = Path(temporary) / "codegg"
            binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            binary.chmod(0o700)
            outputs = []

            def controlled_red(_root, _binary, lane):
                outputs.append(lane)
                return {"lane_id": str(len(outputs)), "terminal": "native_contract_red",
                        "failure_signature": "f" * 64, "exit_code": 1,
                        "output_sha256": str(len(outputs)).zfill(64),
                        "test_sha256": intake._sha((_root / intake.TEST_PATH).read_bytes())}

            with (mock.patch.object(intake, "_build_binary",
                                    return_value=(binary, "aarch64-apple-darwin")),
                  mock.patch.object(intake, "_execute", side_effect=controlled_red)):
                dossier = intake.reproduce_current_defect(root,
                                                          defect_id="native-write-regression")
            self.assertTrue(dossier["defect_eligible"])
            self.assertFalse(dossier["live_authorized"])
            self.assertEqual(len(outputs), 2)
            self.assertNotEqual(outputs[0], outputs[1])
            self.assertEqual(len({row["lane_id"] for row in dossier["reproductions"]}), 2)
            self.assertEqual(dossier["source_revision"], subprocess.check_output(
                ["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip())
            self.assertEqual(dossier["binary_sha256"], intake._sha(binary.read_bytes()))
            with (mock.patch.object(intake, "_build_binary",
                                    return_value=(binary, "aarch64-apple-darwin")),
                  mock.patch.object(intake, "_execute", side_effect=controlled_red)):
                validated = intake.validate_dossier(root, dossier)
            self.assertTrue(validated["defect_eligible"])
            self.assertEqual(validated["eligible_real_defects"], 1)
            self.assertEqual(len(outputs), 4)

    def test_green_and_setup_error_never_become_current_defects(self):
        self.assertEqual(intake._classify(0, "Ran 1 test\n\nOK"), ("completed", None))
        self.assertEqual(intake._classify(1, "Ran 1 test\n\nERROR: setUp"),
                         ("verifier_or_setup_error", None))
        self.assertEqual(intake._classify(0, "Ran 1 test\n\nOK (skipped=1)"),
                         ("unknown", None))

    def test_dirty_source_and_unknown_command_fields_are_not_admitted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            os.chmod(root, 0o700)
            with self.assertRaisesRegex(intake.IntakeError, "repository identity"):
                intake.reproduce_current_defect(root, defect_id="test")
        with self.assertRaises(TypeError):
            intake.reproduce_current_defect(Path.cwd(), defect_id="test",
                                            command="arbitrary shell")

    def test_stored_report_never_self_asserts_live_authority(self):
        dossier = intake.build_synthetic_receipt(
            defect_id="synthetic", source_sha256="b" * 64, failure_signature="fixture")
        dossier["live_authorized"] = True
        dossier["defect_eligible"] = True
        report = intake.report_dossier(dossier)
        self.assertFalse(report["live_authorized"])
        self.assertFalse(report["defect_eligible"])
        self.assertEqual(report["eligible_real_defects"], 0)
        self.assertFalse(report["reproduction_validated"])

    def test_timed_out_reproduction_kills_and_reaps_its_process_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            test_path = root / intake.TEST_PATH
            test_path.parent.mkdir(parents=True)
            test_path.write_text("frozen", encoding="utf-8")
            process = mock.Mock()
            process.pid = 1234
            process.returncode = -9
            process.communicate.side_effect = [
                __import__("subprocess").TimeoutExpired(["python"], 600),
                (b"", b"killed"),
            ]
            lane = root / "lane"
            with (mock.patch.object(intake.subprocess, "Popen", return_value=process),
                  mock.patch.object(intake.os, "killpg") as killpg):
                result = intake._execute(root, root / "codegg", lane)
            killpg.assert_called_once_with(1234, intake.signal.SIGKILL)
            self.assertEqual(process.communicate.call_count, 2)
            self.assertEqual(result["terminal"], "timeout")

    def test_controller_owned_test_path_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            test_path = root / intake.TEST_PATH
            test_path.parent.mkdir(parents=True)
            external = Path(temporary) / "outside.py"
            external.write_text("private external content", encoding="utf-8")
            test_path.symlink_to(external)
            with self.assertRaisesRegex(intake.IntakeError, "symlink"):
                intake._read_pinned(root.resolve(), intake.TEST_PATH)

    def test_optional_intake_target_must_be_private_and_outside_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            inside = root / "target"
            with (mock.patch.dict(os.environ, {"CODEGG_HARNESS_INTAKE_TARGET_DIR": str(inside)}),
                  mock.patch.object(intake.subprocess, "run") as run):
                with self.assertRaisesRegex(intake.IntakeError, "outside the source checkout"):
                    intake._build_binary(root, Path(temporary) / "fallback")
            run.assert_not_called()

            external = Path(temporary) / "external-target"
            external.mkdir()
            link = Path(temporary) / "target-link"
            link.symlink_to(external, target_is_directory=True)
            with mock.patch.dict(os.environ, {"CODEGG_HARNESS_INTAKE_TARGET_DIR": str(link)}):
                with self.assertRaisesRegex(intake.IntakeError, "must not be a symlink"):
                    intake._build_binary(root, Path(temporary) / "fallback")

    def test_cli_report_is_display_only_and_reproduction_rejects_command_override(self):
        dossier = intake.build_synthetic_receipt(
            defect_id="synthetic", source_sha256="d" * 64, failure_signature="fixture")
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "dossier.json"
            path.write_text(json.dumps(dossier), encoding="utf-8")
            with mock.patch("sys.stdout", new_callable=StringIO) as output:
                self.assertEqual(harness_cli.main(["intake", "report", "--dossier", str(path)]), 0)
            self.assertFalse(json.loads(output.getvalue())["defect_eligible"])
            with self.assertRaises(SystemExit):
                harness_cli.main(["intake", "reproduce", "--defect-id", "bad",
                                  "--output", str(path), "--command", "arbitrary"])


if __name__ == "__main__":
    unittest.main()
