#!/usr/bin/env python3
"""Guard: the non-broker writer mediation gate must stay a real authority.

M005 corrective C008 — "Non-Broker Writer Mediation Authority Completeness" —
closed five defects in the gate that C004 put in front of the fourteen
non-broker checkout writers. Three of them are structural, so they are
structural to guard: a guard that only sees the current tree cannot tell a
gate that is *there* from a gate that is *bypassed*.

WHAT THIS GUARD DOES
    It checks three invariants, each of which was a live defect:

    1. **The reservation authority's path resolution does not go through a
       capped list.** The authority resolves "which managed checkout contains
       this path" by canonical containment. It used to read the answer through
       `WorktreeService::list`, which the durable store caps at
       `MAX_WORKTREE_LIST` rows, oldest first. The catalog is user-scoped —
       one database holds every project the user ever opened — so past the cap
       the newest rows are absent, and a newly created, currently *reserved*
       checkout is one of the absent rows. The lookup then reported a reserved
       checkout as unreserved and the gate admitted the writer, silently. The
       authority must read through an unbounded surface instead, and that
       surface must carry no `LIMIT`.

    2. **The HTTP mediation's refusal is not conditional on the daemon's
       presence.** `POST /file`, `DELETE /file` and `POST /workspace` are
       checkout writers. C004 mediated them only `if let Some(daemon)`, so a
       server with no daemon wrote into a reserved checkout with no gate at
       all. The `if let Some(daemon)` shape may not come back: the route must
       refuse, with the conflict disposition the daemon side uses.

    3. **The gate and the effect use one workspace-identifier rule.** The gate
       resolved the workspace with `WorkspaceId::parse` while the effect
       resolved it with `WorkspaceId::new_unchecked`. An identifier that fails
       `parse` therefore left the gate with no target — the write went
       *unmediated* — while the effect resolved it and wrote. A gate stricter
       than the effect is a gate that silently stops mediating. The gate must
       use the effect's rule, and an identifier the registry cannot resolve
       must be refused, not skipped.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove the gate *works*; that is the fixture suite's job. It
    proves the wiring that the defects broke is still there, which is the
    failure mode a static check can honestly see.

    It does not pin line numbers. A line number is a point-in-time fact that
    goes stale when an unrelated line is inserted above it.

USAGE
    python3 scripts/check_checkout_mediation_authority.py            # check the tree
    python3 scripts/check_checkout_mediation_authority.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

RESERVATION_SOURCE = "crates/codegg-core/src/checkout_reservation.rs"
WORKTREE_SOURCE = "crates/codegg-core/src/worktree_service.rs"
HTTP_FILE_SOURCE = "src/server/routes/file.rs"
HTTP_WORKSPACE_SOURCE = "src/server/routes/workspace.rs"
DAEMON_SOURCE = "src/core/daemon.rs"
DAEMON_GOALS_SOURCE = "src/core/daemon_goals.rs"

# The capped `list` surface on the worktree store.
CAPPED_LIST_CALL = re.compile(r"\.list\s*\(")
# The daemon-presence conditional that silently skipped the HTTP gate.
DAEMON_PRESENCE_CONDITIONAL = re.compile(r"if\s+let\s+Some\(daemon\)")


def _block_after(text: str, header: str) -> str | None:
    """The brace-balanced block that follows `header`.

    Returns `None` when the header is absent. Brace counting is used rather
    than a regular expression because the bodies are multi-line Rust; the
    string literals in the scanned bodies carry balanced braces, so the count
    stays correct.
    """
    index = text.find(header)
    if index < 0:
        return None
    start = text.find("{", index)
    if start < 0:
        return None
    depth = 0
    for position in range(start, len(text)):
        char = text[position]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[start : position + 1]
    return None


def _function_body(text: str, signature: str) -> str | None:
    """The body of the first function whose signature contains `signature`."""
    index = text.find(signature)
    if index < 0:
        return None
    start = text.find("{", index)
    if start < 0:
        return None
    depth = 0
    for position in range(start, len(text)):
        char = text[position]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[start : position + 1]
    return None


# ── Check 1: the authority does not read through a capped list ─────────


def check_authority_resolution(reservation_src: str, worktree_src: str) -> list[str]:
    """The defects in the reservation authority's path resolution."""
    problems: list[str] = []

    if "WorktreeQuery" in reservation_src:
        problems.append(
            f"{RESERVATION_SOURCE} names the capped `WorktreeQuery` surface; the "
            "reservation authority must not resolve a checkout through a list that "
            "a catalog larger than MAX_WORKTREE_LIST can truncate"
        )
    if CAPPED_LIST_CALL.search(reservation_src):
        problems.append(
            f"{RESERVATION_SOURCE} calls a capped `.list(...)`; the reservation "
            "authority must read through the unbounded authority surface"
        )
    if "list_authority_candidates" not in reservation_src:
        problems.append(
            f"{RESERVATION_SOURCE} does not call the unbounded authority read; a "
            "capped lookup can hide a reserved checkout from the gate"
        )

    trait = _block_after(worktree_src, "pub trait WorktreeStore")
    if trait is None:
        problems.append(f"{WORKTREE_SOURCE} no longer declares the `WorktreeStore` trait")
    elif "list_authority_candidates" not in trait:
        problems.append(
            f"{WORKTREE_SOURCE}: the `WorktreeStore` trait has no "
            "`list_authority_candidates` method; the durable store must expose an "
            "unbounded read for the reservation authority"
        )

    for impl_header, label in (
        ("impl WorktreeStore for SqliteWorktreeStore", "durable"),
        ("impl WorktreeStore for InMemoryWorktreeStore", "in-memory"),
    ):
        block = _block_after(worktree_src, impl_header)
        if block is None:
            problems.append(f"{WORKTREE_SOURCE} no longer declares `{impl_header}`")
            continue
        body = _function_body(block, "async fn list_authority_candidates")
        if body is None:
            problems.append(
                f"{WORKTREE_SOURCE}: the {label} store has no "
                "`list_authority_candidates` implementation"
            )
            continue
        if "LIMIT" in body.upper():
            problems.append(
                f"{WORKTREE_SOURCE}: the {label} store's authority read carries a "
                "LIMIT; a bound that returns the oldest rows hides a newly created, "
                "currently reserved checkout"
            )

    return problems


