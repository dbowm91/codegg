"""Focused production-path tests for scripts/check-tokio-test-flavors.py.

These tests build temporary Rust source trees and exercise the production
detection path of the guard, not just its isolated regular expressions.
They cover:

- explicit current_thread / multi_thread passes
- historical baseline identity passes
- new bare test fails
- stale baseline entry fails
- duplicate / wildcard / directory / malformed baseline entries fail
- bare test under examples/ is detected (not blanket-excluded)
- intervening #[cfg(...)] / doc comments / attributes map to correct function
- bare attribute without a following function is a hard error
- unresolved marker in baseline is rejected
- --emit-current is deterministic and sorted
- missing / unreadable baseline fails closed
"""

import importlib.util
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent.parent
GUARD_PATH = SCRIPTS_DIR / "check-tokio-test-flavors.py"
REPO_ROOT = SCRIPTS_DIR.parent


def _load_guard():
    spec = importlib.util.spec_from_file_location("check_tokio", GUARD_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class _TempTree:
    """Build an isolated temp tree mirroring a minimal Rust workspace layout."""

    def __init__(self):
        self.root = Path(tempfile.mkdtemp(prefix="codegg-tokio-guard-"))

    def write(self, rel_path: str, content: str) -> Path:
        path = self.root / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def cleanup(self):
        import shutil

        shutil.rmtree(self.root, ignore_errors=True)


class ExtractTests(unittest.TestCase):
    """Direct calls into the guard's identity extraction."""

    def setUp(self):
        self.guard = _load_guard()
        self.tree = _TempTree()

    def tearDown(self):
        self.tree.cleanup()

    def test_explicit_current_thread_pass(self):
        self.tree.write(
            "src/lib.rs",
            '#[tokio::test(flavor = "current_thread")]\nasync fn passes() {}\n',
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(identities, [])

    def test_explicit_multi_thread_pass(self):
        self.tree.write(
            "src/lib.rs",
            '#[tokio::test(flavor = "multi_thread", worker_threads = 2)]\n'
            "async fn passes() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(identities, [])

    def test_intervening_cfg_maps_to_correct_function(self):
        self.tree.write(
            "src/lib.rs",
            "/// doc comment\n"
            "//! inner doc\n"
            "#[cfg(test)]\n"
            "#[allow(dead_code)]\n"
            "#[tokio::test]\n"
            "async fn discovered() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(identities, ["src/lib.rs::discovered"])

    def test_malformed_attribute_no_function_fails_closed(self):
        self.tree.write(
            "src/lib.rs",
            "#[tokio::test]\n// intentionally no function follows\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(identities, [])
        self.assertEqual(len(errors), 1)
        self.assertIn("not followed by an unambiguous function", errors[0])

    def test_malformed_attribute_followed_by_non_fn_fails_closed(self):
        self.tree.write(
            "src/lib.rs",
            "#[tokio::test]\nconst X: u32 = 0;\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(identities, [])
        self.assertEqual(len(errors), 1)

    def test_target_directory_is_excluded(self):
        self.tree.write(
            "target/debug/lib.rs",
            "#[tokio::test]\nasync fn excluded() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(identities, [])

    def test_git_directory_is_excluded(self):
        self.tree.write(
            ".git/hooks/lib.rs",
            "#[tokio::test]\nasync fn excluded() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(identities, [])

    def test_bare_under_examples_is_detected(self):
        # Repository-owned example source must NOT be blanket-excluded.
        self.tree.write(
            "examples/plugins/sdk-rust/tests/sdk_bare.rs",
            "#[tokio::test]\nasync fn bare_in_example() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(
            identities,
            ["examples/plugins/sdk-rust/tests/sdk_bare.rs::bare_in_example"],
        )


class BaselineTests(unittest.TestCase):
    """Direct calls into the guard's baseline loader."""

    def setUp(self):
        self.guard = _load_guard()

    def _write_baseline(self, content: str) -> Path:
        fd, name = tempfile.mkstemp(prefix="codegg-tokio-baseline-", suffix=".txt")
        os.close(fd)
        path = Path(name)
        path.write_text(content)
        return path

    def test_valid_baseline_parses(self):
        path = self._write_baseline("a::b\nc::d\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(errors, [])
        self.assertEqual(entries, ["a::b", "c::d"])

    def test_duplicate_entry_rejected(self):
        path = self._write_baseline("a::b\na::b\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        # First occurrence is accepted; the second is rejected.
        self.assertEqual(entries, ["a::b"])
        self.assertTrue(any("duplicate entry" in e for e in errors))

    def test_wildcard_entry_rejected(self):
        path = self._write_baseline("a::*\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(entries, [])
        self.assertTrue(any("wildcard" in e for e in errors))

    def test_directory_suppression_rejected(self):
        path = self._write_baseline("examples/\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(entries, [])
        self.assertTrue(any("wildcard or directory" in e for e in errors))

    def test_missing_separator_rejected(self):
        path = self._write_baseline("abc\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(entries, [])
        self.assertTrue(any("missing '::'" in e for e in errors))

    def test_unresolved_marker_rejected(self):
        path = self._write_baseline("a::???UNRESOLVED_LINE_5\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(entries, [])
        self.assertTrue(any("unresolved baseline identity" in e for e in errors))

    def test_comments_and_blank_lines_skipped(self):
        path = self._write_baseline("# comment\n\na::b\n")
        entries, errors = self.guard.load_baseline(path)
        path.unlink()
        self.assertEqual(errors, [])
        self.assertEqual(entries, ["a::b"])

    def test_missing_baseline_file_rejected(self):
        entries, errors = self.guard.load_baseline(Path("/nonexistent/path.txt"))
        self.assertEqual(entries, [])
        self.assertTrue(any("not found" in e for e in errors))

    def test_unreadable_baseline_file_rejected(self):
        path = self._write_baseline("a::b\n")
        os.chmod(path, 0)
        entries, errors = self.guard.load_baseline(path)
        try:
            os.chmod(path, stat.S_IRUSR | stat.S_IWUSR)
            path.unlink()
        except OSError:
            pass
        self.assertEqual(entries, [])
        self.assertTrue(any("unreadable" in e or "not found" in e for e in errors))


class DiffTests(unittest.TestCase):
    """Verify new vs stale baseline comparison."""

    def setUp(self):
        self.guard = _load_guard()

    def test_new_violation_detected(self):
        new, stale = self.guard.diff_identities(
            current=["a.rs::new_violation"],
            baseline=["a.rs::old"],
        )
        self.assertEqual(new, ["a.rs::new_violation"])
        self.assertEqual(stale, ["a.rs::old"])

    def test_no_changes(self):
        new, stale = self.guard.diff_identities(
            current=["a.rs::t"],
            baseline=["a.rs::t"],
        )
        self.assertEqual(new, [])
        self.assertEqual(stale, [])


class EmitCurrentTests(unittest.TestCase):
    """Verify --emit-current is deterministic and sorted."""

    def setUp(self):
        self.guard = _load_guard()
        self.tree = _TempTree()

    def tearDown(self):
        self.tree.cleanup()

    def test_emit_current_is_sorted_and_deterministic(self):
        # Two bare tests in different files; should be sorted by file then function.
        self.tree.write(
            "b.rs",
            "#[tokio::test]\nasync fn z_last() {}\n"
            "#[tokio::test]\nasync fn a_first() {}\n",
        )
        self.tree.write(
            "a.rs",
            "#[tokio::test]\nasync fn middle() {}\n",
        )
        identities, errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(errors, [])
        self.assertEqual(
            identities,
            [
                "a.rs::middle",
                "b.rs::a_first",
                "b.rs::z_last",
            ],
        )
        # Re-extract and assert the output is byte-identical.
        again, again_errors = self.guard.collect_current_identities(self.tree.root)
        self.assertEqual(again_errors, [])
        self.assertEqual(again, identities)


class RepositoryIntegrationTests(unittest.TestCase):
    """Run the guard against the actual repository baseline."""

    def setUp(self):
        self.guard = _load_guard()
        self.baseline = REPO_ROOT / "scripts" / "tokio-test-flavor-baseline.txt"

    def test_repository_baseline_passes_against_current_head(self):
        # The committed baseline must match the current bare-test identities
        # at the head under test.
        current, current_errors = self.guard.collect_current_identities(REPO_ROOT)
        self.assertEqual(current_errors, [], msg=f"source errors: {current_errors}")
        baseline, baseline_errors = self.guard.load_baseline(self.baseline)
        self.assertEqual(baseline_errors, [], msg=f"baseline errors: {baseline_errors}")
        new, stale = self.guard.diff_identities(current, baseline)
        self.assertEqual(new, [], msg=f"new violations: {new}")
        self.assertEqual(stale, [], msg=f"stale entries: {stale}")


if __name__ == "__main__":
    unittest.main()
