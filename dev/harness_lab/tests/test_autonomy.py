"""Offline tests for the versioned agent-autonomy benchmark taxonomy.

Standard library only. No provider and no network: these tests parse the
frozen manifest and check that the loader refuses everything the taxonomy
contract forbids. The one exception is the materialized reference case,
whose verifier is run as a local subprocess to prove it fails while the task
is unsolved and passes once the defect is repaired.
"""
from __future__ import annotations

import ast
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from dev.harness_lab import autonomy
from dev.harness_lab.autonomy import (AUTONOMY_SCHEMA_VERSION, SUPPORTED_FAMILIES,
                                      SUPPORT_DEFERRED, SUPPORT_SUPPORTED,
                                      VERIFIER_KINDS, AutonomyCase, AutonomySuite,
                                      load_autonomy_suite, require_supported,
                                      suite_digest, verify_isolation)

MANIFEST_PATH = Path(__file__).parents[1] / "fixtures" / "agent_autonomy" / "cases.jsonl"
MODULE_PATH = Path(__file__).parents[1] / "autonomy.py"

#: The harness's own trees. A hidden verifier must not live inside one of them.
HARNESS_OWN_PREFIXES = ("dev/harness_lab/", "plans/", "tests/")

#: Modules the loader must never import: it parses, it never talks or runs.
FORBIDDEN_IMPORTS = frozenset({"socket", "http", "urllib", "requests", "subprocess", "asyncio"})


def _case(**overrides):
    record = {
        "schema_version": 1,
        "case_id": "synthetic-case",
        "family": "read_search_patch",
        "group_id": "synthetic-group",
        "title": "Synthetic case",
        "prompt": "Do the synthetic thing.",
        "verifier": {"kind": "script", "command": ["python3", "-m", "unittest"],
                     "cwd": "dev/autonomy_fixtures/synthetic-case"},
        "verifier_scope": "dev/autonomy_fixtures/synthetic-case/verifier",
        "budget": {"max_provider_requests": 4, "max_tool_calls": 10,
                   "max_wallclock_seconds": 60, "max_cost_usd": None},
        "abort": {"timeout_seconds": 60, "stall_seconds": 20},
        "allowed_tools": ["read", "bash"],
        "network": "none",
        "protected": False,
        "support": "supported",
        "deferral_reason": None,
    }
    record.update(overrides)
    return record


class ManifestFixture:
    """A temporary manifest directory the loader can be pointed at."""

    def __init__(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)

    def cleanup(self) -> None:
        self._directory.cleanup()

    def write(self, records) -> Path:
        path = self.root / "cases.jsonl"
        body = "".join(json.dumps(record, sort_keys=True) + "\n" for record in records)
        path.write_text(body, encoding="utf-8")
        return path

    def write_bytes(self, payload: bytes) -> Path:
        path = self.root / "cases.jsonl"
        path.write_bytes(payload)
        return path

    def load(self, records):
        self.write(records)
        return load_autonomy_suite(self.root)


class ShippedManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.suite = load_autonomy_suite()

    def test_shipped_manifest_loads_is_versioned_and_matches_its_digest(self) -> None:
        self.assertEqual(self.suite.version, AUTONOMY_SCHEMA_VERSION)
        self.assertEqual(self.suite.version, 1)
        self.assertTrue(self.suite.cases)
        self.assertEqual(self.suite.suite_sha256, suite_digest())
        self.assertEqual(len(self.suite.suite_sha256), 64)
        int(self.suite.suite_sha256, 16)

    def test_manifest_covers_every_supported_family_with_at_least_one_case(self) -> None:
        families = {case.family for case in self.suite.cases}
        self.assertEqual(families, set(SUPPORTED_FAMILIES))
        for family in SUPPORTED_FAMILIES:
            members = [case for case in self.suite.cases if case.family == family]
            self.assertGreaterEqual(len(members), 1, family)

    def test_case_ids_are_unique_and_every_case_is_versioned(self) -> None:
        identifiers = [case.case_id for case in self.suite.cases]
        self.assertEqual(len(set(identifiers)), len(identifiers))
        self.assertEqual(len(self.suite.cases), len(self.suite.as_dict()["cases"]))

    def test_repeated_tool_calls_group_pairs_a_task_with_a_protected_control(self) -> None:
        members = [case for case in self.suite.cases if case.family == "repeated_tool_calls"]
        self.assertGreaterEqual(len(members), 2)
        protected = [case for case in members if case.protected]
        tasks = [case for case in members if not case.protected]
        self.assertEqual(len(protected), 1)
        self.assertTrue(tasks)
        control = protected[0]
        self.assertEqual(control.group_id, tasks[0].group_id)
        self.assertIn("stall", control.title.lower())

    def test_supported_cases_declare_an_independently_executable_verifier(self) -> None:
        self.assertTrue(self.suite.supported_cases())
        for case in self.suite.supported_cases():
            command = case.verifier["command"]
            self.assertIsInstance(command, list, case.case_id)
            self.assertTrue(command, case.case_id)
            for argument in command:
                self.assertIsInstance(argument, str, case.case_id)
                self.assertTrue(argument, case.case_id)
            self.assertIn(case.verifier["kind"], VERIFIER_KINDS)
            self.assertTrue(case.verifier["cwd"].strip(), case.case_id)
            self.assertTrue(case.verifier_scope.strip(), case.case_id)
            for prefix in HARNESS_OWN_PREFIXES:
                self.assertFalse(case.verifier_scope.startswith(prefix),
                                 f"{case.case_id} verifier_scope {case.verifier_scope} "
                                 f"is under the harness tree {prefix}")

    def test_every_case_declares_its_network_posture_explicitly(self) -> None:
        for case in self.suite.cases:
            self.assertIsNotNone(case.network, case.case_id)
            if isinstance(case.network, str):
                self.assertEqual(case.network, "none", case.case_id)
            else:
                self.assertTrue(case.network["endpoints"], case.case_id)

    def test_deferred_cases_carry_a_reason_and_supported_cases_do_not(self) -> None:
        self.assertTrue(self.suite.deferred_cases())
        for case in self.suite.deferred_cases():
            self.assertEqual(case.support, SUPPORT_DEFERRED)
            self.assertTrue((case.deferral_reason or "").strip(), case.case_id)
        for case in self.suite.supported_cases():
            self.assertEqual(case.support, SUPPORT_SUPPORTED)
            self.assertIsNone(case.deferral_reason, case.case_id)

    def test_supported_and_deferred_partition_the_suite(self) -> None:
        self.assertEqual(len(self.suite.supported_cases()) + len(self.suite.deferred_cases()),
                         len(self.suite.cases))
        for case in self.suite.deferred_cases():
            self.assertNotIn(case, self.suite.supported_cases())


class IsolationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = ManifestFixture()
        self.addCleanup(self.fixture.cleanup)

    def test_verify_isolation_refuses_a_verifier_nested_under_a_candidate_prefix(self) -> None:
        suite = self.fixture.load([_case(verifier_scope="crates/codegg-core/verifier")])
        with self.assertRaises(ValueError) as raised:
            verify_isolation(suite, ("crates/",))
        self.assertIn("verifier_scope", str(raised.exception))
        self.assertIn("candidate-editable prefix", str(raised.exception))

    def test_verify_isolation_refuses_a_verifier_scope_equal_to_a_candidate_prefix(self) -> None:
        suite = self.fixture.load([_case(verifier_scope="crates/verifier")])
        with self.assertRaises(ValueError) as raised:
            verify_isolation(suite, ("crates/verifier",))
        self.assertIn("candidate-editable prefix", str(raised.exception))

    def test_verify_isolation_is_path_segment_aware(self) -> None:
        suite = self.fixture.load([_case(verifier_scope="crates-extra/verifier")])
        verify_isolation(suite, ("crates/",))

    def test_verify_isolation_allows_a_verifier_outside_the_candidate_prefixes(self) -> None:
        suite = self.fixture.load([_case()])
        verify_isolation(suite, ("src/", "crates/"))

    def test_verify_isolation_refuses_the_shipped_suite_when_dev_is_candidate_editable(self) -> None:
        suite = load_autonomy_suite()
        verify_isolation(suite, ("src/", "crates/"))
        with self.assertRaises(ValueError):
            verify_isolation(suite, ("dev/",))

    def test_verify_isolation_refuses_an_empty_candidate_prefix(self) -> None:
        suite = self.fixture.load([_case()])
        with self.assertRaises(ValueError):
            verify_isolation(suite, ("  ",))


class ValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = ManifestFixture()
        self.addCleanup(self.fixture.cleanup)

    def _refused(self, records, fragment: str) -> str:
        with self.assertRaises(ValueError) as raised:
            self.fixture.load(records)
        message = str(raised.exception)
        self.assertIn(fragment, message)
        return message

    def test_missing_required_field_is_refused(self) -> None:
        record = _case()
        del record["title"]
        self._refused([record], "is missing required field(s): title")

    def test_missing_network_field_is_refused_not_silently_none(self) -> None:
        record = _case()
        del record["network"]
        message = self._refused([record], "is missing required field(s): network")
        self.assertNotIn("none", message)

    def test_unknown_field_is_refused(self) -> None:
        self._refused([_case(bogus="typo")], "has unknown field(s): bogus")

    def test_duplicate_case_id_is_refused(self) -> None:
        self._refused([_case(), _case()], "duplicate autonomy case_id: 'synthetic-case'")

    def test_unsupported_schema_version_is_refused(self) -> None:
        self._refused([_case(schema_version=2)], "unsupported autonomy schema_version: 2")

    def test_unsupported_family_is_refused(self) -> None:
        self._refused([_case(family="telepathy")], "unsupported autonomy family: 'telepathy'")

    def test_unsupported_support_mode_is_refused(self) -> None:
        self._refused([_case(support="maybe")], "unsupported autonomy support mode: 'maybe'")

    def test_deferred_case_without_a_reason_is_refused(self) -> None:
        self._refused([_case(support="deferred", deferral_reason=None)],
                      "requires a non-empty deferral_reason")
        self._refused([_case(support="deferred", deferral_reason="   ")],
                      "requires a non-empty deferral_reason")

    def test_supported_case_with_a_reason_is_refused(self) -> None:
        self._refused([_case(support="supported", deferral_reason="because")],
                      "must not carry a deferral_reason")

    def test_non_boolean_protected_is_refused(self) -> None:
        self._refused([_case(protected="yes")], "protected must be a boolean")

    def test_empty_allowed_tools_is_refused(self) -> None:
        self._refused([_case(allowed_tools=[])], "requires a non-empty allowed_tools list")

    def test_invalid_budget_values_are_refused(self) -> None:
        for value, fragment in ((0, "must be a positive integer"),
                                (-1, "must be a positive integer"),
                                (1.5, "must be an integer"),
                                (True, "must be an integer"),
                                ("10", "must be an integer")):
            with self.subTest(max_provider_requests=value):
                self._refused([_case(budget={"max_provider_requests": value,
                                             "max_tool_calls": 10,
                                             "max_wallclock_seconds": 60,
                                             "max_cost_usd": None})],
                              f"budget max_provider_requests {fragment}")
        for value, fragment in ((0, "must be a positive integer"),
                                (-1, "must be a positive integer"),
                                (2.0, "must be an integer"),
                                ("10", "must be an integer")):
            with self.subTest(max_tool_calls=value):
                self._refused([_case(budget={"max_provider_requests": 4,
                                             "max_tool_calls": value,
                                             "max_wallclock_seconds": 60,
                                             "max_cost_usd": None})],
                              f"budget max_tool_calls {fragment}")
        for value, fragment in ((0, "must be a positive integer"),
                                (-1, "must be a positive integer"),
                                (2.0, "must be an integer"),
                                ("60", "must be an integer")):
            with self.subTest(max_wallclock_seconds=value):
                self._refused([_case(budget={"max_provider_requests": 4,
                                             "max_tool_calls": 10,
                                             "max_wallclock_seconds": value,
                                             "max_cost_usd": None})],
                              f"budget max_wallclock_seconds {fragment}")

    def test_invalid_cost_budget_values_are_refused(self) -> None:
        for value in (-1.0, float("inf"), float("nan"), True, "1.0"):
            with self.subTest(max_cost_usd=value):
                self._refused([_case(budget={"max_provider_requests": 4,
                                             "max_tool_calls": 10,
                                             "max_wallclock_seconds": 60,
                                             "max_cost_usd": value})],
                              "budget max_cost_usd must be a non-negative finite number or null")

    def test_valid_cost_budget_is_accepted(self) -> None:
        suite = self.fixture.load([_case(budget={"max_provider_requests": 4,
                                                  "max_tool_calls": 10,
                                                  "max_wallclock_seconds": 60,
                                                  "max_cost_usd": 0.5})])
        self.assertEqual(suite.case("synthetic-case").budget["max_cost_usd"], 0.5)

    def test_network_must_be_none_or_an_explicit_disclosure(self) -> None:
        self._refused([_case(network="maybe")],
                      'network must be "none" or an object with a non-empty endpoints list')
        self._refused([_case(network={"endpoints": []})],
                      "network requires a non-empty endpoints list")
        self._refused([_case(network={"reason": "no endpoints listed"})],
                      "network is missing required field(s): endpoints")

    def test_explicit_network_disclosure_is_accepted(self) -> None:
        suite = self.fixture.load([_case(network={"endpoints": ["https://example.invalid/api"],
                                                    "reason": "the task needs the API"})])
        case = suite.case("synthetic-case")
        self.assertEqual(case.network["endpoints"], ["https://example.invalid/api"])

    def test_verifier_without_a_command_list_is_refused(self) -> None:
        self._refused([_case(verifier={"kind": "script", "cwd": "dev/autonomy_fixtures/x"})],
                      "verifier is missing required field(s): command")
        self._refused([_case(verifier={"kind": "script", "command": "python3 -m unittest",
                                       "cwd": "dev/autonomy_fixtures/x"})],
                      "verifier requires a command list of strings")
        self._refused([_case(verifier={"kind": "script", "command": ["python3", 7],
                                       "cwd": "dev/autonomy_fixtures/x"})],
                      "verifier requires a command list of strings")

    def test_unknown_verifier_kind_is_refused(self) -> None:
        self._refused([_case(verifier={"kind": "magic", "command": ["python3"],
                                       "cwd": "dev/autonomy_fixtures/x"})],
                      "unsupported autonomy verifier kind: 'magic'")

    def test_unknown_verifier_field_is_refused(self) -> None:
        self._refused([_case(verifier={"kind": "script", "command": ["python3"],
                                       "cwd": "dev/autonomy_fixtures/x", "shell": True})],
                      "verifier has unknown field(s): shell")

    def test_invalid_abort_criteria_are_refused(self) -> None:
        self._refused([_case(abort={"timeout_seconds": 0, "stall_seconds": 20})],
                      "abort timeout_seconds must be a positive integer")
        self._refused([_case(abort={"timeout_seconds": 60, "stall_seconds": 600})],
                      "abort stall_seconds must not exceed timeout_seconds")

    def test_non_kebab_case_identifier_is_refused(self) -> None:
        self._refused([_case(case_id="Synthetic Case")], "case_id must be a kebab-case identifier")

    def test_manifest_without_a_case_is_refused(self) -> None:
        self._refused([], "autonomy manifest contains no cases")

    def test_malformed_json_line_is_refused(self) -> None:
        self.fixture.write_bytes(b"{not json}\n")
        with self.assertRaises(ValueError) as raised:
            load_autonomy_suite(self.fixture.root)
        self.assertIn("is not valid JSON", str(raised.exception))


class DeferredCaseTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = ManifestFixture()
        self.addCleanup(self.fixture.cleanup)

    def test_deferred_case_is_excluded_from_supported_and_refused_by_name(self) -> None:
        deferred = _case(case_id="deferred-case", support="deferred",
                         deferral_reason="the feature milestone is not closed")
        suite = self.fixture.load([_case(case_id="supported-case"), deferred])
        self.assertEqual([case.case_id for case in suite.supported_cases()], ["supported-case"])
        self.assertEqual([case.case_id for case in suite.deferred_cases()], ["deferred-case"])
        self.assertNotIn(suite.case("deferred-case"), suite.supported_cases())
        with self.assertRaises(ValueError) as raised:
            require_supported(suite.case("deferred-case"))
        message = str(raised.exception)
        self.assertIn("deferred-case", message)
        self.assertIn("the feature milestone is not closed", message)
        with self.assertRaises(ValueError):
            suite.require_supported("deferred-case")

    def test_require_supported_returns_a_supported_case(self) -> None:
        suite = self.fixture.load([_case()])
        self.assertIs(require_supported(suite.case("synthetic-case")),
                      suite.case("synthetic-case"))

    def test_shipped_deferred_cases_are_refused_with_their_recorded_reason(self) -> None:
        suite = load_autonomy_suite()
        self.assertTrue(suite.deferred_cases())
        for case in suite.deferred_cases():
            with self.assertRaises(ValueError) as raised:
                require_supported(case)
            self.assertIn(case.deferral_reason, str(raised.exception))


class DigestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = ManifestFixture()
        self.addCleanup(self.fixture.cleanup)

    def test_changed_manifest_bytes_change_the_digest(self) -> None:
        shipped = MANIFEST_PATH.read_bytes()
        self.fixture.write_bytes(shipped)
        copied = load_autonomy_suite(self.fixture.root)
        self.assertEqual(copied.suite_sha256, suite_digest(self.fixture.root))
        self.assertEqual(copied.suite_sha256, suite_digest())

        self.fixture.write_bytes(shipped + b" ")
        self.assertNotEqual(suite_digest(self.fixture.root), copied.suite_sha256)

        changed = shipped.replace(b'"title":"Repair a Rust crate',
                                  b'"title":"Repair the Rust crate')
        self.assertNotEqual(changed, shipped)
        self.fixture.write_bytes(changed)
        edited = load_autonomy_suite(self.fixture.root)
        self.assertNotEqual(edited.suite_sha256, copied.suite_sha256)
        self.assertEqual(len(edited.cases), len(copied.cases))

    def test_digest_does_not_depend_on_parsing(self) -> None:
        suite = load_autonomy_suite()
        self.assertEqual(suite.suite_sha256, suite_digest())
        self.assertEqual(len(suite.suite_sha256), 64)


class SerializationTests(unittest.TestCase):
    def test_suite_and_case_views_are_json_serializable(self) -> None:
        suite = load_autonomy_suite()
        encoded = json.dumps(suite.as_dict(), sort_keys=True)
        self.assertIn(suite.suite_sha256, encoded)
        decoded = json.loads(encoded)
        self.assertEqual(decoded["version"], AUTONOMY_SCHEMA_VERSION)
        self.assertEqual(len(decoded["cases"]), len(suite.cases))
        for case in suite.cases:
            payload = json.dumps(case.as_dict(), sort_keys=True)
            self.assertIn(case.case_id, payload)
            self.assertEqual(json.loads(payload)["allowed_tools"], list(case.allowed_tools))


