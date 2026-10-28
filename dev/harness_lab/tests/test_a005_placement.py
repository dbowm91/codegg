from __future__ import annotations

import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest

from dev.harness_lab import autonomy
from dev.harness_lab.autonomy_run import case_root


REPO_ROOT = Path(__file__).resolve().parents[3]
MANIFEST = REPO_ROOT / "dev" / "harness_lab" / "fixtures" / "a005_placement"
EXPECTED = {
    "a005-rust-dependency-inspection": ("auto", True),
    "a005-rust-isolated-build": ("isolated_new", True),
    "a005-python-read-only": ("auto", True),
    "a005-python-isolated-debug": ("isolated_new", True),
    "a005-spanish-isolated-edit": ("isolated_new", True),
    "a005-existing-unavailable-refusal": ("existing_managed_read_only", False),
}


class A005PlacementFixtureTests(unittest.TestCase):
    def test_manifest_freezes_each_required_placement_workload(self) -> None:
        suite = autonomy.load_autonomy_suite(MANIFEST)
        self.assertEqual({case.case_id for case in suite.cases}, set(EXPECTED))
        self.assertEqual({case.family for case in suite.cases}, {"delegation"})
        for case in suite.cases:
            root = case_root(REPO_ROOT, case.case_id)
            self.assertTrue((root / "fixture").is_dir(), case.case_id)
            self.assertTrue((root / "verifier" / "verify_placement.py").is_file(),
                            case.case_id)

    def test_hidden_verifiers_require_observed_placement_and_outcome(self) -> None:
        suite = autonomy.load_autonomy_suite(MANIFEST)
        for case in suite.cases:
            expected, accepted = EXPECTED[case.case_id]
            with self.subTest(case=case.case_id), tempfile.TemporaryDirectory() as temp:
                root = case_root(REPO_ROOT, case.case_id)
                transcript_path = Path(temp) / "transcript.json"
                status = "completed" if accepted else "failed"
                call = {
                    "tool": "task",
                    "status": status,
                    "task_placement": {
                        "requested": expected,
                        "worktree_id_present": not accepted,
                    },
                }
                if accepted:
                    call["placement_accepted"] = True
                    status_call = {"tool": "task", "status": "completed",
                                   "task_run_status": "completed"}
                    tool_calls = [call, status_call]
                else:
                    tool_calls = [call]
                transcript_path.write_text(json.dumps({
                    "case_id": case.case_id,
                    "tool_calls": tool_calls,
                }), encoding="utf-8")
                env = dict(os.environ)
                env["CODEGG_AUTONOMY_TRANSCRIPT"] = str(transcript_path)
                command = list(case.verifier["command"])
                command[0] = os.sys.executable
                completed = subprocess.run(command, cwd=root, env=env,
                                           capture_output=True, text=True,
                                           timeout=10, check=False)
                self.assertEqual(completed.returncode, 0,
                                 f"{case.case_id}: {completed.stderr}")

    def test_unavailable_checkout_control_accepts_queued_refusal_and_runtime_artifacts(self) -> None:
        case = next(case for case in autonomy.load_autonomy_suite(MANIFEST).cases
                    if case.case_id == "a005-existing-unavailable-refusal")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "case"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "test@example.invalid"],
                           check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Fixture"],
                           check=True)
            (root / "fixture").mkdir()
            (root / "fixture" / "README.md").write_text("frozen\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "fixture/README.md"], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-q", "-m",
                            "frozen placement fixture"], check=True)
            verifier_root = Path(temp) / "private"
            shutil.copytree(case_root(REPO_ROOT, case.case_id) / "verifier",
                            verifier_root / "verifier")
            (root / ".codegg").mkdir()
            (root / ".codegg" / "run.json").write_text("{}\n", encoding="utf-8")
            (root / "codegg_debug.log").write_text("runtime log\n", encoding="utf-8")
            self.assertEqual(subprocess.run(
                ["git", "-C", str(root), "log", "-1", "--format=%s"],
                check=True, capture_output=True, text=True).stdout.strip(),
                "frozen placement fixture")
            self.assertEqual(len([line for line in subprocess.run(
                ["git", "-C", str(root), "worktree", "list", "--porcelain"],
                check=True, capture_output=True, text=True).stdout.splitlines()
                if line.startswith("worktree ")]), 1)

            exact_request = {
                "requested": "existing_managed_read_only",
                "worktree_id_present": True,
            }
            transcript_path = Path(temp) / "transcript.json"
            transcript_path.write_text(json.dumps({
                "case_id": case.case_id,
                "tool_calls": [
                    {"tool": "task", "index": 0, "status": "failed",
                     "task_placement": exact_request},
                    {"tool": "task", "index": 1, "status": "completed",
                     "placement_accepted": True,
                     "task_run_status": "failed", "task_placement": exact_request},
                    {"tool": "task", "index": 2, "status": "completed",
                     "task_run_status": "failed"},
                ],
            }), encoding="utf-8")
            env = dict(os.environ)
            env["CODEGG_AUTONOMY_TRANSCRIPT"] = str(transcript_path)
            env["CODEGG_AUTONOMY_CASE_ROOT"] = str(root)
            command = list(case.verifier["command"])
            command[0] = os.sys.executable
            completed = subprocess.run(command, cwd=verifier_root, env=env,
                                       capture_output=True, text=True, timeout=10, check=False)
            self.assertEqual(completed.returncode, 0,
                             f"stdout={completed.stdout!r} stderr={completed.stderr!r}")

    def test_unavailable_checkout_control_rejects_a_completed_child(self) -> None:
        case = next(case for case in autonomy.load_autonomy_suite(MANIFEST).cases
                    if case.case_id == "a005-existing-unavailable-refusal")
        with tempfile.TemporaryDirectory() as temp:
            transcript_path = Path(temp) / "transcript.json"
            transcript_path.write_text(json.dumps({
                "case_id": case.case_id,
                "tool_calls": [{
                    "tool": "task", "status": "completed", "placement_accepted": True,
                    "task_run_status": "completed",
                    "task_placement": {"requested": "existing_managed_read_only",
                                       "worktree_id_present": True},
                }],
            }), encoding="utf-8")
            env = dict(os.environ)
            env["CODEGG_AUTONOMY_TRANSCRIPT"] = str(transcript_path)
            command = list(case.verifier["command"])
            command[0] = os.sys.executable
            completed = subprocess.run(command, cwd=case_root(REPO_ROOT, case.case_id),
                                       env=env, capture_output=True, text=True,
                                       timeout=10, check=False)
            self.assertEqual(completed.returncode, 1)


if __name__ == "__main__":
    unittest.main()
