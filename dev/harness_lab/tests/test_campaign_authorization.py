"""Regression tests for live campaign authorization accounting."""

from __future__ import annotations

import copy
import json
import unittest

from dev.harness_lab.campaign_authorization import (
    CampaignLedgerError,
    preflight_campaign_run_start,
    reconcile_campaign_run_start,
    safe_campaign_run_fields,
)


GRANT_ID = "test-grant"
MODEL = "opencode_go/muse-spark-1.3-contributor"


def _ledger() -> dict:
    return {
        "runs": [{
            "attempt": 1,
            "start_number": 2,
            "authorization_grant_id": GRANT_ID,
            "supplemental_grant_id": GRANT_ID,
            "model": MODEL,
            "route": "opencode_go",
            "manifest_sha256": "a" * 64,
            "binary_sha256": "b" * 64,
            "counted_as_start": True,
            "provider_requests_observed": 25,
            "status": "completed_unqualified",
        }],
        "unrepresented_counted_starts": 1,
        "total_authorized_starts": 200,
        "supplemental_authorizations": [{
            "grant_id": GRANT_ID,
            "maximum_additional_starts": 40,
            "starts_consumed": 1,
            "starts_remaining": 39,
            "model": "muse-spark-1.3-contributor",
            "route": MODEL,
            "status": "active",
        }],
    }


def _run(**overrides: object) -> dict:
    run = {
        "attempt": 2,
        "start_number": 3,
        "authorization_grant_id": GRANT_ID,
        "supplemental_grant_id": GRANT_ID,
        "model": MODEL,
        "route": "opencode_go",
        "manifest_sha256": "c" * 64,
        "binary_sha256": "d" * 64,
        "provider_request_limit": 1200,
        "wallclock_limit_seconds": 3600,
        "preflight_evidence_path": "plans/evidence/preflight.json",
        "credential": "must-not-be-copied",
    }
    run.update(overrides)
    return run


