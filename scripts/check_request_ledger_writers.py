#!/usr/bin/env python3
"""Static guard for the durable request ledger (M002).

The ledger is only honest while four properties hold, and each of them fails
silently rather than loudly:

1. **One writer.** A second `INSERT INTO turn_request_ledger` produces two
   rows for one attempt, and nothing about the turn shows it.
2. **The write is inside the attempt function, before the stream is
   consumed.** A row written after the request went out is not a record of
   what was sent; a row written from another path is a second writer wearing
   an allocator's clothes.
3. **The retention bound exists and the writer is the one that applies it.**
   A bound in a constant that the writer does not reference bounds nothing,
   and the writer must open `BEGIN IMMEDIATE` because it counts before it
   inserts (the C002 shape: a deferred transaction cannot upgrade a shared
   read lock).
4. **The reader has a production consumer.** A reader used only by tests is
   dead code, and dead code with a table behind it is worse than no table.
   The production consumer is `codegg doctor`'s read-only `session` section.

It also checks that the table is created by the migration chain and that
`STORAGE_LAYOUT_VERSION` tracks the highest wired migration.

Rust is never scanned with a bare regex here: every structural question is
answered over a *production view* of the source, in which `#[cfg(test)] mod`
blocks are removed. That view is `rust_test_scan.production_view`, the shared
helper of `scripts/rust_test_scan.py`, the repository's Rust-aware scanner --
never a hand-rolled brace counter: a counter over raw text reads a brace
spelled inside a string literal as structural, runs past the module's real
closing brace, and silently truncates the file. So a test that exercises a
second writer cannot hide from the check and a test consumer cannot satisfy
it. Function bodies come from `rust_test_scan.find_functions`, whose brace
matching is literal-aware for the same reason.

Run with `--self-test` to prove the guard rejects all three injections the
milestone names: a second writer, a missing bound, and a reader whose only
consumer is a test.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

STORE = "crates/codegg-core/src/session/request_ledger.rs"
SEAM = "src/agent/provider_turn.rs"
SCHEMA = "crates/codegg-core/src/session/schema.rs"
STORAGE = "crates/codegg-core/src/storage/mod.rs"
DOCTOR = "src/main.rs"

TABLE = "turn_request_ledger"
BOUND = "MAX_REQUEST_LEDGER_ROWS_PER_SESSION"
WRITER = "record_attempt"
SEAM_CALL = "ledger_open_attempt("
SEAM_INNER = "stream_once_inner("
DOCTOR_READ = "ledger_health("

INSERT = f"INSERT INTO {TABLE}"
WRITER_CALL = re.compile(rf"\.{WRITER}\(")


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def production_view(source: str) -> str:
    """The source with every `#[cfg(test)] mod .. { .. }` block removed.

    Thin delegation to `rust_test_scan.production_view`, which owns the
    boundaries. The scanner's length-preserving mask blanks each literal and
    comment, so a brace that is *spelled* in a string -- a
    `'{ this is not a configuration document'` fixture, a `{}` in a format
    template -- is not a brace to the module matcher. The attributes that
    *govern* a module come from the scanner too, so the attribute-separated
    form `#[cfg(test)] #[allow(..)] mod tests` is recognised and an
    attribute belonging to the item above cannot take a module with it.

    The hand-rolled counter this replaced read one such fixture as
    structural, never saw the module's own closing brace return its depth to
    zero, and ran to end-of-file: in `src/main.rs` it dropped every line from
    the first test module onward, including the diagnostic's production
    `ledger_health()` call far below it.
    """
    return rust_test_scan.production_view(source)


def function_bodies(source: str, rel: str = "<inline>") -> dict[str, str]:
    """Map each function name in the source to its parsed body.

    Bodies are matched by `rust_test_scan.find_functions` against the same
    length-preserving mask, so a brace inside a literal cannot end a body
    early and leave the next function's statements inside it.
    """
    bodies: dict[str, str] = {}
    tree = rel.split("/")[0]
    for function in rust_test_scan.find_functions(source, Path(rel), rel, tree):
        bodies.setdefault(function.name, function.body_text)
    return bodies


def _check_single_writer(sources: dict[str, str]) -> list[str]:
    """Exactly one production insert, and exactly one call to the writer."""
    violations: list[str] = []
    inserts = {rel: text.count(INSERT) for rel, text in sources.items()}
    inserts = {rel: count for rel, count in inserts.items() if count}
    if sum(inserts.values()) != 1:
        detail = ", ".join(f"{rel} ({count})" for rel, count in sorted(inserts.items()))
        violations.append(
            f"the ledger has {sum(inserts.values())} production writer(s) "
            f"({detail or 'none'}); one attempt must have exactly one writer"
        )
    elif STORE not in inserts:
        violations.append(
            f"the ledger's only insert is not in {STORE}; the store owns the "
            f"transaction shape, so the writer cannot live anywhere else"
        )

    callers = [rel for rel, text in sources.items() if WRITER_CALL.search(text)]
    if callers != [SEAM]:
        violations.append(
            f"{WRITER}() is called from {sorted(callers) or 'nowhere'}; it must "
            f"be called from exactly {SEAM}, or two rows can exist for one attempt"
        )
    return violations


def _check_seam(seam: str) -> list[str]:
    """The write is inside the attempt function and precedes the stream."""
    violations: list[str] = []
    bodies = function_bodies(seam, SEAM)
    definition = f"fn {SEAM_CALL.rstrip('(')}("
    seam_calls = seam.count(SEAM_CALL) - seam.count(definition)
    if seam_calls != 1:
        violations.append(
            f"{SEAM_CALL[:-1]}() appears {seam_calls} times in the seam module; "
            f"the attempt function must open exactly one ledger row"
        )
    attempt = bodies.get("stream_once", "")
    if SEAM_CALL not in attempt:
        violations.append(
            f"{SEAM_CALL[:-1]}() is not inside stream_once(), the attempt "
            f"function; a request that fails at setup would record nothing"
        )
    elif SEAM_INNER not in attempt:
        violations.append(
            f"stream_once() does not reach {SEAM_INNER[:-1]}(); the stream "
            f"consumer is gone and the write precedes nothing"
        )
    elif attempt.index(SEAM_CALL) > attempt.index(SEAM_INNER):
        violations.append(
            "the ledger row is written after the stream is consumed; a row "
            "that appears after dispatch is not a record of what was sent"
        )
    if seam.count("provider.stream(") != 1:
        violations.append(
            "the seam module must consume the provider stream exactly once, "
            "inside the attempt function"
        )
    elif "provider.stream(" not in bodies.get("stream_once_inner", ""):
        violations.append(
            "provider.stream() is not inside stream_once_inner(), so the write "
            "in stream_once() is not ordered before it"
        )
    return violations


def _check_bound(store: str) -> list[str]:
    """The bound is defined and the writer is what applies it."""
    violations: list[str] = []
    if not re.search(rf"pub const {BOUND}\s*:\s*i64\s*=", store):
        violations.append(f"{BOUND} is not defined as an i64 constant")
    body = function_bodies(store, STORE).get(WRITER, "")
    if not body:
        violations.append(f"{WRITER}() is not defined in the store")
        return violations
    if BOUND not in body:
        violations.append(
            f"{WRITER}() does not reference {BOUND}; an unenforced bound is not "
            f"a bound, and a long session would grow the ledger without limit"
        )
    if "begin_immediate(" not in body:
        violations.append(
            f"{WRITER}() does not open its transaction with begin_immediate. It "
            f"counts and prunes before it inserts, so a deferred BEGIN would "
            f"have to upgrade a shared read lock and SQLite would answer "
            f"SQLITE_BUSY without waiting (the C002 shape)"
        )
    if "pool.begin()" in body:
        violations.append(
            f"{WRITER}() opens a deferred transaction; use begin_immediate"
        )
    return violations


def _check_reader_consumer(doctor: str) -> list[str]:
    """The reader is read by something that ships."""
    violations: list[str] = []
    if DOCTOR_READ not in doctor:
        violations.append(
            f"{DOCTOR} never reads the ledger ({DOCTOR_READ}); a reader used "
            f"only by tests is dead code, and the diagnostic is its production "
            f"consumer"
        )
    return violations


def _check_migration(schema: str, storage: str) -> list[str]:
    """The table is created by the chain and the marker tracks it."""
    violations: list[str] = []
    bodies = function_bodies(schema, SCHEMA)
    migrator = bodies.get("migrate_v95", "")
    if TABLE not in migrator:
        violations.append(f"migrate_v95() does not create {TABLE}")
    if f"95 => migrate_v95" not in schema:
        violations.append("the migration dispatch has no `95 => migrate_v95` arm")
    if "migrate_and_record(pool, 95)" not in schema:
        violations.append("the migration chain never reaches v95")
    marker = re.search(r"STORAGE_LAYOUT_VERSION\s*:\s*u32\s*=\s*(\d+)", storage)
    if marker is None:
        violations.append("STORAGE_LAYOUT_VERSION is not declared")
        return violations
    highest = max(
        int(version)
        for version in re.findall(r"migrate_and_record\(pool,\s*(\d+)\)", schema)
    )
    if int(marker.group(1)) != highest:
        violations.append(
            f"STORAGE_LAYOUT_VERSION is {marker.group(1)} but the highest wired "
            f"migration is v{highest}"
        )
    return violations


def check(sources: dict[str, str]) -> list[str]:
    violations: list[str] = []
    violations += _check_single_writer(sources)
    violations += _check_seam(sources.get(SEAM, ""))
    violations += _check_bound(sources.get(STORE, ""))
    violations += _check_reader_consumer(sources.get(DOCTOR, ""))
    violations += _check_migration(sources.get(SCHEMA, ""), sources.get(STORAGE, ""))
    return violations


def collect() -> dict[str, str]:
    """Every Rust source in the tree, reduced to its production view."""
    sources: dict[str, str] = {}
    for root in ("src", "crates"):
        for path in sorted((REPO_ROOT / root).rglob("*.rs")):
            rel = path.relative_to(REPO_ROOT).as_posix()
            sources[rel] = production_view(_read(path))
    return sources


def _self_test() -> int:
    """Prove the guard fails on each injected defect."""
    sources = collect()
    base = check(sources)
    if base:
        print("  FAIL: the tree already violates the guard, so a rejection proves nothing")
        for message in base:
            print(f"    {message}")
        return 1
    print("  ok: the unmodified tree passes")

    injected = dict(sources)
    injected[SEAM] = production_view(
        sources[SEAM]
        + f'\nfn ledger_second_writer() {{\n    let _ = "{INSERT} (id) VALUES (1)";\n}}\n'
    )
    rejected = _check_single_writer(injected)
    if not any("writer(s)" in message for message in rejected):
        print("  FAIL: an injected second ledger writer was accepted")
        return 1
    print("  ok: injected second writer rejected")

    store = sources[STORE]
    unbounded = store.replace(f"if retained >= {BOUND} {{", "if false {").replace(
        f" - {BOUND} + 1", " - 0"
    )
    if unbounded == store or BOUND in function_bodies(unbounded, STORE)[WRITER]:
        print(f"  FAIL: self-test could not remove the writer's {BOUND} references")
        return 1
    injected = dict(sources)
    injected[STORE] = unbounded
    rejected = _check_bound(injected[STORE])
    if not any("does not reference" in message for message in rejected):
        print("  FAIL: a writer that no longer enforces the bound was accepted")
        return 1
    print("  ok: injected missing bound rejected")

    doctor = sources[DOCTOR]
    hidden = doctor.replace(DOCTOR_READ, "// ", 1)
    if hidden == doctor:
        print("  FAIL: self-test could not find the doctor's ledger read")
        return 1
    injected = dict(sources)
    injected[DOCTOR] = production_view(
        hidden
        + "\n#[cfg(test)]\nmod ledger_only_tests {\n    fn read() {\n        "
        + DOCTOR_READ
        + "\n    }\n}\n"
    )
    rejected = _check_reader_consumer(injected[DOCTOR])
    if not any("never reads the ledger" in message for message in rejected):
        print("  FAIL: a reader whose only consumer is a test was accepted")
        return 1
    print("  ok: injected test-only reader rejected")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_request_ledger_writers --self-test")
        return _self_test()

    print("check_request_ledger_writers")
    violations = check(collect())
    for message in violations:
        print(f"  FAIL: {message}")
    if violations:
        print(f"{len(violations)} request-ledger violation(s)")
        return 1
    print("  ok: one writer, ordered before dispatch, bounded, and read by doctor")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
