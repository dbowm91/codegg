#!/usr/bin/env python3
"""Static guard for the write-transaction shape of the read-then-write paths (C002).

`SqlitePool::begin()` issues a **deferred** `BEGIN`, which takes no lock. The
first `SELECT` in such a transaction holds a shared read lock, and the first
`UPDATE` has to *upgrade* it. SQLite refuses that upgrade with `SQLITE_BUSY` —
code 6, `database is deadlocked` — and does not call the busy handler, because
waiting could deadlock. `busy_timeout` therefore never applies, and two
connections that each read before either wrote could **both** fail instead of one
winning and the other waiting its turn.

M003 shipped every queue write path in that shape. M006 found the consequence
while repeating the suite; C002 measured it at **204 of 256** concurrent claim
rounds and fixed it by taking the write lock at `BEGIN` instead.

This guard exists so the shape cannot come back one `pool.begin()` at a time:

1. Every transaction in the user prompt queue store is opened either by
   `begin_immediate` (which issues `BEGIN IMMEDIATE`) or is on an explicit,
   reasoned allowlist of deferred paths whose first statement is already a write.
2. The daemon's scratch retention sweep takes the write lock at `BEGIN`
   unconditionally: it reads a session's activity and liveness before it writes
   its reclaim decision, so a deferred transaction would have to upgrade a
   shared read lock there too.
3. The immediate path must actually be immediate: the shared helper issues
   `BEGIN IMMEDIATE`, and an uncommitted transaction queues a `ROLLBACK` on its
   connection and never closes it.

Run with `--self-test` to prove the guard fails on an injected violation. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
QUEUE_STORE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "user_prompt_queue.rs"
# The shared home of the pattern. The queue store established it; the scratch
# retention sweep reuses it.
IMMEDIATE_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "storage" / "immediate.rs"
# The daemon's scratch retention sweep: it reads the durable session set and the
# live-lease facts before it writes its reclaim decision.
SWEEP_MODULE = REPO_ROOT / "src" / "core" / "scratch_sweep.rs"

# A deferred transaction. Matches both `pool.begin()` and `self.pool.begin()`.
DEFERRED_BEGIN = re.compile(r"\bpool\.begin\(\)")
IMMEDIATE_BEGIN = "begin_immediate("

# The only deferred transactions the queue store is allowed to keep, each with the
# reason it is not a lock-upgrade site.
#
# `ownership` writes nothing at all: it reads the owner row and then whether any
# entry exists, and a decision taken for a mutation is never made here — every
# mutating method resolves ownership again inside its own immediate transaction.
#
# `list_live` does write (the lazy expiry sweep), but its **first** statement is
# that sweep, so it already takes the write lock at that statement. Taking it
# earlier would buy nothing and would serialize every queue read in the daemon.
PERMITTED_DEFERRED = {
    "ownership": "read-only; writes nothing, so there is no read lock to upgrade",
    "list_live": "first statement is the expiry sweep, so it already takes the write lock",
}

# SQL that writes a queue table. A function containing any of these and opening a
# deferred transaction is exactly the deadlock shape.
WRITE_SQL = re.compile(
    r"\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+user_prompt_queue_",
    re.IGNORECASE,
)


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def function_bodies(source: str) -> dict[str, str]:
    """Map each function name in the module to its body text.

    Brace-matched from the signature rather than by indentation, so a body
    containing a nested `fn` or a closure cannot truncate its own span.
    """
    bodies: dict[str, str] = {}
    for match in re.finditer(r"(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*[<(]", source):
        name = match.group(1)
        depth, index, started = 0, match.end() - 1, False
        while index < len(source):
            char = source[index]
            if char == "{":
                depth += 1
                started = True
            elif char == "}":
                depth -= 1
                if started and depth == 0:
                    break
            index += 1
        bodies.setdefault(name, source[match.end() : index])
    return bodies


def _check_queue_store(source: str) -> list[str]:
    violations: list[str] = []
    bodies = function_bodies(source)

    for name, body in bodies.items():
        if not DEFERRED_BEGIN.search(body):
            continue
        if name not in PERMITTED_DEFERRED:
            violations.append(
                f"{name}() opens a deferred transaction. A read-then-write queue "
                f"transaction cannot upgrade a shared read lock: SQLite returns "
                f"SQLITE_BUSY (code 6) without waiting. Use {IMMEDIATE_BEGIN}."
            )
            continue
        if WRITE_SQL.search(body) and name != "list_live":
            violations.append(
                f"{name}() opens a deferred transaction and writes queue tables. "
                f"Its allowlist reason says it writes nothing, so that is now false."
            )

    # Every deferred site must be a *named* decision, so a new one fails instead
    # of being silently permitted — and a name that no longer matches a deferred
    # site is stale, which would quietly narrow what the allowlist constrains.
    deferred = {name for name, body in bodies.items() if DEFERRED_BEGIN.search(body)}
    for missing in sorted(set(PERMITTED_DEFERRED) - deferred):
        violations.append(
            f"{missing}() no longer opens a deferred transaction, but is still on "
            f"the allowlist. An allowlist that lists code which does not exist is "
            f"not a constraint."
        )

    return violations


def _check_immediate_module(source: str) -> list[str]:
    """The shared helper must really be immediate, and must roll back safely."""
    violations: list[str] = []

    # The immediate path must actually be immediate, not a renamed `begin()`.
    if "BEGIN IMMEDIATE" not in source:
        violations.append(
            f"{IMMEDIATE_BEGIN} exists but never issues BEGIN IMMEDIATE; the "
            f"transaction is deferred again."
        )

    # The rollback contract: an uncommitted write transaction must roll back
    # without closing. Closing would force the rollback, and it is destructive
    # for a shared-cache in-memory database, where dropping the last connection
    # destroys the database.
    if "impl Drop for ImmediateWrite" not in source:
        violations.append(
            "ImmediateWrite has no Drop, so a dropped future leaks its transaction"
        )
    elif "start_rollback" not in source:
        violations.append("ImmediateWrite::drop does not queue a rollback")
    else:
        match = re.search(r"impl Drop for ImmediateWrite.*?\n\}", source, re.DOTALL)
        if match and "detach" in match.group(0):
            violations.append(
                "ImmediateWrite::drop closes the connection instead of rolling back"
            )

    return violations


def _check_sweep_module(source: str) -> list[str]:
    """The sweep reads before it writes, so it must never defer its transaction."""
    violations: list[str] = []

    for match in re.finditer(r"(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*[<(]", source):
        name = match.group(1)
        depth, index, started = 0, match.end() - 1, False
        while index < len(source):
            char = source[index]
            if char == "{":
                depth += 1
                started = True
            elif char == "}":
                depth -= 1
                if started and depth == 0:
                    break
            index += 1
        body = source[match.end() : index]
        if DEFERRED_BEGIN.search(body):
            violations.append(
                f"{name}() opens a deferred transaction. The sweep reads a session's "
                f"activity and liveness before it writes its reclaim decision, so it "
                f"cannot upgrade a shared read lock: SQLite returns SQLITE_BUSY (code "
                f"6) without waiting. Use {IMMEDIATE_BEGIN}."
            )

    if IMMEDIATE_BEGIN not in source:
        violations.append(
            f"the sweep module never opens an immediate transaction; it must decide "
            f"each candidate under the writer lock with {IMMEDIATE_BEGIN}."
        )

    return violations


def check_queue_store() -> bool:
    if not QUEUE_STORE.is_file():
        print(f"  FAIL: queue store not found at {QUEUE_STORE}")
        return False
    violations = _check_queue_store(_read(QUEUE_STORE))
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def check_immediate_module() -> bool:
    if not IMMEDIATE_MODULE.is_file():
        print(f"  FAIL: the shared immediate module not found at {IMMEDIATE_MODULE}")
        return False
    violations = _check_immediate_module(_read(IMMEDIATE_MODULE))
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def check_sweep_module() -> bool:
    if not SWEEP_MODULE.is_file():
        print(f"  FAIL: the scratch sweep module not found at {SWEEP_MODULE}")
        return False
    violations = _check_sweep_module(_read(SWEEP_MODULE))
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def _self_test() -> int:
    """Prove the guard fails on injected violations."""
    queue_source = _read(QUEUE_STORE)
    immediate_source = _read(IMMEDIATE_MODULE)
    sweep_source = _read(SWEEP_MODULE)

    # Violation 1: a read-then-write path back on a deferred transaction.
    injected = queue_source.replace(
        "begin_immediate(&self.pool).await.map_err(storage_error)?",
        "self.pool.begin().await.map_err(storage_error)?",
        1,
    )
    if injected == queue_source:
        print("  FAIL: self-test could not find the immediate begin to replace")
        return 1
    violations = _check_queue_store(injected)
    if not violations:
        print(
            "  FAIL: self-test reinstated a deferred read-then-write "
            "transaction and the guard still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected deferred read-then-write path rejected")

    # Violation 2: the sweep's transaction deferred again.
    sweep_injected = sweep_source.replace(
        "begin_immediate(pool)",
        "pool.begin()",
        1,
    )
    if sweep_injected == sweep_source:
        print("  FAIL: self-test could not find the sweep's immediate begin to replace")
        return 1
    violations = _check_sweep_module(sweep_injected)
    if not violations:
        print(
            "  FAIL: self-test deferred the sweep's transaction and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected deferred sweep transaction rejected")

    # Violation 3: the shared helper stops being immediate.
    immediate_injected = immediate_source.replace("BEGIN IMMEDIATE", "BEGIN")
    if immediate_injected == immediate_source:
        print("  FAIL: self-test could not find the BEGIN IMMEDIATE statement")
        return 1
    violations = _check_immediate_module(immediate_injected)
    if not violations:
        print(
            "  FAIL: self-test downgraded the shared helper to a deferred "
            "transaction and the guard still passed."
        )
        return 1
    print("  ok: injected deferred shared helper rejected")

    # Violation 4: a rollback that closes the connection.
    closing = immediate_source.replace("SqliteTx::start_rollback(&mut conn);", "drop(conn.detach());")
    if closing == immediate_source:
        print("  FAIL: self-test could not find the rollback call to replace")
        return 1
    match = re.search(r"impl Drop for ImmediateWrite.*?\n\}", closing, re.DOTALL)
    if not match or "detach" not in match.group(0):
        print("  FAIL: self-test injected a closing drop and the guard still passed")
        return 1
    print("  ok: injected closing rollback rejected")

    # And the unmodified tree must still pass, so the self-test proves the guard
    # rejects rather than being broken.
    if _check_queue_store(queue_source) or _check_sweep_module(sweep_source):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    if _check_immediate_module(immediate_source):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_queue_write_transaction_shape --self-test")
        return _self_test()

    checks = [
        ("queue write paths take the write lock at BEGIN", check_queue_store),
        ("the shared helper issues BEGIN IMMEDIATE and rolls back safely", check_immediate_module),
        ("the scratch sweep decides each candidate under the writer lock", check_sweep_module),
    ]
    failed = []
    for name, check in checks:
        try:
            passed = check()
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            passed = False
        print(f"  {'ok' if passed else 'FAIL'}: {name}")
        if not passed:
            failed.append(name)
    if failed:
        print(f"check_queue_write_transaction_shape: {len(failed)} failing")
        return 1
    print("check_queue_write_transaction_shape: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
