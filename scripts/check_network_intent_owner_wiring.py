#!/usr/bin/env python3
"""Static guard: the network-intent / sandbox-ceiling owner wiring (M005, C002).

`NetworkIntent` and `SandboxProfile` are two independent axes of a principal's
accepted execution policy.  Neither one is implied by the other: an offline
request is not satisfied by a narrower filesystem profile, and a narrower
filesystem profile never implies an offline request.

Every route that can launch a process therefore has to carry both axes from
the accepted decision all the way to the owner that enforces them.  This guard
checks that the wiring is present, and — just as importantly — that a route
which *drops* an axis is caught.

The route this guard was written for is the Tool Program delegated child job:

  accepted decision
    -> ToolProgramExecutionContext (durable, restart-surviving carrier)
      -> ToolProgramExecutor (binds the ceiling)
        -> delegated child job payload (narrowed, never widened)
          -> scheduler executor (required_sandbox_profile / required_network_intent)
            -> ManagedProcessService owner (refuses an unconstrained launch)

Before C002 the delegated child payload hard-coded
`network_intent: LegacyUnrestricted` and `sandbox_profile: None`, so a
principal that had accepted offline execution still got a child job with host
network.  The individual links below are the ones that broke.

Checks:
  1. the durable carrier (`ToolProgramExecutionContext`) holds both axes and
     covers them in the replay-safety digest;
  2. the tool boundary stamps the accepted decision into that carrier;
  3. the executor binds the carrier and the delegated child payload is
     narrowed from it rather than hard-coded;
  4. every durable payload variant that can launch a process carries both
     axes;
  5. the scheduler executor forwards both axes to the process owner;
  6. the process owner refuses an unconstrained launch for an offline intent;
  7. the native Linux workflow triggers on the Tool Program / job / policy
     paths that carry the axes.

Run:
  python3 scripts/check_network_intent_owner_wiring.py
  python3 scripts/check_network_intent_owner_wiring.py --self-test
"""

from __future__ import annotations

import argparse
import fnmatch
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

CORE_JOBS = Path("crates/codegg-core/src/jobs/mod.rs")
TOOL_CONTEXT = Path("src/tool/tool_program_context.rs")
EXECUTOR = Path("src/scheduler/tool_program_executor.rs")
SCHEDULER_EXECUTORS = Path("src/scheduler/executors.rs")
MANAGED_PROCESS = Path("src/managed_process.rs")
M004_WORKFLOW = Path(".github/workflows/linux-m004-native.yml")


def fail(msg: str) -> int:
    print(f"network-intent-owner-wiring guard failed: {msg}", file=sys.stderr)
    return 1


def read(root: Path, rel: Path) -> str | None:
    path = root / rel
    try:
        return path.read_text()
    except Exception as exc:  # noqa: BLE001 - reported as a guard failure
        print(f"cannot read {rel}: {exc}", file=sys.stderr)
        return None


