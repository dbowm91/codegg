import tempfile
import unittest
from pathlib import Path
import sqlite3

from dev.harness_lab.ledger import (canonical_digest, compare, record, replay, report,
                                    validate_candidate_resume_rows)


class LedgerTests(unittest.TestCase):
    def test_report_uses_provider_usage_and_summarizes_repeats_and_recovery(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            record(root / "ledger", {"codegg_usage": {"turn_id": "turn-1", "input_tokens": 17,
                                                       "output_tokens": 9, "cached_tokens": 3,
                                                       "reasoning_tokens": 2}},
                   task_id="recover", group_id="g", candidate="c", workspace=workspace,
                   expected_file="receipt.txt", expected_text="ok",
                   trace=[{"kind": "tool_started", "tool": "read", "arguments_sha256": "same"},
                          {"kind": "tool_finished", "tool": "read", "status": "failed"},
                          {"kind": "tool_started", "tool": "read", "arguments_sha256": "same"},
                          {"kind": "tool_finished", "tool": "read", "status": "completed"}])
            summary = report(root / "ledger")
            self.assertEqual(summary["token_usage"], {"measured_runs": 1, "input_tokens": 17,
                                                       "output_tokens": 9, "cached_tokens": 3,
                                                       "reasoning_tokens": 2})
            self.assertEqual(summary["repeated_tool_calls"], 1)
            self.assertEqual(summary["recovery_followups"], 1)

    def test_verifier_artifact_replays_offline_and_compare_pairs_task(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("expected", encoding="utf-8")
            receipt = {"daemon": {"daemon_id": "d1"}, "session_id": "s1", "tool_started": 1,
                       "tool_finished": 1, "provider_calls": 2, "policy": {"effective": "workspace_write"}}
            first = record(root / "ledger", receipt, task_id="write", group_id="safe", candidate="base",
                           workspace=workspace, expected_file="receipt.txt", expected_text="expected",
                           repeat=0, suite_sha256="suite", case_sha256="case",
                           trace=[{"kind": "tool_started", "elapsed_ms": 4},
                                  {"kind": "tool_finished", "elapsed_ms": 9}])
            (workspace / "receipt.txt").write_text("wrong", encoding="utf-8")
            second = record(root / "ledger", receipt, task_id="write", group_id="safe", candidate="candidate",
                            workspace=workspace, expected_file="receipt.txt", expected_text="expected",
                            repeat=0, suite_sha256="suite", case_sha256="case")
            self.assertEqual(replay(root / "ledger", first["run_id"])["replay_status"], "passed")
            self.assertEqual(report(root / "ledger")["runs"], 2)
            comparison = compare(root / "ledger", "base", "candidate")
            self.assertEqual(comparison["denominator"], 1)
            self.assertEqual((comparison["left_passed"], comparison["right_passed"]), (1, 0))
            self.assertEqual(first["trace_count"], 2)
            self.assertEqual(second["terminal"], "assertion_failed")
            database = root / "ledger" / "index.sqlite3"
            self.assertTrue(database.is_file())
            with sqlite3.connect(database) as connection:
                self.assertEqual(connection.execute("SELECT count(*) FROM runs").fetchone()[0], 2)
                with self.assertRaises(sqlite3.IntegrityError):
                    connection.execute("UPDATE runs SET candidate='changed' WHERE run_id=?", (second["run_id"],))
            artifact = root / "ledger" / "runs" / first["run_id"] / "artifacts" / "verified-output"
            artifact.write_text("tampered", encoding="utf-8")
            self.assertEqual(replay(root / "ledger", first["run_id"])["replay_status"], "assertion_failed")

    def test_sqlite_index_mismatch_is_detected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            record(root / "ledger", {}, task_id="one", group_id="g", candidate="c",
                   workspace=workspace, expected_file="receipt.txt", expected_text="ok")
            with sqlite3.connect(root / "ledger" / "index.sqlite3") as connection:
                connection.execute("DROP TRIGGER runs_no_update")
                connection.execute("UPDATE runs SET task_id='tampered'")
            with self.assertRaises(ValueError):
                report(root / "ledger")

    def test_paired_comparison_refuses_different_case_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            for candidate, digest in (("base", "a"), ("candidate", "b")):
                record(root / "ledger", {}, task_id="same", group_id="g", candidate=candidate,
                       workspace=workspace, expected_file="receipt.txt", expected_text="ok",
                       suite_sha256="suite", case_sha256=digest)
            with self.assertRaises(ValueError):
                compare(root / "ledger", "base", "candidate")

    def test_candidate_resume_preserves_completed_prefix_and_rejects_changed_suite(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "one.txt").write_text("one", encoding="utf-8")
            cases = [{"task_id": "one", "group_id": "g", "repeats": 2},
                     {"task_id": "two", "group_id": "g"}]
            suite_digest = "frozen-suite"
            record(root / "ledger", {}, task_id="one", group_id="g", candidate="candidate",
                   workspace=workspace, expected_file="one.txt", expected_text="one", repeat=0,
                   suite_sha256=suite_digest, case_sha256=canonical_digest(cases[0]))

            existing = validate_candidate_resume_rows(root / "ledger", "candidate", cases,
                                                      suite_digest, "default")
            self.assertEqual(set(existing), {("one", 0)})
            with self.assertRaisesRegex(ValueError, "immutable suite"):
                validate_candidate_resume_rows(root / "ledger", "candidate", cases,
                                               "changed-suite", "default")

    def test_verifier_refuses_workspace_escape(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            with self.assertRaises(ValueError):
                record(root / "ledger", {}, task_id="escape", group_id="g", candidate="c",
                       workspace=workspace, expected_file="../outside")


if __name__ == "__main__":
    unittest.main()
