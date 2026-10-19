import json
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from summary_report import SummaryError, render_report, validate_summary

FIXTURES = Path(__file__).resolve().parents[2] / "fixtures"


class ActorV2Tests(unittest.TestCase):
    def setUp(self):
        self.summary = json.loads((FIXTURES / "expected-summary-v2.json").read_text())

    def test_v2_actor_validates_and_matches_frozen_report(self):
        self.assertEqual(validate_summary(self.summary), self.summary)
        self.assertEqual(render_report(self.summary), (FIXTURES / "expected-report-v2.md").read_text())

    def test_v1_stays_supported_and_invalid_actors_fail(self):
        v1 = json.loads((FIXTURES / "expected-summary.json").read_text())
        self.assertEqual(validate_summary(v1), v1)
        invalid = ["", "has space", "bad|actor", "é", "a" * 65]
        for actor in invalid:
            summary = json.loads(json.dumps(self.summary))
            summary["events"][0]["actor"] = actor
            with self.subTest(actor=actor), self.assertRaises(SummaryError):
                validate_summary(summary)

    def test_actor_is_rejected_by_v1_marker(self):
        summary = json.loads(json.dumps(self.summary))
        summary["schema"] = "codegg.project-summary.v1"
        with self.assertRaisesRegex(SummaryError, "actor|schema|version"):
            validate_summary(summary)


if __name__ == "__main__":
    unittest.main()
