import unittest
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from summary_report import SummaryError, render_report, validate_summary


FIXTURES = Path(__file__).resolve().parents[2] / "fixtures"


class SummaryConsumerTests(unittest.TestCase):
    def setUp(self):
        self.summary = json.loads((FIXTURES / "expected-summary.json").read_text())

    def test_frozen_summary_validates_and_renders_exact_markdown(self):
        self.assertEqual(validate_summary(self.summary), self.summary)
        self.assertEqual(render_report(self.summary), (FIXTURES / "expected-report.md").read_text())

    def test_rejects_unknown_schema(self):
        summary = dict(self.summary, schema="codegg.project-summary.v2")
        with self.assertRaisesRegex(SummaryError, "schema|version"):
            validate_summary(summary)

    def test_rejects_inconsistent_totals(self):
        summary = json.loads(json.dumps(self.summary))
        summary["totals"]["events"] += 1
        with self.assertRaisesRegex(SummaryError, "total|aggregate"):
            render_report(summary)

    def test_rejects_boolean_counts_and_unknown_fields(self):
        summary = json.loads(json.dumps(self.summary))
        summary["totals"]["events"] = True
        with self.assertRaisesRegex(SummaryError, "integer|count|total"):
            validate_summary(summary)

        summary = json.loads(json.dumps(self.summary))
        summary["surprise"] = "no"
        with self.assertRaisesRegex(SummaryError, "unknown|field|key"):
            validate_summary(summary)

    def test_rejects_unsafe_path_and_duplicate_id(self):
        summary = json.loads(json.dumps(self.summary))
        summary["events"][0]["path"] = "../secret"
        with self.assertRaisesRegex(SummaryError, "path"):
            validate_summary(summary)

        summary = json.loads(json.dumps(self.summary))
        summary["events"][1]["id"] = summary["events"][0]["id"]
        with self.assertRaisesRegex(SummaryError, "duplicate|unique"):
            validate_summary(summary)

    def test_rejects_unsorted_events_and_inconsistent_kind_counts(self):
        summary = json.loads(json.dumps(self.summary))
        summary["events"].reverse()
        with self.assertRaisesRegex(SummaryError, "order|sort"):
            validate_summary(summary)

        summary = json.loads(json.dumps(self.summary))
        summary["by_kind"]["test_failed"] = 0
        with self.assertRaisesRegex(SummaryError, "kind|aggregate|count"):
            validate_summary(summary)

    def test_rejects_test_event_with_file_fields(self):
        summary = json.loads(json.dumps(self.summary))
        summary["events"][2]["path"] = "src/lib.rs"
        with self.assertRaisesRegex(SummaryError, "field|path|test"):
            validate_summary(summary)


if __name__ == "__main__":
    unittest.main()
