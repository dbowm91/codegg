#!/usr/bin/env python3
"""Static guard: the recorded-session replay corpus cannot rot silently.

The replay tier (`tests/session_replay/`) drives the shipped agent loop from a
committed fixture with no key and no network. That makes the fixture corpus an
input to a test, so it needs the same discipline as the code around it:

- **Every fixture declares a supported `format`.** An unreadable document is a
  scenario that silently stops being a scenario.
- **Every fixture is bound to exactly one family member.** A *scenario* is run
  by exactly one member; a *decode* fixture is loaded by exactly one member and
  is never driven. A fixture nobody binds is dead weight, and a scenario two
  members run is two tests asserting one thing.
- **Every member binds a fixture that exists.** A reference to a missing file
  compiles and fails at run time, which is later and noisier.
- **No fixture exceeds the size bound.** One turn, text only. A fixture that
  outgrows the bound has become a different artifact, and the answer is a
  smaller scenario rather than a raised bound.
- **No fixture carries a credential-shaped string or an absolute home path.**
  A committed fixture is a public artifact.
- **The replay reader and recorder are referenced only from the test tree.**
  They are compiled into integration test binaries alone; a reference from
  `src/` or a crate's `src/` would make test scaffolding a production
  dependency.
- **The family is not `required-features`-gated.** Nine root integration
  targets are gated today and no CI workflow enables those features, so a gated
  family is never compiled there. This one must run in the routine sweep.

The Rust-side scan uses `scripts/rust_test_scan.py`, the shared Rust-aware
scanner: a regular expression over Rust source misreads lifetimes as character
literals and undercounts `tests/` by 76%. Only the *call* that binds a fixture
is matched against a test body's scanner view.

Modes:
    python3 scripts/check_session_replay_fixtures.py             # static guard
    python3 scripts/check_session_replay_fixtures.py --self-test # non-vacuity

`--self-test` copies the corpus, injects one defect of each class, and requires
the guard to reject every one of them while accepting the unmodified copy.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

FAMILY_DIR = Path("tests/session_replay")
FIXTURE_DIR = FAMILY_DIR / "fixtures"
CARGO_MANIFEST = Path("Cargo.toml")

# The fixture-format version this reader understands, and the byte bound a
# single fixture may occupy. Both mirror `tests/common/session_replay.rs`.
SUPPORTED_FORMAT = 1
FIXTURE_SIZE_BOUND = 8 * 1024

# A scenario is run by exactly one member through this call.
RUN_SCENARIO = re.compile(r'\brun_scenario\s*\(\s*"([a-z0-9_]+)"\s*\)')
# A decode fixture (and, for a scenario, a perturbation test) loads it here.
LOAD_FIXTURE = re.compile(r'\bReplayFixture\s*::\s*named\s*\(\s*"([a-z0-9_]+)"\s*\)')

KINDS = ("scenario", "decode")

# Credential shapes a committed fixture must never carry. Deliberately narrow:
# a broad "secret-looking" rule would reject ordinary transcript text.
CREDENTIAL_PATTERNS = (
    ("sk-", re.compile(r"sk-[A-Za-z0-9_-]{16,}")),
    ("aws-access-key", re.compile(r"AKIA[0-9A-Z]{16}")),
    ("github-token", re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}")),
    ("slack-token", re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}")),
    ("private-key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    (
        "assigned-secret",
        re.compile(
            r'(?i)(?:api[_-]?key|secret|access[_-]?token|password)"\s*:\s*"[A-Za-z0-9_\-]{16,}"'
        ),
    ),
)

# Absolute home paths, in the three shapes a developer machine produces.
HOME_PATH_PATTERNS = (
    ("unix-home", re.compile(r"/Users/[A-Za-z0-9._-]+/")),
    ("linux-home", re.compile(r"/home/[A-Za-z0-9._-]+/")),
    ("windows-home", re.compile(r"[A-Za-z]:\\\\Users\\\\[A-Za-z0-9._-]+")),
)

# Symbols that exist only in the shared test tree. A production module that
# names one would compile against test scaffolding.
READER_SYMBOLS = ("ReplayFixture", "RecordingProvider", "replay_recorder", "session_replay")

# Production source trees. `tests/` is excluded by path, not by prefix list, so
# a crate's own test tree stays out of scope.
PRODUCTION_TREES = ("src", "crates", "apps")


def _fixture_paths(root: Path) -> list[Path]:
    directory = root / FIXTURE_DIR
    if not directory.is_dir():
        return []
    return sorted(directory.glob("*.json"))


def _family_sources(root: Path) -> list[Path]:
    directory = root / FAMILY_DIR
    if not directory.is_dir():
        return []
    return sorted({path for pattern in ("*.rs", "*.inc") for path in directory.glob(pattern)})


def _family_members(root: Path) -> list[tuple[str, str, str]]:
    """Every test function in the family, as `(rel, name, body_code)`."""
    members: list[tuple[str, str, str]] = []
    for path in _family_sources(root):
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(root).as_posix()
        for function in rts.find_test_functions(text, path, rel, "tests"):
            members.append((rel, function.name, function.body_code))
    return members


def _is_production_source(rel: str) -> bool:
    parts = rel.split("/")
    if "tests" in parts:
        return False
    return parts[0] in PRODUCTION_TREES


def check_corpus(root: Path) -> list[str]:
    """Every fixture is readable, bounded, versioned and safe to commit."""
    violations: list[str] = []
    paths = _fixture_paths(root)
    if not paths:
        return [f"{FIXTURE_DIR.as_posix()}: no fixtures found"]

    for path in paths:
        rel = path.relative_to(root).as_posix()
        raw = path.read_text(encoding="utf-8")
        size = len(raw.encode("utf-8"))
        if size > FIXTURE_SIZE_BOUND:
            violations.append(
                f"{rel}: {size} bytes exceeds the {FIXTURE_SIZE_BOUND}-byte fixture bound"
            )
        try:
            document = json.loads(raw)
        except json.JSONDecodeError as error:
            violations.append(f"{rel}: not valid JSON: {error}")
            continue
        if not isinstance(document, dict):
            violations.append(f"{rel}: a fixture must be a JSON object")
            continue
        if document.get("format") != SUPPORTED_FORMAT:
            violations.append(
                f"{rel}: format {document.get('format')!r} is not the supported "
                f"format {SUPPORTED_FORMAT}"
            )
        if document.get("id") != path.stem:
            violations.append(
                f"{rel}: fixture id {document.get('id')!r} does not match its file "
                f"stem {path.stem!r}"
            )
        if document.get("kind", "scenario") not in KINDS:
            violations.append(
                f"{rel}: kind {document.get('kind')!r} is not one of {', '.join(KINDS)}"
            )
        for label, pattern in CREDENTIAL_PATTERNS:
            if pattern.search(raw):
                violations.append(
                    f"{rel}: contains a credential-shaped string ({label}); a "
                    "committed fixture is a public artifact"
                )
                break
        for label, pattern in HOME_PATH_PATTERNS:
            if pattern.search(raw):
                violations.append(
                    f"{rel}: contains an absolute home path ({label}); fixtures "
                    "carry repository-relative paths only"
                )
                break
    return violations


def check_references(root: Path) -> list[str]:
    """Every fixture is bound by exactly one member, and every binding resolves."""
    violations: list[str] = []
    paths = {path.stem: path for path in _fixture_paths(root)}
    runs: dict[str, list[tuple[str, str]]] = {stem: [] for stem in paths}
    loads: dict[str, list[tuple[str, str]]] = {stem: [] for stem in paths}
    dangling: list[str] = []

    for rel, name, body in _family_members(root):
        for match in RUN_SCENARIO.finditer(body):
            stem = match.group(1)
            if stem not in paths:
                dangling.append(f"{rel}::{name} runs `{stem}`, which is not a fixture in {FIXTURE_DIR.as_posix()}")
            else:
                runs[stem].append((rel, name))
        for match in LOAD_FIXTURE.finditer(body):
            stem = match.group(1)
            if stem not in paths:
                dangling.append(
                    f"{rel}::{name} loads `{stem}`, which is not a fixture in {FIXTURE_DIR.as_posix()}"
                )
            else:
                loads[stem].append((rel, name))

    violations.extend(dangling)

    for stem, path in sorted(paths.items()):
        rel = path.relative_to(root).as_posix()
        kind = json.loads(path.read_text(encoding="utf-8")).get("kind", "scenario")
        bound_to = runs[stem] if kind == "scenario" else loads[stem]
        other = loads[stem] if kind == "scenario" else runs[stem]
        if not bound_to:
            violations.append(
                f"{rel}: no family member binds this {kind} fixture; an unbound "
                f"fixture is a scenario that silently stopped being one "
                f"(a {kind} fixture must be bound by exactly one member)"
            )
        elif len(bound_to) > 1:
            members = ", ".join(f"{file}::{name}" for file, name in bound_to)
            violations.append(
                f"{rel}: bound by {len(bound_to)} family members ({members}); a "
                f"{kind} fixture must be bound by exactly one"
            )
        if kind == "decode" and other:
            members = ", ".join(f"{file}::{name}" for file, name in other)
            violations.append(
                f"{rel}: declared `decode` but run as a scenario by {members}; a "
                "decode fixture pins the document format and is never driven"
            )
    return violations


def check_production_reachability(root: Path) -> list[str]:
    """The replay reader and recorder are named only from the test tree."""
    violations: list[str] = []
    for tree in PRODUCTION_TREES:
        base = root / tree
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if not _is_production_source(rel):
                continue
            text = path.read_text(encoding="utf-8", errors="ignore")
            # Scan the *production* view, not the whole file. `#[cfg(test)]`
            # modules are never compiled into the shipped binary, so a test
            # double that happens to share a name with the shared tree is not a
            # dependency on it -- matching the bare name anywhere made every
            # such coincidence a violation. Comments and literals are blanked
            # as before: a doc comment naming the tier is documentation, not a
            # dependency.
            masked = rts.scan_source(rts.production_view(text)).mask()
            for symbol in READER_SYMBOLS:
                if re.search(rf"\b{re.escape(symbol)}\b", masked):
                    violations.append(
                        f"{rel}: names `{symbol}`, which exists only in the shared "
                        "integration-test tree; the replay tier must not be "
                        "reachable from a production target"
                    )
                    break
    return violations


def check_target_declaration(root: Path) -> list[str]:
    """The family is declared, and deliberately not `required-features`-gated."""
    manifest = root / CARGO_MANIFEST
    if not manifest.is_file():
        return [f"{CARGO_MANIFEST.as_posix()}: missing"]
    text = manifest.read_text(encoding="utf-8")
    pattern = re.compile(
        r"\[\[test\]\]\s*\n\s*name\s*=\s*\"session_replay\"(.*?)(?=\n\[\[|\Z)", re.S
    )
    match = pattern.search(text)
    if match is None:
        return [
            f"{CARGO_MANIFEST.as_posix()}: no `[[test]]` target named `session_replay`; "
            "the family would not be compiled at all"
        ]
    block = match.group(1)
    if "required-features" in block:
        return [
            f"{CARGO_MANIFEST.as_posix()}: the `session_replay` target is "
            "`required-features`-gated. Nine root targets are already gated and no "
            "CI workflow enables those features, so a gated replay family is never "
            "compiled there. Remove the gate."
        ]
    return []


def check(root: Path) -> list[str]:
    violations: list[str] = []
    violations += check_corpus(root)
    violations += check_references(root)
    violations += check_production_reachability(root)
    violations += check_target_declaration(root)
    return violations


# =============================================================================
# Self-test
# =============================================================================


def _copy_tree(source: Path, destination: Path) -> Path:
    """Copy only what the guard reads, so the self-test stays bounded."""
    (destination / "src").mkdir(parents=True, exist_ok=True)
    (destination / "src" / "lib.rs").write_text(
        "//! A production crate root with no replay dependency.\n", encoding="utf-8"
    )
    manifest = (source / CARGO_MANIFEST).read_text(encoding="utf-8")
    (destination / CARGO_MANIFEST).write_text(manifest, encoding="utf-8")
    shutil.copytree(source / FAMILY_DIR, destination / FAMILY_DIR)
    return destination


def _append(path: Path, text: str) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(text)


def _self_test() -> int:
    failures: list[str] = []
    if not (ROOT / FAMILY_DIR).is_dir():
        print("self-test requires the replay family to exist", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="codegg-session-replay-") as raw:
        clean = _copy_tree(ROOT, Path(raw) / "clean")
        baseline = check(clean)
        if baseline:
            failures.append("the unmodified copy must pass the guard")
            for violation in baseline:
                print(f"  baseline violation: {violation}", file=sys.stderr)
            _report(failures)
            return 1

        def case(label: str, inject) -> None:
            with tempfile.TemporaryDirectory(prefix="codegg-session-replay-case-") as case_raw:
                copy = _copy_tree(ROOT, Path(case_raw) / "case")
                inject(copy)
                violations = check(copy)
                if not violations:
                    failures.append(f"the guard did not reject {label}")
                else:
                    print(f"  rejected {label}: {violations[0]}")

        def inject_orphan(copy: Path) -> None:
            path = copy / FIXTURE_DIR / "orphan_fixture.json"
            path.write_text(
                json.dumps(
                    {
                        "format": SUPPORTED_FORMAT,
                        "id": "orphan_fixture",
                        "kind": "scenario",
                        "session_id": "orphan_fixture",
                        "user_input": "unbound",
                        "responses": [[{"type": "finish", "stop_reason": "stop"}]],
                        "expect": {
                            "assistant_text": "",
                            "terminal": "completed",
                            "request_count": 1,
                        },
                    }
                ),
                encoding="utf-8",
            )

        def inject_dangling(copy: Path) -> None:
            _append(
                copy / FAMILY_DIR / "scenarios.inc",
                "\n#[test]\nfn injected_dangling_reference() {\n"
                '    let _ = run_scenario("missing_fixture");\n}\n',
            )

        def inject_oversized(copy: Path) -> None:
            path = copy / FIXTURE_DIR / "oversized_fixture.json"
            payload = {
                "format": SUPPORTED_FORMAT,
                "id": "oversized_fixture",
                "kind": "decode",
                "session_id": "oversized_fixture",
                "user_input": "x" * (FIXTURE_SIZE_BOUND + 1),
                "responses": [[{"type": "finish", "stop_reason": "stop"}]],
                "expect": {
                    "assistant_text": "",
                    "terminal": "completed",
                    "request_count": 1,
                },
            }
            path.write_text(json.dumps(payload), encoding="utf-8")

        def inject_credential(copy: Path) -> None:
            path = copy / FIXTURE_DIR / "all_variants.json"
            document = json.loads(path.read_text(encoding="utf-8"))
            document["expect"]["assistant_text"] = "sk-proj-0123456789abcdefghijklmn"
            path.write_text(json.dumps(document, indent=2), encoding="utf-8")

        def inject_production_reference(copy: Path) -> None:
            _append(
                copy / "src" / "lib.rs",
                "\npub fn leak() {\n    let _ = crate::tests::ReplayFixture;\n}\n",
            )

        def inject_gated_target(copy: Path) -> None:
            manifest = copy / CARGO_MANIFEST
            text = manifest.read_text(encoding="utf-8")
            needle = 'name = "session_replay"\n'
            patched = text.replace(
                needle,
                needle + 'required-features = ["lsp-test-support"]\n',
                1,
            )
            if patched == text:
                raise SystemExit("self-test could not find the session_replay target")
            manifest.write_text(patched, encoding="utf-8")

        case("an orphan fixture", inject_orphan)
        case("a dangling reference", inject_dangling)
        case("an oversized fixture", inject_oversized)
        case("a credential-shaped value", inject_credential)
        case("a reference from a production path", inject_production_reference)
        case("a `required-features` gate on the family", inject_gated_target)

    if failures:
        _report(failures)
        return 1
    print("session replay fixture guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it exists to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    violations = check(ROOT)
    if violations:
        for violation in violations:
            print(f"ERROR: {violation}", file=sys.stderr)
        print(f"{len(violations)} session replay fixture violation(s)", file=sys.stderr)
        return 1
    print("session replay fixture guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