# ── Check 2: the HTTP mediation is never conditional on the daemon ─────


def check_http_mediation(file_src: str, workspace_src: str) -> list[str]:
    """The defects in the HTTP routes' mediation."""
    problems: list[str] = []

    for rel_path, source in ((HTTP_FILE_SOURCE, file_src), (HTTP_WORKSPACE_SOURCE, workspace_src)):
        if DAEMON_PRESENCE_CONDITIONAL.search(source):
            problems.append(
                f"{rel_path} gates its checkout mediation on the daemon's presence; a "
                "server with no daemon would write into a reserved checkout with no "
                "gate at all — the route must refuse instead"
            )
        if "mediate_http_checkout_write" not in source:
            problems.append(
                f"{rel_path} no longer calls the checkout mediation; the HTTP file "
                "and workspace routes are checkout writers"
            )
        if "StorageError::Conflict" not in source:
            problems.append(
                f"{rel_path} no longer refuses with the conflict disposition; a "
                "refused checkout writer is a conflict, not an authorization failure"
            )

    return problems


# ── Check 3: one workspace-identifier rule on the gate and the effect ──


def check_identifier_rule(daemon_src: str, goals_src: str) -> list[str]:
    """The defects in the gate/effect identifier rule."""
    problems: list[str] = []

    gate = _function_body(daemon_src, "async fn workspace_root_for")
    if gate is None:
        problems.append(
            f"{DAEMON_SOURCE} no longer declares `workspace_root_for`; the gate's "
            "target resolution is the thing this guard protects"
        )
    else:
        if "WorkspaceId::parse" in gate:
            problems.append(
                f"{DAEMON_SOURCE}: the gate resolves the workspace with "
                "`WorkspaceId::parse` while the effect uses `WorkspaceId::new_unchecked`; "
                "a gate stricter than the effect is a gate that silently stops mediating"
            )
        if "WorkspaceId::new_unchecked" not in gate:
            problems.append(
                f"{DAEMON_SOURCE}: the gate does not resolve the workspace with the "
                "effect side's `WorkspaceId::new_unchecked` rule"
            )

    if "WorkspaceId::parse" in goals_src:
        problems.append(
            f"{DAEMON_GOALS_SOURCE} resolves a workspace with `WorkspaceId::parse`; "
            "the effect side and the gate must use one identifier rule"
        )
    if "WorkspaceId::new_unchecked" not in goals_src:
        problems.append(
            f"{DAEMON_GOALS_SOURCE} no longer resolves a workspace with "
            "`WorkspaceId::new_unchecked`"
        )

    if "Unresolvable(super::checkout_mediation::CheckoutWriterSink, String)" not in daemon_src:
        problems.append(
            f"{DAEMON_SOURCE} no longer has an explicit unresolvable-target arm; an "
            "identifier the workspace registry cannot resolve must be refused, not "
            "skipped into an unmediated write"
        )
    if "CheckoutWriteTargetResolution" not in daemon_src:
        problems.append(
            f"{DAEMON_SOURCE} no longer distinguishes a resolved target from an "
            "unresolvable one; the gate needs three outcomes, not two"
        )

    return problems


# ── The tree ──────────────────────────────────────────────────────────


def load_sources(root: Path = ROOT) -> dict[str, str]:
    """The scanned sources, keyed by their repository-relative path."""
    sources: dict[str, str] = {}
    for rel in (
        RESERVATION_SOURCE,
        WORKTREE_SOURCE,
        HTTP_FILE_SOURCE,
        HTTP_WORKSPACE_SOURCE,
        DAEMON_SOURCE,
        DAEMON_GOALS_SOURCE,
    ):
        path = root / rel
        sources[rel] = path.read_text(encoding="utf-8") if path.is_file() else ""
    return sources


