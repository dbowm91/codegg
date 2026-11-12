"""Redacted verifier and reviewer lifecycle projections."""
from __future__ import annotations

import hashlib
import json
import pathlib
import sqlite3
import sys
import tempfile
import unittest

from dev.harness_lab import autonomy_run, diagnostics


class VerifierDiagnosticTests(unittest.TestCase):
    def test_passed_verbose_output_keeps_only_test_ids_and_counts(self) -> None:
        result = diagnostics.summarize_unittest_output(
            "test_load (test_catalog.CatalogTests.test_load) ... ok\n"
            "test_query (test_catalog.CatalogTests.test_query) ... ok\n"
            "Ran 2 tests in 0.02s\n\nOK\n",
            exit_code=0,
        )

        self.assertEqual(result["state"], "passed")
        self.assertEqual(result["tests_run"], 2)
        self.assertEqual(result["passed_count"], 2)
        self.assertEqual(result["failed_tests"], [])
        self.assertEqual(result["failure_domain_details"], {})
        self.assertEqual(result["schema"], "h007_verifier_diagnostic_v4")
        self.assertFalse(result["raw_output_retained"])

    def test_assertion_failure_keeps_identity_class_and_safe_reason_terms(self) -> None:
        output = (
            "test_import (test_catalog.CatalogTests.test_import) ... FAIL\n"
            "test_query (test_catalog.CatalogTests.test_query) ... ok\n"
            "\n======================================================================\n"
            "FAIL: test_import (test_catalog.CatalogTests.test_import)\n"
            "----------------------------------------------------------------------\n"
            "Traceback (most recent call last):\n"
            "  File \"private.py\", line 4, in test_import\n"
            "    self.assertEqual(value, 'SENSITIVE_ASSERTION_VALUE')\n"
            "AssertionError: expected completed but got failed for job "
            "SENSITIVE_ASSERTION_VALUE\n"
            "\n----------------------------------------------------------------------\n"
            "Ran 2 tests in 0.02s\n\nFAILED (failures=1)\n"
        )
        result = diagnostics.summarize_unittest_output(output, exit_code=1)

        self.assertEqual(result["state"], "assertion_failures")
        self.assertEqual(result["failure_counts"]["failures"], 1)
        self.assertEqual(result["failed_tests"][0]["test_id"],
                         "test_import (test_catalog.CatalogTests.test_import)")
        self.assertEqual(result["failed_tests"][0]["exception_class"], "AssertionError")
        self.assertEqual(result["failed_tests"][0]["exception_message"], {
            "kind": "expected_actual_mismatch",
            "terms": ["completed", "expected", "failed", "got", "job"],
            "shape": ["expected", "completed", "but", "got", "failed", "for", "job", "<word>"],
            "shape_truncated": False,
        })
        encoded = json.dumps(result)
        self.assertNotIn("SENSITIVE_ASSERTION_VALUE", encoded)
        self.assertNotIn("private.py", encoded)

    def test_regex_mismatch_exposes_only_bounded_marker_groups(self) -> None:
        output = (
            "private_test_identity (private.Case.test) ... FAIL\n"
            "FAIL: private_test_identity (private.Case.test)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 7, in test\n"
            "AssertionError: Regex didn't match: "
            "'required next_phase durable_import PRIVATE_EXPECTED_SENTINEL' "
            "not found in 'job_state_catalog_store_repair: completed "
            "PRIVATE_OBSERVED_SENTINEL'\n"
            "Ran 1 test\nFAILED (failures=1)\n"
        )

        result = diagnostics.summarize_unittest_output(output, exit_code=1)
        message = result["failed_tests"][0]["exception_message"]
        signature = message["pattern_signature"]
        self.assertEqual(signature["state"], "classified")
        self.assertEqual(signature["matcher_kind"], "regex_not_found")
        self.assertIn("continuation_boundary", signature["expected_marker_groups"])
        self.assertIn("job_lifecycle", signature["observed_marker_groups"])
        self.assertIn("repair_sequence", signature["observed_marker_groups"])
        self.assertTrue(signature["expected_length"] > 0)
        self.assertTrue(signature["observed_length"] > 0)
        self.assertFalse(signature["expected_text_retained"])
        self.assertFalse(signature["observed_text_retained"])
        encoded = json.dumps(signature)
        for private_value in (
            "private_test_identity", "private_verifier.py", "PRIVATE_EXPECTED_SENTINEL",
            "PRIVATE_OBSERVED_SENTINEL", "durable_import", "job_state_catalog_store_repair",
        ):
            self.assertNotIn(private_value, encoded)

    def test_malformed_regex_mismatch_is_unknown_and_body_free(self) -> None:
        signature = diagnostics._summarize_regex_mismatch(
            "Regex didn't match: malformed PRIVATE_PATTERN not found in malformed PRIVATE_TEXT"
        )

        self.assertEqual(signature["state"], "unclassified")
        self.assertEqual(signature["matcher_kind"], "regex_not_found")
        self.assertEqual(signature["expected_marker_groups"], [])
        self.assertEqual(signature["observed_marker_groups"], [])
        self.assertNotIn("PRIVATE_PATTERN", json.dumps(signature))
        self.assertNotIn("PRIVATE_TEXT", json.dumps(signature))

    def test_job_tool_sequence_aliases_exact_ids_and_redacts_unbound_calls(self) -> None:
        first_id = "11111111-1111-4111-8111-111111111111"
        second_id = "22222222-2222-4222-8222-222222222222"
        result = diagnostics.summarize_job_tool_sequence([
            {"index": 4, "tool": "job", "status": "completed",
             "started_elapsed_ms": 40, "finished_elapsed_ms": 50,
             "job": {"job_id": first_id, "action": "status", "status": "running"}},
            {"index": 1, "tool": "test", "status": "completed",
             "started_elapsed_ms": 10, "finished_elapsed_ms": 20,
             "job": {"job_id": first_id, "action": "submit", "status": "queued"}},
            {"index": 6, "tool": "job", "status": "failed",
             "job": {"job_id": second_id, "action": "wait", "status": "failed"}},
            {"index": 8, "tool": "test", "status": "unknown",
             "job": {"job_id": "PRIVATE_MALFORMED_JOB_ID", "action": "submit"}},
        ])

        self.assertEqual(result["schema"], "h007_job_tool_sequence_v1")
        self.assertEqual(result["job_tool_call_count"], 4)
        self.assertEqual(result["unbound_job_tool_call_count"], 1)
        self.assertEqual([event["index"] for event in result["events"]], [1, 4, 6, 8])
        self.assertEqual([event["job_alias"] for event in result["events"]],
                         ["J01", "J01", "J02", None])
        self.assertEqual(result["events"][1]["job_status"], "running")
        self.assertEqual(result["events"][2]["call_status"], "failed")
        encoded = json.dumps(result)
        for private_id in (first_id, second_id, "PRIVATE_MALFORMED_JOB_ID"):
            self.assertNotIn(private_id, encoded)
        self.assertFalse(result["raw_job_ids_retained"])

    def test_failed_submit_error_classes_are_bounded_and_content_free(self) -> None:
        result = diagnostics.summarize_job_tool_sequence([
            {"index": 1, "tool": "test", "status": "failed",
             "tool_error_class": "tool_execution_failed",
             "job": {"job_id": None, "action": "submit", "status": None}},
            {"index": 2, "tool": "test", "status": "failed",
             "tool_error_class": "PRIVATE_ERROR_CLASS",
             "job": {"job_id": None, "action": "submit", "status": None}},
        ])
        self.assertEqual(result["failed_test_submit_error_class_counts"], {
            "tool_execution_failed": 1, "unobserved": 1,
        })
        self.assertEqual([event["tool_error_class"] for event in result["events"]], [
            "tool_execution_failed", "unobserved",
        ])
        encoded = json.dumps(result)
        self.assertNotIn("PRIVATE_ERROR_CLASS", encoded)
        self.assertNotIn("tool execution failed:", encoded)

    def test_ordered_tool_activity_proves_or_rejects_productive_overlap_without_bodies(self) -> None:
        first_id = "11111111-1111-4111-8111-111111111111"
        second_id = "22222222-2222-4222-8222-222222222222"
        result = diagnostics.summarize_ordered_tool_activity([
            {"index": 1, "tool": "test", "status": "completed", "failed": False,
             "started_elapsed_ms": 10, "finished_elapsed_ms": 20,
             "arguments": {"path": "/PRIVATE_TOOL_PATH"},
             "rawOutput": "PRIVATE_TOOL_OUTPUT",
             "job": {"job_id": first_id, "action": "submit", "status": "queued"}},
            {"index": 2, "tool": "job", "status": "completed", "failed": False,
             "job": {"job_id": first_id, "action": "status", "status": "running"}},
            {"index": 3, "tool": "edit", "status": "completed", "failed": False,
             "arguments": {"path": "/PRIVATE_EDIT_PATH"}},
            {"index": 4, "tool": "job", "status": "completed", "failed": False,
             "job": {"job_id": first_id, "action": "status", "status": "completed"}},
            {"index": 5, "tool": "test", "status": "completed", "failed": False,
             "job": {"job_id": second_id, "action": "submit", "status": "queued"}},
            {"index": 6, "tool": "job", "status": "completed", "failed": False,
             "job": {"job_id": second_id, "action": "status", "status": "running"}},
            {"index": 7, "tool": "job", "status": "completed", "failed": False,
             "job": {"job_id": second_id, "action": "status", "status": "completed"}},
            {"index": 8, "tool": "task", "status": "completed", "failed": False,
             "task_action": "spawn", "task_id": "PRIVATE_TASK_ID",
             "task_result": "PRIVATE_TASK_RESULT"},
        ])

        self.assertEqual(result["schema"], "h007_ordered_tool_activity_v1")
        self.assertEqual(result["tool_call_count"], 8)
        self.assertEqual(result["productive_overlap_interval_count"], 2)
        self.assertEqual(result["zero_productive_overlap_interval_count"], 1)
        self.assertEqual(result["productive_overlap_intervals"], [
            {"job_alias": "J01", "running_call_index": 2,
             "completed_call_index": 4, "productive_call_count_between": 1,
             "productive_overlap": True},
            {"job_alias": "J02", "running_call_index": 6,
             "completed_call_index": 7, "productive_call_count_between": 0,
             "productive_overlap": False},
        ])
        encoded = json.dumps(result)
        for private_value in (
            first_id, second_id, "/PRIVATE_TOOL_PATH", "/PRIVATE_EDIT_PATH",
            "PRIVATE_TOOL_OUTPUT", "PRIVATE_TASK_ID", "PRIVATE_TASK_RESULT",
        ):
            self.assertNotIn(private_value, encoded)
        self.assertFalse(result["raw_job_ids_retained"])
        self.assertFalse(result["tool_arguments_retained"])
        self.assertFalse(result["tool_outputs_retained"])
        self.assertFalse(result["task_ids_retained"])

    def test_open_tool_calls_at_boundary_are_redacted_and_do_not_claim_process_state(self) -> None:
        result = diagnostics.summarize_open_tool_calls_at_boundary([
            {"index": 4, "tool": "bash", "status": "unknown",
             "started_elapsed_ms": 2_500_000,
             "arguments": {"command": "PRIVATE_COMMAND"},
             "rawOutput": "PRIVATE_OUTPUT", "path": "/PRIVATE_PATH"},
            {"index": 5, "tool": "test", "status": "completed",
             "started_elapsed_ms": 2_600_000, "finished_elapsed_ms": 2_700_000},
        ], boundary_elapsed_ms=3_300_000, terminal="completed")

        self.assertEqual(result["schema"], "h007_open_tool_calls_at_boundary_v1")
        self.assertEqual(result["run_terminal"], "completed")
        self.assertEqual(result["unpaired_call_count"], 1)
        self.assertEqual(result["unpaired_calls"], [{
            "index": 4, "tool_class": "bash", "action": "invoke",
            "started_elapsed_ms": 2_500_000, "age_at_boundary_ms": 800_000,
            "stream_state": "start_without_terminal",
        }])
        self.assertFalse(result["underlying_process_state_inferred"])
        encoded = json.dumps(result)
        for private_value in ("PRIVATE_COMMAND", "PRIVATE_OUTPUT", "/PRIVATE_PATH"):
            self.assertNotIn(private_value, encoded)

    def test_open_tool_boundary_preserves_timeout_terminal_class(self) -> None:
        result = diagnostics.summarize_open_tool_calls_at_boundary(
            [{"index": 1, "tool": "read", "status": "completed",
              "started_elapsed_ms": 10, "finished_elapsed_ms": 20}],
            boundary_elapsed_ms=30, terminal="timeout",
        )

        self.assertEqual(result["run_terminal"], "timeout")
        self.assertEqual(result["unpaired_call_count"], 0)

    def test_assertion_operation_counts_are_coarse_and_body_free(self) -> None:
        output = (
            "private_test_identifier_one (private.Case.one) ... FAIL\n"
            "FAIL: private_test_identifier_one (private.Case.one)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 17, in one\n"
            "    self.assertEqual(PRIVATE_LEFT, PRIVATE_RIGHT)\n"
            "AssertionError: PRIVATE_ASSERTION_TEXT\n"
            "private_test_identifier_two (private.Case.two) ... FAIL\n"
            "FAIL: private_test_identifier_two (private.Case.two)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 28, in two\n"
            "    self.assertIn(PRIVATE_ITEM, PRIVATE_CONTAINER)\n"
            "AssertionError: PRIVATE_ASSERTION_TEXT\n"
            "Ran 2 tests\nFAILED (failures=2)\n"
        )

        result = diagnostics.summarize_unittest_output(output, exit_code=1)
        operation_counts = result["assertion_operation_counts"]

        self.assertEqual(operation_counts, {"equality": 1, "membership": 1})
        self.assertEqual(sum(operation_counts.values()), 2)
        projected = json.dumps(operation_counts, sort_keys=True)
        for private_value in (
                "private_test_identifier", "private_verifier.py", "PRIVATE_LEFT",
                "PRIVATE_RIGHT", "PRIVATE_ITEM", "PRIVATE_CONTAINER",
                "PRIVATE_ASSERTION_TEXT", "assertEqual", "assertIn"):
            self.assertNotIn(private_value, projected)

    def test_assertion_operation_count_marks_unavailable_trace_as_unclassified(self) -> None:
        result = diagnostics.summarize_unittest_output(
            "private_test_identifier (private.Case.test) ... FAIL\n"
            "FAIL: private_test_identifier (private.Case.test)\n"
            "AssertionError: \nRan 1 test\nFAILED (failures=1)\n",
            exit_code=1,
        )

        self.assertEqual(result["assertion_operation_counts"], {"unclassified": 1})
        self.assertEqual(result["failure_domain_details"]["task_behavior"], {
            "failure_count": 1,
            "failure_kinds": {"assertion_failure": 1},
            "exception_classes": {"AssertionError": 1},
            "assertion_operations": {"unclassified": 1},
            "message_categories": {"assertion_message": 1},
            "safe_term_counts": {},
        })

    def test_failure_domains_are_safe_for_continuation_and_transcript_checks(self) -> None:
        output = (
            "test_durable_continuation_records_completed_phases_and_next_boundary "
            "(test_catalog.CatalogContract.test_durable_continuation_records_completed_phases_and_next_boundary) ... FAIL\n"
            "test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation "
            "(test_catalog.CatalogContract.test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation) ... FAIL\n"
            "FAIL: test_durable_continuation_records_completed_phases_and_next_boundary "
            "(test_catalog.CatalogContract.test_durable_continuation_records_completed_phases_and_next_boundary)\n"
            "AssertionError: private continuation assertion text\n"
            "FAIL: test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation "
            "(test_catalog.CatalogContract.test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation)\n"
            "AssertionError: private transcript assertion text\n"
            "Ran 5 tests\nFAILED (failures=2)\n"
        )

        result = diagnostics.summarize_unittest_output(output, exit_code=1)

        self.assertEqual(result["failure_domain_counts"], {
            "continuation_boundary": 1,
            "transcript_job_lifecycle": 1,
        })
        encoded = json.dumps(result)
        self.assertNotIn("private continuation assertion text", encoded)
        self.assertNotIn("private transcript assertion text", encoded)

    def test_failure_domain_details_aggregate_only_allowlisted_categories(self) -> None:
        output = (
            "private_continuation_boundary_case (private.Case.private_continuation_boundary_case) ... FAIL\n"
            "private_transcript_job_lifecycle_case (private.Case.private_transcript_job_lifecycle_case) ... FAIL\n"
            "private_task_case (private.Case.private_task_case) ... ERROR\n"
            "FAIL: private_continuation_boundary_case (private.Case.private_continuation_boundary_case)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 17, in check\n"
            "    self.assertEqual(PRIVATE_LEFT, PRIVATE_RIGHT)\n"
            "AssertionError: expected completed but got failed for job SECRET_ID\n"
            "FAIL: private_transcript_job_lifecycle_case (private.Case.private_transcript_job_lifecycle_case)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 28, in check\n"
            "    self.assertIn(PRIVATE_ITEM, PRIVATE_CONTAINER)\n"
            "AssertionError: missing running job SECRET_ID\n"
            "ERROR: private_task_case (private.Case.private_task_case)\n"
            "Traceback (most recent call last):\n"
            "  File \"private_verifier.py\", line 39, in check\n"
            "    self.assertTrue(PRIVATE_VALUE)\n"
            "TypeError: PRIVATE_TYPE_DETAIL\n"
            "Ran 3 tests\nFAILED (failures=2, errors=1)\n"
        )

        result = diagnostics.summarize_unittest_output(output, exit_code=1)
        details = result["failure_domain_details"]

        self.assertEqual(details["continuation_boundary"], {
            "failure_count": 1,
            "failure_kinds": {"assertion_failure": 1},
            "exception_classes": {"AssertionError": 1},
            "assertion_operations": {"equality": 1},
            "message_categories": {"expected_actual_mismatch": 1},
            "safe_term_counts": {
                "completed": 1, "expected": 1, "failed": 1, "got": 1, "job": 1,
            },
        })
        self.assertEqual(details["transcript_job_lifecycle"]["assertion_operations"], {
            "membership": 1,
        })
        self.assertEqual(details["task_behavior"]["exception_classes"], {"TypeError": 1})
        self.assertEqual(details["task_behavior"]["message_categories"], {
            "exception_message": 1,
        })
        encoded = json.dumps(details, sort_keys=True)
        for private_value in (
                "private_continuation_boundary_case", "private_transcript_job_lifecycle_case",
                "private_task_case",
                "private_verifier.py", "PRIVATE_LEFT", "PRIVATE_RIGHT", "PRIVATE_ITEM",
                "PRIVATE_CONTAINER", "PRIVATE_VALUE", "PRIVATE_TYPE_DETAIL", "SECRET_ID",
                "assertEqual", "assertIn", "assertTrue", "missing running job"):
            self.assertNotIn(private_value, encoded)

    def test_value_error_reason_is_classified_without_retaining_literal(self) -> None:
        result = diagnostics.summarize_unittest_output(
            "test_phase (test_catalog.CatalogTests.test_phase) ... ERROR\n"
            "ERROR: test_phase (test_catalog.CatalogTests.test_phase)\n"
            "ValueError: invalid literal for int() with base 10: 'SENSITIVE_PHASE_VALUE'\n"
            "Ran 1 test\nFAILED (errors=1)\n",
            exit_code=1,
        )

        self.assertEqual(result["state"], "test_errors")
        self.assertEqual(result["failed_tests"][0]["exception_class"], "ValueError")
        self.assertEqual(result["failed_tests"][0]["exception_message"], {
            "kind": "invalid_integer_literal", "terms": ["invalid"],
            "shape": ["invalid", "literal", "for", "int", "(", ")", "with", "base",
                      "<number>", ":", "'", "<word>", "'"],
            "shape_truncated": False,
        })
        encoded = json.dumps(result)
        self.assertNotIn("SENSITIVE_PHASE_VALUE", encoded)
        self.assertNotIn("base 10", encoded)

    def test_setup_error_is_classified_without_exception_message(self) -> None:
        output = (
            "Traceback (most recent call last):\n"
            "  File \"private_path.py\", line 1, in <module>\n"
            "ModuleNotFoundError: PRIVATE_MODULE_NAME\n"
        )
        result = diagnostics.summarize_unittest_output(output, exit_code=1)

        self.assertEqual(result["state"], "setup_error")
        self.assertEqual(result["setup_exception_class"], "ModuleNotFoundError")
        self.assertNotIn("PRIVATE_MODULE_NAME", json.dumps(result))
        self.assertNotIn("private_path.py", json.dumps(result))

    def test_unittest_setup_warning_is_not_misreported_as_exception_class(self) -> None:
        output = (
            "setUpClass (test_catalog_contract.CatalogContract) ... ERROR\n"
            "warning: failed, waiting for file lock on BUILD_PATH_SECRET\n"
            "Ran 0 tests in 0.01s\nFAILED (errors=1)\n"
        )
        result = diagnostics.summarize_unittest_output(output, exit_code=1)

        self.assertEqual(result["state"], "setup_error")
        self.assertIsNone(result["failed_tests"][0].get("exception_class"))
        self.assertEqual(result["setup_failure_reason"], "build_lock_wait")
        encoded = json.dumps(result)
        self.assertNotIn("BUILD_PATH_SECRET", encoded)
        self.assertNotIn("waiting for file lock", encoded)

    def test_timeout_and_malformed_output_are_distinguished(self) -> None:
        timed_out = diagnostics.summarize_unittest_output(
            "test_partial (test_case.Case.test_partial) ...",
            exit_code=None,
            timed_out=True,
        )
        malformed = diagnostics.summarize_unittest_output(
            "not a unittest report", exit_code=3)

        self.assertEqual(timed_out["state"], "timeout")
        self.assertEqual(malformed["state"], "unclassified_failure")
        self.assertFalse(malformed["diagnostic_detail_complete"])
        self.assertFalse(malformed["traceback_retained"])

    def test_only_complete_nonempty_test_runs_count_as_verifier_execution(self) -> None:
        setup_timeout = diagnostics.summarize_unittest_output(
            "setUpClass (test_catalog.Catalog) ... ERROR\n"
            "TimeoutError: setup timed out\nRan 0 tests\nFAILED (errors=1)\n",
            exit_code=1,
        )
        actual_failure = diagnostics.summarize_unittest_output(
            "test_catalog (test_catalog.Catalog.test_catalog) ... FAIL\n"
            "Ran 1 test\nFAILED (failures=1)\n",
            exit_code=1,
        )

        self.assertEqual(setup_timeout["state"], "setup_error")
        self.assertFalse(diagnostics.verifier_tests_observed(setup_timeout))
        self.assertTrue(diagnostics.verifier_tests_observed(actual_failure))

    def test_large_output_and_test_names_are_bounded_and_sanitized(self) -> None:
        test_id = "test_" + "a" * 600 + "\x1b[31m (module.Case.test)"
        output = (f"{test_id} ... FAIL\nFAIL: {test_id}\n"
                  "AssertionError: private detail\nRan 1 test\nFAILED (failures=1)\n"
                  + ("x" * 2_000_001))
        result = diagnostics.summarize_unittest_output(output, exit_code=1)

        self.assertTrue(result["input_truncated"])
        self.assertFalse(result["diagnostic_detail_complete"])
        self.assertLessEqual(result["verbose_tests_observed"], diagnostics.MAX_DIAGNOSTIC_TESTS)
        self.assertLessEqual(len(result["failed_tests"][0]["test_id"]),
                             diagnostics.MAX_TEST_ID_CHARS)
        self.assertNotIn("\x1b", json.dumps(result))
        self.assertFalse(result["raw_output_retained"])

    def test_task_failure_projection_accepts_only_canonical_class(self) -> None:
        self.assertEqual(
            diagnostics.extract_task_failure_class(
                "Run: opaque\nStatus: failed\n"
                "Failure stage/class: provider_transient\n"
                "Failure detail: do not keep this text"),
            "provider_transient",
        )
        self.assertIsNone(diagnostics.extract_task_failure_class(
            "Failure stage/class: secret text"))

    def test_verifier_runner_does_not_persist_raw_output(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            script = pathlib.Path(temp) / "verifier.py"
            script.write_text(
                "print('test_safe (sample.Case.test_safe) ... FAIL')\n"
                "print('FAIL: test_safe (sample.Case.test_safe)')\n"
                "print('AssertionError: SENSITIVE_TEST_OUTPUT')\n"
                "print('Ran 1 test')\nprint('FAILED (failures=1)')\n"
                "raise SystemExit(1)\n",
                encoding="utf-8",
            )
            result = autonomy_run._run_verifier(
                [sys.executable, str(script)],
                pathlib.Path(temp), {}, 10, diagnostics_only=True,
            )

        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["diagnostic"]["state"], "assertion_failures")
        self.assertNotIn("output_tail", result)
        self.assertNotIn("SENSITIVE_TEST_OUTPUT", json.dumps(result))


