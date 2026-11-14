from __future__ import annotations

import contextlib
import io
import json
import pathlib
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

from dev.harness_lab import __main__ as harness_cli


class AutonomyCompareCliTests(unittest.TestCase):
    def test_writes_nested_comparison_decision_without_live_provider(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"test binary")
            binary.chmod(0o755)
            budget_path = root / "budget.json"
            budget_path.write_text(json.dumps({
                "cost_observable": True,
                "cost_limit_usd": 1.0,
                "cost_enforcement": "controller_enforced",
                "max_provider_requests": 2,
                "max_wallclock_seconds": 30,
            }), encoding="utf-8")
            output_path = root / "comparison.json"
            expected = {
                "schema_version": 1,
                "comparison": {"outcome": {"decision": "NO_GO", "objective": "no gains"}},
            }
            stdout = io.StringIO()
            with (mock.patch.object(harness_cli.autonomy_run_module,
                                    "run_paired_comparison", return_value=expected),
                  mock.patch.object(harness_cli, "_binary", return_value=binary)):
                with contextlib.redirect_stdout(stdout):
                    status = harness_cli.main([
                        "autonomy-compare", "--model", "opencode_go/muse-spark-1.3-contributor",
                        "--baseline-binary", str(binary), "--candidate-binary", str(binary),
                        "--manifest", "dev/harness_lab/fixtures/a005_placement",
                        "--budget", str(budget_path), "--live-provider",
                        "--output", str(output_path),
                    ])

            self.assertEqual(status, 0)
            self.assertEqual(json.loads(output_path.read_text()), expected)
            self.assertEqual(json.loads(stdout.getvalue()), {
                "ok": True, "output": str(output_path),
                "decision": "NO_GO", "objective": "no gains",
            })


