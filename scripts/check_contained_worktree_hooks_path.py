#!/usr/bin/env python3
"""Guard: the contained worktree child must not run the repository's hooks.

WHAT THIS GUARD DOES
    ADR-0048 grants the contained managed-worktree creation child a **shared**
    administration root — ``<common-gitdir>/worktrees/`` — so the child can
    write **any** linked worktree's ``HEAD``. The path to that write is real:
    ``git worktree add`` runs the repository's ``post-checkout`` hook, and
    without a pin that hook runs inside the containment, where the shared root
    is writable.

    The maintainer's chosen disposition is **defence in depth**: pin
    ``core.hooksPath`` for the contained child, so the repository's own hooks
    cannot run at all. This is not a narrowing of ADR-0048's accepted grant —
    the shared root stays writable — and it does not close the capability. It
    removes the most obvious path to it.

    This script enforces the disposition statically:

    1. **The pin is on the child's argv.** The child's argv is built in one
       place, ``worktree_child_args``, and it must carry the pin
       ``-c core.hooksPath=/dev/null``. The pin must precede every
       step-specific subcommand, because a Git **global** option only applies
       ahead of the subcommand — a pin after it is not a pin at all.
    2. **The environment does not carry the pin.** Git's command-scope
       configuration variables (``GIT_CONFIG_COUNT`` and friends) are
       **hard-denied** by the child's environment policy, so a pin carried
       there would be silently dropped and the disposition would be a no-op.
       The guard rejects an environment-carried pin by name.
    3. **The ADR records the disposition as chosen.** ADR-0048's disposition
       section must name defence in depth, name the pin, name the argv as the
       mechanism, and must no longer say the maintainer has not chosen. A
       disposition that the code implements but the ADR still records as
       unchosen is a disagreement between the record and the tree.
    4. **The behavioural proof exists, and the containment test agrees with
       it.** The dedicated integration test must be present and must derive its
       control from the production pin. The containment test that asserts the
       grant must no longer require the repository's hook to have run — the two
       tests would otherwise contradict each other.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not narrow the grant. The shared administration root stays
    writable, and the capability it carries stays open: the pin removes the
    most obvious path to it, and the sandbox remains the security property.

    It does not claim the capability is closed, and it does not claim that
    hooks are the only hostile configuration the child runs. A hostile
    ``filter.*.smudge`` still runs, by ADR-0048's own accepted clause 5.

    It does not prove that the repository's hook does not **run**. That is the
    integration test's claim, and the guard only proves the pin is wired.

USAGE
    python3 scripts/check_contained_worktree_hooks_path.py            # check the tree
    python3 scripts/check_contained_worktree_hooks_path.py --self-test  # prove it bites

Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

SOURCE_REL = "src/git_contained_worktree.rs"
ADR_REL = "plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md"
TEST_REL = "tests/contained_worktree_hooks_path.rs"
CONTAINMENT_TEST_REL = "tests/contained_worktree_c008.rs"

# The constant that carries the pin, and the argv builder that has to apply it.
PIN_CONSTANT = "WORKTREE_CHILD_HOOKS_PATH_PIN"
ARGV_BUILDER_SIGNATURE = "fn worktree_child_args("

# The pinned key, and the value that names no hook directory at all.
PINNED_KEY = "core.hooksPath"
PINNED_VALUE = "/dev/null"
PINNED_OPTION = "-c"

# The Git command-scope configuration variables. The child's environment policy
# hard-denies these, so a pin carried there never reaches the child.
COMMAND_SCOPE_VARIABLES: tuple[str, ...] = (
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_VALUE_0",
    "GIT_CONFIG_PARAMETERS",
)

# The step-specific subcommands. A Git global option only applies ahead of the
# subcommand, so the pin has to precede every one of them.
SUBCOMMANDS: tuple[str, ...] = ("worktree", "symbolic-ref")

# The environment builder, checked for a command-scope pin it cannot carry.
ENVIRONMENT_SIGNATURE = "pub fn worktree_child_environment("


@dataclass(frozen=True)
class Finding:
    """One violation, with the surface and the line it was found on."""

    surface: str
    line: int
    message: str


# --- Reading Rust ---------------------------------------------------------

# The regions of a Rust source that are not code, in one pass: a raw string, a
# string literal, a line comment or a block comment. The alternatives are
# ordered so a `//` inside a string is consumed by the string, not by the
# comment.
_TOKEN_RE = re.compile(
    r'(?P<raw>r(?P<hashes>#+)"(?:[^"]|"(?! (?P=hashes)))*"(?P=hashes))'
    r'|(?P<str>"(?:\\.|[^"\\\n])*")'
    r"|(?P<line>//[^\n]*)"
    r"|(?P<block>/\*.*?\*/)",
    re.DOTALL,
)


def _blank(match: re.Match[str]) -> str:
    """The match replaced by spaces, keeping every offset and every newline."""
    return re.sub(r"[^\n]", " ", match.group(0))


def _mask_comments(text: str) -> str:
    """Blank the comments, keeping the string literals readable."""
    return _TOKEN_RE.sub(
        lambda match: _blank(match) if match.group("line") or match.group("block") else match.group(0),
        text,
    )


def _mask_code(text: str) -> str:
    """Blank the comments and the string literals, keeping every offset."""
    return _TOKEN_RE.sub(_blank, text)


def _function_body(source: str, signature: str) -> str | None:
    """The body of a Rust function, located by its signature.

    The signature must end with the opening `(` of the parameter list. The
    body's braces are matched on the masked text, so a brace inside a string
    literal or a comment is not counted.
    """
    masked = _mask_code(source)
    index = masked.find(signature)
    if index < 0:
        return None
    depth = 1  # the signature's own `(`
    open_at: int | None = None
    for position in range(index + len(signature), len(masked)):
        char = masked[position]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "{" and depth == 0:
            open_at = position
            break
    if open_at is None:
        return None
    depth = 0
    for position in range(open_at, len(masked)):
        char = masked[position]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[open_at : position + 1]
    return None


def _line_of(source: str, offset: int) -> int:
    """The one-based line an offset falls on."""
    return source.count("\n", 0, offset) + 1


# --- The pin --------------------------------------------------------------


def _declared_pin(source: str) -> tuple[list[str], int] | None:
    """The strings the pin constant is declared with, and where it is declared."""
    match = re.search(
        r"pub\s+const\s+" + re.escape(PIN_CONSTANT) + r"\s*:\s*\[\s*&\s*str\s*;\s*2\s*\]\s*=\s*\[",
        source,
    )
    if match is None:
        return None
    body = source[match.end() : source.find("]", match.end()) + 1]
    return re.findall(r'"([^"]*)"', body), _line_of(source, match.start())


def scan_argv_pin(source: str) -> list[Finding]:
    """The child's argv must carry the pin, ahead of every subcommand."""
    findings: list[Finding] = []
    if not source:
        return [Finding(SOURCE_REL, 0, f"{SOURCE_REL} is missing")]

    declared = _declared_pin(source)
    if declared is None:
        return [
            Finding(
                SOURCE_REL,
                0,
                f"{SOURCE_REL} no longer declares `{PIN_CONSTANT}`, so the contained "
                "child's argv carries no hooks-path pin at all",
            )
        ]
    values, line = declared
    if values != [PINNED_OPTION, f"{PINNED_KEY}={PINNED_VALUE}"]:
        findings.append(
            Finding(
                SOURCE_REL,
                line,
                f"`{PIN_CONSTANT}` is declared with {values}, expected "
                f"{[PINNED_OPTION, f'{PINNED_KEY}={PINNED_VALUE}']}: the pin must be a "
                f"Git global option naming `{PINNED_KEY}` and a value that names no "
                "hook directory",
            )
        )

    body = _function_body(source, ARGV_BUILDER_SIGNATURE)
    if body is None:
        findings.append(
            Finding(
                SOURCE_REL,
                0,
                f"{SOURCE_REL} no longer defines `{ARGV_BUILDER_SIGNATURE}`, so the "
                "contained child's argv cannot be checked for the hooks-path pin",
            )
        )
        return findings

    readable = _mask_comments(body)
    pin_at = readable.find(PIN_CONSTANT)
    if pin_at < 0:
        findings.append(
            Finding(
                SOURCE_REL,
                _line_of(source, source.find(ARGV_BUILDER_SIGNATURE)),
                f"`{ARGV_BUILDER_SIGNATURE}` does not apply `{PIN_CONSTANT}`, so the "
                "contained child's argv carries no hooks-path pin",
            )
        )
        return findings

    subcommand_positions: list[tuple[str, int]] = []
    for subcommand in SUBCOMMANDS:
        match = re.search(r'"' + re.escape(subcommand) + r'"', readable)
        if match is None:
            findings.append(
                Finding(
                    SOURCE_REL,
                    _line_of(source, source.find(ARGV_BUILDER_SIGNATURE)),
                    f"the contained child's argv no longer names the {subcommand!r} "
                    "subcommand, so the pin's position cannot be checked against it",
                )
            )
            continue
        subcommand_positions.append((subcommand, match.start()))

    for subcommand, position in subcommand_positions:
        if pin_at > position:
            findings.append(
                Finding(
                    SOURCE_REL,
                    _line_of(source, source.find(ARGV_BUILDER_SIGNATURE)),
                    f"the hooks-path pin is applied after the {subcommand!r} subcommand; "
                    "a Git global option only applies ahead of the subcommand, so a pin "
                    "there is not a pin at all",
                )
            )
    return findings


