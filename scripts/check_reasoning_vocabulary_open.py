#!/usr/bin/env python3
"""Static guard for the open provider-reasoning vocabulary (M002).

The reasoning vocabulary is **open and provider-scoped** (ADR-0044): an
arbitrary vendor label such as `xhigh`, `max`, `none` or `default` is
selectable and is carried verbatim end to end. The closed four-level
`ReasoningEffort` enum that used to stand in for the vocabulary is gone,
and a daemon owns the durable selection that applies it.

This guard proves three properties that are easy to regress silently:

1. **No closed reasoning-effort enum survives.** A closed enum anywhere in
   the protocol, provider, or agent crates silently truncates the
   vocabulary: a vendor label becomes unrepresentable, and the only
   symptom is a model that quietly runs at the wrong depth.
2. **The provider bridge still carries the label verbatim.** The wire
   projection must not normalize, lower-case, or map the label onto a
   fixed level set.
3. **Every turn-admission path resolves the daemon-owned selection.** The
   three paths that used to reconstruct a turn with `reasoning: None`
   (session-prompt submit, queued-prompt delivery, delegated idle
   continuation) must call the resolution helper instead. A persisted
   selection that is dropped on one of them applies to a direct submit
   and vanishes everywhere else.

Exit code 0 if all checks pass, 1 if any fail.

`--self-test` injects each violation into a temporary copy of the tree
and proves the guard rejects it, so a green run is not vacuous.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def protocol_core(root: Path) -> Path:
    return root / "crates" / "codegg-protocol" / "src" / "core.rs"


def provider_wire(root: Path) -> Path:
    return root / "crates" / "codegg-providers" / "src" / "wire.rs"


def agent_request_preparation(root: Path) -> Path:
    """Where the protocol's label enters the provider request."""
    return root / "src" / "agent" / "request_preparation.rs"


def drop_path_sites(root: Path) -> tuple[tuple[str, Path], ...]:
    """The three turn-admission paths that used to drop the selection."""
    return (
        ("session_prompt_submit", root / "src" / "core" / "daemon_turns.rs"),
        ("queued_prompt_delivery", root / "src" / "core" / "daemon_queue_delivery.rs"),
        (
            "delegated_idle_continuation",
            root / "src" / "core" / "daemon_continuation.rs",
        ),
    )


def vocabulary_scan_roots(root: Path) -> tuple[Path, ...]:
    """Crates that must not reintroduce a closed reasoning-effort level set."""
    return (
        root / "crates" / "codegg-protocol" / "src",
        root / "crates" / "codegg-providers" / "src",
        root / "src" / "agent",
    )

# A closed reasoning-effort level set: an enum whose variants are all drawn
# from the effort-level vocabulary. The *variants* are the signal, not the
# name — an earlier version of this guard matched only names containing
# "Reasoning", and a closed set named `ThinkingEffortLevel` walked straight
# through it.
REASONING_LEVEL_VARIANTS = (
    "Minimal",
    "Low",
    "Medium",
    "High",
    "Max",
    "Xhigh",
    "None",
    "Off",
    "On",
    "Default",
)
# A closed set needs at least two levels to be a closed *level set*. One
# lone variant is not a vocabulary.
CLOSED_ENUM_MIN_VARIANTS = 2
CLOSED_ENUM_RE = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?enum\s+(?P<name>\w+)\s*\{(?P<body>[^}]*)\}",
    re.DOTALL,
)


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _strip_test_modules(source: str) -> str:
    """Remove `#[cfg(test)]` module blocks, keeping the rest of the file.

    Truncating at the **first** `#[cfg(test)]` marker is wrong: a file may
    carry a small test module in the middle, and truncating there hides every
    line of production code that follows — which is exactly the code a guard
    about turn-admission paths has to see. The blocks are removed by brace
    matching instead, so the production half is the whole file minus its
    test modules.
    """
    out: list[str] = []
    index = 0
    length = len(source)
    while True:
        marker = source.find("#[cfg(test)]", index)
        if marker < 0:
            out.append(source[index:])
            break
        out.append(source[index:marker])
        brace = source.find("{", marker)
        if brace < 0:
            out.append(source[marker:])
            break
        depth = 0
        cursor = brace
        in_string = False
        escaped = False
        while cursor < length:
            char = source[cursor]
            if in_string:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
            elif char == '"':
                in_string = True
            elif char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    cursor += 1
                    break
            cursor += 1
        index = cursor
    return "".join(out)