class AutonomyCampaignCliTests(unittest.TestCase):
    def test_h007_campaign_projects_case_diagnostics_from_suite_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"test binary")
            binary.chmod(0o755)
            binary_sha = harness_cli._sha256(binary)
            manifest_sha = "e" * 64
            budget_path = root / "budget.json"
            budget_path.write_text(json.dumps({
                "cost_observable": True,
                "cost_limit_usd": 1.0,
                "cost_enforcement": "controller_enforced",
                "max_provider_requests": 1200,
                "max_wallclock_seconds": 3600,
            }), encoding="utf-8")
            ledger_path = root / "campaign-ledger.json"
            ledger_path.write_text(json.dumps({"runs": []}), encoding="utf-8")
            preflight_path = root / "preflight.json"
            preflight = {
                "attempt": 2,
                "start_number": 2,
                "authorization_grant_id": "test-grant",
                "model": "opencode_go/muse-spark-1.3-contributor",
                "route": "opencode_go",
                "manifest_sha256": manifest_sha,
                "fixture_manifest_sha256": manifest_sha,
                "binary_sha256": binary_sha,
                "provider_request_limit": 1200,
                "wallclock_limit_seconds": 3600,
                "start_counted": False,
                "provider_budget_admission": "pending",
                "current_goal_authorized_starts": 120,
                "current_goal_starts_consumed_before": 27,
                "current_goal_starts_remaining_before": 93,
                "preflight_disk_reserve_bytes": 30 * 1024**3,
                "preflight_disk_reserve_met": True,
            }
            preflight_path.write_text(json.dumps(preflight), encoding="utf-8")
            evidence_path = root / "safe-campaign.json"
            receipt = {
                "solved_cases": 0,
                "model": "opencode_go/muse-spark-1.3-contributor",
                "runs": [{
                    "case_id": harness_cli.autonomy_run_module.H007_CASE_ID,
                    "provider_request_count": 1,
                    "turn_timeout_diagnostic": {
                        "schema": "h007_prompt_timeout_diagnostic_v1",
                        "state": "observed",
                        "prompt_request_count": 2,
                        "autonomous_followup_count": 1,
                        "last_request_timeout_seconds": 8.5,
                        "last_request_elapsed_ms": 8500,
                        "remaining_wallclock_seconds_at_prompt_start": 308.5,
                        "remaining_wallclock_seconds_at_timeout": 300.0,
                        "verifier_reserve_seconds": 300.0,
                        "exception_text_retained": False,
                        "provider_text_retained": False,
                    },
                    # This per-case digest has a different meaning from the
                    # canonical suite manifest in the launch preflight.
                    "manifest_sha256": "c" * 64,
                    "task_completion_diagnostic": {
                        "schema": "h007_task_completion_diagnostic_v2",
                        "state": "agent_incomplete",
                        "reasons": ["required_stages_incomplete"],
                        "verifier_outcome": "not_run",
                        "raw_note_retained": False,
                        "raw_job_ids_retained": False,
                        "verifier_public_reconciliation": {
                            "schema": "h007_verifier_public_reconciliation_v1",
                            "failure_domain_counts": {},
                            "public_domain_states": {},
                            "classification": {},
                            "public_checks": {
                                "continuation_boundary": {
                                    "document_valid": False,
                                    "next_gate_none": None,
                                    "private_check": "PRIVATE CHECK",
                                },
                                "transcript_job_lifecycle": {
                                    "test_jobs_terminal": True,
                                },
                            },
                            "unbound_job_tool_call_count": 1,
                            "unbound_call_state_counts": {"status:missing:failed": 1},
                            "job_id_request_result_correlation": {
                                "instrumentation_observed": True,
                                "applicable_call_count": 2,
                                "state_counts": {"matched": 1, "mismatched": 1},
                            },
                            "assertion_text_retained": False,
                            "raw_output_retained": False,
                            "raw_job_ids_retained": False,
                            "transcript_retained": False,
                        },
                    },
                    "continuation_diagnostic": {
                        "schema": "h007_continuation_diagnostic_v1",
                        "present": True,
                        "document_valid": False,
                        "raw_note_retained": False,
                        "raw_job_ids_retained": False,
                        "stages": {"versioned_records": "complete",
                                   "durable_import": "pending"},
                        "gates": {"starter_repair_format": "pending"},
                        "required_review_observed": False,
                        "release_overlap_confirmed": False,
                        "all_public_artifacts_present": False,
                        "line_layout": {
                            "state": "observed",
                            "progress_section_count": 3,
                            "pointer_field_count": 4,
                            "progress_sections_ordered": True,
                            "pointer_fields_follow_progress_sections": False,
                            "pointer_fields_in_expected_order": True,
                            "pointer_block_is_terminal": True,
                            "raw_note_retained": False,
                        },
                        "repair_job_diagnostic": {
                            "groups": {"catalog_format": {"qualified": False}},
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
                }],
            }
            suite = SimpleNamespace(suite_sha256=manifest_sha, cases=())
            preview = {
                "attempt": 2,
                "start_number": 2,
                "combined_starts_consumed": 2,
                "combined_starts_remaining": 198,
                "grant_starts_consumed": 1,
                "grant_starts_remaining": 39,
            }
            stdout = io.StringIO()

            with (
                mock.patch.object(harness_cli.autonomy_module, "load_autonomy_suite",
                                  return_value=suite),
                mock.patch.object(harness_cli.autonomy_run_module, "run_suite",
                                  return_value=receipt),
                mock.patch.object(harness_cli, "_binary", return_value=binary),
                mock.patch.object(
                    harness_cli.shutil, "disk_usage",
                    return_value=SimpleNamespace(free=40 * 1024**3)),
                mock.patch.object(harness_cli.campaign_authorization,
                                  "preflight_campaign_run_start", return_value=preview),
                mock.patch.object(harness_cli.campaign_authorization,
                                  "safe_campaign_run_fields",
                                  wraps=harness_cli.campaign_authorization.safe_campaign_run_fields
                                  ) as project,
                mock.patch.object(harness_cli.campaign_authorization,
                                  "reconcile_campaign_run_start", return_value=preview
                                  ) as reconcile,
                contextlib.redirect_stdout(stdout),
            ):
                status = harness_cli.main([
                    "autonomy", "--model", "opencode_go/muse-spark-1.3-contributor",
                    "--case", harness_cli.autonomy_run_module.H007_CASE_ID,
                    "--case-manifest", str(root / "cases.jsonl"),
                    "--binary", str(binary), "--profile", "workspace-write",
                    "--timeout", "3600", "--min-free-disk-bytes", str(10 * 1024**3),
                    "--lane-root", str(root / "lane"), "--budget", str(budget_path),
                    "--live-provider", "--output", str(root / "private-receipt.json"),
                    "--campaign-ledger", str(ledger_path),
                    "--campaign-preflight", str(preflight_path),
                    "--campaign-evidence", str(evidence_path),
                ])

            self.assertEqual(status, 2)
            self.assertIs(project.call_args.kwargs["case_receipt"], receipt["runs"][0])
            safe_evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            projected = safe_evidence["safe_run_fields"]
            self.assertEqual(projected["manifest_sha256"], manifest_sha)
            self.assertEqual(
                projected["h007_completion_diagnostic"]["continuation"]
                ["first_missing_stage"], "durable_import")
            self.assertEqual(
                projected["h007_completion_diagnostic"]["verifier_public_reconciliation"]
                ["public_checks"],
                {
                    "continuation_boundary": {
                        "document_valid": False,
                        "next_gate_none": None,
                    },
                    "transcript_job_lifecycle": {"test_jobs_terminal": True},
                },
            )
            self.assertNotIn("PRIVATE CHECK", json.dumps(projected, sort_keys=True))
            diagnostic = projected["h007_completion_diagnostic"]
            self.assertEqual(diagnostic["timeout"]["remaining_wallclock_seconds_at_timeout"],
                             300.0)
            self.assertEqual(
                diagnostic["continuation"]["line_layout"]["pointer_field_count"], 4)
            self.assertFalse(
                diagnostic["continuation"]["repair_job_diagnostic"]
                ["groups"]["catalog_format"]["qualified"])
            self.assertEqual(
                diagnostic["verifier_public_reconciliation"]["unbound_job_tool_call_count"],
                1)
            self.assertEqual(
                diagnostic["verifier_public_reconciliation"]
                ["job_id_request_result_correlation"]["state_counts"],
                {"matched": 1, "mismatched": 1})
            reconciled = reconcile.call_args.args[1]
            self.assertEqual(reconciled["manifest_sha256"], manifest_sha)


class ToolTrialCliTests(unittest.TestCase):
    def test_explicit_deny_expects_one_refused_tool_call(self) -> None:
        binary = pathlib.Path("/fixture/codegg")
        with (mock.patch.object(harness_cli, "_binary", return_value=binary),
              mock.patch.object(harness_cli, "run_tool_trial", return_value={"ok": True}) as run):
            status = harness_cli.main([
                "tool-trial", "--profile", "workspace-write", "--deny-bash",
                "--binary", str(binary),
            ])

        self.assertEqual(status, 0)
        self.assertEqual(run.call_args.kwargs["expected_tool_failures"], 1)
