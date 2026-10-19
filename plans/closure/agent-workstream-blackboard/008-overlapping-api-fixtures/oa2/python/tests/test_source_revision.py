import json
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from summary_report import SummaryError, render_report, validate_summary

FIXTURES = Path(__file__).resolve().parents[2] / "fixtures"


class SourceRevisionTests(unittest.TestCase):
    def setUp(self):
        self.summary = json.loads((FIXTURES / "expected-summary.json").read_text())

    def test_revision_validates_and_uses_a_safe_dynamic_code_span(self):
        self.summary["source_revision"] = "rev`|<x>"
        self.assertEqual(validate_summary(self.summary), self.summary)
        empty = dict(
            self.summary,
            events=[],
            totals={"events": 0, "files_changed": 0, "insertions": 0, "deletions": 0},
            by_kind={"file_changed": 0, "test_passed": 0, "test_failed": 0},
        )
        report = render_report(empty)
        expected = (FIXTURES / "expected-report-source-revision.md").read_text()
        self.assertEqual(report, expected)

    def test_empty_controlled_and_unbounded_revisions_are_rejected(self):
        for revision in ["", " leading", "trailing ", "line\nbreak", "bad\x00control", "x" * 129]:
            summary = json.loads(json.dumps(self.summary))
            summary["source_revision"] = revision
            with self.subTest(revision=revision), self.assertRaises(SummaryError):
                validate_summary(summary)


if __name__ == "__main__":
    unittest.main()