def _production_source(source: str) -> str:
    """The production half of a file, with its test modules removed.

    A test fixture may legitimately build a request with an explicit
    `reasoning: None` — that is how "a client that sends no override" is
    expressed. Only the production turn-admission sites are guarded.
    """
    return _strip_test_modules(source)


def _closed_reasoning_enums(source: str) -> list[str]:
    """Names of enums that look like a closed reasoning-effort level set."""
    offenders: list[str] = []
    for match in CLOSED_ENUM_RE.finditer(source):
        body = match.group("body")
        variants = [
            line.strip().rstrip(",")
            for line in body.splitlines()
            if line.strip() and not line.strip().startswith("//")
        ]
        if not variants:
            continue
        names = {variant.split()[0] for variant in variants if variant.split()}
        if len(names) < CLOSED_ENUM_MIN_VARIANTS:
            continue
        if names.issubset(set(REASONING_LEVEL_VARIANTS)):
            offenders.append(match.group("name"))
    return offenders


def check_no_closed_reasoning_enum(root: Path) -> bool:
    """No closed reasoning-effort level set survives in the vocabulary crates."""
    ok = True
    for scan_root in vocabulary_scan_roots(root):
        if not scan_root.is_dir():
            print(f"  FAIL: vocabulary scan root missing: {scan_root}")
            ok = False
            continue
        for path in sorted(scan_root.rglob("*.rs")):
            offenders = _closed_reasoning_enums(_read(path))
            for name in offenders:
                print(
                    f"  FAIL: {path.relative_to(root)} declares the closed "
                    f"reasoning level enum `{name}`; the vocabulary is open and "
                    "provider-scoped (ADR-0044)"
                )
                ok = False
    return ok


def check_protocol_carries_an_open_label(root: Path) -> bool:
    """The protocol's effort selection carries an open, provider-scoped string."""
    source = _read(protocol_core(root))
    if not re.search(r"pub const KNOWN_REASONING_EFFORT_LABELS", source):
        print("  FAIL: protocol does not publish the known convenience labels")
        return False
    effort = re.search(
        r"Effort\s*\{\s*level:\s*String\s*\}", source
    )
    if not effort:
        print(
            "  FAIL: the protocol's reasoning effort selection does not carry a "
            "String label; an open label must be a provider-scoped string"
        )
        return False
    if "ReasoningEffort" in source and "pub enum ReasoningEffort" in source:
        print("  FAIL: the protocol still declares `enum ReasoningEffort`")
        return False
    return True


