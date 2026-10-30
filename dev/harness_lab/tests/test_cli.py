from __future__ import annotations

import contextlib
import io
import json
import pathlib
import tempfile
import unittest
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
