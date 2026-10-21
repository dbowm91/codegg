from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import tempfile
import time
import unittest

from dev.harness_lab.supervisor import SubjectSpec, run_exec_trial


#: A fake headless `codegg exec`.
#:
#: It records the argv it was given and replays one fixed outcome, so the
#: headless trial is exercised without a model, a daemon, or a provider. The
#: artifact is written relative to the child's cwd, which is the trial's
#: private workspace.
_FAKE_EXEC_TEMPLATE = '''#!/usr/bin/env python3
import json
import sys
import time

MODE = "__MODE__"
LOG = "__LOG__"
ARTIFACT = "__ARTIFACT__"
CONTENT = "__CONTENT__"

with open(LOG, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(sys.argv[1:]) + "\\n")

if MODE == "hang":
    while True:
        time.sleep(3600)

tokens = None if MODE == "no-usage" else 26
if MODE in ("success", "no-usage"):
    with open(ARTIFACT, "w", encoding="utf-8") as handle:
        handle.write(CONTENT)
if MODE == "refusal":
    sys.stderr.write("Tool 'write' non-retryable error: permission denied: "
                     "verified tool authority is missing workspace path policy\\n")

print(json.dumps({"success": True, "result": "fixture turn complete",
                  "toolsUsed": ["write"], "tokensUsed": tokens,
                  "durationMs": 12, "error": None, "code": None}))
'''

#: Terminals the ledger records for a trial that did not pass.
FAILURE_TERMINALS = {"assertion_failed", "execution_failed", "cancelled", "denied",
                     "timeout", "budget_exhausted", "infrastructure_error",
                     "indeterminate", "interrupted_unknown"}


class SubjectEnvironmentTests(unittest.TestCase):
    def test_lane_env_replaces_runtime_and_user_roots(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            binary.chmod(0o755)
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            original = dict(os.environ)
            os.environ.update({
                "CODEGG_DAEMON_HOME": "/shared/daemon",
                "CODEGG_DATA_HOME": "/shared/data",
                "CODEGG_TUI_CONFIG": "/shared/config.jsonc",
                "CODEGG_CORE_ENDPOINT": "unix:///shared.sock",
                "OPENAI_API_KEY": "must-not-inherit",
                "XDG_RUNTIME_DIR": "/shared/runtime",
            })
            try:
                env = spec.environment()
            finally:
                os.environ.clear()
                os.environ.update(original)
            self.assertEqual(env["CODEGG_DAEMON_HOME"], str(root / "lane" / "daemon"))
            self.assertEqual(env["CODEGG_DATA_HOME"], str(root / "lane" / "data"))
            self.assertEqual(env["HOME"], str(root / "lane" / "home"))
            self.assertEqual(env["XDG_RUNTIME_DIR"], str(root / "lane" / "runtime"))
            self.assertNotIn("CODEGG_CORE_ENDPOINT", env)
            self.assertNotIn("OPENAI_API_KEY", env)
            self.assertTrue((root / "lane" / "config").is_dir())

    def test_subject_hash_is_pinned_at_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"one")
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            binary.write_bytes(b"two")
            self.assertNotEqual(spec.binary_sha256, hashlib.sha256(binary.read_bytes()).hexdigest())