def brace_block(text: str, start: int) -> str | None:
    """Return the brace-balanced block that starts at `start` (index of `{`)."""
    depth = 0
    for index in range(start, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[start : index + 1]
    return None


def struct_body(text: str, declaration: str) -> str | None:
    """Return the body of a Rust struct/enum declaration block."""
    match = re.search(re.escape(declaration) + r"\s*\{", text)
    if not match:
        return None
    block = brace_block(text, match.end() - 1)
    if block is None:
        return None
    return block[1:-1]


def impl_body(text: str, declaration: str) -> str | None:
    """Return the body of a Rust impl block.

    Scoping matters here: several types in this workspace define a
    `compute_digest`, so a whole-file search would find the wrong one and
    report a false pass.
    """
    match = re.search(re.escape(declaration) + r"\s*\{", text)
    if not match:
        return None
    block = brace_block(text, match.end() - 1)
    if block is None:
        return None
    return block[1:-1]


def fn_body(text: str, declaration: str) -> str | None:
    """Return the body of a Rust function whose signature starts with `declaration`."""
    match = re.search(re.escape(declaration), text)
    if not match:
        return None
    brace = text.find("{", match.end())
    if brace < 0:
        return None
    block = brace_block(text, brace)
    if block is None:
        return None
    return block[1:-1]


def has_defaulted_field(body: str, field: str) -> bool:
    """True when `body` declares `field` with a `#[serde(default)]` attribute."""
    pattern = (
        r"#\[serde\(default[^\]]*\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?"
        + re.escape(field)
    )
    return re.search(pattern, body) is not None


def check_carrier(root: Path) -> list[str]:
    """Check 1: the durable carrier holds both axes and digests them."""
    problems: list[str] = []
    text = read(root, CORE_JOBS)
    if text is None:
        return [f"{CORE_JOBS}: unreadable"]
    body = struct_body(text, "pub struct ToolProgramExecutionContext")
    if body is None:
        return [f"{CORE_JOBS}: ToolProgramExecutionContext declaration not found"]
    if not has_defaulted_field(body, "sandbox_profile"):
        problems.append(
            f"{CORE_JOBS}: ToolProgramExecutionContext.sandbox_profile must exist with "
            "#[serde(default)] so pre-M005 payloads still decode"
        )
    if not has_defaulted_field(body, "network_intent"):
        problems.append(
            f"{CORE_JOBS}: ToolProgramExecutionContext.network_intent must exist with "
            "#[serde(default)] so pre-M005 payloads still decode"
        )
    digest = fn_body(text, "pub fn compute_digest")
    impl_block = impl_body(text, "impl ToolProgramExecutionContext")
    if impl_block is None:
        problems.append(
            f"{CORE_JOBS}: the ToolProgramExecutionContext impl block was not found, so the "
            "replay-safety digest could not be located"
        )
        return problems
    if digest is None or "self.network_intent" not in impl_block:
        problems.append(
            f"{CORE_JOBS}: compute_digest does not cover network_intent, so a changed "
            "ceiling would share a replay-safety digest with the one it replaced"
        )
    if digest is None or "self.sandbox_profile" not in impl_block:
        problems.append(
            f"{CORE_JOBS}: compute_digest does not cover sandbox_profile, so a changed "
            "ceiling would share a replay-safety digest with the one it replaced"
        )
    return problems


def check_tool_stamp(root: Path) -> list[str]:
    """Check 2: the tool boundary stamps the accepted decision into the carrier."""
    problems: list[str] = []
    text = read(root, TOOL_CONTEXT)
    if text is None:
        return [f"{TOOL_CONTEXT}: unreadable"]
    body = fn_body(text, "pub fn to_core_context")
    if body is None:
        return [f"{TOOL_CONTEXT}: to_core_context not found"]
    if "accepted_execution_ceiling" not in body:
        problems.append(
            f"{TOOL_CONTEXT}: to_core_context does not stamp the accepted execution ceiling"
        )
    # The constructed context must actually carry both axes.
    if not re.search(r"\bsandbox_profile\b\s*,", body):
        problems.append(f"{TOOL_CONTEXT}: to_core_context does not set sandbox_profile")
    if not re.search(r"\bnetwork_intent\b\s*,", body):
        problems.append(f"{TOOL_CONTEXT}: to_core_context does not set network_intent")
    if "accepted_execution_ceiling" not in text:
        problems.append(f"{TOOL_CONTEXT}: accepted_execution_ceiling helper not found")
    else:
        ceiling = fn_body(text, "fn accepted_execution_ceiling")
        if ceiling is None:
            problems.append(f"{TOOL_CONTEXT}: accepted_execution_ceiling has no body")
        else:
            if "network_intent()" not in ceiling:
                problems.append(
                    f"{TOOL_CONTEXT}: accepted_execution_ceiling does not read the accepted "
                    "network intent from the authorization snapshot"
                )
            if "sandbox_profile()" not in ceiling:
                problems.append(
                    f"{TOOL_CONTEXT}: accepted_execution_ceiling does not read the accepted "
                    "sandbox profile from the authorization snapshot"
                )
    return problems


def check_executor_binding(root: Path) -> list[str]:
    """Check 3: the executor binds the ceiling and the child payload narrows it."""
    problems: list[str] = []
    text = read(root, EXECUTOR)
    if text is None:
        return [f"{EXECUTOR}: unreadable"]
    if "fn narrow_child_ceiling(" not in text:
        return [f"{EXECUTOR}: narrow_child_ceiling helper not found"]
    if "with_execution_ceiling" not in text:
        problems.append(f"{EXECUTOR}: the execution ceiling is never bound to the adapter")
    else:
        bind = fn_body(text, "pub fn with_execution_ceiling")
        if bind is None:
            problems.append(f"{EXECUTOR}: with_execution_ceiling has no body")
        else:
            if "self.execution_ceiling_profile" not in bind:
                problems.append(
                    f"{EXECUTOR}: with_execution_ceiling does not store the profile ceiling"
                )
            if "self.execution_ceiling_intent" not in bind:
                problems.append(
                    f"{EXECUTOR}: with_execution_ceiling does not store the network intent"
                )
    if not re.search(r"with_execution_ceiling\(\s*execution_context\.sandbox_profile", text):
        problems.append(
            f"{EXECUTOR}: the adapter ceiling is not bound from the accepted "
            "execution context's sandbox_profile"
        )
    if not re.search(r"with_execution_ceiling\([^)]*execution_context\.network_intent", text):
        problems.append(
            f"{EXECUTOR}: the adapter ceiling is not bound from the accepted "
            "execution context's network_intent"
        )

    # The delegated child payload must be derived from the ceiling.  Scope the
    # region to the production child-payload builder so a test fixture
    # elsewhere in the file cannot satisfy (or trip) this check.
    region = fn_body(text, "pub fn child_job_payload")
    if region is None:
        return problems + [f"{EXECUTOR}: child_job_payload not found"]
    if "narrow_child_ceiling(" not in region:
        problems.append(
            f"{EXECUTOR}: the delegated child job does not narrow against the parent ceiling"
        )
    if "sandbox_profile: child_profile" not in region:
        problems.append(
            f"{EXECUTOR}: the delegated child payload does not carry the narrowed "
            "sandbox_profile"
        )
    if "network_intent: child_intent" not in region:
        problems.append(
            f"{EXECUTOR}: the delegated child payload does not carry the narrowed "
            "network_intent"
        )
    # A hard-coded legacy value inside the child payload is exactly the C002
    # defect: it silently drops the principal's accepted intent.
    for forbidden in (
        "network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted",
        "sandbox_profile: None",
    ):
        if forbidden in region:
            problems.append(
                f"{EXECUTOR}: the delegated child payload hard-codes `{forbidden}`, "
                "which drops the principal's accepted ceiling"
            )
    # The submission path must use the extracted builder rather than building
    # a payload of its own.
    submit = fn_body(text, "async fn submit_child_job_with_checkpoint")
    if submit is None:
        problems.append(f"{EXECUTOR}: submit_child_job_with_checkpoint not found")
    else:
        if "child_job_payload(" not in submit:
            problems.append(
                f"{EXECUTOR}: the delegated child job is not built through child_job_payload, "
                "so the ceiling is not guaranteed to reach the submitted payload"
            )
    return problems


def check_durable_payload_variants(root: Path) -> list[str]:
    """Check 4: every durable payload variant that can launch a process carries both axes."""
    problems: list[str] = []
    text = read(root, CORE_JOBS)
    if text is None:
        return [f"{CORE_JOBS}: unreadable"]
    body = struct_body(text, "pub enum JobPayload")
    if body is None:
        return [f"{CORE_JOBS}: JobPayload declaration not found"]
    for variant in ("Shell", "ManagedArgv", "Test", "Python"):
        match = re.search(r"\b" + variant + r"\s*\{", body)
        if not match:
            problems.append(f"{CORE_JOBS}: JobPayload::{variant} not found")
            continue
        block = brace_block(body, match.end() - 1)
        if block is None:
            problems.append(f"{CORE_JOBS}: JobPayload::{variant} has no body")
            continue
        if "network_intent" not in block:
            problems.append(f"{CORE_JOBS}: JobPayload::{variant} does not carry network_intent")
        if "sandbox_profile" not in block:
            problems.append(f"{CORE_JOBS}: JobPayload::{variant} does not carry sandbox_profile")
    return problems


def check_scheduler_forwarding(root: Path) -> list[str]:
    """Check 5: the scheduler executor forwards both axes to the process owner."""
    problems: list[str] = []
    text = read(root, SCHEDULER_EXECUTORS)
    if text is None:
        return [f"{SCHEDULER_EXECUTORS}: unreadable"]
    if "request.required_sandbox_profile = sandbox_profile;" not in text:
        problems.append(
            f"{SCHEDULER_EXECUTORS}: the managed executor does not forward the payload's "
            "sandbox_profile to the process owner"
        )
    if "request.required_network_intent = network_intent;" not in text:
        problems.append(
            f"{SCHEDULER_EXECUTORS}: the managed executor does not forward the payload's "
            "network_intent to the process owner"
        )
    return problems


def check_owner_refusal(root: Path) -> list[str]:
    """Check 6: the process owner refuses an unconstrained launch for an offline intent."""
    problems: list[str] = []
    text = read(root, MANAGED_PROCESS)
    if text is None:
        return [f"{MANAGED_PROCESS}: unreadable"]
    if "OfflineRequired currently requires a constrained filesystem profile" not in text:
        problems.append(
            f"{MANAGED_PROCESS}: the owner no longer refuses an offline intent without a "
            "constrained filesystem profile"
        )
    if "accepted OfflineRequired policy cannot run without a required sandbox launch" not in text:
        problems.append(
            f"{MANAGED_PROCESS}: the owner no longer refuses an offline intent without a "
            "required sandbox launch"
        )
    return problems


def workflow_trigger_paths(text: str, trigger: str) -> list[str]:
    """Return the `paths:` entries of one `on:` trigger block."""
    lines = text.splitlines()
    entries: list[str] = []
    inside = False
    for line in lines:
        stripped = line.strip()
        if stripped == f"{trigger}:":
            inside = True
            continue
        if inside:
            # A new key at the trigger indent ends this block.
            if line.startswith("  ") and not line.startswith("    ") and stripped.endswith(":"):
                break
            if line.startswith("      - "):
                entries.append(stripped[2:].strip().strip("'\""))
    return entries


def path_is_covered(path: str, entries: list[str]) -> bool:
    """True when a workflow `paths:` entry selects `path` (git glob semantics)."""
    for entry in entries:
        if entry == path:
            return True
        if fnmatch.fnmatch(path, entry):
            return True
    return False


def check_workflow_triggers(root: Path) -> list[str]:
    """Check 7: the native Linux workflow triggers on the paths that carry the axes.

    Both trigger blocks are checked: a path that only qualifies a pull request
    would leave a direct push to `main` unqualified.
    """
    problems: list[str] = []
    text = read(root, M004_WORKFLOW)
    if text is None:
        return [f"{M004_WORKFLOW}: unreadable"]
    required = (
        "src/scheduler/tool_program_executor.rs",
        "src/tool/tool_program.rs",
        "src/tool/tool_program_context.rs",
        "crates/codegg-core/src/jobs/mod.rs",
        "crates/codegg-core/src/approval.rs",
    )
    for trigger in ("pull_request", "push"):
        entries = workflow_trigger_paths(text, trigger)
        if not entries:
            problems.append(f"{M004_WORKFLOW}: the `{trigger}` trigger has no `paths:` list")
            continue
        for path in required:
            if not path_is_covered(path, entries):
                problems.append(
                    f"{M004_WORKFLOW}: the `{trigger}` trigger does not cover `{path}`, so a "
                    "change to the network-intent owner wiring would not be natively qualified"
                )
    return problems


CHECKS = (
    check_carrier,
    check_tool_stamp,
    check_executor_binding,
    check_durable_payload_variants,
    check_scheduler_forwarding,
    check_owner_refusal,
    check_workflow_triggers,
)


def run_checks(root: Path) -> list[str]:
    problems: list[str] = []
    for check in CHECKS:
        problems.extend(check(root))
    return problems


def mutate(root: Path, rel: Path, old: str, new: str) -> None:
    path = root / rel
    text = path.read_text()
    if old not in text:
        raise AssertionError(f"self-test fixture text not found in {rel}: {old!r}")
    path.write_text(text.replace(old, new, 1))


def self_test() -> int:
    """Prove the guard is not vacuous: each injected defect must be caught."""
    failures: list[str] = []
    cases: list[tuple[str, callable]] = []

    def case(name: str):
        def register(fn):
            cases.append((name, fn))
            return fn

        return register

    @case("delegated child payload drops the narrowed network intent")
    def _drop_intent(root: Path) -> None:
        mutate(
            root,
            EXECUTOR,
            "network_intent: child_intent,",
            "network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,",
        )

    @case("delegated child payload drops the narrowed sandbox profile")
    def _drop_profile(root: Path) -> None:
        mutate(root, EXECUTOR, "sandbox_profile: child_profile,", "sandbox_profile: None,")

    @case("the durable carrier loses its network_intent field")
    def _drop_carrier_field(root: Path) -> None:
        mutate(
            root,
            CORE_JOBS,
            "    pub network_intent: crate::approval::NetworkIntent,\n",
            "",
        )

    @case("the replay-safety digest stops covering the ceiling")
    def _drop_digest_coverage(root: Path) -> None:
        mutate(root, CORE_JOBS, "            self.network_intent.as_str(),\n", "")

    @case("the tool boundary stops stamping the ceiling")
    def _drop_stamp(root: Path) -> None:
        mutate(root, TOOL_CONTEXT, "        network_intent,\n", "        network_intent: Default::default(),\n")

    @case("the executor stops binding the ceiling")
    def _drop_binding(root: Path) -> None:
        mutate(
            root,
            EXECUTOR,
            "            .with_execution_ceiling(\n"
            "                execution_context.sandbox_profile,\n"
            "                execution_context.network_intent,\n"
            "            )",
            "            .with_execution_ceiling(None, Default::default())",
        )

    @case("the native workflow stops triggering on the Tool Program paths")
    def _drop_trigger(root: Path) -> None:
        mutate(root, M004_WORKFLOW, "      - 'src/tool/tool_program_context.rs'\n", "")

    with tempfile.TemporaryDirectory() as tmp:
        pristine = Path(tmp) / "pristine"
        shutil.copytree(ROOT, pristine, ignore=shutil.ignore_patterns("target", ".git"))
        baseline = run_checks(pristine)
        if baseline:
            failures.append(
                "the guard reports failures on an unmodified tree, so its checks are "
                f"not measuring what they claim: {baseline}"
            )

        for name, mutate_case in cases:
            work = Path(tmp) / name.replace(" ", "_").replace("/", "_")
            shutil.copytree(pristine, work)
            try:
                mutate_case(work)
            except AssertionError as exc:
                failures.append(f"{name}: fixture could not be injected: {exc}")
                continue
            problems = run_checks(work)
            if not problems:
                failures.append(f"{name}: the guard did not reject the injected defect")
            shutil.rmtree(work, ignore_errors=True)

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"network-intent-owner-wiring self-test passed: {len(cases)} injected defects "
        "were each rejected by the guard"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=ROOT,
        help="repository root to check (default: the repository this script lives in)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each injected defect instead of passing vacuously",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    problems = run_checks(args.root.resolve())
    if problems:
        for problem in problems:
            print(f"network-intent-owner-wiring guard failed: {problem}", file=sys.stderr)
        return 1
    print("network-intent-owner-wiring guard passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