class ProviderAttemptDiagnosticTests(unittest.TestCase):
    def test_first_response_timeout_and_retry_deadline_are_summarized_safely(self) -> None:
        log = (
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_started '
            'attempt_id="ATTEMPT_PRIVATE" provider_span_id=19\n'
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_completed '
            'attempt_id="ATTEMPT_PRIVATE" setup_elapsed_ms=41\n'
            'WARN codegg::agent::provider_turn: provider_stream_idle_timeout '
            'attempt_id="ATTEMPT_PRIVATE" decoded_event_count=0 '
            'elapsed_since_setup_ms=241750 since_last_decoded_event_ms=240001 '
            'idle_wait_ms=240001\n'
            'WARN codegg::agent::provider_turn: provider retry chain deadline reached; stopping '
            'session_id="PRIVATE_SESSION" attempt_id="ATTEMPT_PRIVATE" '
            'attempt_index=0 error_class=timeout attempts_left=5 remaining_ms=0\n'
            'DEBUG other_target: unrelated request_body="PRIVATE_PROVIDER_BODY"\n'
        )
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "codegg_debug.log"
            path.write_text(log, encoding="utf-8")
            result = diagnostics.summarize_provider_attempt_log(path, run_id="safe-run")

        self.assertEqual(result["state"], "observed")
        self.assertEqual(result["provider_attempts_started"], 1)
        self.assertEqual(result["provider_attempts_succeeded"], 0)
        self.assertEqual(result["provider_attempts_failed"], 1)
        self.assertEqual(result["unresolved_provider_attempt_count"], 0)
        self.assertTrue(result["attempt_accounting_consistent"])
        self.assertEqual(result["stream_idle_timeout_count"], 1)
        self.assertEqual(result["first_response_event_timeout_count"], 1)
        self.assertEqual(result["retry_chain_deadline_count"], 1)
        failure = result["last_failure"]
        self.assertEqual(failure["attempt_index"], 0)
        self.assertEqual(failure["error_class"], "timeout")
        self.assertEqual(failure["attempts_left"], 5)
        self.assertEqual(failure["remaining_ms"], 0)
        self.assertEqual(failure["failure_phase"], "first_response_event")
        self.assertEqual(failure["retry_outcome"], "retry_chain_deadline")
        self.assertEqual(failure["decoded_event_count"], 0)
        self.assertEqual(failure["idle_wait_ms"], 240001)
        encoded = json.dumps(result)
        self.assertNotIn("ATTEMPT_PRIVATE", encoded)
        self.assertNotIn("PRIVATE_SESSION", encoded)
        self.assertNotIn("PRIVATE_PROVIDER_BODY", encoded)
        self.assertFalse(result["raw_log_in_receipt"])
        self.assertFalse(result["raw_provider_text_in_receipt"])
        self.assertFalse(result["attempt_ids_retained"])

    def test_retry_then_success_counts_attempts_without_raw_ids(self) -> None:
        log = (
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_started '
            'attempt_id="FIRST_PRIVATE"\n'
            'INFO codegg::agent::provider_turn: provider attempt failed before visible output; retrying '
            'attempt_id="FIRST_PRIVATE" attempt_index=0 error_class=timeout '
            'delay_ms=10 attempts_left=1\n'
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_started '
            'attempt_id="SECOND_PRIVATE"\n'
            'DEBUG codegg::agent::provider_turn: provider attempt succeeded '
            'attempt_id="SECOND_PRIVATE" attempt_index=1\n'
        )
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "codegg_debug.log"
            path.write_text(log, encoding="utf-8")
            result = diagnostics.summarize_provider_attempt_log(path, run_id="safe-run")

        self.assertEqual(result["provider_attempts_started"], 2)
        self.assertEqual(result["provider_attempts_succeeded"], 1)
        self.assertEqual(result["provider_attempts_failed"], 1)
        self.assertEqual(result["retry_scheduled_count"], 1)
        self.assertEqual(result["unresolved_provider_attempt_count"], 0)
        self.assertTrue(result["attempt_accounting_consistent"])
        encoded = json.dumps(result)
        self.assertNotIn("FIRST_PRIVATE", encoded)
        self.assertNotIn("SECOND_PRIVATE", encoded)

    def test_unfinished_provider_attempt_is_counted_without_exposing_its_identity(self) -> None:
        log = (
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_started '
            'attempt_id="INCOMPLETE_PRIVATE"\n'
            'DEBUG codegg::agent::provider_turn: provider_stream_setup_completed '
            'attempt_id="INCOMPLETE_PRIVATE" setup_elapsed_ms=17\n'
        )
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "codegg_debug.log"
            path.write_text(log, encoding="utf-8")
            result = diagnostics.summarize_provider_attempt_log(path, run_id="safe-run")

        self.assertEqual(result["provider_attempts_started"], 1)
        self.assertEqual(result["provider_attempts_succeeded"], 0)
        self.assertEqual(result["provider_attempts_failed"], 0)
        self.assertEqual(result["unresolved_provider_attempt_count"], 1)
        self.assertTrue(result["attempt_accounting_consistent"])
        self.assertNotIn("INCOMPLETE_PRIVATE", json.dumps(result))

    def test_missing_or_oversized_provider_log_is_typed_and_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            missing = diagnostics.summarize_provider_attempt_log(
                pathlib.Path(temp) / "missing.log", run_id="safe-run")
            path = pathlib.Path(temp) / "large.log"
            path.write_bytes(b"x" * (diagnostics.MAX_PROVIDER_LOG_BYTES + 1))
            oversized = diagnostics.summarize_provider_attempt_log(path, run_id="safe-run")

        self.assertEqual(missing["state"], "missing")
        self.assertEqual(oversized["state"], "too_large")
        self.assertTrue(oversized["input_truncated"])
        self.assertEqual(oversized["recognized_events"], 0)
        self.assertIsNone(oversized["unresolved_provider_attempt_count"])
        self.assertIsNone(oversized["attempt_accounting_consistent"])


