#!/usr/bin/env python3
"""Guard: the shared-checkout profile must stay opt-in, mediated and gated.

WHAT THIS GUARD DOES
    M005 corrective C006 adds the restricted **shared checkout** profile: two
    bounded agents on one managed checkout, with CodeGG-mediated serialized
    native writes and every process-capable surface refused. This guard is
    the static backstop for the parts of that contract that a static check
    can honestly see:

    1. The contract's own landmarks must exist: the placement type and its
       platform gate, the shared-checkout coordinator, the daemon's writer
       fence factory, the executor's shared branch, the task tool's opt-in
       parsing and the worker's use of the resolved fence. Losing one is not
       a style change; it is the contract disappearing.
    2. The profile must stay **opt-in**. The default placement must remain
       the isolated one, and the durable payload field must remain defaulted,
       so a payload written before the placement existed still decodes as the
       default and an older client keeps the behaviour it had.
    3. The shared profile must deny every tool that can reach a process in
       the checkout. The code's own capability table is the source of truth
       for which tools those are, and the shared-profile denial list must
       cover all of them.
    4. The shared placement must consult the C005 reservation preflight, and
       the executor must attach the run to the checkout it resolved — the
       fence reads the run's own durable record, so an unattached run would
       hold no authority at all.
    5. The daemon must wire the authority: the coordinator and the fence
       factory are what make the profile real rather than advisory.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not decide whether a checkout is *safe* to share, and it does not
    grant any permission. It reasons about declared landmarks, defaults and
    table agreement -- never about command text or file contents.

    It also does not pretend that a landmark proves the behaviour. The proof
    is the fixture suite (``tests/shared_checkout_c006.rs``); this guard
    proves the wiring is still there, which is the failure mode a static
    check can catch.

    It does not require the shared profile to be available on every
    platform. The platform gate is part of the contract: a platform without
    native containment evidence must have the profile **disabled**, and this
    guard fails if the gate itself is removed.

USAGE
    python3 scripts/check_shared_checkout.py            # check the tree
    python3 scripts/check_shared_checkout.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# --- The C006 contract's landmarks -----------------------------------------

# Each landmark is a file plus the markers that must be present in it. A
# landmark is the *contract itself*: the placement, the authority, the
# refusal points.
CONTRACT_LANDMARKS: list[tuple[str, tuple[str, ...]]] = [
    (
        "crates/codegg-core/src/run_placement.rs",
        (
            "pub enum RunPlacement",
            "SerializedShared",
            "pub fn shared_checkout_availability",
            "QUALIFIED_SHARED_CHECKOUT_PLATFORMS",
        ),
    ),
    (
        "crates/codegg-core/src/shared_checkout.rs",
        (
            "pub struct SharedCheckoutCoordinator",
            "pub async fn acquire",
            "pub async fn resolve",
            "pub enum SharedCheckoutError",
            "preflight_reservation",
        ),
    ),
    (
        "crates/codegg-core/src/jobs/mod.rs",
        ("placement: crate::run_placement::RunPlacement",),
    ),
    (
        "src/core/writer_fence.rs",
        (
            "pub struct DaemonWriterFenceFactory",
            "impl crate::tool::writer_fence::WriterFenceFactory for DaemonWriterFenceFactory",
        ),
    ),
    (
        "src/scheduler/executors.rs",
        (
            "c006-shared-placement",
            "pub struct SubagentAuthority",
            "pub fn with_authority",
        ),
    ),
    (
        "src/tool/task.rs",
        (
            "fn parse_run_placement",
            "serialized_shared",
            "shared_profile_denied_tools",
        ),
    ),
    (
        "src/tool/process_capability.rs",
        ("pub fn shared_profile_denied_tools",),
    ),
    (
        "src/agent/worker.rs",
        ("pub writer_fence: Option<crate::tool::writer_fence::SharedWriterFence>",),
    ),
    (
        "src/core/daemon_construct.rs",
        (
            "SharedCheckoutCoordinator::new",
            "DaemonWriterFenceFactory::new",
        ),
    ),
]

CAPABILITY_TABLE_PATH = "src/tool/process_capability.rs"
PLACEMENT_PATH = "crates/codegg-core/src/run_placement.rs"
PAYLOAD_PATH = "crates/codegg-core/src/jobs/mod.rs"
EXECUTOR_PATH = "src/scheduler/executors.rs"
TASK_PATH = "src/tool/task.rs"

# The capability table's entries: a tool name and the shape of its effective
# runtime effect.
_TABLE_ENTRY = re.compile(
    r'\(\s*"(?P<name>[a-z_]+)"\s*,\s*'
    r"ToolProcessCapability::(?P<shape>Spawns|Delegates|UsesPersistentProcess|SubmitsProcessWork)"
)

# The tools a shared checkout denies: the ones that start a process in it and
# the ones that use a persistent process the host owns.
SHARED_DENIED_SHAPES = ("Spawns", "UsesPersistentProcess")


def _normalize(text: str) -> str:
    """A marker compared against source that rustfmt may have reflowed."""
    collapsed = "".join(text.split())
    return collapsed.replace(",)", ")")


def capability_entries(source: str) -> list[tuple[str, str]]:
    """The code's declared tool capabilities, in declaration order."""
    entries: list[tuple[str, str]] = []
    for match in _TABLE_ENTRY.finditer(source):
        entries.append((match.group("name"), match.group("shape")))
    return entries


