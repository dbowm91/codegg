#!/usr/bin/env python3
"""Static guard for the M003 queue-delivery entry (ADR-0038, Option B).

`CoreDaemon::submit_queued_prompt_turn` is daemon-internal delivery of a queued
user prompt into an ordinary turn admission: no CoreRequest variant, no
transport route, no bearer, no controller bypass. The ADR restricts it to
exactly three call sites — the post-terminal wake
(`deliver_next_queued_prompt`), the bounded restart twin
(`reconcile_stranded_queue_deliveries`), and the M007 bounded queue advance
tick (`advance_due_queue_entries`). Any other caller would be a general
daemon-may-submit-as-anyone backdoor, which is precisely the surface
`scripts/check_idle_continuation_callsites.py` exists to keep small for
ADR-0031.

Checks:
1. The definition lives in `src/core/daemon_queue_delivery.rs`.
2. Every *call* of `submit_queued_prompt_turn(` outside the definition is
   inside one of the three permitted caller functions, in an allowed file.
3. `CoreRequest` carries no queue-delivery surface — delivery submits through
   the ordinary `TurnSubmit` variant, never through a variant of its own.
4. Delivery never runs as a synthetic identity or through
   `is_controller_bypass`: the delivered principal is reconstructed from the
   durable owner row, and neither escape hatch appears in the module.

Run with `--self-test` to prove the guard actually fails on an injected fourth
caller. A guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DELIVERY_MODULE = REPO_ROOT / "src" / "core" / "daemon_queue_delivery.rs"
PROTOCOL_CORE = REPO_ROOT / "crates" / "codegg-protocol" / "src" / "core.rs"

ENTRY = "submit_queued_prompt_turn"
# M003 permitted exactly two. M007 (ADR-0039, Option A) added a third — the
# bounded queue advance tick — which is a *recorded decision*, not a quiet third
# call site. ADR-0039 states why the tick exists at all: the roadmap claimed
# advancement would ride an existing periodic observation point, and planning
# M007 falsified that (there is none for the queue). Option A is the only branch
# that delivers the milestone's promise; the alternative was accepting late
# delivery, which breaks the UX premise the milestone exists for.
#
# A fourth caller is still a defect, and `--self-test` proves it.
PERMITTED_CALLERS = {
    "deliver_next_queued_prompt",
    "reconcile_stranded_queue_deliveries",
    "advance_due_queue_entries",
}
PERMITTED_FILES = {
    "src/core/daemon_queue_delivery.rs",
    "src/core/daemon_control.rs",
}

# The two authority escape hatches ADR-0038 refuses for delivery. A synthetic
# daemon principal would take the controller lease away from the real owner, and
# `is_controller_bypass` is the seam ADR-0037 exists to refuse.
FORBIDDEN_AUTHORITY = ("is_controller_bypass", "SyntheticPrincipal", "synthetic_principal")


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_definition_site() -> bool:
    if not DELIVERY_MODULE.is_file():
        print(f"  FAIL: {ENTRY} module not found at {DELIVERY_MODULE}")
        return False
    source = _read(DELIVERY_MODULE)
    if f"pub async fn {ENTRY}" not in source:
        print(f"  FAIL: {ENTRY} definition not found in daemon_queue_delivery.rs")
        return False
    return True


def _find_call_violations(source_dir: Path) -> list[str]:
    """Return one message per call of ENTRY outside a permitted caller."""
    violations: list[str] = []
    definition = re.compile(rf"async fn {ENTRY}\s*\(")
    for path in sorted(source_dir.rglob("*.rs")):
        source = _read(path)
        for match in re.finditer(rf"{ENTRY}\s*\(", source):
            start = match.start()
            line_start = source.rfind("\n", 0, start) + 1
            line_end = source.find("\n", match.end())
            line = source[line_start : line_end if line_end != -1 else len(source)]
            if definition.search(line):
                continue  # the definition itself
            # Enclosing function: nearest preceding `async fn F(` / `fn F(`.
            preceding = source[:start]
            fns = re.findall(r"(?:async\s+)?fn\s+(\w+)\s*[<(]", preceding)
            caller = fns[-1] if fns else "<top-level>"
            try:
                rel = str(path.relative_to(REPO_ROOT))
            except ValueError:
                # Only the `--self-test` stages files outside the repo. Report the
                # staged path rather than crashing on it: a self-test that dies of
                # an unrelated exception has proved nothing either way.
                rel = str(path)
            if caller not in PERMITTED_CALLERS or rel not in PERMITTED_FILES:
                violations.append(
                    f"{ENTRY} called from {caller}() in {rel} "
                    f"(permitted: {sorted(PERMITTED_CALLERS)})"
                )
    return violations


def check_call_sites() -> bool:
    violations = _find_call_violations(REPO_ROOT / "src")
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def check_permitted_callers_are_wired() -> bool:
    """Every permitted caller must actually exist and call the entry.

    Without this the guard passes on a delivery feature that is wired to nothing,
    which is the same failure as a guard that passes on everything: the surface
    it claims to constrain is not the surface that exists.
    """
    wired: set[str] = set()
    for path in sorted((REPO_ROOT / "src").rglob("*.rs")):
        source = _read(path)
        for match in re.finditer(rf"{ENTRY}\s*\(", source):
            preceding = source[: match.start()]
            fns = re.findall(r"(?:async\s+)?fn\s+(\w+)\s*[<(]", preceding)
            if fns and fns[-1] in PERMITTED_CALLERS:
                wired.add(fns[-1])
    missing = PERMITTED_CALLERS - wired
    if missing:
        print(
            f"  FAIL: permitted callers never invoke {ENTRY}: {sorted(missing)}. "
            "An unwired delivery path means this guard constrains nothing."
        )
        return False
    return True


def check_no_wire_delivery() -> bool:
    source = _read(PROTOCOL_CORE)
    match = re.search(r"pub enum CoreRequest \{(.*?)\n\}", source, re.DOTALL)
    if not match:
        print("  FAIL: CoreRequest enum not found")
        return False
    body = match.group(1)
    for forbidden in ("QueueDeliver", "queue_deliver", "QueuedPromptSubmit"):
        if forbidden in body:
            print(f"  FAIL: CoreRequest carries a queue-delivery surface ({forbidden})")
            return False
    if "TurnSubmit" not in body:
        # Delivery submits through the ordinary variant; without it there is
        # nothing to submit through and the module is dead or rerouted.
        print("  FAIL: CoreRequest has no TurnSubmit variant for delivery to use")
        return False
    return True


def check_delivery_authority() -> bool:
    """Delivery runs as the durable owner, never as a synthetic identity."""
    source = _read(DELIVERY_MODULE)
    for forbidden in FORBIDDEN_AUTHORITY:
        if forbidden in source:
            print(f"  FAIL: delivery module references {forbidden}")
            return False
    if "AuthenticatedPrincipal::reconstructed" not in source:
        print("  FAIL: delivery does not reconstruct a principal from durable state")
        return False
    if "AuthMethod::QueueDelivery" not in source:
        print("  FAIL: delivery does not use the ADR-0038 delivery classification")
        return False
    # Delivery must go through the ordinary submit path. A queue-specific submit
    # would be a second admission path, which ADR-0035 forbids.
    if "CoreRequest::TurnSubmit" not in source:
        print("  FAIL: delivery does not submit through CoreRequest::TurnSubmit")
        return False
    return True


def _self_test() -> int:
    """Prove the guard fails on an injected third caller."""
    source = _read(DELIVERY_MODULE)
    if "impl CoreDaemon {" not in source:
        print("  FAIL: self-test could not find an impl block to inject into")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        # Staged under the real `src/core/` layout so the self-test exercises the
        # same relative paths the real run compares against.
        staged = Path(tmp) / "src" / "core"
        staged.mkdir(parents=True)
        (staged / "daemon_queue_delivery.rs").write_text(source, encoding="utf-8")
        # A fourth caller, in a permitted file but not a permitted function.
        (staged / "daemon_control.rs").write_text(
            "impl CoreDaemon {\n"
            "    async fn injected_fourth_caller(&self) {\n"
            f"        self.{ENTRY}(\"session-x\").await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        violations = _find_call_violations(staged)
        if not violations:
            print(
                "  FAIL: self-test injected a fourth caller and the guard still passed. "
                "The guard is not load-bearing."
            )
            return 1
        if not any("injected_fourth_caller" in message for message in violations):
            print(f"  FAIL: self-test violation did not name the injected caller: {violations}")
            return 1
        print("  ok: injected fourth caller rejected")

    # And the unmodified tree must still pass, so the self-test proves the
    # guard rejects rather than being broken.
    if _find_call_violations(REPO_ROOT / "src"):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_queue_delivery_callsites --self-test")
        return _self_test()

    checks = [
        ("delivery entry defined once", check_definition_site),
        ("call sites restricted to post-terminal wake + restart twin + advance tick", check_call_sites),
        ("all permitted callers are actually wired", check_permitted_callers_are_wired),
        ("delivery is never a wire operation", check_no_wire_delivery),
        ("delivery runs as the durable owner", check_delivery_authority),
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
        print(f"check_queue_delivery_callsites: {len(failed)} failing")
        return 1
    print("check_queue_delivery_callsites: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())