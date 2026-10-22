#!/usr/bin/env python3
"""Static guard for the fidelity of the reasoning intent at the wire boundary.

The provider bridge (`crates/codegg-providers/src/wire.rs`) is CodeGG's only
seam onto the pinned `eggpool-wire` kernel. The kernel's `ReasoningIntent`
carries five fields, and the two that matter for a *selection* are `effort`
(an open, provider-scoped label) and `budget_tokens` (a token count). Two more
carry the *request's own state*: `requested` (`None` = no preference,
`Some(false)` = explicitly off) and `explicit_disable`.

M001 of `provider-reasoning-control` found the bridge collapsing that state.
It built the intent from two nullable request fields alone, so it could only
ever produce three shapes:

* a fixed budget (`requested: Some(true)`, `mode: FixedBudget`),
* an effort label (`requested: Some(true)`, `mode: Effort`), or
* the default (`requested: None`).

`explicit_disable` was hard-coded to `false` at the single construction site,
so the kernel's explicit-disable intent (`ReasoningIntent::disabled()`) was
never constructed and "off" was indistinguishable from "no preference" on
every surface. `ReasoningMode::Adaptive` and `ReasoningMode::Toggle` were
unreachable, and because only `Effort` and `FixedBudget` were reachable an
effort selection was a no-op on the Anthropic-compatible surfaces while a
budget selection was a no-op on the OpenAI-compatible ones.

That is a *silent* loss: the request looked honoured and the provider
received something else. A guard is the right instrument because the loss is
invisible at review time — the code compiles, the tests pass, and the wire
still carries a reasoning field, just the wrong one.

This guard asserts that the bridge cannot go back to that shape:

1. No `explicit_disable: false` literal anywhere in the provider crate. The
   explicit disable is the kernel's own constructor
   (`ReasoningIntent::disabled()`); every other case inherits the kernel
   default. A literal at a construction site is exactly the bug.
2. No closed reasoning-effort enum in the provider crate. The effort
   vocabulary is provider-scoped and open — a vendor label such as `xhigh`
   has to reach the wire verbatim — so a closed CodeGG enum would silently
   drop or rewrite a legitimate label.
3. No closed effort-label *ladder* in the provider crate. A `match` over a
   fixed set of label strings is the same closed vocabulary wearing a
   different syntax, and it would rewrite a vendor label the same way.
4. The bridge still reaches every mode it must: the kernel's
   `ReasoningIntent::default()` for the unspecified intent, the kernel's
   `ReasoningIntent::disabled()` for the explicit disable, the kernel's
   `ReasoningIntent::fixed(..)` for the budget, and the `Effort` and
   `Adaptive` modes. Without this the guard could pass on a bridge that had
   simply deleted the distinctions.
5. The bridge does not **hand-roll** a disable. `requested: Some(false)`,
   `explicit_disable: true`, and `ReasoningMode::Toggle` are the shapes a
   literal disable would take; the disable must be the kernel's own
   constructor, so none of them may be written in the bridge.

Rule 4 is what keeps the guard from being satisfied by deleting the feature.
Rules 1 and 2 are what keep the two ways this regressed from coming back.
Rule 3 is the ladder-shaped spelling of the same regression.

**The scan looks at code, not at text.** A doc comment that *names* the
forbidden shape must not trip a rule, and a **string literal** must not
satisfy a rule that asks whether the code *uses* a constructor. An earlier
version of this guard was audited and three bypasses were demonstrated
against it:

* a bridge whose only mention of `ReasoningIntent::disabled()` was inside a
  string literal passed rule 4;
* a file containing `const MARKER: &str = "/*";` made the comment stripper
  treat the rest of the file as a block comment, hiding a hard-coded
  `explicit_disable: false` from rule 1;
* a hand-rolled `ReasoningIntent { requested: Some(false), mode:
  ReasoningMode::Toggle, .. }` wrote no `explicit_disable: false` literal
  and so passed rules 1 and 2.

So the scanner below is string-aware: it consumes string and character
literals before it looks for comment delimiters, and it can blank literal
*contents* so that a rule asking "does the code use this?" only ever sees
real code. The `--self-test` injects each of those three bypasses, plus the
original three, and proves every one of them is rejected.

Run with `--self-test` to prove the guard fails on injected violations. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
# The whole provider crate: the literal must not reappear at any construction
# site, not only at the canonical request builder.
PROVIDER_CRATE = REPO_ROOT / "crates" / "codegg-providers" / "src"
# CodeGG's only seam onto the pinned kernel's reasoning vocabulary.
BRIDGE = PROVIDER_CRATE / "wire.rs"

# The literal that made the kernel's explicit-disable intent unreachable. A
# struct-literal field initializer is always followed by `,` or `}`, so the
# anchor keeps a doc comment that merely *names* the shape from tripping the
# guard.
EXPLICIT_DISABLE_FALSE = re.compile(r"explicit_disable\s*:\s*false\s*[,}]")

# A closed CodeGG effort vocabulary. The names are the ones a closed ladder
# would take; the effort label on the wire is an open, provider-scoped string.
# The list is deliberately wide: the regression does not need a suggestive
# name to come back.
CLOSED_EFFORT_ENUM = re.compile(
    r"\benum\s+(?:[A-Za-z_][A-Za-z0-9_]*::)*"
    r"(?:ReasoningEffort|ReasoningEffortLevel|EffortLevel|EffortTier|ReasoningLevel"
    r"|ThinkingLevel|ThinkingEffort|ThinkingTier|Effort)\b"
)

# A closed effort-label ladder: a match arm whose pattern is one of the
# canonical ladder labels. Three or more distinct labels in match-arm
# position is a closed vocabulary; a single label in a comparison is not.
LADDER_ARM = re.compile(r'"(minimal|low|medium|high)"\s*(?:\|[^=\n]*)?=>')
LADDER_LABELS = ("minimal", "low", "medium", "high")

# The kernel constructors the bridge must use, and the modes it must reach.
# `ReasoningIntent::default()` is the unspecified intent,
# `ReasoningIntent::disabled()` is the explicit disable, and
# `ReasoningIntent::fixed(..)` is the fixed budget; all three are the kernel's
# own constructors, never hand-rolled literals. The effort and adaptive modes
# have no kernel constructor, so the bridge names them directly.
REQUIRED_BRIDGE_TOKENS = (
    "ReasoningIntent::default()",
    "ReasoningIntent::disabled()",
    "ReasoningIntent::fixed(",
    "ReasoningMode::Effort",
    "ReasoningMode::Adaptive",
)

# The shapes a hand-rolled explicit disable would take. None of them may be
# written in the bridge: the disable must be the kernel's own constructor.
HAND_ROLLED_DISABLE = re.compile(
    r"requested\s*:\s*Some\(\s*false\s*\)|explicit_disable\s*:\s*true|ReasoningMode::Toggle"
)


def provider_sources() -> list[Path]:
    return sorted(PROVIDER_CRATE.rglob("*.rs"))


def _blank(chunk: str) -> str:
    """Blank a chunk, preserving newlines so line numbers stay honest."""
    return "".join("\n" if char == "\n" else " " for char in chunk)


def _scan(source: str, blank_strings: bool) -> str:
    """Scan Rust source, blanking comments (and optionally string contents).

    The scanner is **string-aware**: a string or character literal is consumed
    as a unit before any comment delimiter inside it can be mistaken for a
    comment. An earlier, string-blind version of this guard was bypassed by a
    file containing `const MARKER: &str = "/*";`, which made the rest of the
    file read as a block comment and hid a hard-coded `explicit_disable:
    false` from the check.

    With `blank_strings=True` the *contents* of every literal are blanked as
    well, so a token that appears only inside a string literal cannot satisfy
    a rule that asks whether the code uses it.
    """
    out: list[str] = []
    index = 0
    length = len(source)
    while index < length:
        char = source[index]

        # A raw string: r"…", r#"…"#, r##"…"##.
        if char == "r" and index + 1 < length and source[index + 1] in '"#':
            cursor = index + 1
            hashes = 0
            while cursor < length and source[cursor] == "#":
                hashes += 1
                cursor += 1
            if cursor < length and source[cursor] == '"':
                terminator = '"' + ("#" * hashes)
                end = source.find(terminator, cursor + 1)
                end = length if end == -1 else end + len(terminator)
                chunk = source[index:end]
                out.append(_blank(chunk) if blank_strings else chunk)
                index = end
                continue

        # An ordinary string literal, with backslash escapes.
        if char == '"':
            cursor = index + 1
            while cursor < length:
                if source[cursor] == "\\":
                    cursor += 2
                    continue
                if source[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            chunk = source[index:cursor]
            out.append(_blank(chunk) if blank_strings else chunk)
            index = cursor
            continue

        # A character literal. A lifetime (`'a`) matches neither shape and
        # falls through as ordinary text.
        if char == "'":
            cursor = index + 1
            if cursor < length and source[cursor] == "\\":
                while cursor < length and source[cursor] != "'":
                    cursor += 1
                cursor = min(cursor + 1, length)
                chunk = source[index:cursor]
                out.append(_blank(chunk) if blank_strings else chunk)
                index = cursor
                continue
            if cursor + 1 < length and source[cursor + 1] == "'":
                cursor += 2
                chunk = source[index:cursor]
                out.append(_blank(chunk) if blank_strings else chunk)
                index = cursor
                continue

        if source.startswith("/*", index):
            end = source.find("*/", index + 2)
            end = length if end == -1 else end + 2
            out.append(_blank(source[index:end]))
            index = end
            continue

        if source.startswith("//", index):
            end = source.find("\n", index)
            end = length if end == -1 else end
            out.append(" " * (end - index))
            index = end
            continue

        out.append(char)
        index += 1
    return "".join(out)


def _code(source: str) -> str:
    """The source with comments and string contents blanked."""
    return _scan(source, blank_strings=True)


def _implementation(bridge: str) -> str:
    """The bridge's implementation, without its in-module test module."""
    return bridge.split("#[cfg(test)]", 1)[0]