def scan_environment_pin(source: str) -> list[Finding]:
    """The environment must not carry the pin the argv carries."""
    findings: list[Finding] = []
    if not source:
        return []
    body = _function_body(source, ENVIRONMENT_SIGNATURE)
    if body is None:
        return [
            Finding(
                SOURCE_REL,
                0,
                f"{SOURCE_REL} no longer defines `{ENVIRONMENT_SIGNATURE}`, so the "
                "child's environment cannot be checked for a command-scope pin",
            )
        ]
    readable = _mask_comments(body)
    for variable in COMMAND_SCOPE_VARIABLES:
        if f'"{variable}"' in readable:
            findings.append(
                Finding(
                    SOURCE_REL,
                    _line_of(source, source.find(ENVIRONMENT_SIGNATURE)),
                    f"the child's environment sets `{variable}`; Git's command-scope "
                    "configuration variables are hard-denied by the child's environment "
                    "policy, so a hooks-path pin carried there is silently dropped and "
                    "the disposition would be a no-op — the pin belongs on the argv",
                )
            )
    return findings


# --- The ADR --------------------------------------------------------------


def _disposition_section(adr: str) -> str | None:
    """ADR-0048's disposition section."""
    start = adr.find("### Disposition:")
    if start < 0:
        return None
    following = adr.find("\n## ", start + 1)
    return adr[start : following] if following > 0 else adr[start:]