def check_tree(sources: dict[str, str]) -> list[str]:
    """Every defect in the scanned sources, as human-readable messages."""
    problems: list[str] = []
    problems += check_authority_resolution(
        sources.get(RESERVATION_SOURCE, ""), sources.get(WORKTREE_SOURCE, "")
    )
    problems += check_http_mediation(
        sources.get(HTTP_FILE_SOURCE, ""), sources.get(HTTP_WORKSPACE_SOURCE, "")
    )
    problems += check_identifier_rule(
        sources.get(DAEMON_SOURCE, ""), sources.get(DAEMON_GOALS_SOURCE, "")
    )
    return problems


# ── Self-test ─────────────────────────────────────────────────────────


def run_self_test() -> int:
    """Prove the guard rejects every class of drift it claims to catch."""
    failures: list[str] = []

    baseline = load_sources()
    if not baseline[RESERVATION_SOURCE] or not baseline[DAEMON_SOURCE]:
        print("self-test: cannot read the scanned sources", file=sys.stderr)
        return 1

    def expect_rejected(label: str, sources: dict[str, str]) -> None:
        problems = check_tree(sources)
        if not problems:
            failures.append(f"self-test: {label} was accepted but should have been rejected")
        else:
            print(f"  ok: rejected {label}")

    def expect_accepted(label: str, sources: dict[str, str]) -> None:
        problems = check_tree(sources)
        if problems:
            failures.append(
                f"self-test: {label} was rejected but should have been accepted: {problems}"
            )
        else:
            print(f"  ok: accepted {label}")

    expect_accepted("the real tree", baseline)

    # 1. The reservation authority goes back to the capped list.
    drifted = dict(baseline)
    drifted[RESERVATION_SOURCE] = baseline[RESERVATION_SOURCE].replace(
        ".list_authority_candidates()", ".list(WorktreeQuery::default())"
    )
    expect_rejected("a reservation authority that reads through the capped list", drifted)

    # 2. The durable store's authority read grows a LIMIT.
    drifted = dict(baseline)
    drifted[WORKTREE_SOURCE] = baseline[WORKTREE_SOURCE].replace(
        "FROM managed_worktree WHERE state <> 'removed' ORDER BY created_at",
        "FROM managed_worktree WHERE state <> 'removed' ORDER BY created_at LIMIT 512",
    )
    expect_rejected("a durable authority read that carries a LIMIT", drifted)

    # 3. The HTTP route makes its mediation conditional on the daemon again.
    drifted = dict(baseline)
    drifted[HTTP_FILE_SOURCE] = baseline[HTTP_FILE_SOURCE].replace(
        "let daemon = state.daemon.as_ref().ok_or_else(|| {",
        "if let Some(daemon) = state.daemon.as_ref() {",
    )
    expect_rejected("an HTTP route that skips the gate without a daemon", drifted)

    # 4. The workspace route makes its mediation conditional on the daemon again.
    drifted = dict(baseline)
    drifted[HTTP_WORKSPACE_SOURCE] = baseline[HTTP_WORKSPACE_SOURCE].replace(
        "let daemon = state.daemon.as_ref().ok_or_else(|| {",
        "if let Some(daemon) = state.daemon.as_ref() {",
    )
    expect_rejected("a workspace route that skips the gate without a daemon", drifted)

    # 5. The gate goes back to the stricter identifier rule.
    drifted = dict(baseline)
    drifted[DAEMON_SOURCE] = baseline[DAEMON_SOURCE].replace(
        "let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.to_owned());",
        "let id = codegg_core::workspace::WorkspaceId::parse(workspace_id).ok()?;",
    )
    expect_rejected("a gate that resolves the workspace more strictly than the effect", drifted)

    # 6. The effect side goes back to the stricter identifier rule.
    drifted = dict(baseline)
    drifted[DAEMON_GOALS_SOURCE] = (
        baseline[DAEMON_GOALS_SOURCE]
        + "\n// probe\nfn _probe() { let _ = codegg_core::workspace::WorkspaceId::parse(\"x\"); }\n"
    )
    expect_rejected("an effect side that resolves the workspace more strictly", drifted)

    # 7. The explicit unresolvable-target arm disappears.
    drifted = dict(baseline)
    drifted[DAEMON_SOURCE] = baseline[DAEMON_SOURCE].replace(
        "Unresolvable(super::checkout_mediation::CheckoutWriterSink, String),",
        "Unresolved(super::checkout_mediation::CheckoutWriterSink, String),",
    )
    expect_rejected("a gate with no explicit unresolvable-target arm", drifted)

    if failures:
        for failure in failures:
            print(f"self-test failure: {failure}", file=sys.stderr)
        return 1
    print("checkout-mediation-authority self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    problems = check_tree(load_sources())
    if problems:
        print("checkout mediation authority: FAILED", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(
        "checkout-mediation-authority check ok: the reservation authority reads an "
        "unbounded candidate set, the HTTP routes refuse without a daemon, and the "
        "gate and the effect share one workspace-identifier rule"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