def check_no_hardcoded_explicit_disable(sources: dict[str, str]) -> list[str]:
    """No `explicit_disable: false` literal anywhere in the provider crate."""
    violations = []
    for name, source in sources.items():
        code = _code(source)
        for match in EXPLICIT_DISABLE_FALSE.finditer(code):
            line = code.count("\n", 0, match.start()) + 1
            violations.append(
                f"{name}:{line}: hard-coded `explicit_disable: false` — the explicit "
                "disable is the kernel's `ReasoningIntent::disabled()`, and every other "
                "case must inherit the kernel default"
            )
    return violations


def check_no_closed_effort_enum(sources: dict[str, str]) -> list[str]:
    """No closed reasoning-effort enum in the provider crate."""
    violations = []
    for name, source in sources.items():
        code = _code(source)
        for match in CLOSED_EFFORT_ENUM.finditer(code):
            line = code.count("\n", 0, match.start()) + 1
            violations.append(
                f"{name}:{line}: closed reasoning-effort enum "
                f"`{match.group(0).split()[-1]}` — the effort vocabulary is "
                "provider-scoped and open, so a label must reach the wire verbatim"
            )
    return violations


def check_no_closed_effort_ladder(sources: dict[str, str]) -> list[str]:
    """No closed effort-label ladder in the provider crate.

    A `match` over a fixed set of label strings is the same closed vocabulary
    as an enum, and it rewrites a vendor label exactly the way an enum would.
    """
    violations = []
    for name, source in sources.items():
        code = _scan(source, blank_strings=False)
        labels = {match.group(1) for match in LADDER_ARM.finditer(code)}
        if len(labels) >= 3:
            line = code.count("\n", 0, next(
                match.start() for match in LADDER_ARM.finditer(code)
            )) + 1
            violations.append(
                f"{name}:{line}: closed effort-label ladder over "
                f"{sorted(labels)} — the effort vocabulary is provider-scoped and "
                "open, so a label must reach the wire verbatim"
            )
    return violations