def scan_adr_disposition(adr: str) -> list[Finding]:
    """ADR-0048 must record the disposition as chosen, and name the mechanism."""
    findings: list[Finding] = []
    if not adr:
        return [Finding(ADR_REL, 0, f"{ADR_REL} is missing")]
    section = _disposition_section(adr)
    if section is None:
        return [
            Finding(
                ADR_REL,
                0,
                f"{ADR_REL} has no '### Disposition:' section, so the chosen "
                "disposition cannot be checked",
            )
        ]
    line = _line_of(adr, adr.find("### Disposition:"))
    if "defence in depth" not in section.lower():
        findings.append(
            Finding(
                ADR_REL,
                line,
                "ADR-0048's disposition section does not name defence in depth, so the "
                "disposition the code implements is not the one the ADR records",
            )
        )
    if PINNED_KEY not in section:
        findings.append(
            Finding(
                ADR_REL,
                line,
                f"ADR-0048's disposition section does not name {PINNED_KEY}, so the pin "
                "the code implements is not the one the ADR records",
            )
        )
    if "argv" not in section.lower():
        findings.append(
            Finding(
                ADR_REL,
                line,
                "ADR-0048's disposition section does not name the argv as the "
                "mechanism, so a reader cannot tell that the pin is not carried by the "
                "environment — which is hard-denied and would be a no-op",
            )
        )
    if "has not chosen a disposition" in section:
        findings.append(
            Finding(
                ADR_REL,
                line,
                "ADR-0048's disposition section still records the maintainer as having "
                "not chosen a disposition, while the code implements one",
            )
        )
    return findings


# --- The behavioural proof -------------------------------------------------


