import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


CASE = Path(os.environ["CODEGG_AUTONOMY_CASE_ROOT"]) / "fixture"
TRANSCRIPT = Path(os.environ["CODEGG_AUTONOMY_TRANSCRIPT"])


class EventLedgerContract(unittest.TestCase):
    def test_workspace_tests_and_independent_scale_oracle(self):
        subprocess.run(["cargo", "test", "--offline", "--locked", "--workspace"], cwd=CASE,
                       check=True, timeout=240, stdout=subprocess.DEVNULL)
        rows = 50_000
        with tempfile.TemporaryDirectory() as directory:
            shards = [Path(directory) / f"shard-{index}.txt" for index in range(5)]
            expected = {}
            records = {}
            for index in range(rows):
                timestamp = (rows - index) % 601
                amount = (index % 17) - 8
                records[index] = f"{timestamp}|id-{index:06}|transfer|{amount}|src"
                window = timestamp // 60
                total, count = expected.get(window, (0, 0))
                expected[window] = (total + amount, count + 1)
            for shard_index, shard in enumerate(shards):
                indexes = [index for index in range(rows) if index % len(shards) == shard_index]
                with shard.open("w", encoding="utf-8") as stream:
                    for index in reversed(indexes):
                        stream.write(records[index] + "\n")
                    stream.write(records[shard_index] + "\n")
                    stream.write("invalid-row\n")
            completed = subprocess.run(
                ["cargo", "run", "--offline", "--locked", "--quiet", "-p", "ledger-cli", "--",
                 "summarize", *map(str, shards)], cwd=CASE, check=True, timeout=240,
                capture_output=True, text=True)
            lines = completed.stdout.splitlines()
            self.assertEqual(lines[0], f"records={rows} rejected={len(shards)}")
            actual = [tuple(map(int, line.split("|"))) for line in lines[1:]]
            self.assertEqual(actual, [(window, *expected[window]) for window in sorted(expected)])

    def test_transcript_proves_real_job_overlap_and_continuation(self):
        transcript = json.loads(TRANSCRIPT.read_text(encoding="utf-8"))
        calls = transcript["tool_calls"]
        submissions = [call for call in calls if call.get("job", {}).get("action") == "submit"]
        self.assertTrue(submissions, "no background test job was submitted")
        job_ids = {call["job"]["job_id"] for call in submissions if call["job"].get("job_id")}
        self.assertTrue(job_ids, "the test submission did not expose its exact job id")
        observations = [call for call in calls if call.get("job", {}).get("job_id") in job_ids
                        and call.get("tool") == "job"]
        self.assertTrue(observations, "the submitted job was never continued by exact id")
        running = [call for call in observations if call.get("job", {}).get("status") == "running"]
        self.assertTrue(running, "no observation proved the job was still running")
        submitted_at = min(call.get("finished_elapsed_ms") or 0 for call in submissions)
        running_call = running[0]
        running_at = running_call.get("finished_elapsed_ms")
        self.assertIsInstance(running_at, int)
        intervening = [call for call in calls
                       if submitted_at < (call.get("started_elapsed_ms") or 0) < running_at
                       and call.get("tool") in {"write", "edit", "apply_patch", "test"}]
        self.assertGreaterEqual(len(intervening), 2)
        self.assertTrue(any(call.get("job", {}).get("status") in
                            {"completed", "failed", "cancelled", "canceled", "timed_out"}
                            for call in observations), "job never reached an observed terminal state")

    def test_transcript_proves_two_failure_diagnosis_repair_cycles(self):
        calls = json.loads(TRANSCRIPT.read_text(encoding="utf-8"))["tool_calls"]
        tests = [(index, call) for index, call in enumerate(calls) if call.get("tool") == "test"]
        failures = [(index, call) for index, call in tests if call.get("failed")]
        self.assertGreaterEqual(len(failures), 2, "two distinct test failures were not observed")
        completed = [(index, call) for index, call in tests
                     if not call.get("failed") and call.get("status") == "completed"]
        cycles = 0
        last_repair_end = -1
        for failure_index, _ in failures:
            if failure_index <= last_repair_end:
                continue
            next_pass = next(((index, call) for index, call in completed if index > failure_index), None)
            if next_pass is None:
                continue
            pass_index, _ = next_pass
            repairs = [call for call in calls[failure_index + 1:pass_index]
                       if call.get("tool") in {"write", "edit", "apply_patch"}]
            if repairs:
                cycles += 1
                last_repair_end = pass_index
        self.assertGreaterEqual(cycles, 2)

    def test_frozen_public_contract_and_checks_were_not_changed(self):
        metadata = json.loads((TRANSCRIPT.parent / "case-metadata.json").read_text(encoding="utf-8"))
        verifier_hash = __import__("hashlib").sha256(Path(__file__).read_bytes()).hexdigest()
        self.assertEqual(verifier_hash, metadata["hidden_verifier_sha256"])
        for relative, expected in metadata["fixture_sha256"].items():
            actual = __import__("hashlib").sha256((CASE / relative).read_bytes()).hexdigest()
            self.assertEqual(actual, expected, f"frozen task input changed: {relative}")
        for relative, required_names in metadata["required_public_tests"].items():
            contents = (CASE / relative).read_text(encoding="utf-8")
            for name in required_names:
                self.assertIn(f"fn {name}", contents)

    def test_end_to_end_journal_ingest_inspect_and_export(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shard_a = root / "shard-a.txt"
            shard_b = root / "shard-b.txt"
            journal = root / "ledger.log"
            export = root / "export.txt"
            shard_a.write_text("61|b|open|2|src\ninvalid\n", encoding="utf-8")
            shard_b.write_text("1|a|close|3|source\n61|b|open|2|source\n", encoding="utf-8")
            ingested = subprocess.run(["cargo", "run", "--offline", "--locked", "--quiet", "-p", "ledger-cli", "--",
                                       "ingest", str(journal), str(shard_a), str(shard_b)], cwd=CASE, check=True, timeout=120,
                                      capture_output=True, text=True)
            self.assertIn("ingested=2 rejected=1", ingested.stdout)
            inspected = subprocess.run(["cargo", "run", "--offline", "--locked", "--quiet", "-p", "ledger-cli", "--",
                                        "inspect", str(journal)], cwd=CASE, check=True, timeout=120,
                                       capture_output=True, text=True)
            self.assertIn("records=2", inspected.stdout)
            subprocess.run(["cargo", "run", "--offline", "--locked", "--quiet", "-p", "ledger-cli", "--",
                            "export", str(shard_a), str(export)], cwd=CASE, check=True, timeout=120)
            self.assertEqual(export.read_text(encoding="utf-8"),
                             "61|b|open|2|source\n")


if __name__ == "__main__":
    unittest.main()
