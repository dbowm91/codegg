import json
import unittest
from pathlib import Path


class FixtureManifestTests(unittest.TestCase):
    def test_smoke_manifest_covers_required_trajectory_classes(self) -> None:
        path = Path(__file__).parents[1] / "fixtures" / "smoke.jsonl"
        cases = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        self.assertGreaterEqual(len(cases), 8)
        self.assertLessEqual(len(cases), 13)
        self.assertEqual(len({case["task_id"] for case in cases}), len(cases))
        tasks = {case["task_id"] for case in cases}
        self.assertTrue({"edit-existing", "grep-existing", "distinct-read-calls", "failed-shell-then-recovered",
                         "projected-output-context-recovery", "cancel-after-read", "timeout-after-read",
                         "repeated-identical-read-error", "explicit-bash-denial", "verifier-negative",
                         "headless-exec-workspace-write"}.issubset(tasks))
        self.assertEqual({case["profile"] for case in cases}, {"workspace-write", "full-host"})
        self.assertEqual({case["schema_version"] for case in cases}, {1})
        self.assertEqual({case.get("transport", "acp") for case in cases}, {"acp", "exec"})

    def test_the_headless_case_is_the_only_one_on_the_exec_transport(self) -> None:
        path = Path(__file__).parents[1] / "fixtures" / "smoke.jsonl"
        cases = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        headless = [case for case in cases if case.get("transport") == "exec"]
        self.assertEqual([case["task_id"] for case in headless], ["headless-exec-workspace-write"])
        # The headless case is an objective case, never a protected control:
        # its expected terminal is the healthy one, so a candidate that fixes
        # the headless path improves the objective instead of moving a guard.
        self.assertEqual(headless[0].get("expected_terminal", "passed"), "passed")


if __name__ == "__main__":
    unittest.main()
