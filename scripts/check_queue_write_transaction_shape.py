#!/usr/bin/env python3
"""Static guard for the queue store's write-transaction shape (C002).

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

1. Every transaction in the store is opened either by `begin_immediate` (which
   issues `BEGIN IMMEDIATE`) or is on an explicit, reasoned allowlist of deferred
   paths whose first statement is already a write.
2. No function that writes queue tables may open a deferred transaction.
3. The rollback contract holds: an uncommitted `ImmediateWrite` queues a
   `ROLLBACK` on its connection and never closes it.

Run with `--self-test` to prove the guard fails on an injected violation. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
STORE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "user_prompt_queue.rs"

# The `BEGIN IMMEDIATE` primitive itself lives in one shared module. C002
# extracted it out of the queue store so the new writer-permit store could use
# the same mechanism instead of forking a subtle one — and this guard follows
# it, because the rollback contract is a property of the *primitive*, not of
# whichever store happens to call it.
IMMEDIATE_MODULE = (
    REPO_ROOT / "crates" / "codegg-core" / "src" / "storage" / "immediate.rs"
)

DEFERRED_BEGIN = "self.pool.begin()"
IMMEDIATE_BEGIN = "begin_immediate("

# The only deferred transactions this store is allowed to keep, each with the
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


def _check_source(source: str) -> list[str]:
    violations: list[str] = []
    bodies = function_bodies(source)

    for name, body in bodies.items():
        if DEFERRED_BEGIN not in body:
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
    deferred = {name for name, body in bodies.items() if DEFERRED_BEGIN in body}
    for missing in sorted(set(PERMITTED_DEFERRED) - deferred):
        violations.append(
            f"{missing}() no longer opens a deferred transaction, but is still on "
            f"the allowlist. An allowlist that lists code which does not exist is "
            f"not a constraint."
        )

    return violations


def check_immediate_is_immediate() -> bool:
    """The immediate path must actually be immediate, not a renamed `begin()`.

    This lives with the shared primitive, because that is where the statement is
    issued.
    """
    if not IMMEDIATE_MODULE.is_file():
        print(f"  FAIL: immediate-transaction module not found at {IMMEDIATE_MODULE}")
        return False
    if "BEGIN IMMEDIATE" not in _read(IMMEDIATE_MODULE):
        print(
            f"  FAIL: {IMMEDIATE_BEGIN} exists but never issues BEGIN IMMEDIATE; "
            f"the transaction is deferred again."
        )
        return False
    return True


def check_store() -> bool:
    if not STORE.is_file():
        print(f"  FAIL: queue store not found at {STORE}")
        return False
    violations = _check_source(_read(STORE))
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def check_rollback_contract() -> bool:
    """An uncommitted write transaction must roll back without closing.

    Closing would force the rollback, and it is destructive for a shared-cache
    in-memory database, where dropping the last connection destroys the
    database — the mutation suite caught that as an unowned-queue refusal
    turning into `no such table` two operations later.

    The contract belongs to the shared `ImmediateWrite` primitive, so it is
    checked where the primitive now lives.
    """
    if not IMMEDIATE_MODULE.is_file():
        print(f"  FAIL: immediate-transaction module not found at {IMMEDIATE_MODULE}")
        return False
    source = _read(IMMEDIATE_MODULE)
    ok = True
    if "impl Drop for ImmediateWrite" not in source:
        print("  FAIL: ImmediateWrite has no Drop, so a dropped future leaks its transaction")
        ok = False
    elif "start_rollback" not in source:
        print("  FAIL: ImmediateWrite::drop does not queue a rollback")
        ok = False
    match = re.search(r"impl Drop for ImmediateWrite.*?\n\}", source, re.DOTALL)
    if match and "detach" in match.group(0):
        print("  FAIL: ImmediateWrite::drop closes the connection instead of rolling back")
        ok = False
    if not ok:
        return False
    return True


def _self_test() -> int:
    """Prove the guard fails on injected violations."""
    source = _read(STORE)
    if "pub async fn claim_next_for_delivery" not in source:
        print("  FAIL: self-test could not find claim_next_for_delivery to inject into")
        return 1

    # Violation 1: a read-then-write path back on a deferred transaction.
    injected = source.replace(
        "let mut conn = begin_immediate(&self.pool).await?;",
        "let mut conn = self.pool.begin().await.map_err(storage_error)?;",
        1,
    )
    if injected == source:
        print("  FAIL: self-test could not find the immediate begin to replace")
        return 1
    violations = _check_source(injected)
    if not violations:
        print(
            "  FAIL: self-test reinstated a deferred read-then-write "
            "transaction and the guard still passed. The guard is not load-bearing."
        )
        return 1
    if not any("claim_next_for_delivery" in message for message in violations):
        print(f"  FAIL: self-test violation did not name the injected function: {violations}")
        return 1
    print("  ok: injected deferred read-then-write path rejected")

    # Violation 2: a rollback that closes the connection. The contract lives in
    # the shared primitive, so that is where the violation is injected.
    primitive = _read(IMMEDIATE_MODULE)
    closing = primitive.replace("SqliteTx::start_rollback(&mut conn);", "drop(conn.detach());")
    if closing == primitive:
        print("  FAIL: self-test could not find the rollback call to replace")
        return 1
    match = re.search(r"impl Drop for ImmediateWrite.*?\n\}", closing, re.DOTALL)
    if not match or "detach" not in match.group(0):
        print("  FAIL: self-test injected a closing drop and the guard still passed")
        return 1
    print("  ok: injected closing rollback rejected")

    # And the unmodified tree must still pass, so the self-test proves the guard
    # rejects rather than being broken.
    if _check_source(source):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_queue_write_transaction_shape --self-test")
        return _self_test()

    checks = [
        ("queue write paths take the write lock at BEGIN", check_store),
        ("the immediate path issues BEGIN IMMEDIATE", check_immediate_is_immediate),
        ("an uncommitted queue transaction rolls back without closing", check_rollback_contract),
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