def check_provider_bridge_carries_the_label_verbatim(root: Path) -> bool:
    """The provider bridge must not normalize or translate the label.

    The label is written into the request in **two** places, and both have to
    carry it verbatim: the agent layer that turns the protocol's selection
    into the provider request, and the provider bridge that projects the
    request onto the kernel's intent. An earlier version of this guard
    scanned only the bridge, so a lower-casing in the agent layer — the place
    the label actually enters the request — passed unnoticed.
    """
    bridge_path = provider_wire(root)
    bridge_source = _read(bridge_path)
    ok = True
    if "fn reasoning_intent" not in bridge_source:
        print("  FAIL: the provider bridge no longer projects the reasoning intent")
        ok = False
    elif not re.search(r"effort:\s*Some\(label\.clone\(\)\)", bridge_source):
        print(
            "  FAIL: the provider bridge no longer carries the reasoning label "
            "verbatim; the label must reach the wire unchanged"
        )
        ok = False

    # The agent layer, where the protocol's label enters the request.
    agent_path = agent_request_preparation(root)
    if not agent_path.is_file():
        print(f"  FAIL: agent request-preparation source missing: {agent_path}")
        return False
    agent_source = _production_source(_read(agent_path))
    if "request.reasoning_effort = Some(level)" not in agent_source:
        print(
            "  FAIL: the agent layer no longer carries the protocol's effort "
            "label into the request verbatim"
        )
        ok = False

    # The patterns are anchored on the *label* identifiers. A blanket
    # `to_lowercase()` would flag an unrelated prompt normalisation — the
    # agent layer lower-cases prompts for its own reasons.
    for source, description, path in (
        (bridge_source, "the provider bridge normalizes the label", bridge_path),
        (agent_source, "the agent layer normalizes the label", agent_path),
    ):
        for pattern, why in (
            (r"\blabel\.to_lowercase\(\)", "the label must not be lower-cased"),
            (r"\blevel\.to_lowercase\(\)", "the label must not be lower-cased"),
            (r"\blabel\.to_uppercase\(\)", "the label must not be upper-cased"),
            (r"\blevel\.to_uppercase\(\)", "the label must not be upper-cased"),
            (r"\blabel\.trim\(\)", "the label must not be trimmed"),
            (r"\blevel\.trim\(\)", "the label must not be trimmed"),
            (
                r"match\s+label\.as_str\(\)\s*\{",
                "the label must not be mapped onto a closed level set",
            ),
            (
                r"match\s+level\.as_str\(\)\s*\{",
                "the label must not be mapped onto a closed level set",
            ),
        ):
            if re.search(pattern, source):
                print(f"  FAIL: {description}: {why} ({path.name})")
                ok = False
    return ok


def check_drop_paths_resolve_the_selection(root: Path) -> bool:
    """Each turn-admission path resolves the daemon-owned selection."""
    ok = True
    for name, path in drop_path_sites(root):
        if not path.is_file():
            print(f"  FAIL: {name} site missing: {path}")
            ok = False
            continue
        source = _read(path)
        production = _production_source(source)
        if "resolve_reasoning_selection" not in production:
            print(
                f"  FAIL: {name} ({path.relative_to(root)}) does not resolve "
                "the daemon-owned reasoning selection"
            )
            ok = False
            continue
        # The field initializer ends at a `,` or a `}`. Requiring the comma
        # would miss `reasoning: None` written as the *last* field of a
        # struct literal, which is legal Rust — and a path that drops the
        # selection that way is exactly what this rule exists to catch.
        if re.search(r"reasoning:\s*None\s*[,}]", production):
            print(
                f"  FAIL: {name} ({path.relative_to(root)}) still hard-codes "
                "`reasoning: None`; the persisted selection must be applied there"
            )
            ok = False
    return ok