class ModuleBoundaryTests(unittest.TestCase):
    def test_module_imports_no_network_or_process_modules(self) -> None:
        tree = ast.parse(MODULE_PATH.read_text(encoding="utf-8"))
        imported = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    imported.add(alias.name.split(".")[0])
            elif isinstance(node, ast.ImportFrom) and node.module:
                imported.add(node.module.split(".")[0])
        self.assertEqual(imported & FORBIDDEN_IMPORTS, set())
        for name in FORBIDDEN_IMPORTS:
            self.assertFalse(hasattr(autonomy, name), name)

    def test_public_api_is_exported(self) -> None:
        for name in ("AUTONOMY_SCHEMA_VERSION", "SUPPORTED_FAMILIES", "AutonomyCase",
                     "AutonomySuite", "load_autonomy_suite", "suite_digest",
                     "verify_isolation", "require_supported"):
            self.assertTrue(hasattr(autonomy, name), name)
        self.assertEqual(autonomy.AUTONOMY_SCHEMA_VERSION, 1)
        self.assertEqual(autonomy.SUPPORTED_FAMILIES,
                         ("read_search_patch", "compile_debug", "background_job",
                          "repeated_tool_calls", "delegation", "integration"))

    def test_suite_lookup_of_an_unknown_case_raises_key_error(self) -> None:
        suite = load_autonomy_suite()
        with self.assertRaises(KeyError):
            suite.case("no-such-case")

    def test_dataclasses_are_frozen(self) -> None:
        suite = load_autonomy_suite()
        case = suite.cases[0]
        self.assertIsInstance(case, AutonomyCase)
        self.assertIsInstance(suite, AutonomySuite)
        with self.assertRaises(Exception):
            case.case_id = "mutated"
        with self.assertRaises(Exception):
            suite.version = 2


if __name__ == "__main__":
    unittest.main()


class MaterializedCaseTests(unittest.TestCase):
    """The reference case is materialized, so its verifier is a standing guard.

    The taxonomy freezes where and how each verifier runs. This case has its
    fixture and verifier written out, so the two properties that make a
    verifier trustworthy are proven rather than asserted: it FAILS while the
    task is unsolved, and it PASSES once the defect is repaired.
    """

    CASE_ROOT = pathlib.Path(__file__).resolve().parents[2] / "autonomy_fixtures" / "read-search-patch-python"

    def _run_verifier(self, root: pathlib.Path) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", "verifier"],
                              cwd=str(root), capture_output=True, text=True, timeout=120)

    def test_the_case_fixture_and_verifier_are_present(self) -> None:
        self.assertTrue(self.CASE_ROOT.is_dir())
        self.assertTrue((self.CASE_ROOT / "fixture" / "src" / "parser.py").is_file())
        self.assertTrue((self.CASE_ROOT / "verifier" / "test_parser_contract.py").is_file())

    def test_the_verifier_fails_while_the_task_is_unsolved(self) -> None:
        result = self._run_verifier(self.CASE_ROOT)
        self.assertNotEqual(result.returncode, 0,
                            "a verifier that passes on the unsolved baseline proves nothing")

    def test_the_verifier_passes_once_the_defect_is_repaired(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            case = pathlib.Path(temp) / "case"
            shutil.copytree(self.CASE_ROOT, case)
            parser = case / "fixture" / "src" / "parser.py"
            text = parser.read_text(encoding="utf-8")
            self.assertIn('while fields and fields[-1] == "":', text)
            parser.write_text(text.replace(
                '    fields = split_fields(line)\n'
                '    while fields and fields[-1] == "":\n'
                '        fields.pop()\n',
                '    fields = split_fields(line)\n'), encoding="utf-8")
            result = self._run_verifier(case)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_the_verifier_refuses_an_edited_frozen_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            case = pathlib.Path(temp) / "case"
            shutil.copytree(self.CASE_ROOT, case)
            frozen = case / "fixture" / "tests" / "test_parser.py"
            frozen.write_text(frozen.read_text(encoding="utf-8") + "\n# weakened\n", encoding="utf-8")
            result = self._run_verifier(case)
            self.assertNotEqual(result.returncode, 0,
                                "editing the provided test must not be accepted")

    def test_the_verifier_scope_is_outside_the_candidate_editable_trees(self) -> None:
        scope = "dev/autonomy_fixtures/read-search-patch-python/verifier"
        for prefix in ("dev/harness_lab/", "plans/", "tests/"):
            self.assertFalse(scope.startswith(prefix), f"{scope} must not live under {prefix}")