def check_contract_landmarks(tree_root: Path = ROOT) -> list[str]:
    """The C006 contract's own landmarks must exist."""
    problems: list[str] = []
    for relative, markers in CONTRACT_LANDMARKS:
        path = tree_root / relative
        if not path.is_file():
            problems.append(f"the C006 contract landmark '{relative}' is missing")
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        for marker in markers:
            if _normalize(marker) not in _normalize(source):
                problems.append(
                    f"the C006 contract landmark '{relative}' lost '{marker}'"
                )
    return problems


def check_opt_in(tree_root: Path = ROOT) -> list[str]:
    """The shared profile must stay opt-in."""
    problems: list[str] = []

    placement_path = tree_root / PLACEMENT_PATH
    if not placement_path.is_file():
        problems.append(f"the placement type '{PLACEMENT_PATH}' is missing")
    else:
        source = placement_path.read_text(encoding="utf-8", errors="replace")
        # The default placement is the isolated one.
        if not re.search(r"#\[default\]\s*\n\s*Auto,", source):
            problems.append(
                "the default placement is no longer the isolated one: the shared "
                "profile must stay opt-in"
            )
        # The platform gate must survive: it is what keeps the profile off a
        # platform with no native containment evidence.
        if "shared_checkout_availability" not in source:
            problems.append("the placement lost its platform availability gate")

    payload_path = tree_root / PAYLOAD_PATH
    if not payload_path.is_file():
        problems.append(f"the durable payload '{PAYLOAD_PATH}' is missing")
    else:
        source = payload_path.read_text(encoding="utf-8", errors="replace")
        match = re.search(
            r"placement:\s*crate::run_placement::RunPlacement", source
        )
        if match is None:
            problems.append("the durable job payload lost its placement field")
        else:
            # The field must be defaulted, so a payload written before the
            # placement existed still decodes.
            window = source[max(0, match.start() - 400) : match.start()]
            if "#[serde(default)]" not in window:
                problems.append(
                    "the durable placement field is not defaulted: an older "
                    "payload would fail to decode"
                )

    executor_path = tree_root / EXECUTOR_PATH
    if executor_path.is_file():
        source = executor_path.read_text(encoding="utf-8", errors="replace")
        # The executor must attach the run to the checkout it resolved: the
        # fence reads the run's own durable record.
        if "attach_worktree" not in source:
            problems.append(
                "the executor no longer attaches the run to its checkout: the "
                "writer fence would resolve no scope"
            )
    return problems