def check_bridge_reaches_every_mode(bridge: str) -> list[str]:
    """The bridge must still construct every intent and mode it must.

    Only the implementation counts, and only its code: the module's own tests
    assert against the kernel's constructors, and a comment — or a string
    literal — that names a constructor is not a construction site.
    """
    implementation = _code(_implementation(bridge))
    return [
        f"wire.rs: the bridge no longer uses `{token}` in code — the reasoning "
        "intent must keep reaching the unspecified intent, the explicit disable, "
        "and the effort, budget, and adaptive modes"
        for token in REQUIRED_BRIDGE_TOKENS
        if token not in implementation
    ]


def check_bridge_does_not_hand_roll_a_disable(bridge: str) -> list[str]:
    """The explicit disable must be the kernel's constructor, not a literal."""
    implementation = _code(_implementation(bridge))
    violations = []
    for match in HAND_ROLLED_DISABLE.finditer(implementation):
        line = implementation.count("\n", 0, match.start()) + 1
        violations.append(
            f"wire.rs:{line}: hand-rolled explicit disable "
            f"(`{match.group(0)}`) — the disable must be the kernel's own "
            "`ReasoningIntent::disabled()`"
        )
    return violations


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _sources() -> dict[str, str]:
    return {
        str(path.relative_to(REPO_ROOT)): _read(path) for path in provider_sources()
    }


