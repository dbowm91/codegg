import hashlib
import json
import tempfile
import unittest
from pathlib import Path
import sqlite3

from dev.harness_lab.ledger import (SCHEMA_VERSION, SUPPORTED_SCHEMA_VERSIONS, TERMINALS,
                                    canonical_digest, compare, load_runs, record, replay, report,
                                    validate_candidate_resume_rows)


def _write_v1_ledger(root: Path, *, run_id: str = "v1-run", candidate: str = "base",
                     task_id: str = "one", group_id: str = "g", suite: str = "suite",
                     terminal: str = "passed", usage: bool = True) -> dict:
    """Write a hand-written v1 ledger: index row, manifest, and event row.

    The v1 shape is written by hand on purpose: it proves the reader accepts a
    stored v1 pilot ledger instead of only the rows this writer produces.
    """
    run_dir = root / "runs" / run_id
    run_dir.mkdir(parents=True)
    metrics = {"tool_started": 1, "tool_finished": 1, "provider_calls": 1, "wall_time_ms": 12,
               "input_tokens": 10 if usage else None, "output_tokens": 4 if usage else None,
               "cached_tokens": 2 if usage else None, "reasoning_tokens": 1 if usage else None,
               "cost_usd": None, "repeated_tool_calls": 0, "recovery_followups": 0,
               "unavailable": []}
    manifest = {
        "schema_version": 1, "run_id": run_id, "task_id": task_id, "group_id": group_id,
        "repeat": 0, "candidate": candidate, "suite_sha256": suite, "case_sha256": "case",
        "created_unix_ms": 1, "subject": {}, "policy": None, "session_id": "s", "turn_id": "t",
        "terminal": terminal, "failure_class": None,
        "verifier": {"kind": "workspace_file", "path": "receipt.txt", "expected_sha256": None,
                     "expected_exists": True, "expected_text_sha256": None, "status": "passed",
                     "path": "receipt.txt", "observed_sha256": None},
        "metrics": metrics, "receipt_digest": "digest", "trace_count": 0, "receipt": {},
    }
    manifest_bytes = json.dumps(manifest, sort_keys=True, indent=2).encode("utf-8") + b"\n"
    (run_dir / "manifest.json").write_bytes(manifest_bytes)
    events = [{"schema_version": 1, "seq": 1, "run_id": run_id, "kind": "terminal",
               "terminal": terminal, "receipt_digest": "digest"}]
    (run_dir / "events.jsonl").write_text(
        "".join(json.dumps(event, sort_keys=True, separators=(",", ":")) + "\n" for event in events),
        encoding="utf-8")
    row = {"schema_version": 1, "run_id": run_id, "task_id": task_id, "group_id": group_id,
           "repeat": 0, "candidate": candidate,
           "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest()}
    with (root / "index.jsonl").open("a", encoding="utf-8") as output:
        output.write(json.dumps(row, sort_keys=True) + "\n")
    return row


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
            first = record(root / "ledger", receipt, task_id="write", group_id="g", candidate="base",
                           workspace=workspace, expected_file="receipt.txt", expected_text="expected",
                           repeat=0, suite_sha256="suite", case_sha256="case",
                           trace=[{"kind": "tool_started", "elapsed_ms": 4},
                                  {"kind": "tool_finished", "elapsed_ms": 9}])
            (workspace / "receipt.txt").write_text("wrong", encoding="utf-8")
            second = record(root / "ledger", receipt, task_id="write", group_id="g", candidate="candidate",
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


class LedgerSchemaVersionTests(unittest.TestCase):
    def test_stored_v1_ledger_loads_and_reports(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "ledger"
            root.mkdir()
            _write_v1_ledger(root)
            runs = load_runs(root)
            self.assertEqual(len(runs), 1)
            self.assertEqual(runs[0]["schema_version"], 1)
            self.assertEqual(runs[0]["terminal"], "passed")
            summary = report(root)
            self.assertEqual(summary["runs"], 1)
            self.assertEqual(summary["terminals"], {"passed": 1})
            self.assertEqual(summary["token_usage"], {"measured_runs": 1, "input_tokens": 10,
                                                       "output_tokens": 4, "cached_tokens": 2,
                                                       "reasoning_tokens": 1})

    def test_v2_ledger_round_trips_and_reports(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            manifest = record(root / "ledger", {"codegg_usage": {"turn_id": "t", "input_tokens": 3,
                                                                  "output_tokens": 2}},
                              task_id="one", group_id="g", candidate="c", workspace=workspace,
                              expected_file="receipt.txt", expected_text="ok",
                              suite_sha256="suite", case_sha256="case")
            self.assertEqual(manifest["schema_version"], SCHEMA_VERSION)
            runs = load_runs(root / "ledger")
            self.assertEqual(len(runs), 1)
            self.assertEqual(runs[0]["schema_version"], SCHEMA_VERSION)
            self.assertEqual(report(root / "ledger")["runs"], 1)
            self.assertEqual(report(root / "ledger")["token_usage"]["input_tokens"], 3)

    def test_supported_schema_version_range_rejects_other_versions(self) -> None:
        self.assertEqual(SCHEMA_VERSION, 2)
        self.assertEqual(SUPPORTED_SCHEMA_VERSIONS, (1, 2))
        self.assertIn("interrupted_unknown", TERMINALS)

    def test_unsupported_index_and_event_versions_are_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "ledger"
            root.mkdir()
            row = _write_v1_ledger(root)
            index = root / "index.jsonl"
            index.write_text(json.dumps({**row, "schema_version": 99}, sort_keys=True) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unsupported schema"):
                load_runs(root)
            index.write_text(json.dumps(row, sort_keys=True) + "\n", encoding="utf-8")
            events = root / "runs" / row["run_id"] / "events.jsonl"
            events.write_text(events.read_text(encoding="utf-8").replace('"schema_version":1', '"schema_version":99'),
                              encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "run event identity or sequence is invalid"):
                load_runs(root)


class LedgerUsageReasonTests(unittest.TestCase):
    def _record(self, root: Path, workspace: Path, receipt: dict) -> dict:
        return record(root, receipt, task_id="one", group_id="g", candidate="c",
                      workspace=workspace, expected_file="receipt.txt", expected_text="ok")

    def test_usage_unavailable_is_named_and_never_zero(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            unmeasured = self._record(root / "ledger", workspace, {})
            self.assertEqual(unmeasured["usage_unavailable"], "unspecified")
            named = self._record(root / "ledger", workspace, {"usage_unavailable_reason": "not_observed"})
            self.assertEqual(named["usage_unavailable"], "not_observed")
            measured = self._record(root / "ledger", workspace,
                                    {"codegg_usage": {"turn_id": "t", "input_tokens": 4, "output_tokens": 2}})
            self.assertIsNone(measured["usage_unavailable"])

    def test_v1_row_defaults_usage_reason_when_read_back(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "ledger"
            root.mkdir()
            _write_v1_ledger(root, run_id="v1-absent", usage=False)
            _write_v1_ledger(root, run_id="v1-measured", usage=True)
            runs = {row["run_id"]: row for row in load_runs(root)}
            self.assertEqual(runs["v1-absent"]["usage_unavailable"], "not_reported")
            self.assertIsNone(runs["v1-measured"]["usage_unavailable"])

    def test_failure_category_is_persisted_as_is(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            categorized = self._record(root / "ledger", workspace, {"failure_category": "provider_timeout"})
            self.assertEqual(categorized["failure_category"], "provider_timeout")
            absent = self._record(root / "ledger", workspace, {})
            self.assertIsNone(absent["failure_category"])
            self.assertNotEqual(absent["failure_category"], "native_failure_unspecified")

    def test_report_never_sums_a_missing_token_count_as_zero(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            record(root / "ledger", {"codegg_usage": {"turn_id": "t", "input_tokens": 5, "output_tokens": 3}},
                   task_id="one", group_id="g", candidate="c", workspace=workspace,
                   expected_file="receipt.txt", expected_text="ok")
            summary = report(root / "ledger")
            self.assertEqual(summary["token_usage"], {"measured_runs": 1, "input_tokens": 5,
                                                       "output_tokens": 3, "cached_tokens": None,
                                                       "reasoning_tokens": None})
            record(root / "empty", {}, task_id="one", group_id="g", candidate="c",
                   workspace=workspace, expected_file="receipt.txt", expected_text="ok")
            self.assertIsNone(report(root / "empty")["token_usage"])


class LedgerCompareTests(unittest.TestCase):
    def _record(self, root: Path, workspace: Path, *, task_id: str, candidate: str, repeat: int = 0,
                daemon: dict | None = None, suite: str = "suite") -> dict:
        return record(root, {"daemon": daemon or {}}, task_id=task_id, group_id="g",
                      candidate=candidate, workspace=workspace, expected_file="receipt.txt",
                      expected_text="ok", repeat=repeat, suite_sha256=suite, case_sha256="case")

    def test_compare_refuses_partial_pairing_same_candidate_and_zero_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record(ledger, workspace, task_id="one", candidate="base")
            self._record(ledger, workspace, task_id="two", candidate="candidate")
            with self.assertRaisesRegex(ValueError, "incomplete"):
                compare(ledger, "base", "candidate")
            with self.assertRaisesRegex(ValueError, "distinct candidates"):
                compare(ledger, "base", "base")
            with self.assertRaisesRegex(ValueError, "denominator"):
                compare(root / "empty", "base", "candidate")

    def test_compare_refuses_subject_identity_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            # The subject identity is pinned per candidate arm: one arm that
            # changed its subject binary fingerprint is not one comparable arm.
            self._record(ledger, workspace, task_id="one", candidate="base",
                         daemon={"binary_sha256": "aaa", "revision": "r1"})
            self._record(ledger, workspace, task_id="two", candidate="base",
                         daemon={"binary_sha256": "bbb", "revision": "r1"})
            self._record(ledger, workspace, task_id="one", candidate="candidate",
                         daemon={"binary_sha256": "aaa", "revision": "r1"})
            with self.assertRaisesRegex(ValueError, "subject binary fingerprint"):
                compare(ledger, "base", "candidate")

    def test_compare_refuses_revision_mismatch_between_paired_trials(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record(ledger, workspace, task_id="one", candidate="base",
                         daemon={"binary_sha256": "aaa", "revision": "r1"})
            self._record(ledger, workspace, task_id="one", candidate="candidate",
                         daemon={"binary_sha256": "bbb", "revision": "r2"})
            with self.assertRaisesRegex(ValueError, "revision"):
                compare(ledger, "base", "candidate")

    def test_compare_pairs_by_task_and_repeat_and_counts_repeats(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            for candidate in ("base", "candidate"):
                for repeat in (0, 1):
                    self._record(ledger, workspace, task_id="task", candidate=candidate, repeat=repeat)
            comparison = compare(ledger, "base", "candidate")
            self.assertEqual(comparison["denominator"], 2)
            self.assertEqual(comparison["paired_task_ids"], ["task"])
            self.assertEqual([row["repeat"] for row in comparison["paired_trials"]], [0, 1])

            partial = root / "partial"
            for repeat in (0, 1):
                self._record(partial, workspace, task_id="task", candidate="base", repeat=repeat)
            self._record(partial, workspace, task_id="task", candidate="candidate", repeat=0)
            with self.assertRaisesRegex(ValueError, "incomplete"):
                compare(partial, "base", "candidate")

    def _record_terminal(self, ledger: Path, workspace: Path, *, candidate: str, task_id: str,
                         terminal: str, expected_terminal: str) -> dict:
        return record(ledger, {"daemon": {"binary_sha256": "aaa", "revision": "r1"}},
                      task_id=task_id, group_id="g", candidate=candidate, workspace=workspace,
                      expected_file="receipt.txt", expected_text="ok", repeat=0,
                      suite_sha256="suite", case_sha256="case",
                      terminal_override=terminal, expected_terminal=expected_terminal)

    def test_a_negative_control_the_candidate_satisfies_is_not_a_regression(self) -> None:
        """A control that expects a failure is judged against that failure.

        The baseline recorded ``passed`` for a control whose declared terminal
        is ``execution_failed``, so the baseline did not satisfy the control.
        The candidate recorded the declared failure and therefore did. Under the
        raw-terminal rule that candidate was reported as a regression for being
        more correct than the baseline.
        """
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record_terminal(ledger, workspace, candidate="base", task_id="control",
                                  terminal="passed", expected_terminal="execution_failed")
            self._record_terminal(ledger, workspace, candidate="candidate", task_id="control",
                                  terminal="execution_failed", expected_terminal="execution_failed")
            comparison = compare(ledger, "base", "candidate")
            self.assertEqual(comparison["regressions"], [])
            self.assertEqual(len(comparison["improvements"]), 1)
            self.assertEqual(comparison["improvements"][0]["right_terminal"], "execution_failed")
            self.assertEqual(len(comparison["unmet_expectations"]), 1)

    def test_a_negative_control_the_candidate_breaks_is_a_regression(self) -> None:
        """The expectation-relative rule is stricter for controls, not looser.

        A baseline that satisfied the control and a candidate that stopped
        satisfying it is a regression, even though the candidate's raw terminal
        is the literal string ``passed``.
        """
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record_terminal(ledger, workspace, candidate="base", task_id="control",
                                  terminal="execution_failed", expected_terminal="execution_failed")
            self._record_terminal(ledger, workspace, candidate="candidate", task_id="control",
                                  terminal="passed", expected_terminal="execution_failed")
            comparison = compare(ledger, "base", "candidate")
            self.assertEqual(len(comparison["regressions"]), 1)
            self.assertEqual(comparison["regressions"][0]["task_id"], "control")
            self.assertEqual(comparison["improvements"], [])

    def test_an_objective_case_that_stops_passing_is_a_regression(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record_terminal(ledger, workspace, candidate="base", task_id="objective",
                                  terminal="passed", expected_terminal="passed")
            self._record_terminal(ledger, workspace, candidate="candidate", task_id="objective",
                                  terminal="execution_failed", expected_terminal="passed")
            comparison = compare(ledger, "base", "candidate")
            self.assertEqual(len(comparison["regressions"]), 1)
            self.assertEqual(len(comparison["improvements"]), 0)

    def test_compare_refuses_a_trial_with_no_declared_expectation(self) -> None:
        """An unknown expectation is refused, never guessed.

        A legacy row written before the manifest carried a declared expectation
        cannot be compared: the harness would have to guess what the trial was
        supposed to prove.
        """
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ledger = root / "ledger"
            ledger.mkdir(parents=True)
            _write_v1_ledger(ledger, run_id="v1-base", candidate="base",
                             task_id="one", terminal="passed")
            _write_v1_ledger(ledger, run_id="v1-candidate", candidate="candidate",
                             task_id="one", terminal="passed")
            with self.assertRaisesRegex(ValueError, "expected terminal"):
                compare(ledger, "base", "candidate")

    def test_an_explicit_expectation_overrides_the_recorded_one(self) -> None:
        """The frozen suite owns the expectation when it supplies one."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            self._record_terminal(ledger, workspace, candidate="base", task_id="control",
                                  terminal="passed", expected_terminal="passed")
            self._record_terminal(ledger, workspace, candidate="candidate", task_id="control",
                                  terminal="execution_failed", expected_terminal="passed")
            comparison = compare(ledger, "base", "candidate",
                                 expected_terminals={("control", 0): "execution_failed"})
            self.assertEqual(comparison["regressions"], [])
            self.assertEqual(len(comparison["improvements"]), 1)

    def test_a_suite_that_varies_its_policy_between_cases_is_comparable(self) -> None:
        """A case may use a different policy than its neighbours.

        The frozen suite runs one case under the full-host policy and the rest
        under the workspace-write policy, and the headless adapter records no
        ACP policy handshake at all. Requiring one policy across an arm made
        the frozen suite impossible to compare at all.
        """
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            policies = {"host": {"sandbox": "full_host"}, "write": {"sandbox": "workspace_write"}}
            for candidate, terminal in (("base", "passed"), ("candidate", "passed")):
                for task_id, policy in policies.items():
                    record(ledger, {"daemon": {"binary_sha256": "aaa", "revision": "r1"},
                                    "policy": policy},
                           task_id=task_id, group_id="g", candidate=candidate, workspace=workspace,
                           expected_file="receipt.txt", expected_text="ok", repeat=0,
                           suite_sha256="suite", case_sha256="case-" + task_id,
                           terminal_override=terminal)
            comparison = compare(ledger, "base", "candidate",
                                 expected_terminals={("host", 0): "passed", ("write", 0): "passed"})
            self.assertEqual(comparison["regressions"], [])
            self.assertEqual(comparison["denominator"], 2)

    def test_an_infrastructure_error_is_not_a_measured_trial(self) -> None:
        """A measurement failure is re-run, not scored as a candidate failure."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            for candidate, terminal in (("base", "passed"), ("candidate", "infrastructure_error")):
                record(ledger, {"daemon": {"binary_sha256": "aaa", "revision": "r1"}},
                       task_id="one", group_id="g", candidate=candidate, workspace=workspace,
                       expected_file="receipt.txt", expected_text="ok", repeat=0,
                       suite_sha256="suite", case_sha256="case",
                       terminal_override=terminal)
            # The candidate's only trial carries no measurement, so the arm has
            # no completed trial to pair and the comparison refuses rather than
            # scoring the measurement failure as a candidate failure.
            with self.assertRaisesRegex(ValueError, "no completed trials"):
                compare(ledger, "base", "candidate",
                        expected_terminals={("one", 0): "passed"})


class LedgerRecoveryTests(unittest.TestCase):
    def test_orphan_run_directory_is_reconciled_as_interrupted_unknown(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            manifest = record(ledger, {"codegg_usage": {"turn_id": "t", "input_tokens": 4, "output_tokens": 2},
                                       "tool_started": 3},
                              task_id="one", group_id="g", candidate="c", workspace=workspace,
                              expected_file="receipt.txt", expected_text="ok",
                              suite_sha256="suite", case_sha256="case")
            # Simulate the crash between the manifest write and the index append.
            orphan_id = "orphan-run"
            orphan_dir = ledger / "runs" / orphan_id
            orphan_dir.mkdir(parents=True)
            orphan = dict(manifest)
            orphan["run_id"] = orphan_id
            (orphan_dir / "manifest.json").write_text(json.dumps(orphan, sort_keys=True, indent=2) + "\n",
                                                       encoding="utf-8")
            runs = {row["run_id"]: row for row in load_runs(ledger)}
            self.assertIn(orphan_id, runs)
            reconciled = runs[orphan_id]
            self.assertEqual(reconciled["terminal"], "interrupted_unknown")
            self.assertEqual(reconciled["recorded_terminal"], "passed")
            self.assertTrue(reconciled["reconciled"])
            self.assertIsNone(reconciled["metrics"]["input_tokens"])
            self.assertIsNone(reconciled["metrics"]["output_tokens"])
            self.assertIsNone(reconciled["metrics"]["cached_tokens"])
            self.assertIsNone(reconciled["metrics"]["reasoning_tokens"])
            self.assertEqual(reconciled["usage_unavailable"], "unspecified")
            summary = report(ledger)
            self.assertEqual(summary["terminals"]["interrupted_unknown"], 1)
            self.assertEqual(summary["token_usage"]["measured_runs"], 1)
            # The reconciled attempt has no measured counts: it contributes
            # nothing and is never summed as zero.
            self.assertEqual(summary["tool_started"], 3)
            self.assertEqual(summary["usage_unavailable"]["unspecified"], 1)

            # An attempt directory without a manifest is reconciled too.
            (ledger / "runs" / "orphan-empty").mkdir(parents=True)
            runs = {row["run_id"]: row for row in load_runs(ledger)}
            self.assertEqual(runs["orphan-empty"]["terminal"], "interrupted_unknown")
            self.assertIsNone(runs["orphan-empty"]["metrics"]["input_tokens"])

    def test_torn_final_index_line_is_tolerated_and_middle_corruption_is_fatal(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "receipt.txt").write_text("ok", encoding="utf-8")
            ledger = root / "ledger"
            record(ledger, {}, task_id="one", group_id="g", candidate="c",
                   workspace=workspace, expected_file="receipt.txt", expected_text="ok")
            index = ledger / "index.jsonl"
            with index.open("a", encoding="utf-8") as output:
                output.write('{"schema_version": 2, "run_id": "torn')
            self.assertEqual(report(ledger)["runs"], 1)
            record(ledger, {}, task_id="two", group_id="g", candidate="c",
                   workspace=workspace, expected_file="receipt.txt", expected_text="ok")
            self.assertEqual(report(ledger)["runs"], 2)
            rows = [json.loads(line) for line in index.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(len(rows), 2)

            corrupt = root / "corrupt"
            record(corrupt, {}, task_id="one", group_id="g", candidate="c",
                   workspace=workspace, expected_file="receipt.txt", expected_text="ok")
            corrupt_index = corrupt / "index.jsonl"
            corrupt_index.write_text(corrupt_index.read_text(encoding="utf-8") + "GARBAGE\n",
                                     encoding="utf-8")
            with self.assertRaises(ValueError):
                report(corrupt)


if __name__ == "__main__":
    unittest.main()
