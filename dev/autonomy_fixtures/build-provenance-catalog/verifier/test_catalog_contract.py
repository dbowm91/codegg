from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest

CASE = pathlib.Path(os.environ.get("CODEGG_AUTONOMY_CASE_ROOT", ""))
FIXTURE = CASE / "fixture"
TRANSCRIPT = pathlib.Path(os.environ.get("CODEGG_AUTONOMY_TRANSCRIPT", ""))
META = pathlib.Path(__file__).resolve().parents[1] / "case-metadata.json"


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], *, cwd: pathlib.Path, expected: int = 0) -> subprocess.CompletedProcess:
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=240)
    if result.returncode != expected:
        raise AssertionError(f"{command!r} exit={result.returncode}: {result.stdout}\n{result.stderr}")
    return result


class CatalogContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not FIXTURE.is_dir():
            raise AssertionError("agent fixture path is unavailable")
        cls.metadata = json.loads(META.read_text())
        run(["cargo", "test", "--offline", "--locked", "--workspace"], cwd=FIXTURE)
        run(["cargo", "build", "--offline", "--locked", "-p", "catalog-cli"], cwd=FIXTURE)
        cls.cli = FIXTURE / "target/debug/catalog-cli"

    def call(self, *args: str, expected: int = 0) -> subprocess.CompletedProcess:
        return run([str(self.cli), *args], cwd=FIXTURE, expected=expected)

    def test_end_to_end_import_recovery_lineage_diff_and_deterministic_export(self):
        corpus = FIXTURE / "corpus"
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            first, second = root / "first.catalog", root / "second.catalog"
            imported = self.call("import", str(first), str(corpus / "snapshot-0.records"))
            self.assertIn("imported=32", imported.stdout)
            self.assertIn("imported=0", self.call("import", str(first), str(corpus / "snapshot-0.records")).stdout)
            self.assertIn("verified=32", self.call("verify", str(first)).stdout)
            lineage = self.call("lineage", str(first), "pkg-31")
            self.assertIn("provenance=complete", lineage.stdout)
            self.call("import", str(second), str(corpus / "snapshot-1.records"))
            diff = self.call("diff", str(first), str(second))
            self.assertEqual(sum(line.startswith("added|") for line in diff.stdout.splitlines()), 32)
            export1 = self.call("export", str(first)).stdout
            export2 = self.call("export", str(first)).stdout
            self.assertEqual(export1, export2)
            self.assertTrue(all(line.startswith("v2|") for line in export1.splitlines()))
            before = digest(first)
            self.call("import", str(first), str(corpus / "invalid-checksum.records"), expected=2)
            self.assertEqual(before, digest(first), "failed import changed committed catalog")

    def test_durable_continuation_records_completed_phases_and_next_boundary(self):
        note = (FIXTURE / "CONTINUATION.md").read_text()
        required = ["versioned_records", "durable_import", "dependency_index",
                    "provenance_queries", "cli_export"]
        positions = [note.index(phase) for phase in required]
        self.assertEqual(positions, sorted(positions), "phase note must preserve dependency order")
        self.assertIn("next_phase: none", note)
        self.assertRegex(note, r"job_id: \S+")

    def test_missing_references_and_cycles_are_reported(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            invalid = root / "invalid.records"
            def row(package: str, deps: str) -> str:
                prefix = f"v2|{package}|b1|r1|{deps}"
                checksum = 0x811C9DC5
                for byte in prefix.encode(): checksum = ((checksum ^ byte) * 0x01000193) & 0xFFFFFFFF
                return f"{prefix}|{checksum:08x}"
            invalid.write_text(row("app", "core,ghost") + "\n" + row("core", "app") + "\n")
            state = root / "state.catalog"
            self.call("import", str(state), str(invalid))
            diagnostic = self.call("verify", str(state), expected=2)
            self.assertIn("dependency cycles", diagnostic.stderr)
            self.assertIn("app", diagnostic.stderr)
            missing = root / "missing.records"
            missing.write_text(row("orphan", "ghost") + "\n")
            self.call("import", str(missing), str(missing))
            missing_diagnostic = self.call("verify", str(missing), expected=2)
            self.assertIn("missing dependencies", missing_diagnostic.stderr)

    def test_frozen_inputs_and_manifest_are_unchanged(self):
        for relative, expected in self.metadata["frozen_sha256"].items():
            self.assertEqual(digest(FIXTURE / relative), expected, relative)
        self.assertEqual(len(self.metadata["manifest_sha256"]), 64)

    def test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation(self):
        if not TRANSCRIPT.is_file():
            self.skipTest("live transcript evidence is not part of offline fixture calibration")
        transcript = json.loads(TRANSCRIPT.read_text())
        calls = transcript["tool_calls"]
        jobs = [(index, call["tool"], call.get("job", {}))
                for index, call in enumerate(calls) if isinstance(call.get("job"), dict)]
        submissions = [job for _index, tool, job in jobs
                       if tool == "test" and job.get("action") == "submit"]
        ids = {job.get("job_id") for job in submissions if job.get("job_id")}
        lifecycle = None
        for job_id in ids:
            observations = [(index, job.get("status")) for index, _tool, job in jobs
                            if job.get("job_id") == job_id]
            running = [index for index, status in observations if status == "running"]
            terminal = [index for index, status in observations if status == "completed"]
            if running and terminal and min(terminal) > min(running) and any(
                    call["tool"] in {"write", "edit", "apply_patch", "bash", "test"}
                    for call in calls[min(running) + 1:min(terminal)]):
                lifecycle = (job_id, min(running), min(terminal))
                break
        self.assertIsNotNone(lifecycle, "one exact submitted ID must run during productive edits")
        test_terminals = [(index, job.get("job_id"), job.get("status"))
                          for index, tool, job in jobs if tool == "job"
                          and job.get("status") in {"failed", "completed"}]
        failures = {job_id: index for index, job_id, status in test_terminals
                    if status == "failed" and job_id}
        successes = [index for index, _job_id, status in test_terminals if status == "completed"]
        repairs = sum(any(success > failed for success in successes)
                      for failed in failures.values())
        self.assertGreaterEqual(repairs, 2, "two distinct failed checks need successful later repairs")
        last_terminal = max(index for index, _job_id, _status in test_terminals)
        self.assertTrue(any(call["tool"] in {"write", "edit", "apply_patch", "test"}
                            for call in calls[last_terminal + 1:]),
                        "a continuation must resume productive work after terminal observation")
        note = (FIXTURE / "CONTINUATION.md").read_text()
        self.assertRegex(note, rf"job_id: {re.escape(lifecycle[0])}(?:\s|$)")
        self.assertEqual(transcript.get("schema_version"), 4)
        self.assertEqual(transcript.get("terminal"), "completed")


if __name__ == "__main__":
    unittest.main()
