import tempfile
import unittest
from pathlib import Path
import subprocess

from dev.harness_lab.improve import (Budget, Checkpoint, candidate_patch_sha256, check_scope,
                                     changed_paths, create_candidate_worktree, manual_dry_run,
                                     OptimizationCheckpoint, parse_opencode_events, run_opencode)


class ImproveTests(unittest.TestCase):
    def test_budgets_and_first_terminal_transition(self) -> None:
        budget = Budget(max_iterations=1, max_candidate_retries=0, max_proposer_calls=1)
        budget.consume("iteration")
        with self.assertRaises(RuntimeError):
            budget.consume("iteration")
        with tempfile.TemporaryDirectory() as temp:
            checkpoint = Checkpoint(Path(temp) / "run.json", "abc", "suite")
            resumable = Checkpoint(Path(temp) / "resume.json", "abc", "suite")
            resumable.transition("DIAGNOSED", {})
            self.assertEqual(Checkpoint.resume(Path(temp) / "resume.json", "abc", "suite").state, "DIAGNOSED")
            for state in ("DIAGNOSED", "PROPOSING", "PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING", "REJECTED"):
                checkpoint.transition(state, {})
            with self.assertRaises(RuntimeError):
                checkpoint.transition("ACCEPTED", {})
            with self.assertRaises(ValueError):
                Checkpoint.resume(Path(temp) / "run.json", "changed-base", "suite")
            (Path(temp) / "resume.json").write_text("{}", encoding="utf-8")
            with self.assertRaises(ValueError):
                Checkpoint.resume(Path(temp) / "resume.json", "abc", "suite")
            with self.assertRaises(FileExistsError):
                Checkpoint(Path(temp) / "resume.json", "abc", "suite")

            retry_checkpoint = Checkpoint(Path(temp) / "retry.json", "abc", "suite")
            retry_checkpoint.transition("DIAGNOSED", {})
            retry_checkpoint.transition("PROPOSING", {"attempt_kind": "initial"})
            retry_checkpoint.transition("PATCHED", {"patch_sha256": "first"})
            retry_checkpoint.transition("BUILDING", {})
            retry_checkpoint.transition("PROPOSING", {"attempt_kind": "compile_correction", "retry": 1})
            retry_checkpoint.transition("PATCHED", {"patch_sha256": "corrected"})
            retry_checkpoint.transition("BUILDING", {})
            resumed = Checkpoint.resume(Path(temp) / "retry.json", "abc", "suite")
            self.assertEqual(resumed.state, "BUILDING")
            self.assertEqual(resumed.attempt_id, 2)

    def test_opencode_json_event_parser_and_scope_gate(self) -> None:
        parsed = parse_opencode_events('{"type":"text","part":{"text":"proposal"}}\n{"type":"step_finish"}')
        self.assertEqual(parsed["text"], "proposal")
        with self.assertRaises(ValueError):
            parse_opencode_events("provider prose, not JSON")
        with self.assertRaises(ValueError):
            parse_opencode_events('{"type":"permission"}')
        with self.assertRaises(ValueError):
            check_scope(["dev/harness_lab/supervisor.py"], ("src/",))
        check_scope(["src/acp.rs"], ("src/",))

    def test_manual_dry_run_and_fake_opencode_process(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            dry = manual_dry_run(root / "dry.json")
            self.assertEqual(dry["status"], "REJECTED")
            fake = root / "opencode"
            fake.write_text("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.19.0; else echo '{\"type\":\"text\",\"part\":{\"text\":\"fixture\"}}'; fi\n", encoding="utf-8")
            fake.chmod(0o755)
            result = run_opencode(root, "inspect", executable=str(fake))
            self.assertEqual(result["version"], "1.19.0")
            self.assertEqual(result["event_count"], 1)
            slow = root / "opencode-slow"
            slow.write_text("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.19.0; else sleep 5; fi\n", encoding="utf-8")
            slow.chmod(0o755)
            with self.assertRaises(TimeoutError):
                run_opencode(root, "inspect", timeout=0.1, executable=str(slow))

    def test_patch_digest_includes_untracked_candidate_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "harness@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Harness Test"], check=True)
            (root / "base.txt").write_text("base", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "base.txt"], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "baseline"], check=True)
            (root / "CHANGELOG.harness.md").write_text("first", encoding="utf-8")
            first = candidate_patch_sha256(root, changed_paths(root))
            (root / "CHANGELOG.harness.md").write_text("second", encoding="utf-8")
            second = candidate_patch_sha256(root, changed_paths(root))
            self.assertNotEqual(first, second)

    def test_candidate_worktree_uses_exact_local_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "harness@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "Harness Test"], check=True)
            (root / "base.txt").write_text("base", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", "base.txt"], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "baseline"], check=True)
            revision = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                                      check=True, capture_output=True, text=True).stdout.strip()
            destination = Path(temp) / "candidate"
            result = create_candidate_worktree(root, "candidate/test", destination, revision)
            actual = subprocess.run(["git", "-C", str(destination), "rev-parse", "HEAD"],
                                    check=True, capture_output=True, text=True).stdout.strip()
            self.assertEqual(actual, revision)
            self.assertEqual(result["branch"], "candidate/test")

    def test_optimization_checkpoint_is_hash_linked_and_resume_validates_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "optimize.json"
            inputs = {"base_revision": "abc", "suite_sha256": "suite", "max_iterations": 2}
            checkpoint = OptimizationCheckpoint(path, inputs)
            checkpoint.record("iteration_started", {"iteration": 1, "candidate": "one"})
            resumed = OptimizationCheckpoint.resume(path, inputs)
            self.assertEqual(resumed.run_id, checkpoint.run_id)
            resumed.record("iteration_finished", {"iteration": 1, "candidate": "one"})
            with self.assertRaises(ValueError):
                OptimizationCheckpoint.resume(path, {**inputs, "max_iterations": 3})
            rows = path.with_suffix(".jsonl").read_text(encoding="utf-8").splitlines()
            rows[-1] = rows[-1].replace('"iteration":1', '"iteration":2')
            path.with_suffix(".jsonl").write_text("\n".join(rows) + "\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                OptimizationCheckpoint.resume(path, inputs)


if __name__ == "__main__":
    unittest.main()
