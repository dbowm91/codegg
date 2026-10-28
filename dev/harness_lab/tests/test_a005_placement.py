from __future__ import annotations

import json
import os
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


if __name__ == "__main__":
    unittest.main()