def _self_test() -> int:
    """Prove the guard fails on injected violations."""
    sources = _sources()
    bridge = sources[str(BRIDGE.relative_to(REPO_ROOT))]
    bridge_name = str(BRIDGE.relative_to(REPO_ROOT))

    def with_bridge(replacement: str) -> dict[str, str]:
        return {**sources, bridge_name: replacement}

    # Violation 1: the explicit-disable literal is hard-coded again.
    injected = bridge.replace(
        "..eggpool_wire::ir::ReasoningIntent::default()",
        "explicit_disable: false,\n            ..eggpool_wire::ir::ReasoningIntent::default()",
        1,
    )
    if injected == bridge:
        print("  FAIL: self-test could not find the kernel default to inject a literal into")
        return 1
    if not check_no_hardcoded_explicit_disable(with_bridge(injected)):
        print(
            "  FAIL: self-test reinstated a hard-coded `explicit_disable: false` and the "
            "guard still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected hard-coded `explicit_disable: false` rejected")

    # Violation 2: a closed effort vocabulary appears in the provider crate.
    enum_injected = bridge.replace(
        "fn reasoning_intent(",
        "pub enum ReasoningEffort {\n    Minimal,\n    Low,\n    Medium,\n    High,\n}\n\nfn reasoning_intent(",
        1,
    )
    if enum_injected == bridge:
        print("  FAIL: self-test could not find the bridge's reasoning intent function")
        return 1
    if not check_no_closed_effort_enum(with_bridge(enum_injected)):
        print(
            "  FAIL: self-test added a closed reasoning-effort enum and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected closed reasoning-effort enum rejected")

    # Violation 3: the explicit disable stops being the kernel's constructor.
    disable_injected = bridge.replace(
        "eggpool_wire::ir::ReasoningIntent::disabled()",
        "eggpool_wire::ir::ReasoningIntent::default()",
        1,
    )
    if disable_injected == bridge:
        print("  FAIL: self-test could not find the kernel's explicit-disable constructor")
        return 1
    if not check_bridge_reaches_every_mode(disable_injected):
        print(
            "  FAIL: self-test dropped the explicit-disable constructor and the guard "
            "still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected dropped explicit-disable constructor rejected")

    # Violation 4: the constructor survives only inside a string literal.
    # The audit demonstrated that a token-presence check is satisfied by a
    # string, so the check must be about code.
    string_injected = bridge.replace(
        "eggpool_wire::ir::ReasoningIntent::disabled()",
        "eggpool_wire::ir::ReasoningIntent::default()",
        1,
    )
    if string_injected == bridge:
        print("  FAIL: self-test could not find the kernel's explicit-disable constructor")
        return 1
    string_injected = string_injected.replace(
        "pub(crate) fn reasoning_intent(",
        'const NOTE: &str = "eggpool_wire::ir::ReasoningIntent::disabled()";\n\n'
        "pub(crate) fn reasoning_intent(",
        1,
    )
    if not check_bridge_reaches_every_mode(string_injected):
        print(
            "  FAIL: self-test moved the explicit-disable constructor into a string "
            "literal and the guard still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected string-literal constructor rejected")

    # Violation 5: a string literal opens a phantom block comment and hides a
    # hard-coded literal from the scan.
    hidden = bridge.replace(
        "pub(crate) fn reasoning_intent(",
        'const MARKER: &str = "/*";\n\npub(crate) fn reasoning_intent(',
        1,
    )
    hidden = hidden.replace(
        "..eggpool_wire::ir::ReasoningIntent::default()",
        "explicit_disable: false,\n            ..eggpool_wire::ir::ReasoningIntent::default()",
        1,
    )
    if hidden == bridge:
        print("  FAIL: self-test could not find the bridge's reasoning intent function")
        return 1
    if not check_no_hardcoded_explicit_disable(with_bridge(hidden)):
        print(
            "  FAIL: self-test hid a hard-coded `explicit_disable: false` behind a "
            "string literal and the guard still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected string-hidden explicit-disable literal rejected")

    # Violation 6: the disable is hand-rolled instead of using the kernel's
    # constructor. It writes no `explicit_disable: false`, so only a rule
    # about the hand-rolled shape can catch it.
    hand_rolled = bridge.replace(
        "ReasoningControl::Disabled => eggpool_wire::ir::ReasoningIntent::disabled(),",
        "ReasoningControl::Disabled => eggpool_wire::ir::ReasoningIntent {\n"
        "            requested: Some(false),\n"
        "            mode: ReasoningMode::Toggle,\n"
        "            ..eggpool_wire::ir::ReasoningIntent::default()\n"
        "        },",
        1,
    )
    if hand_rolled == bridge:
        print("  FAIL: self-test could not find the bridge's explicit-disable arm")
        return 1
    if not check_bridge_does_not_hand_roll_a_disable(hand_rolled):
        print(
            "  FAIL: self-test hand-rolled the explicit disable and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected hand-rolled explicit disable rejected")

    # Violation 7: a closed effort-label ladder in match-arm position.
    ladder = bridge.replace(
        "pub(crate) fn reasoning_intent(",
        "fn legacy_ladder(label: &str) -> u8 {\n"
        '    match label {\n'
        '        "low" => 1,\n'
        '        "medium" => 2,\n'
        '        "high" => 3,\n'
        "        _ => 0,\n"
        "    }\n"
        "}\n\n"
        "pub(crate) fn reasoning_intent(",
        1,
    )
    if ladder == bridge:
        print("  FAIL: self-test could not find the bridge's reasoning intent function")
        return 1
    if not check_no_closed_effort_ladder(with_bridge(ladder)):
        print(
            "  FAIL: self-test added a closed effort-label ladder and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected closed effort-label ladder rejected")

    # And the unmodified tree must still pass, so the self-test proves the
    # guard rejects rather than being broken.
    if check_no_hardcoded_explicit_disable(sources) or check_no_closed_effort_enum(sources):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    if check_no_closed_effort_ladder(sources):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    if check_bridge_reaches_every_mode(bridge):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    if check_bridge_does_not_hand_roll_a_disable(bridge):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_reasoning_intent_fidelity --self-test")
        return _self_test()

    checks = [
        (
            "the provider crate carries no hard-coded `explicit_disable: false`",
            check_no_hardcoded_explicit_disable,
        ),
        (
            "the provider crate carries no closed reasoning-effort enum",
            check_no_closed_effort_enum,
        ),
        (
            "the provider crate carries no closed effort-label ladder",
            check_no_closed_effort_ladder,
        ),
        (
            "the bridge reaches the unspecified intent, the explicit disable, and every mode",
            check_bridge_reaches_every_mode,
        ),
        (
            "the bridge does not hand-roll the explicit disable",
            check_bridge_does_not_hand_roll_a_disable,
        ),
    ]
    sources = _sources()
    bridge = sources[str(BRIDGE.relative_to(REPO_ROOT))]
    failed = []
    for name, check in checks:
        try:
            if check in (check_bridge_reaches_every_mode, check_bridge_does_not_hand_roll_a_disable):
                violations = check(bridge)
            else:
                violations = check(sources)
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            violations = [str(error)]
        for violation in violations:
            print(f"  FAIL: {violation}")
        print(f"  {'ok' if not violations else 'FAIL'}: {name}")
        if violations:
            failed.append(name)
    if failed:
        print(f"check_reasoning_intent_fidelity: {len(failed)} failing")
        return 1
    print("check_reasoning_intent_fidelity: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