def check_shared_profile_denials(tree_root: Path = ROOT) -> list[str]:
    """The shared profile must deny every tool that can reach a process.

    The denial list is *computed* from the capability table rather than
    hardcoded, so the check is about the computation: the list must be
    derived from the table, and it must select both shapes that can reach a
    process in the checkout — the tools that start one and the tools that
    use a persistent one the host owns.

    Which tools those are is the capability table's business, and the C005
    guard keeps that table honest. The Rust suite
    (``tests/shared_checkout_c006.rs``) proves the resulting list is the
    expected one; this guard proves the *computation* is still there.
    """
    problems: list[str] = []
    table_path = tree_root / CAPABILITY_TABLE_PATH
    if not table_path.is_file():
        return [f"the capability table '{CAPABILITY_TABLE_PATH}' is missing"]
    source = table_path.read_text(encoding="utf-8", errors="replace")
    entries = capability_entries(source)
    if not entries:
        return ["the capability table declares no tool capability"]
    expected = {
        name for name, shape in entries if shape in SHARED_DENIED_SHAPES
    }
    if not expected:
        return ["the capability table classifies no tool as a shared-profile denial"]

    match = re.search(
        r"pub fn shared_profile_denied_tools.*?\n\}", source, re.DOTALL
    )
    if match is None:
        return ["the shared profile lost its denial list"]
    body = match.group(0)
    if "BUILTIN_PROCESS_CAPABILITIES" not in body:
        problems.append(
            "the shared profile's denial list is no longer derived from the "
            "capability table: a tool that can reach a process would not be "
            "refused"
        )
    for predicate in ("spawns_itself()", "uses_persistent_process()"):
        if predicate not in body:
            problems.append(
                f"the shared profile's denial list no longer selects "
                f"'{predicate}': a tool that can reach a process in the shared "
                "checkout would not be refused"
            )

    # The task tool must apply the list rather than compute its own.
    task_path = tree_root / TASK_PATH
    if task_path.is_file():
        task_source = task_path.read_text(encoding="utf-8", errors="replace")
        if "shared_profile_denied_tools" not in task_source:
            problems.append(
                "the task tool no longer applies the shared-profile denial list: "
                "a shared child would keep its process-capable tools"
            )
    return problems


def check_preflight(tree_root: Path = ROOT) -> list[str]:
    """The shared placement must consult the C005 reservation preflight."""
    problems: list[str] = []
    coordinator = tree_root / "crates/codegg-core/src/shared_checkout.rs"
    if coordinator.is_file():
        source = coordinator.read_text(encoding="utf-8", errors="replace")
        if "preflight_reservation" not in source:
            problems.append(
                "the shared-checkout coordinator lost the C005 reservation "
                "preflight: a live persistent process would no longer refuse "
                "the placement"
            )
    return problems


def check_tree() -> list[str]:
    problems: list[str] = []
    problems.extend(check_contract_landmarks())
    problems.extend(check_opt_in())
    problems.extend(check_shared_profile_denials())
    problems.extend(check_preflight())
    return problems


# --- Self-test -------------------------------------------------------------


def _run_against(tree: Path) -> list[str]:
    """Run the guard's checks against an alternate tree."""
    problems: list[str] = []
    problems.extend(check_contract_landmarks(tree))
    problems.extend(check_opt_in(tree))
    problems.extend(check_shared_profile_denials(tree))
    problems.extend(check_preflight(tree))
    return problems


def _copy_tree(destination: Path) -> Path:
    """A writable copy of the parts of the tree the guard reads."""
    relatives = {relative for relative, _ in CONTRACT_LANDMARKS}
    relatives.update({CAPABILITY_TABLE_PATH, PLACEMENT_PATH, PAYLOAD_PATH})
    for relative in sorted(relatives):
        source = ROOT / relative
        if not source.is_file():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return destination


