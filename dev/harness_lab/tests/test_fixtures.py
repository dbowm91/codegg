import json
import unittest
from pathlib import Path


class FixtureManifestTests(unittest.TestCase):
    def test_smoke_manifest_covers_required_trajectory_classes(self) -> None:
        path = Path(__file__).parents[1] / "fixtures" / "smoke.jsonl"
        cases = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        self.assertGreaterEqual(len(cases), 8)
        self.assertLessEqual(len(cases), 12)
        self.assertEqual(len({case["task_id"] for case in cases}), len(cases))
        tasks = {case["task_id"] for case in cases}
        self.assertTrue({"edit-existing", "grep-existing", "distinct-read-calls", "failed-shell-then-recovered",
                         "projected-output-context-recovery", "cancel-after-read", "timeout-after-read",
                         "repeated-identical-read-error", "explicit-bash-denial", "verifier-negative"}.issubset(tasks))
        self.assertEqual({case["profile"] for case in cases}, {"workspace-write", "full-host"})
        self.assertEqual({case["schema_version"] for case in cases}, {1})


if __name__ == "__main__":
    unittest.main()