class ExecTrialTests(unittest.TestCase):
    """The headless adapter is measured the same way the ACP adapter is."""

    def _fake_binary(self, root: Path, mode: str, *, artifact: str = "tool-receipt.txt",
                     content: str = "harness-lab-tool-ok") -> tuple[Path, Path]:
        """Write a fake headless `codegg` and return it with its argv log."""
        log = root / "exec.argv.jsonl"
        binary = root / "codegg"
        body = (_FAKE_EXEC_TEMPLATE.replace("__MODE__", mode)
                .replace("__LOG__", str(log))
                .replace("__ARTIFACT__", artifact)
                .replace("__CONTENT__", content))
        binary.write_text(body, encoding="utf-8")
        binary.chmod(0o755)
        log.write_text("", encoding="utf-8")
        return binary, log

    def _lane_root(self, receipt: dict) -> Path:
        """The lane root of a finished trial: the workspace's parent."""
        return Path(receipt["workspace"]).parent

    def test_a_headless_trial_that_writes_the_artifact_passes(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIs(receipt["ok"], True)
            self.assertTrue(receipt["tool_receipt_created"])
            self.assertEqual(receipt["tool_failures"], 0)
            self.assertEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_a_refused_headless_tool_is_never_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "refusal")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            # The turn completed, so the child reported success. The tool did
            # not: a completed turn with a refused tool is not a pass.
            self.assertIs(receipt["ok"], False)
            self.assertGreaterEqual(receipt["tool_failures"], 1)
            terminal = receipt["ledger_manifest"]["terminal"]
            self.assertNotEqual(terminal, "passed")
            self.assertIn(terminal, FAILURE_TERMINALS)

    def test_a_tool_call_without_an_artifact_is_a_tool_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "silent")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            # The child named no failure: the missing artifact is the only
            # evidence, and it is enough.
            self.assertIs(receipt["ok"], False)
            self.assertGreaterEqual(receipt["tool_failures"], 1)
            self.assertNotEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_a_missing_usage_is_recorded_as_none(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "no-usage")
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIsNone(receipt["codegg_usage"])
            metrics = receipt["ledger_manifest"]["metrics"]
            self.assertIsNone(metrics["input_tokens"])
            self.assertIsNone(metrics["output_tokens"])

    def test_the_lane_is_removed_after_a_successful_trial(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            receipt = run_exec_trial(binary, "workspace-write", 30.0)
            self.assertIs(receipt["ok"], True)
            self.assertFalse(self._lane_root(receipt).exists())

    def test_the_lane_is_removed_after_a_failed_trial(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "refusal")
            receipt = run_exec_trial(binary, "workspace-write", 30.0)
            self.assertIs(receipt["ok"], False)
            self.assertFalse(self._lane_root(receipt).exists())

    def test_a_child_that_never_exits_is_bounded_by_the_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "hang")
            started = time.monotonic()
            receipt = run_exec_trial(binary, "workspace-write", 3.0,
                                     ledger_root=root / "ledger")
            elapsed = time.monotonic() - started
            self.assertLess(elapsed, 60.0)
            self.assertIs(receipt["ok"], False)
            self.assertEqual(receipt["stop_reason"], "timeout")
            self.assertEqual(receipt["ledger_manifest"]["terminal"], "timeout")
            self.assertFalse(self._lane_root(receipt).exists())

    def test_the_headless_child_is_invoked_with_the_documented_argv(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, log = self._fake_binary(root, "success")
            run_exec_trial(binary, "workspace-write", 30.0)
            recorded = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines() if line]
            self.assertEqual(len(recorded), 1)
            argv = recorded[0]
            self.assertEqual(argv[0], "exec")
            self.assertEqual(argv[1], "--json")
            request = json.loads(argv[2])
            self.assertEqual(request["model"], "openai/harness-fixture")
            self.assertEqual(request["agent"], "build")
            self.assertTrue(request["prompt"])
            self.assertEqual(argv[3:], ["--format", "json", "--approval-mode", "yolo",
                                        "--sandbox", "workspace-write"])

    def test_an_unparseable_child_result_is_never_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary, _ = self._fake_binary(root, "success")
            binary.write_text("#!/bin/sh\necho 'not json'\n", encoding="utf-8")
            binary.chmod(0o755)
            receipt = run_exec_trial(binary, "workspace-write", 30.0,
                                     ledger_root=root / "ledger")
            self.assertIs(receipt["ok"], False)
            self.assertNotEqual(receipt["ledger_manifest"]["terminal"], "passed")

    def test_the_real_headless_binary_never_passes_without_the_artifact(self) -> None:
        binary = Path(__file__).resolve().parents[2] / "target" / "debug" / "codegg"
        if not binary.exists():
            self.skipTest("the debug binary is not built")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt = run_exec_trial(binary, "workspace-write", 120.0,
                                     ledger_root=root / "ledger")
            terminal = receipt["ledger_manifest"]["terminal"]
            # A pass is only ever recorded for an artifact that exists: the
            # headless child reporting `success` proves nothing on its own.
            if terminal == "passed":
                self.assertTrue(receipt["tool_receipt_created"])
            self.assertFalse(self._lane_root(receipt).exists())


if __name__ == "__main__":
    unittest.main()