def self_test() -> int:
    failures: list[str] = []

    def case(name: str, mutate, expect_fragment: str) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tree = _copy_tree(Path(tmp))
            mutate(tree)
            problems = _run_against(tree)
            if not any(expect_fragment in problem for problem in problems):
                failures.append(
                    f"the '{name}' violation was not reported (expected a problem "
                    f"containing '{expect_fragment}')"
                )

    def drop_marker(relative: str, marker: str):
        def mutate(tree: Path) -> None:
            path = tree / relative
            text = path.read_text(encoding="utf-8")
            path.write_text(text.replace(marker, "removed-by-self-test"), encoding="utf-8")

        return mutate

    def make_the_default_shared(tree: Path) -> None:
        path = tree / PLACEMENT_PATH
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace("    #[default]\n    Auto,", "    SerializedShared { group_id: AgentRunGroupId::new() },"),
            encoding="utf-8",
        )

    def undefault_the_payload(tree: Path) -> None:
        path = tree / PAYLOAD_PATH
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace(
                "        #[serde(default)]\n        placement: crate::run_placement::RunPlacement,",
                "        placement: crate::run_placement::RunPlacement,",
            ),
            encoding="utf-8",
        )

    def drop_a_shared_denial(tree: Path) -> None:
        path = tree / CAPABILITY_TABLE_PATH
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace(
                "capability.spawns_itself() || capability.uses_persistent_process()",
                "capability.spawns_itself()",
                1,
            ),
            encoding="utf-8",
        )

    def hardcode_the_denial_list(tree: Path) -> None:
        path = tree / CAPABILITY_TABLE_PATH
        text = path.read_text(encoding="utf-8")
        start = text.index("pub fn shared_profile_denied_tools")
        head, tail = text[:start], text[start:]
        path.write_text(
            head + tail.replace("BUILTIN_PROCESS_CAPABILITIES", "&[]", 1),
            encoding="utf-8",
        )

    def drop_the_preflight(tree: Path) -> None:
        path = tree / "crates/codegg-core/src/shared_checkout.rs"
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace("preflight_reservation", "removed-by-self-test"),
            encoding="utf-8",
        )

    def drop_the_executor_branch(tree: Path) -> None:
        path = tree / EXECUTOR_PATH
        text = path.read_text(encoding="utf-8")
        path.write_text(
            text.replace("c006-shared-placement", "removed-by-self-test"),
            encoding="utf-8",
        )

    case(
        "the placement type lost its shared variant",
        drop_marker(PLACEMENT_PATH, "SerializedShared"),
        "lost 'SerializedShared'",
    )
    case(
        "the default placement became the shared one",
        make_the_default_shared,
        "the default placement is no longer the isolated one",
    )
    case(
        "the durable placement field stopped being defaulted",
        undefault_the_payload,
        "is not defaulted",
    )
    case(
        "the shared profile stopped denying the persistent-process tools",
        drop_a_shared_denial,
        "no longer selects 'uses_persistent_process()'",
    )
    case(
        "the shared profile's denial list stopped being derived from the table",
        hardcode_the_denial_list,
        "no longer derived from the capability table",
    )
    case(
        "the shared placement lost the C005 reservation preflight",
        drop_the_preflight,
        "lost the C005 reservation",
    )
    case(
        "the executor's shared branch lost its marker",
        drop_the_executor_branch,
        "lost 'c006-shared-placement'",
    )

    if failures:
        for failure in failures:
            print(f"shared-checkout self-test FAIL: {failure}", file=sys.stderr)
        return 1
    print("shared-checkout self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard fails on injected violations",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    problems = check_tree()
    if problems:
        for problem in problems:
            print(f"shared-checkout check: {problem}", file=sys.stderr)
        return 1
    print(
        "shared-checkout check ok: "
        f"{len(CONTRACT_LANDMARKS)} contract landmarks, "
        "opt-in default, shared-profile denials and C005 preflight verified"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