def _self_test() -> int:
    """Prove the guard rejects each violation it claims to catch."""
    failures = 0

    def run_case(description: str, mutate) -> None:
        nonlocal failures
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            tree = tmp / "tree"
            shutil.copytree(
                REPO_ROOT,
                tree,
                ignore=shutil.ignore_patterns(
                    ".git", "target", "node_modules", "__pycache__"
                ),
            )
            mutate(tree)
            results = {
                "closed enum": check_no_closed_reasoning_enum(tree),
                "open label": check_protocol_carries_an_open_label(tree),
                "verbatim bridge": check_provider_bridge_carries_the_label_verbatim(tree),
                "drop paths": check_drop_paths_resolve_the_selection(tree),
            }
            rejected = not all(results.values())
            if rejected:
                print(f"  [PASS] self-test: {description} is rejected")
            else:
                print(f"  [FAIL] self-test: {description} was NOT rejected")
                failures += 1

    def inject_closed_enum(tree: Path) -> None:
        target = tree / "crates" / "codegg-protocol" / "src" / "core.rs"
        target.write_text(
            target.read_text(encoding="utf-8")
            + "\npub enum ReasoningEffort {\n    Minimal,\n    Low,\n    Medium,\n    High,\n}\n",
            encoding="utf-8",
        )

    def inject_closed_string_label(tree: Path) -> None:
        target = tree / "crates" / "codegg-protocol" / "src" / "core.rs"
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace("Effort { level: String }", "Effort { level: u8 }"),
            encoding="utf-8",
        )

    def inject_normalized_bridge(tree: Path) -> None:
        target = tree / "crates" / "codegg-providers" / "src" / "wire.rs"
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "effort: Some(label.clone())",
                "effort: Some(label.to_lowercase())",
            ),
            encoding="utf-8",
        )

    def inject_dropped_path(tree: Path) -> None:
        target = tree / "src" / "core" / "daemon_queue_delivery.rs"
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace("resolve_reasoning_selection", "ignored_resolution"),
            encoding="utf-8",
        )

    def inject_closed_enum_without_reasoning_in_the_name(tree: Path) -> None:
        """A closed level set whose name does not say "Reasoning"."""
        target = tree / "crates" / "codegg-protocol" / "src" / "core.rs"
        target.write_text(
            target.read_text(encoding="utf-8")
            + "\npub enum ThinkingEffortLevel {\n    Minimal,\n    Low,\n    Medium,\n    High,\n}\n",
            encoding="utf-8",
        )

    def inject_trailing_field_none(tree: Path) -> None:
        """A dropped path written as `reasoning: None` with no trailing comma.

        The field is written **last** in the struct literal, so there is no
        comma after it. A rule that requires the comma misses it entirely.
        """
        target = tree / "src" / "core" / "daemon_queue_delivery.rs"
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "                reasoning,\n"
                "                human_shell_promotion_ids: Vec::new(),\n",
                "                human_shell_promotion_ids: Vec::new(),\n"
                "                reasoning: None\n",
            ),
            encoding="utf-8",
        )

    def inject_normalized_agent_layer(tree: Path) -> None:
        """A label lower-cased where it enters the request.

        The bridge is untouched; the agent layer is where the protocol's
        label is written into the provider request.
        """
        target = tree / "src" / "agent" / "request_preparation.rs"
        source = target.read_text(encoding="utf-8")
        target.write_text(
            source.replace(
                "request.reasoning_effort = Some(level);",
                "request.reasoning_effort = Some(level.to_lowercase());",
            ),
            encoding="utf-8",
        )

    run_case("a closed reasoning-effort enum", inject_closed_enum)
    run_case("a non-string protocol label", inject_closed_string_label)
    run_case("a normalized provider bridge", inject_normalized_bridge)
    run_case("a dropped turn-admission path", inject_dropped_path)
    run_case(
        "a closed level set whose name does not say 'Reasoning'",
        inject_closed_enum_without_reasoning_in_the_name,
    )
    run_case(
        "a dropped path written as a trailing `reasoning: None`",
        inject_trailing_field_none,
    )
    run_case("a label normalized in the agent layer", inject_normalized_agent_layer)

    if failures:
        print(f"{failures} self-test case(s) FAILED.")
        return 1
    print("All guard self-test cases passed.")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    checks = (
        ("no closed reasoning-effort enum survives", check_no_closed_reasoning_enum),
        ("the protocol carries an open label", check_protocol_carries_an_open_label),
        (
            "the provider bridge carries the label verbatim",
            check_provider_bridge_carries_the_label_verbatim,
        ),
        (
            "every turn-admission path resolves the selection",
            check_drop_paths_resolve_the_selection,
        ),
    )
    results = [(name, fn(REPO_ROOT)) for name, fn in checks]

    print()
    passed = sum(1 for _, ok in results if ok)
    failed = sum(1 for _, ok in results if not ok)
    for name, ok in results:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    print(f"\n{passed}/{len(results)} checks passed.")
    if failed:
        print(f"{failed} check(s) FAILED.")
        return 1
    print("All open reasoning vocabulary invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
