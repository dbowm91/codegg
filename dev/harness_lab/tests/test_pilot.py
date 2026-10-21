"""Provider-free tests for the live pilot driver.

The driver itself makes real provider calls, so these tests cover only the
parts that must be decided *before* any request is made: the frozen-fixture
identity, the credential lookup, the model-route shape, and the budget
admission gate.
"""
import json
import shutil
import tempfile
import unittest
import unittest.mock
from pathlib import Path

from dev.harness_lab import budget, pilot
from dev.harness_lab.supervisor import HarnessError

REPO_ROOT = Path(__file__).resolve().parents[3]


class FixtureDigestTest(unittest.TestCase):
    """The frozen fixtures must stay byte-identical to the M005 receipts."""

    def test_worklog_digest_matches_the_recorded_m005_receipt(self):
        directory = REPO_ROOT / pilot.PILOT_ROOT_RELPATH / "worklog"
        self.assertEqual(
            pilot.fixture_digest(directory),
            "5adfbae3d68ea0857a5ebbb212e62333d76aaecd6ef178f3bc3f14d612b12135")

    def test_ranges_digest_matches_the_recorded_m005_receipt(self):
        directory = REPO_ROOT / pilot.PILOT_ROOT_RELPATH / "ranges"
        self.assertEqual(
            pilot.fixture_digest(directory),
            "89075ca1e92c7e1b829d5f6db99320d5c7bb437830cd2c6d1840b4b7a1ad0d53")

    def test_digest_changes_when_a_frozen_file_changes(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "task"
            source.mkdir()
            (source / "a.py").write_text("x = 1\n", encoding="utf-8")
            before = pilot.fixture_digest(source)
            (source / "a.py").write_text("x = 2\n", encoding="utf-8")
            self.assertNotEqual(before, pilot.fixture_digest(source))


class FixtureCopyTest(unittest.TestCase):
    """A run must start from a byte-identical copy of the frozen fixture."""

    def test_copy_is_byte_identical_and_reports_the_source_digest(self):
        source = REPO_ROOT / pilot.PILOT_ROOT_RELPATH / "ranges"
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "workspace"
            digest = pilot._copy_fixture(source, destination)
            self.assertEqual(digest, pilot.fixture_digest(source))
            for entry in source.iterdir():
                if entry.is_file():
                    self.assertEqual(
                        pilot.sha256_file(destination / entry.name),
                        pilot.sha256_file(entry),
                        f"{entry.name} was not copied byte-identically")

    def test_a_stale_destination_is_reset_to_the_frozen_bytes(self):
        source = REPO_ROOT / pilot.PILOT_ROOT_RELPATH / "ranges"
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "workspace"
            destination.mkdir()
            (destination / "ranges.py").write_text("tampered\n", encoding="utf-8")
            pilot._copy_fixture(source, destination)
            self.assertEqual(pilot.sha256_file(destination / "ranges.py"),
                             pilot.sha256_file(source / "ranges.py"))

    def test_a_copy_that_diverges_from_the_source_is_refused(self):
        source = REPO_ROOT / pilot.PILOT_ROOT_RELPATH / "ranges"
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "workspace"
            real_copy = shutil.copy2

            def corrupting_copy(src, dst, *args, **kwargs):
                real_copy(src, dst, *args, **kwargs)
                Path(dst).write_text("corrupted\n", encoding="utf-8")

            with unittest.mock.patch.object(pilot.shutil, "copy2", corrupting_copy):
                with self.assertRaises(HarnessError):
                    pilot._copy_fixture(source, destination)


class CredentialLookupTest(unittest.TestCase):
    """The credential is looked up, never invented and never printed."""

    def test_missing_store_yields_no_credential(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertIsNone(pilot.opencode_go_credential(Path(temp)))

    def test_malformed_store_yields_no_credential(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            path = home / ".local" / "share" / "opencode" / "auth.json"
            path.parent.mkdir(parents=True)
            path.write_text("not json", encoding="utf-8")
            self.assertIsNone(pilot.opencode_go_credential(home))

    def test_store_without_the_opencode_go_entry_yields_no_credential(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            path = home / ".local" / "share" / "opencode" / "auth.json"
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps({"other": {"type": "api", "key": "x"}}),
                            encoding="utf-8")
            self.assertIsNone(pilot.opencode_go_credential(home))

    def test_store_with_the_entry_yields_the_key(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            path = home / ".local" / "share" / "opencode" / "auth.json"
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps({"opencode-go": {"type": "api", "key": "secret"}}),
                            encoding="utf-8")
            self.assertEqual(pilot.opencode_go_credential(home), "secret")


class RefusalTest(unittest.TestCase):
    """Every refusal happens before a provider request is made."""

    def _policy(self, **overrides):
        mapping = {"token_limit": 1000, "max_provider_requests": 1,
                   "max_wallclock_seconds": 60}
        mapping.update(overrides)
        return budget.budget_policy_from_mapping(mapping)

    def test_unknown_task_is_refused(self):
        with self.assertRaises(HarnessError):
            pilot.run_live_task(Path("/nonexistent"), "not-a-task",
                                "opencode_go/muse-spark-1.3-contributor",
                                budget_policy=self._policy())

    def test_a_model_without_a_provider_half_is_refused(self):
        with self.assertRaises(HarnessError):
            pilot.run_live_task(Path("/nonexistent"), "ranges",
                                "muse-spark-1.3-contributor",
                                budget_policy=self._policy())

    def test_an_unknown_profile_is_refused(self):
        with self.assertRaises(HarnessError):
            pilot.run_live_task(Path("/nonexistent"), "ranges",
                                "opencode_go/muse-spark-1.3-contributor",
                                budget_policy=self._policy(), profile="full-host-here")

    def test_a_refused_budget_is_refused_before_any_provider_work(self):
        # No enforceable cap at all: the gate must refuse rather than let an
        # unbounded live run start.
        policy = budget.budget_policy_from_mapping({"cost_limit_usd": 1.0,
                                                     "cost_enforcement": "controller_enforced",
                                                     "cost_observable": False})
        admission = budget.admit_budget(policy)
        self.assertNotEqual(admission.status, budget.STATUS_ADMITTED)
        with self.assertRaises(HarnessError):
            pilot.run_live_task(Path("/nonexistent"), "ranges",
                                "opencode_go/muse-spark-1.3-contributor",
                                budget_policy=policy)


class VerifierSummaryTest(unittest.TestCase):
    """The verifier summary must report the real counts, never invented ones."""

    def test_a_passing_suite_reports_zero_exit_and_the_test_count(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            (workspace / "test_sample.py").write_text(
                "import unittest\n"
                "class T(unittest.TestCase):\n"
                "    def test_one(self):\n"
                "        self.assertTrue(True)\n"
                "    def test_two(self):\n"
                "        self.assertTrue(True)\n",
                encoding="utf-8")
            summary = pilot._run_verifier(workspace, ["python3", "-m", "unittest", "-v"], 60)
            self.assertEqual(summary["exit_code"], 0)
            self.assertEqual(summary["tests_run"], 2)
            self.assertIsNone(summary["failure_summary"])

    def test_a_failing_suite_reports_the_failure_summary(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            (workspace / "test_sample.py").write_text(
                "import unittest\n"
                "class T(unittest.TestCase):\n"
                "    def test_one(self):\n"
                "        self.assertTrue(False)\n",
                encoding="utf-8")
            summary = pilot._run_verifier(workspace, ["python3", "-m", "unittest", "-v"], 60)
            self.assertNotEqual(summary["exit_code"], 0)
            self.assertIsNotNone(summary["failure_summary"])


if __name__ == "__main__":
    unittest.main()
