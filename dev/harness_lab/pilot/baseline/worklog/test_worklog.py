import json
import tempfile
import unittest
from datetime import datetime, timezone, timedelta
from pathlib import Path
from worklog import Worklog

class WorklogTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / "nested" / "events.jsonl"
        self.log = Worklog(self.path)
    def tearDown(self):
        self.tmp.cleanup()
    def test_missing_file_is_empty(self):
        self.assertEqual(self.log.events(), [])
        self.assertEqual(self.log.summarize(), {"count": 0, "authors": {}, "first_at": None, "last_at": None})
    def test_append_creates_parent_and_assigns_ids(self):
        one = self.log.append("start", "Ada", datetime(2026, 1, 2, tzinfo=timezone.utc))
        two = self.log.append("finish", "Lin", datetime(2026, 1, 3, tzinfo=timezone.utc))
        self.assertEqual((one["id"], two["id"]), (1, 2))
        self.assertTrue(self.path.is_file())
        self.assertEqual(len(self.path.read_text().splitlines()), 2)
    def test_timestamp_is_normalized_to_utc_z(self):
        event = self.log.append("x", "Ada", datetime(2026, 1, 2, 3, tzinfo=timezone(timedelta(hours=3))))
        self.assertEqual(event["created_at"], "2026-01-02T00:00:00Z")
    def test_default_timestamp_is_utc(self):
        event = self.log.append("x", "Ada")
        parsed = datetime.fromisoformat(event["created_at"].replace("Z", "+00:00"))
        self.assertEqual(parsed.utcoffset(), timedelta(0))
    def test_filter_and_newest_order(self):
        self.log.append("a", "Ada", datetime(2026, 1, 1, tzinfo=timezone.utc))
        self.log.append("b", "Lin", datetime(2026, 1, 2, tzinfo=timezone.utc))
        self.log.append("c", "Ada", datetime(2026, 1, 3, tzinfo=timezone.utc))
        self.assertEqual([e["text"] for e in self.log.events(author="Ada")], ["a", "c"])
        self.assertEqual([e["id"] for e in self.log.events(newest_first=True)], [3, 2, 1])
    def test_summary_has_exact_shape_and_sorted_authors(self):
        self.log.append("a", "Zed", datetime(2026, 1, 1, tzinfo=timezone.utc))
        self.log.append("b", "Ada", datetime(2026, 1, 2, tzinfo=timezone.utc))
        self.log.append("c", "Ada", datetime(2026, 1, 3, tzinfo=timezone.utc))
        self.assertEqual(self.log.summarize(), {"count": 3, "authors": {"Ada": 2, "Zed": 1}, "first_at": "2026-01-01T00:00:00Z", "last_at": "2026-01-03T00:00:00Z"})
    def test_invalid_append_arguments_are_rejected(self):
        for text, author in (("", "Ada"), ("x", ""), (" ", "Ada")):
            with self.subTest(text=text, author=author), self.assertRaises(ValueError):
                self.log.append(text, author)
    def test_naive_timestamp_is_rejected(self):
        with self.assertRaises(ValueError):
            self.log.append("x", "Ada", datetime(2026, 1, 1))
    def test_malformed_row_reports_line(self):
        self.path.parent.mkdir(parents=True)
        self.path.write_text('{"id":1}\nnot-json\n')
        with self.assertRaisesRegex(ValueError, "line 2"):
            self.log.events()
    def test_non_monotonic_ids_are_rejected(self):
        self.path.parent.mkdir(parents=True)
        self.path.write_text('\n'.join(json.dumps({"id": i, "author": "A", "text": "x", "created_at": "2026-01-01T00:00:00Z"}) for i in (1, 3)) + "\n")
        with self.assertRaises(ValueError):
            self.log.events()

if __name__ == "__main__":
    unittest.main()