def scan_behavioural_proof(test: str, containment_test: str) -> list[Finding]:
    """The dedicated integration test must be present, and the two agree."""
    findings: list[Finding] = []
    if not test:
        findings.append(
            Finding(
                TEST_REL,
                0,
                f"{TEST_REL} is missing, so the pin has no behavioural proof: the "
                "static claims say the pin is wired, and only the test says the "
                "repository's hook does not run",
            )
        )
    else:
        if "post-checkout" not in test:
            findings.append(
                Finding(
                    TEST_REL,
                    0,
                    f"{TEST_REL} no longer exercises a `post-checkout` hook, so it no "
                    "longer proves that the repository's hook does not run",
                )
            )
        if PIN_CONSTANT not in test:
            findings.append(
                Finding(
                    TEST_REL,
                    0,
                    f"{TEST_REL} does not derive its control from `{PIN_CONSTANT}`, so "
                    "the control is not the production argv with the pin removed and "
                    "the claim could be vacuous",
                )
            )
        if "control" not in test.lower():
            findings.append(
                Finding(
                    TEST_REL,
                    0,
                    f"{TEST_REL} no longer runs a control, so its claim would be "
                    "vacuous: a child that never ran hooks for an unrelated reason "
                    "would satisfy it",
                )
            )

    if not containment_test:
        return findings
    # The containment test and the hooks-path test must not contradict each
    # other: one of them asserts the repository's hook ran.
    for match in re.finditer(r"(?<!!)\b(\w+)\.is_file\(\)", containment_test):
        marker = match.group(1)
        if "hook" not in marker.lower():
            continue
        findings.append(
            Finding(
                CONTAINMENT_TEST_REL,
                _line_of(containment_test, match.start()),
                f"the containment test still asserts that `{marker}` exists, so it "
                "requires the repository's hook to have run — the hooks-path pin "
                "means it does not run at all",
            )
        )
    return findings


CHECKS = (
    ("argv-pin", scan_argv_pin),
    ("environment-pin", scan_environment_pin),
    ("adr-disposition", scan_adr_disposition),
    ("behavioural-proof", scan_behavioural_proof),
)


def _read(relative: str) -> str:
    path = ROOT / relative
    if not path.is_file():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def _sources(overrides: dict[str, str] | None = None) -> dict[str, str]:
    sources = {
        SOURCE_REL: _read(SOURCE_REL),
        ADR_REL: _read(ADR_REL),
        TEST_REL: _read(TEST_REL),
        CONTAINMENT_TEST_REL: _read(CONTAINMENT_TEST_REL),
    }
    if overrides:
        sources.update(overrides)
    return sources


def collect_findings(sources: dict[str, str] | None = None) -> list[Finding]:
    sources = _sources(sources)
    findings: list[Finding] = []
    findings.extend(scan_argv_pin(sources[SOURCE_REL]))
    findings.extend(scan_environment_pin(sources[SOURCE_REL]))
    findings.extend(scan_adr_disposition(sources[ADR_REL]))
    findings.extend(
        scan_behavioural_proof(sources[TEST_REL], sources[CONTAINMENT_TEST_REL])
    )
    return findings


# --- Self-test -------------------------------------------------------------


def _without_the_pin(source: str) -> str:
    """The argv builder with the pin application removed."""
    return source.replace(
        f"""    let mut args: Vec<String> = {PIN_CONSTANT}
        .iter()
        .copied()
        .map(str::to_string)
        .collect();
""",
        "    let mut args: Vec<String> = Vec::new();\n",
    )


def _with_the_pin_after_the_subcommand(source: str) -> str:
    """The argv builder with the pin applied after the subcommand."""
    return source.replace(
        """    let mut args: Vec<String> = WORKTREE_CHILD_HOOKS_PATH_PIN
        .iter()
        .copied()
        .map(str::to_string)
        .collect();
    args.extend(match step {
        WorktreeChildStep::Create => vec![
            format!("--git-dir={}", paths.gitdir.display()),
            "worktree".to_string(),""",
        """    let mut args: Vec<String> = Vec::new();
    args.extend(match step {
        WorktreeChildStep::Create => vec![
            format!("--git-dir={}", paths.gitdir.display()),
            "worktree".to_string(),""",
    ).replace(
        """            paths.checkout.display().to_string(),
            paths.base_commit.clone(),
        ],""",
        """            paths.checkout.display().to_string(),
            paths.base_commit.clone(),
            WORKTREE_CHILD_HOOKS_PATH_PIN[0].to_string(),
            WORKTREE_CHILD_HOOKS_PATH_PIN[1].to_string(),
        ],""",
    )


def _with_the_environment_carrying_the_pin(source: str) -> str:
    """The child's environment carrying the pin the policy hard-denies."""
    return source.replace(
        """    EnvironmentPolicy::sanitized()
        .with_var("GIT_CONFIG_NOSYSTEM", "1")""",
        """    EnvironmentPolicy::sanitized()
        .with_var("GIT_CONFIG_COUNT", "1")
        .with_var("GIT_CONFIG_KEY_0", "core.hooksPath")
        .with_var("GIT_CONFIG_VALUE_0", "/dev/null")
        .with_var("GIT_CONFIG_NOSYSTEM", "1")""",
    )