class CampaignAuthorizationTests(unittest.TestCase):
    def test_preflight_previews_start_without_mutating_campaign_ledger(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)

        result = preflight_campaign_run_start(
            ledger, _run(), expected_model=MODEL, expected_route="opencode_go",
            expected_manifest_sha256="c" * 64, expected_binary_sha256="d" * 64,
            expected_provider_request_limit=1200,
            expected_wallclock_limit_seconds=3600)

        self.assertEqual(result["start_number"], 3)
        self.assertEqual(result["combined_starts_consumed"], 3)
        self.assertEqual(result["grant_starts_consumed"], 2)
        self.assertEqual(ledger, before)

    def test_preflight_missing_canonical_identity_refuses_without_mutation(self) -> None:
        for missing in ("start_number", "manifest_sha256"):
            with self.subTest(missing=missing):
                ledger = _ledger()
                before = copy.deepcopy(ledger)
                run = _run()
                run.pop(missing)

                with self.assertRaises(CampaignLedgerError):
                    preflight_campaign_run_start(
                        ledger, run, expected_model=MODEL,
                        expected_route="opencode_go",
                        expected_manifest_sha256="c" * 64,
                        expected_binary_sha256="d" * 64,
                        expected_provider_request_limit=1200,
                        expected_wallclock_limit_seconds=3600)

                self.assertEqual(ledger, before)

    def test_preflight_rejects_manifest_alias_drift_without_mutation(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)
        run = _run(fixture_manifest_sha256="e" * 64)

        with self.assertRaisesRegex(
                CampaignLedgerError, "fixture manifest digest conflicts"):
            preflight_campaign_run_start(
                ledger, run, expected_model=MODEL, expected_route="opencode_go",
                expected_manifest_sha256="c" * 64,
                expected_binary_sha256="d" * 64,
                expected_provider_request_limit=1200,
                expected_wallclock_limit_seconds=3600)

        self.assertEqual(ledger, before)

    def test_preflight_rejects_nonpositive_caps_without_mutation(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)
        run = _run(provider_request_limit=0, wallclock_limit_seconds=0)

        with self.assertRaises(CampaignLedgerError):
            preflight_campaign_run_start(
                ledger, run, expected_model=MODEL, expected_route="opencode_go",
                expected_manifest_sha256="c" * 64,
                expected_binary_sha256="d" * 64,
                expected_provider_request_limit=0,
                expected_wallclock_limit_seconds=0)

        self.assertEqual(ledger, before)

    def test_preflight_rejects_launch_binding_mismatch_without_mutation(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)

        with self.assertRaisesRegex(CampaignLedgerError, "model does not match"):
            preflight_campaign_run_start(
                ledger, _run(), expected_model="opencode_go/other-model",
                expected_route="opencode_go",
                expected_manifest_sha256="c" * 64,
                expected_binary_sha256="d" * 64,
                expected_provider_request_limit=1200,
                expected_wallclock_limit_seconds=3600)

        self.assertEqual(ledger, before)

    def test_safe_campaign_projection_drops_raw_and_unknown_fields(self) -> None:
        projected = safe_campaign_run_fields({
            "attempt": 2,
            "provider_requests_observed": 3,
            "credential": "secret-value",
            "prompt": "private task text",
            "error": "raw provider message",
        })

        self.assertEqual(projected, {"attempt": 2, "provider_requests_observed": 3})

    def test_safe_campaign_projection_preserves_identity_and_case_h007_progress(self) -> None:
        run = _run(manifest_sha256="c" * 64)
        canonical = _run(manifest_sha256="e" * 64)
        case_receipt = {
            "task_completion_diagnostic": {
                "schema": "h007_task_completion_diagnostic_v2",
                "state": "agent_incomplete",
                "reasons": ["agent_terminal_incomplete", "required_stages_incomplete",
                            "PRIVATE_REASON"],
                "verifier_outcome": "assertion_failures",
                "raw_note_retained": False,
                "raw_job_ids_retained": False,
                "verifier_public_reconciliation": {
                    "schema": "h007_verifier_public_reconciliation_v1",
                    "failure_domain_counts": {"continuation_boundary": 1, "private": 9},
                    "public_domain_states": {"continuation_boundary": "incomplete",
                                             "private": "complete"},
                    "classification": {"continuation_boundary": "public_evidence_incomplete",
                                        "private": "raw assertion text"},
                    "public_checks": {
                        "continuation_boundary": {
                            "document_present": True,
                            "next_stage_none": None,
                            "private_check": "PRIVATE CHECK DETAIL",
                        },
                        "transcript_job_lifecycle": {
                            "test_jobs_terminal": False,
                            "job_tool_calls_bound_to_id": True,
                        },
                        "private_domain": {"private_check": "PRIVATE DOMAIN"},
                    },
                    "failed_job_status_error_class_counts": {
                        "job_not_found": 2, "unobserved": 1,
                        "PRIVATE_ERROR_CLASS": 9, "unclassified": 1_601,
                        "tool_execution_failed": True,
                    },
                    "assertion_text_retained": False,
                    "raw_output_retained": False,
                    "raw_job_ids_retained": False,
                    "transcript_retained": False,
                    "assertion_text": "PRIVATE VERIFIER OUTPUT",
                },
                "verifier_output": "PRIVATE VERIFIER OUTPUT",
            },
            "continuation_diagnostic": {
                "schema": "h007_continuation_diagnostic_v1",
                "present": True,
                "document_valid": True,
                "raw_note_retained": False,
                "raw_job_ids_retained": False,
                "stages": {"versioned_records": "complete", "durable_import": "pending",
                           "private_stage_text": "PRIVATE NOTE"},
                "gates": {"starter_repair_format": "complete", "durable_import": "failed"},
                "required_review_observed": False,
                "release_overlap_confirmed": True,
                "all_public_artifacts_present": False,
                "note_body": "PRIVATE NOTE",
                "tool_arguments": "PRIVATE TOOL ARGUMENTS",
            },
            "prompt": "PRIVATE PROMPT",
            "transcript": "PRIVATE TRANSCRIPT",
        }
        run_before = copy.deepcopy(run)
        receipt_before = copy.deepcopy(case_receipt)

        projected = safe_campaign_run_fields(
            run, canonical_identity=canonical, case_receipt=case_receipt)

        self.assertEqual(projected["manifest_sha256"], "e" * 64)
        self.assertEqual(projected["attempt"], canonical["attempt"])
        diagnostic = projected["h007_completion_diagnostic"]
        self.assertEqual(diagnostic["state"], "observed")
        self.assertEqual(diagnostic["task_completion"]["state"], "agent_incomplete")
        self.assertEqual(diagnostic["task_completion"]["reasons"], [
            "agent_terminal_incomplete", "required_stages_incomplete",
        ])
        self.assertEqual(diagnostic["task_completion"]["verifier_outcome"],
                         "assertion_failures")
        self.assertEqual(diagnostic["continuation"]["stage_complete_count"], 1)
        self.assertEqual(diagnostic["continuation"]["first_missing_stage"], "durable_import")
        self.assertEqual(diagnostic["continuation"]["incomplete_gates"], ["durable_import"])
        self.assertEqual(
            diagnostic["verifier_public_reconciliation"]["failure_domain_counts"],
            {"continuation_boundary": 1},
        )
        self.assertEqual(
            diagnostic["verifier_public_reconciliation"]["public_checks"],
            {
                "continuation_boundary": {
                    "document_present": True,
                    "next_stage_none": None,
                },
                "transcript_job_lifecycle": {
                    "test_jobs_terminal": False,
                    "job_tool_calls_bound_to_id": True,
                },
            },
        )
        self.assertEqual(
            diagnostic["verifier_public_reconciliation"][
                "failed_job_status_error_class_counts"],
            {"job_not_found": 2, "unobserved": 1},
        )
        encoded = json.dumps(projected, sort_keys=True)
        for private_marker in (
            "PRIVATE_REASON", "PRIVATE VERIFIER OUTPUT", "PRIVATE NOTE",
            "PRIVATE TOOL ARGUMENTS", "PRIVATE PROMPT", "PRIVATE TRANSCRIPT",
            "private_stage_text", "PRIVATE CHECK DETAIL", "PRIVATE DOMAIN",
        ):
            self.assertNotIn(private_marker, encoded)
        self.assertEqual(run, run_before)
        self.assertEqual(case_receipt, receipt_before)

    def test_missing_or_unredacted_h007_diagnostics_are_unavailable(self) -> None:
        projected = safe_campaign_run_fields(
            _run(), canonical_identity=_run(), case_receipt={
                "task_completion_diagnostic": {
                    "schema": "h007_task_completion_diagnostic_v2",
                    "state": "verified_complete",
                    "raw_note_retained": True,
                },
                "continuation_diagnostic": {
                    "schema": "h007_continuation_diagnostic_v1",
                    "present": True,
                    "document_valid": True,
                    "raw_note_retained": True,
                },
            })

        diagnostic = projected["h007_completion_diagnostic"]
        self.assertEqual(diagnostic["state"], "unavailable")
        self.assertEqual(diagnostic["task_completion"]["state"], "unavailable")
        self.assertEqual(diagnostic["continuation"]["state"], "unavailable")

    def test_h007_projection_exposes_bounded_timeout_and_failure_counts(self) -> None:
        case_receipt = {
            "task_completion_diagnostic": {
                "schema": "h007_task_completion_diagnostic_v2",
                "state": "agent_incomplete",
                "reasons": ["agent_terminal_incomplete"],
                "verifier_outcome": "assertion_failures",
                "raw_note_retained": False,
                "raw_job_ids_retained": False,
                "verifier_public_reconciliation": {
                    "schema": "h007_verifier_public_reconciliation_v1",
                    "failure_domain_counts": {"transcript_job_lifecycle": 1},
                    "public_domain_states": {
                        "transcript_job_lifecycle": "incomplete",
                    },
                    "classification": {
                        "transcript_job_lifecycle": "public_evidence_incomplete",
                    },
                    "public_checks": {
                        "transcript_job_lifecycle": {
                            "repair_pairs_complete": False,
                            "job_tool_calls_bound_to_id": False,
                            "job_request_result_ids_match": False,
                        },
                    },
                    "unbound_job_tool_call_count": 2,
                    "unbound_call_state_counts": {
                        "status:string:failed": 1,
                        "wait:missing:completed": 1,
                        "PRIVATE:VALUE:FAILED": 9,
                    },
                    "job_id_request_result_correlation": {
                        "instrumentation_observed": True,
                        "applicable_call_count": 4,
                        "state_counts": {
                            "matched": 2, "mismatched": 1, "unobserved": 1,
                            "PRIVATE": 9,
                        },
                    },
                    "assertion_text_retained": False,
                    "raw_output_retained": False,
                    "raw_job_ids_retained": False,
                    "transcript_retained": False,
                    "private_diagnostic": "do not retain",
                },
            },
            "continuation_diagnostic": {
                "schema": "h007_continuation_diagnostic_v1",
                "present": True,
                "document_valid": False,
                "raw_note_retained": False,
                "raw_job_ids_retained": False,
                "stages": {"versioned_records": "complete"},
                "gates": {"starter_repair_format": "complete"},
                "line_layout": {
                    "state": "observed",
                    "progress_section_count": True,
                    "pointer_field_count": 4,
                    "line_count": 22,
                    "progress_sections_ordered": None,
                    "pointer_fields_follow_progress_sections": None,
                    "pointer_fields_in_expected_order": True,
                    "pointer_block_is_terminal": False,
                    "progress_end_line_index": 3,
                    "raw_note_retained": False,
                },
                "repair_job_diagnostic": {
                    "groups": {
                        "catalog_format": {
                            "failed_job_submitted_once": True,
                            "repair_terminal_observed": False,
                            "qualified": False,
                            "private_check": "do not retain",
                        },
                        "catalog_store": {"qualified": True},
                        "private_group": {"qualified": True},
                    },
                    "required_pair_count": 2,
                    "qualified_pair_count": 1,
                    "observed_repair_cycle_count": 2,
                    "documented_repair_cycle_count": 2,
                    "repair_cycle_count_matches": True,
                    "job_ids_unique_across_groups": True,
                    "complete": False,
                    "raw_job_ids_retained": False,
                    "raw_job_arguments_retained": False,
                },
            },
            "turn_timeout_diagnostic": {
                "schema": "h007_prompt_timeout_diagnostic_v1",
                "state": "observed",
                "prompt_request_count": 17,
                "autonomous_followup_count": 16,
                "last_request_timeout_seconds": 3.25,
                "last_request_elapsed_ms": 3250,
                "remaining_wallclock_seconds_at_prompt_start": 303.25,
                "remaining_wallclock_seconds_at_timeout": 300.0,
                "verifier_reserve_seconds": 300.0,
                "exception_text_retained": False,
                "provider_text_retained": False,
                "exception": "do not retain",
            },
        }

        diagnostic = safe_campaign_run_fields(
            _run(), canonical_identity=_run(), case_receipt=case_receipt,
        )["h007_completion_diagnostic"]

        self.assertEqual(diagnostic["timeout"], {
            "state": "observed",
            "prompt_request_count": 17,
            "autonomous_followup_count": 16,
            "last_request_elapsed_ms": 3250,
            "last_request_timeout_seconds": 3.25,
            "remaining_wallclock_seconds_at_prompt_start": 303.25,
            "remaining_wallclock_seconds_at_timeout": 300.0,
            "verifier_reserve_seconds": 300.0,
        })
        continuation = diagnostic["continuation"]
        self.assertEqual(continuation["line_layout"], {
            "state": "observed",
            "progress_section_count": None,
            "pointer_field_count": 4,
            "line_count": 22,
            "progress_sections_ordered": None,
            "pointer_fields_follow_progress_sections": None,
            "pointer_fields_in_expected_order": True,
            "pointer_block_is_terminal": False,
        })
        self.assertEqual(continuation["repair_job_diagnostic"]["qualified_pair_count"], 1)
        self.assertEqual(
            continuation["repair_job_diagnostic"]["groups"]["catalog_format"],
            {"failed_job_submitted_once": True, "repair_terminal_observed": False,
             "qualified": False},
        )
        reconciliation = diagnostic["verifier_public_reconciliation"]
        self.assertEqual(reconciliation["unbound_job_tool_call_count"], 2)
        self.assertEqual(reconciliation["unbound_call_state_counts"], {
            "status:string:failed": 1, "wait:missing:completed": 1,
        })
        self.assertEqual(reconciliation["job_id_request_result_correlation"], {
            "state": "observed",
            "instrumentation_observed": True,
            "applicable_call_count": 4,
            "state_counts": {"matched": 2, "mismatched": 1, "unobserved": 1},
        })
        encoded = json.dumps(diagnostic, sort_keys=True)
        for private_marker in (
            "PRIVATE", "do not retain", "progress_end_line_index",
            "exception\"", "private_diagnostic",
        ):
            self.assertNotIn(private_marker, encoded)

    def test_repeated_reconciliation_preserves_other_attempt_and_offset(self) -> None:
        ledger = _ledger()
        first = reconcile_campaign_run_start(
            ledger, _run(), provider_requests=87, status="in_progress")
        second = reconcile_campaign_run_start(
            ledger, _run(), provider_requests=93, status="completed_unqualified")
        third = reconcile_campaign_run_start(
            ledger, _run(), provider_requests=101, status="in_progress")

        rows = {row["attempt"]: row for row in ledger["runs"]}
        self.assertEqual(rows[1]["provider_requests_observed"], 25)
        self.assertEqual(rows[2]["provider_requests_observed"], 101)
        self.assertEqual(rows[2]["status"], "completed_unqualified")
        self.assertEqual(len(ledger["runs"]), 2)
        self.assertEqual(first["combined_starts_consumed"], 3)
        self.assertEqual(second["grant_starts_consumed"], 2)
        self.assertEqual(second["grant_starts_remaining"], 38)
        self.assertEqual(third["combined_starts_consumed"], 3)
        self.assertEqual(ledger["unrepresented_counted_starts"], 1)
        self.assertEqual(ledger["runs_started"], 3)
        self.assertNotIn("credential", rows[2])

    def test_wrong_start_number_is_refused_without_mutation(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)

        with self.assertRaises(CampaignLedgerError):
            reconcile_campaign_run_start(
                ledger, _run(start_number=4), provider_requests=1, status="in_progress")

        self.assertEqual(ledger, before)

    def test_request_overrun_is_refused_without_consuming_a_start(self) -> None:
        ledger = _ledger()
        before = copy.deepcopy(ledger)

        with self.assertRaises(CampaignLedgerError):
            reconcile_campaign_run_start(
                ledger, _run(), provider_requests=1201, status="in_progress")

        self.assertEqual(ledger, before)

    def test_duplicate_attempt_rows_are_refused(self) -> None:
        ledger = _ledger()
        ledger["runs"].append(copy.deepcopy(ledger["runs"][0]))
        before = copy.deepcopy(ledger)

        with self.assertRaises(CampaignLedgerError):
            reconcile_campaign_run_start(
                ledger, _run(attempt=1, start_number=2),
                provider_requests=25, status="completed_unqualified")

        self.assertEqual(ledger, before)

    def test_request_count_cannot_move_backwards(self) -> None:
        ledger = _ledger()
        reconcile_campaign_run_start(
            ledger, _run(), provider_requests=100, status="in_progress")
        before = copy.deepcopy(ledger)

        with self.assertRaises(CampaignLedgerError):
            reconcile_campaign_run_start(
                ledger, _run(), provider_requests=99, status="in_progress")

        self.assertEqual(ledger, before)

    def test_exhausted_grant_refuses_without_mutation(self) -> None:
        ledger = _ledger()
        ledger["supplemental_authorizations"][0]["maximum_additional_starts"] = 1
        before = copy.deepcopy(ledger)

        with self.assertRaises(CampaignLedgerError):
            reconcile_campaign_run_start(
                ledger, _run(), provider_requests=1, status="in_progress")

        self.assertEqual(ledger, before)


if __name__ == "__main__":
    unittest.main()