class ReviewerLifecycleDiagnosticTests(unittest.TestCase):
    def test_transient_deadline_can_continue_same_reviewer_run(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        calls = [
            {"index": 1, "tool": "task", "task_action": "spawn",
             "task_spawn_agent": "verifier",
             "task_spawn_task_id_sha256": task_id,
             "task_spawn_run_id_sha256": run_id,
             "task_spawn_run_status": "running", "status": "completed"},
            {"index": 2, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id,
             "task_run_status": "failed",
             "task_failure_class": "deadline_exceeded", "status": "completed"},
            {"index": 3, "tool": "task", "task_action": "continue",
             "task_run_id_sha256": run_id, "status": "completed"},
            {"index": 4, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id,
             "task_run_status": "completed", "status": "completed"},
            {"index": 5, "tool": "task", "task_action": "get",
             "task_id_sha256": task_id,
             "task_result_task_id_sha256": task_id,
             "task_result_run_id_sha256": run_id,
             "task_result_run_status": "completed", "status": "completed"},
        ]

        result = diagnostics.summarize_reviewer_lifecycle(calls)

        self.assertEqual(result["task_action_counts"]["continue"], 1)
        self.assertEqual(result["reviewer_continue_count"], 1)
        self.assertEqual(result["continue_after_terminal_failure_count"], 1)
        self.assertTrue(result["continue_after_terminal_failure_observed"])
        self.assertEqual(result["ordered_events"][2]["action"], "continue")
        self.assertEqual(result["status_sequence"][0]["failure_class"],
                         "deadline_exceeded")
        self.assertEqual(result["final_status"], "completed")
        self.assertIsNone(result["final_failure_class"])
        self.assertTrue(result["completed_review_result_observed"])
        self.assertTrue(result["compliant_status_then_get_lifecycle"])
        self.assertFalse(result["task_transcript_retained"])
        self.assertFalse(result["task_result_body_retained"])
        self.assertNotIn("run_id", result)

    def test_unrelated_continue_does_not_count_for_failed_reviewer(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        calls = [
            {"index": 1, "tool": "task", "task_action": "spawn",
             "task_spawn_agent": "verifier",
             "task_spawn_task_id_sha256": task_id,
             "task_spawn_run_id_sha256": run_id,
             "task_spawn_run_status": "running", "status": "completed"},
            {"index": 2, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id, "task_run_status": "failed",
             "status": "completed"},
            {"index": 3, "tool": "task", "task_action": "continue",
             "task_run_id_sha256": "c" * 64, "status": "completed"},
        ]

        result = diagnostics.summarize_reviewer_lifecycle(calls)

        self.assertEqual(result["reviewer_continue_count"], 0)
        self.assertEqual(result["continue_after_terminal_failure_count"], 0)
        self.assertFalse(result["continue_after_terminal_failure_observed"])
        self.assertEqual(result["task_action_counts"]["continue"], 1)

    def test_lifecycle_matches_exact_spawned_identity_and_terminal_failure(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        calls = [
            {"index": 1, "tool": "task", "task_action": "spawn",
             "task_spawn_agent": "verifier",
             "task_spawn_task_id_sha256": task_id,
             "task_spawn_run_id_sha256": run_id,
             "task_spawn_run_status": "running", "status": "completed"},
            {"index": 2, "tool": "task", "task_action": "get",
             "task_id_sha256": task_id,
             "task_result_task_id_sha256": task_id,
             "task_result_run_id_sha256": run_id,
             "task_result_run_status": "running", "status": "completed"},
            {"index": 3, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id,
             "task_run_status": "failed",
             "task_failure_class": "provider_transient", "status": "completed"},
            {"index": 4, "tool": "task", "task_action": "get",
             "task_id_sha256": task_id,
             "task_result_task_id_sha256": task_id,
             "task_result_run_id_sha256": run_id,
             "task_result_run_status": "failed", "status": "completed"},
            {"index": 5, "tool": "task", "task_action": "get",
             "task_id_sha256": "c" * 64,
             "task_result_task_id_sha256": "c" * 64,
             "task_result_run_id_sha256": "d" * 64,
             "task_result_run_status": "completed", "status": "completed"},
        ]
        result = diagnostics.summarize_reviewer_lifecycle(calls)

        self.assertEqual(result["reviewer_spawn_count"], 1)
        self.assertEqual(result["exact_identity_matched_call_count"], 4)
        self.assertEqual(result["get_count"], 2)
        self.assertEqual(result["get_nonterminal_count"], 1)
        self.assertEqual(result["completed_get_result_observed_count"], 0)
        self.assertEqual(result["ordered_completed_get_result_count"], 0)
        self.assertEqual(result["duplicate_get_count"], 1)
        self.assertEqual(result["explicit_terminal_observation_count"], 1)
        self.assertEqual(result["get_after_terminal_status_count"], 1)
        self.assertEqual(result["final_status"], "failed")
        self.assertEqual(result["final_failure_class"], "provider_transient")
        self.assertFalse(result["completed_review_result_observed"])
        self.assertFalse(result["compliant_status_then_get_lifecycle"])
        self.assertFalse(result["task_result_body_retained"])

    def test_completed_get_before_status_is_diagnostic_not_compliant_lifecycle(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        calls = [
            {"index": 1, "tool": "task", "task_action": "spawn",
             "task_spawn_agent": "verifier",
             "task_spawn_task_id_sha256": task_id,
             "task_spawn_run_id_sha256": run_id,
             "task_spawn_run_status": "running", "status": "completed"},
            {"index": 2, "tool": "task", "task_action": "get",
             "task_id_sha256": task_id,
             "task_result_task_id_sha256": task_id,
             "task_result_run_id_sha256": run_id,
             "task_result_run_status": "completed", "status": "completed"},
            {"index": 3, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id, "task_run_status": "completed",
             "status": "completed"},
        ]

        result = diagnostics.summarize_reviewer_lifecycle(calls)

        self.assertEqual(result["completed_get_result_observed_count"], 1)
        self.assertEqual(result["ordered_completed_get_result_count"], 0)
        self.assertEqual(result["duplicate_get_count"], 0)
        self.assertEqual(result["explicit_terminal_observation_count"], 1)
        self.assertEqual(result["get_after_terminal_status_count"], 0)
        self.assertFalse(result["completed_review_result_observed"])
        self.assertFalse(result["compliant_status_then_get_lifecycle"])

    def test_duplicate_completed_gets_are_observed_but_not_compliant(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        spawn = {"index": 1, "tool": "task", "task_action": "spawn",
                 "task_spawn_agent": "verifier",
                 "task_spawn_task_id_sha256": task_id,
                 "task_spawn_run_id_sha256": run_id,
                 "task_spawn_run_status": "running", "status": "completed"}
        status = {"index": 2, "tool": "task", "task_action": "status",
                  "task_run_id_sha256": run_id, "task_run_status": "completed",
                  "status": "completed"}
        get_template = {"tool": "task", "task_action": "get",
                        "task_id_sha256": task_id,
                        "task_result_task_id_sha256": task_id,
                        "task_result_run_id_sha256": run_id,
                        "task_result_run_status": "completed", "status": "completed"}
        first_get = dict(get_template, index=3)
        second_get = dict(get_template, index=4)

        result = diagnostics.summarize_reviewer_lifecycle(
            [spawn, status, first_get, second_get])

        self.assertEqual(result["get_count"], 2)
        self.assertEqual(result["duplicate_get_count"], 1)
        self.assertEqual(result["ordered_completed_get_result_count"], 2)
        self.assertTrue(result["completed_review_result_observed"])
        self.assertFalse(result["compliant_status_then_get_lifecycle"])

    def test_event_truncation_counts_only_matching_reviewer_events(self) -> None:
        task_id = "a" * 64
        run_id = "b" * 64
        calls = [{"index": 0, "tool": "task", "task_action": "spawn",
                  "task_spawn_agent": "verifier",
                  "task_spawn_task_id_sha256": task_id,
                  "task_spawn_run_id_sha256": run_id,
                  "task_spawn_run_status": "running"}]
        calls.extend(
            {"index": index + 1, "tool": "task", "task_action": "status",
             "task_run_id_sha256": run_id, "task_run_status": "running"}
            for index in range(diagnostics.MAX_REVIEW_EVENTS)
        )

        result = diagnostics.summarize_reviewer_lifecycle(calls)

        self.assertTrue(result["reviewer_events_truncated"])
        self.assertEqual(result["reviewer_event_count"], diagnostics.MAX_REVIEW_EVENTS)
        self.assertEqual(result["exact_identity_matched_call_count"],
                         diagnostics.MAX_REVIEW_EVENTS + 1)


class ReviewerDurableStateDiagnosticTests(unittest.TestCase):
    def test_projects_terminal_deadline_without_ids_or_failure_body(self) -> None:
        task_id = "reviewer-task-private"
        run_id = "reviewer-run-private"
        with tempfile.TemporaryDirectory() as temporary:
            database = pathlib.Path(temporary) / "codegg.db"
            with sqlite3.connect(database) as connection:
                connection.executescript(
                    "CREATE TABLE agent_run (run_id TEXT, task_id TEXT, status TEXT, "
                    "terminal TEXT, failure_class TEXT, failure_message TEXT, "
                    "started_at INTEGER, finished_at INTEGER);"
                    "CREATE TABLE agent_task (task_id TEXT, status TEXT);"
                )
                connection.execute(
                    "INSERT INTO agent_run VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    (run_id, task_id, "failed", "failed", "deadline_exceeded",
                     "private failure detail must not escape", 1_000, 1_201_000),
                )
                connection.execute(
                    "INSERT INTO agent_task VALUES (?, ?)", (task_id, "failed"),
                )

            result = diagnostics.summarize_reviewer_durable_state(
                database,
                reviewer_task_id_sha256=hashlib.sha256(task_id.encode()).hexdigest(),
                reviewer_run_id_sha256=hashlib.sha256(run_id.encode()).hexdigest(),
            )
            mismatched_identity = diagnostics.summarize_reviewer_durable_state(
                database,
                reviewer_task_id_sha256=hashlib.sha256(b"different-task").hexdigest(),
                reviewer_run_id_sha256=hashlib.sha256(run_id.encode()).hexdigest(),
            )

        self.assertEqual(result["state"], "observed")
        self.assertEqual(result["run_status"], "failed")
        self.assertEqual(result["run_terminal"], "failed")
        self.assertEqual(result["task_status"], "failed")
        self.assertEqual(result["failure_class"], "deadline_exceeded")
        self.assertEqual(result["elapsed_ms"], 1_200_000)
        self.assertFalse(result["raw_identifiers_retained"])
        self.assertFalse(result["failure_message_retained"])
        self.assertFalse(result["task_result_body_retained"])
        self.assertFalse(result["transcript_retained"])
        serialized = json.dumps(result)
        self.assertNotIn(task_id, serialized)
        self.assertNotIn(run_id, serialized)
        self.assertNotIn("private failure detail", serialized)
        self.assertEqual(mismatched_identity["state"], "identity_mismatch")

    def test_missing_identity_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            result = diagnostics.summarize_reviewer_durable_state(
                pathlib.Path(temporary) / "missing.db",
                reviewer_task_id_sha256=None,
                reviewer_run_id_sha256=None,
            )

        self.assertEqual(result["state"], "reviewer_identity_unavailable")
        self.assertIsNone(result["run_status"])
        self.assertFalse(result["raw_identifiers_retained"])

    def test_missing_database_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            result = diagnostics.summarize_reviewer_durable_state(
                pathlib.Path(temporary) / "missing.db",
                reviewer_task_id_sha256=hashlib.sha256(b"task").hexdigest(),
                reviewer_run_id_sha256=hashlib.sha256(b"run").hexdigest(),
            )

        self.assertEqual(result["state"], "database_missing")
        self.assertIsNone(result["run_status"])
        self.assertFalse(result["raw_identifiers_retained"])


if __name__ == "__main__":
    unittest.main()