def _with_the_disposition_unchosen(adr: str) -> str:
    """The ADR with its disposition section still recording the open question."""
    start = adr.find("### Disposition:")
    if start < 0:
        return adr
    end = adr.find("\n## ", start + 1)
    if end < 0:
        end = len(adr)
    unchosen = (
        "### Disposition: an open capability question, not an acceptance\n"
        "\n"
        "**The maintainer has not chosen a disposition for this consequence.**\n"
    )
    return adr[:start] + unchosen + adr[end:]


def self_test() -> int:
    """Prove the guard rejects every class of drift it claims to catch."""
    failures: list[str] = []

    def case(name: str, mutate, expect_fragment: str) -> None:
        original = _sources()
        mutated = mutate(_sources())
        if mutated == original:
            failures.append(
                f"the '{name}' divergence did not change the tree, so the guard was "
                "never asked the question — the self-test would be vacuous"
            )
            return
        findings = collect_findings(mutated)
        if not any(expect_fragment in finding.message for finding in findings):
            failures.append(
                f"the '{name}' divergence was not reported (expected a finding "
                f"containing '{expect_fragment}')"
            )
        else:
            print(f"  ok: rejected {name}")

    def drop_the_pin(sources: dict[str, str]) -> dict[str, str]:
        sources[SOURCE_REL] = _without_the_pin(sources[SOURCE_REL])
        return sources

    def move_the_pin_after_the_subcommand(sources: dict[str, str]) -> dict[str, str]:
        sources[SOURCE_REL] = _with_the_pin_after_the_subcommand(sources[SOURCE_REL])
        return sources

    def carry_the_pin_in_the_environment(sources: dict[str, str]) -> dict[str, str]:
        sources[SOURCE_REL] = _with_the_environment_carrying_the_pin(sources[SOURCE_REL])
        return sources

    def revert_the_disposition(sources: dict[str, str]) -> dict[str, str]:
        sources[ADR_REL] = _with_the_disposition_unchosen(sources[ADR_REL])
        return sources

    def drop_the_test(sources: dict[str, str]) -> dict[str, str]:
        sources[TEST_REL] = ""
        return sources

    def contradict_the_test(sources: dict[str, str]) -> dict[str, str]:
        sources[CONTAINMENT_TEST_REL] = sources[CONTAINMENT_TEST_REL].replace(
            "!hook_ran.is_file()", "hook_ran.is_file()"
        )
        return sources

    findings = collect_findings()
    if findings:
        failures.append(
            "the unmodified tree must pass the guard: "
            + "; ".join(finding.message for finding in findings)
        )
    else:
        print("  ok: accepted the unmodified tree")

    case(
        "a child whose argv no longer carries the pin",
        drop_the_pin,
        "does not apply `WORKTREE_CHILD_HOOKS_PATH_PIN`",
    )
    case(
        "a pin applied after the subcommand",
        move_the_pin_after_the_subcommand,
        "the hooks-path pin is applied after the",
    )
    case(
        "a pin carried by the environment the policy hard-denies",
        carry_the_pin_in_the_environment,
        "hard-denied by the child's environment policy",
    )
    case(
        "an ADR that still records the disposition as unchosen",
        revert_the_disposition,
        "still records the maintainer as having not chosen a disposition",
    )
    case(
        "a tree with no behavioural proof",
        drop_the_test,
        "is missing, so the pin has no behavioural proof",
    )
    case(
        "a containment test that still requires the hook to have run",
        contradict_the_test,
        "still asserts that `hook_ran` exists",
    )

    if failures:
        print("contained-worktree-hooks-path self-test: FAILED", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 2
    print(
        f"contained-worktree-hooks-path self-test: all checks pass ({len(CHECKS)} case(s))"
    )
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects every class of drift it claims to catch",
    )
    arguments = parser.parse_args(argv)
    if arguments.self_test:
        return self_test()

    findings = collect_findings()
    if findings:
        print("contained-worktree-hooks-path check: FAILED", file=sys.stderr)
        for finding in findings:
            where = f"{finding.surface}:{finding.line}" if finding.line else finding.surface
            print(f"  - {where}: {finding.message}", file=sys.stderr)
        return 1

    print(
        "contained-worktree-hooks-path check ok: the contained child's argv pins "
        "core.hooksPath ahead of the subcommand, the environment carries no "
        "command-scope pin, ADR-0048 records the disposition as chosen, and the "
        "behavioural proof is present"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
