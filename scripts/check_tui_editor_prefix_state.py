#!/usr/bin/env python3
"""Static guard: the editor has exactly one pending-prefix mechanism.

Milestone M002 of `plans/implementation/tui-editor-helix-parity/` replaces two
ad-hoc prefix mechanisms on `TuiDocumentPresentation` with one typed state
machine, `EditorPrefix`:

* `pending_command: String`, bounded by `MAX_EDITOR_PENDING_COMMAND`, holding
  the `g` of `gg`, and
* `yank_prefix_pending: bool`, holding the `space` of `space y`.

The two were not merely redundant. They disagreed: `gg` **consumed** an
unmatched second key, while `space y` **disarmed and retried** it. Merging them
therefore had to preserve a per-prefix mismatch policy
(`EditorPrefix::swallows_unmatched`), and the milestone's acceptance criteria
require `gg` and `space y` to behave identically to the baseline.

M005 then replaced the single-glyph accessor (`EditorPrefix::leaf() ->
Option<char>`) with a per-prefix **leaf table** (`EditorPrefix::leaves() ->
&'static [PrefixLeaf]`). The one-glyph shape only worked because goto mode had
exactly one leaf; Helix's has twenty, and expressing them as one `Some` per
prefix would mean either a special case per prefix or a second mechanism
beside this one. The invariant this guard protects is unchanged by that
rewrite — a grammar is still a leaf table and never a field — so no assertion
below was relaxed to accommodate it. The table is what the guard's "a new
grammar adds a leaf, not a flag" rule now means in practice.

M004 then added a second **capture** prefix, `r`, alongside the four find
prefixes. A capture takes its next key as data rather than as a leaf glyph,
which is the same category `f` was and the same reason its row in the leaf
table is empty. Asserting the variant by name keeps it from being
"simplified" away: dropping it would not fail any other assertion here, and
`r` would then resolve through the leaf lookup and swallow a keystroke.

M006 then gave the two unimpaired prefixes, `[` and `]`, their first leaves,
and added a third `PrefixAction` arm — `Edit` — because `]Space` inserts a
newline and `editor_motion` is the one function in the dispatch that promises
never to touch text. Two things about that milestone matter *here*:

* `[`/`]` stopped being the "armed but empty" odd case, so the docstring's old
  claim that every empty `leaves()` row is a capture became true rather than
  aspirational. That is a documentation correction, not a relaxed assertion:
  the guard below never permitted a second prefix field, and M006 added none —
  the paragraph *disclosure* it did add is `paragraph_note`, which is an answer
  about one keystroke, not a mode. A field named for a mode would still fail
  this guard.
* The three `Alt-.` repeat slots (`last_find`, `last_selection_motion`,
  `last_unimpaired`) are **not** prefix state. They are cleared by motions, not
  by arming and disarming, and folding them into `EditorPrefix` would have made
  "what key is armed" and "what can be repeated" the same question.

M007 then added the `"` grammar: two levels in one machine — `Register`
(waiting for a name, a capture) and `RegisterNamed` (the captured name, waiting
for the verb that consumes the selection). The two levels are the `m`
grammar's shape copied rather than invented: a `Copy` payload in the variant,
never a second field on the presentation. That is also why no
`selected_register` field was added — a selection that outlived an unrelated
key would be the second grammar this guard exists to prevent, and the
machine's unconditional disarm already bounds it to one keystroke.

The register grammar is where the mismatch policy's **per-prefix** design earns
its keep, and it is why the expected set below grew:

* `Register` (the name) **swallows**. Its argument is a closed vocabulary —
  `a`–`z`, `"`, `_`, `/` — so every *character* is captured, including the
  destructive ones: `"D` is a rejected name, not a delete. The mismatch path
  therefore only sees non-character keys, and retrying one would run it from a
  gesture the reader had already committed to (`"` then `Tab` toggling the
  composer). Vim consumes the argument the same way; Helix's `select_register`
  is a prompt, which also consumes keys rather than executing them.
* `RegisterNamed` (the verb) **retries**, because `"a` then `j` must still move
  the cursor — Vim's answer — and a verb the register grammar does not honour
  simply drops the selection.

Widening the set to `Register` and narrowing it back are both injections in the
self-test, so the decision cannot be quietly undone in either direction.

## What counts as a violation

**Violations** (exit code 1):

- Any occurrence of `pending_command`, `yank_prefix_pending`,
  `MAX_EDITOR_PENDING_COMMAND`, `bounded_pending`, or `push_pending` anywhere
  under `src/`. These are the deleted mechanisms, their bound, and the two
  helpers that existed only to service them. A field left behind rather than
  deleted satisfies nothing and is read as complete.
- A `TuiDocumentPresentation` that declares more than one prefix field. The
  point of the type is that a new grammar arrives as a leaf, not a flag, so a
  second `*prefix*` field is the regression this milestone exists to prevent —
  and it would be a *new* name, which a fixed forbidden-identifier list cannot
  catch.
- A `TuiDocumentPresentation` with no prefix field at all: prefix state has to
  live somewhere, and "nowhere" means the gestures silently stopped arming.
- A `swallows_unmatched` whose answered set is not exactly `Goto`, `Register`
  and `View`. This is the per-prefix half of the milestone: `gg` consumes an
  unmatched second key and `space y` retries it, so collapsing the policy to
  one answer for every prefix — the obvious "simplification" — would silently
  change one of the gestures the milestone had to preserve exactly. The
  register **name** level joins the set for a closed-vocabulary reason; see the
  M007 paragraph above.
- A dispatch that stopped asking the prefix for the policy. The policy is only
  a policy if `resolve_pending_prefix` reads it; a literal in its place is the
  same collapse one layer up.

## What the guard requires to still be true

Checked by name so it cannot pass against an emptied tree:

- `pub enum EditorPrefix` with an `Idle` variant (the machine needs a resting
  state to disarm into).
- `pub pending_prefix` on the presentation.
- `arm_prefix`, `disarm_prefix`, and `resolve_pending_prefix` all present.
- `pub fn leaves(` — the leaf table itself. A prefix that cannot name the keys
  it accepts has stopped being a grammar and become a flag with extra steps, so
  the accessor is asserted by name and not merely implied by the resolver.
- `fn is_capture(` — the classification that keeps a data-taking prefix
  (`f`, `t`, `F`, `T`, `r`) off the leaf lookup.
- `fn swallows_unmatched(` and the `pending.swallows_unmatched()` call in the
  dispatch — the per-prefix mismatch policy and the one place that consults
  it.
- `MAX_EDITOR_PENDING_PREFIX` present: the bound the indicator is held to.

## Non-vacuity

Every requirement above is an assertion about the real tree, so removing the
type, the field, an operation, or the bound fails the guard. The
forbidden-identifier scan covers `src/` wholesale, so a *test* that resurrects
a deleted name fails the guard too.

`--self-test` proves the other half: it copies `src/tui/` — the subtree the
mechanism lives in — into a throwaway directory, injects one realistic
regression per rule above, and requires the *same* checks to reject each one.
The unmodified copy must pass, and a case that stops being caught fails the
self-test loudly instead of quietly shrinking into nothing.

Exit code 1 on violations, 2 on a shape the guard cannot resolve.

Usage::

    python3 scripts/check_tui_editor_prefix_state.py
    python3 scripts/check_tui_editor_prefix_state.py --self-test

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Deleted mechanisms and the helpers that only existed to service them.
FORBIDDEN = [
    "pending_command",
    "yank_prefix_pending",
    "MAX_EDITOR_PENDING_COMMAND",
    "bounded_pending",
    "push_pending",
]

REQUIRED_MODEL_DECLARATIONS = [
    "pub enum EditorPrefix",
    "Idle",
    # The second capture grammar, alongside `f`. Asserted by name because
    # removing it satisfies every other assertion here while leaving `r`
    # unable to resolve at all.
    "ReplaceChar",
    # The `m` grammar's two levels. The first level's leaf table and the
    # second level's payload are both asserted by name: deleting either
    # satisfies every other assertion here — the one-field rule still
    # holds, the capture classifier still holds — while leaving `m` with
    # nothing to resolve.
    "MATCH_LEAVES",
    "pub enum MatchArg",
    # The arm that carries the second level, and the variant that holds it.
    # Together they are the shape a future two-level grammar must copy
    # rather than invent: a `Copy` payload in the variant, not a second
    # field on the presentation.
    "PrefixAction::Capture",
    "EditorPrefix::MatchArg",
    "MAX_EDITOR_PENDING_PREFIX",
]

REQUIRED_STATE_DECLARATIONS = [
    "pub pending_prefix",
    "fn arm_prefix",
    "fn disarm_prefix",
    "fn resolve_pending_prefix",
    "pub fn leaves(",
    # The one place a capture is classified. If it went away, a new capture
    # grammar could be added to the dispatch without also being added here,
    # and would reach the leaf lookup instead.
    "fn is_capture(",
    # The per-prefix mismatch policy and the dispatch that consults it. The
    # policy is checked for its *content* below; these two assertions keep a
    # deletion from reading as a pass.
    "fn swallows_unmatched(",
    "pending.swallows_unmatched()",
]

# `gg` consumes an unmatched second key, view mode swallows one because it is a
# mode, and the register **name** capture swallows one because its argument is
# a closed vocabulary — every character that could be a register is captured,
# so the mismatch path only sees keys that are not characters at all. Every
# other prefix — the unimpaired ones, the find prefixes, `r`, `m`, and the
# register **verb** level — hands the key back to the composer. Exactly this
# set, because a wider set swallows keystrokes the baseline passed through and
# a narrower one leaks a key the baseline consumed.
EXPECTED_SWALLOWING_PREFIXES = {"Goto", "Register", "View"}

# A struct field: `pub name: Type,` at any indentation, ignoring doc comments.
FIELD_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?([a-z_]\w*)\s*:", re.MULTILINE)


class ParseError(Exception):
    """A shape the guard cannot resolve."""


@dataclass(frozen=True)
class Report:
    """What one pass over a tree found."""

    violations: list[str]
    summary: str | None


def src_root(root: Path) -> Path:
    return root / "src"


def presentation_path(root: Path) -> Path:
    return src_root(root) / "tui" / "document_session.rs"


def struct_body(text: str, name: str) -> str:
    """The brace-matched body of `struct name { ... }`."""
    pattern = re.compile(r"struct\s+" + re.escape(name) + r"\b[^{]*\{")
    match = pattern.search(text)
    if match is None:
        raise ParseError(f"struct {name} not found")
    depth = 0
    for index in range(match.end() - 1, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[match.end() : index]
    raise ParseError(f"struct {name} has unbalanced braces")


def item_body(text: str, needle: str) -> str:
    """The brace-matched body of the first item starting at `needle`."""
    start = text.find(needle)
    if start == -1:
        raise ParseError(f"`{needle}` not found")
    opening = text.find("{", start)
    if opening == -1:
        raise ParseError(f"`{needle}` has no body")
    depth = 0
    for index in range(opening, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[opening + 1 : index]
    raise ParseError(f"`{needle}` has unbalanced braces")


def rust_sources(root: Path) -> list[Path]:
    """Every `.rs` file under `src/`, sorted so output is deterministic."""
    src = src_root(root)
    if not src.is_dir():
        raise ParseError(f"{src} is missing")
    return sorted(src.rglob("*.rs"))


def strip_comment(line: str) -> str:
    """Drop a trailing `//` comment, respecting string and char literals.

    Comments are where the *history* lives — `EditorPrefix`'s doc comment
    explains that it replaced `pending_command` and `yank_prefix_pending`, and
    that prose is worth keeping. Only code is forbidden, so the scan runs over
    comment-free text with the line count preserved.
    """
    # The only edit this function makes is truncating at a `//` outside a
    # literal, so a line without `//` is already the answer. The fast path is
    # the difference between scanning the tree in a second and in ten, and it
    # is exact rather than a heuristic: nothing else can be removed.
    if "//" not in line:
        return line
    in_string = False
    in_char = False
    escaped = False
    index = 0
    while index < len(line):
        char = line[index]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif in_string:
            if char == '"':
                in_string = False
        elif in_char:
            if char == "'":
                in_char = False
        elif char == '"':
            in_string = True
        elif char == "'":
            in_char = True
        elif char == "/" and line[index : index + 2] == "//":
            return line[:index]
        index += 1
    return line


def strip_comments(text: str) -> str:
    """Comment-free text with the line count preserved, so lines stay true."""
    return "\n".join(strip_comment(line) for line in text.splitlines())


def check_mismatch_policy(code_text: str) -> list[str]:
    """The `swallows_unmatched` body must answer for `Goto` and `View` only.

    Structural rather than textual: the body is brace-matched and the prefixes
    it names are collected, so rustfmt reflowing the expression, or a rewrite
    from `==` to `matches!`, is not a finding. Widening the set, narrowing it,
    or replacing the expression with a literal is.
    """
    if "fn swallows_unmatched(" not in code_text:
        # Reported by the required-declaration pass; a second message for the
        # same absence would only inflate the count.
        return []
    try:
        body = item_body(code_text, "fn swallows_unmatched(")
    except ParseError as error:
        return [f"`EditorPrefix::swallows_unmatched` could not be resolved: {error}"]

    # `EditorPrefix::Goto` and `Self::Goto` are the same variant inside the
    # enum's own `impl`; both spellings answer for the same prefix, so both are
    # read. A bare `Goto` does not appear here — the module qualifies its
    # variants — and accepting bare names would let `_ => true` look like a
    # policy.
    swallowing = set(re.findall(r"(?:EditorPrefix|Self)::([A-Za-z_]\w*)", body))
    if swallowing == EXPECTED_SWALLOWING_PREFIXES:
        return []
    answered = ", ".join(sorted(swallowing)) or "every prefix"
    expected = ", ".join(sorted(EXPECTED_SWALLOWING_PREFIXES))
    return [
        f"EditorPrefix::swallows_unmatched answers for {answered} instead of "
        f"{expected}: `gg`, view mode and the register name capture consume an "
        "unmatched key, and every other prefix retries it"
    ]


def check(root: Path) -> Report:
    """Run every rule against `root`; the self-test calls this same function."""
    violations: list[str] = []

    sources = rust_sources(root)
    # Read and comment-strip once per file: both scans below want the same
    # stripped text, and the self-test runs this function once per injected
    # defect, so a second per-line pass is pure cost.
    stripped = [(path, strip_comments(path.read_text())) for path in sources]

    # The required declarations are checked against **comment-free** text. A
    # doc comment that mentions a name is prose about the grammar, not the
    # grammar: `EditorPrefix::Match`'s own doc comment names `MATCH_LEAVES`,
    # so without this the whole `m` grammar could be deleted and the guard
    # would still be green — the prose would still be there. The forbidden
    # names below are checked against the same stripped text, for the same
    # reason.
    code_text = "\n".join(code for _, code in stripped)

    # 1. The deleted names anywhere under src/ — including tests, so a helper
    #    that resurrects one fails the build rather than merely compiling.
    for path, code in stripped:
        for number, line in enumerate(code.splitlines(), start=1):
            for name in FORBIDDEN:
                if re.search(rf"\b{re.escape(name)}\b", line):
                    violations.append(
                        f"{path.relative_to(root)}:{number} references deleted "
                        f"`{name}`: {line.strip()}"
                    )

    # 2. Exactly one prefix field on the presentation. This is the invariant
    #    a fixed name list cannot express: the next milestone's flags would be
    #    new names.
    #
    #    Load-bearing rather than incidental since M007 added the two
    #    **non-prefix** selection fields (`selections`, `primary_index`): the
    #    presentation now carries several pieces of state that a name filter
    #    has to reject, so "exactly one match for `prefix`" is now a real
    #    discrimination between fields rather than a check that happens to pass
    #    on a struct with one field in it. A future field named
    #    `selection_prefix` or `prefix_state` is exactly what it catches.
    presentation = presentation_path(root)
    try:
        presentation_text = strip_comments(presentation.read_text())
        body = struct_body(presentation_text, "TuiDocumentPresentation")
        prefix_fields = [name for name in FIELD_RE.findall(body) if "prefix" in name]
    except (ParseError, OSError) as error:
        raise ParseError(
            f"{error}\nTeach the guard the new shape rather than skipping it."
        ) from error

    if len(prefix_fields) != 1:
        violations.append(
            "TuiDocumentPresentation declares "
            f"{len(prefix_fields)} prefix field(s) {prefix_fields}; exactly one "
            "(`pending_prefix`) is the whole point of M002 — a new grammar adds a "
            "leaf, not a flag"
        )

    # 3. The replacements are still present, anywhere in the tree: the field
    #    and the two operations live on the presentation in `document_session`,
    #    while the resolver lives with the dispatch tables in `commands`.
    for needle in REQUIRED_MODEL_DECLARATIONS + REQUIRED_STATE_DECLARATIONS:
        if needle not in code_text:
            violations.append(f"expected declaration `{needle}` is absent from src/")

    # 4. The mismatch policy itself, not merely its name.
    violations.extend(check_mismatch_policy(code_text))

    if violations:
        return Report(violations, None)

    return Report(
        [],
        "TUI editor prefix-state guard passed — one pending-prefix field "
        f"({prefix_fields[0]}), typed as EditorPrefix, no legacy mechanism left",
    )


# ── Self-test ────────────────────────────────────────────────────────────────

# The files every required declaration lives in, plus the presentation the
# field rule reads. The self-test copies exactly these into a throwaway tree
# before mutating one, so a case cannot depend on a file the guard does not
# reason about and a mutation can never touch the working tree. The plain run
# still scans all of `src/`; this is the subset the assertions are about, kept
# small so the self-test stays as cheap as its siblings.
SELF_TEST_PATHS = (
    Path("src/tui/editor.rs"),
    Path("src/tui/document_session.rs"),
    Path("src/tui/components/editor.rs"),
    Path("src/tui/app/state/editor.rs"),
    Path("src/tui/commands/editor.rs"),
)

# One injection per rule the guard enforces. The assertion below fails if a
# case is deleted, because a shrinking self-test is the same vacuity it exists
# to prevent.
EXPECTED_INJECTIONS = 9


@dataclass(frozen=True)
class Injection:
    """One realistic regression, and the reason the guard must report."""

    name: str
    relative: Path
    old: str
    new: str
    expected: str


def copy_guard_tree(root: Path, destination: Path) -> None:
    for relative in SELF_TEST_PATHS:
        source = root / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, target)
        else:
            shutil.copy2(source, target)


def mutate(root: Path, relative: Path, old: str, new: str) -> tuple[Path, str]:
    """Replace the one occurrence and return what is needed to undo it."""
    path = root / relative
    text = path.read_text()
    occurrences = text.count(old)
    if occurrences != 1:
        raise AssertionError(
            f"self-test fixture text must appear exactly once in {relative} "
            f"({occurrences} occurrence(s)): {old!r}"
        )
    path.write_text(text.replace(old, new, 1))
    return path, text


def restore(undo: list[tuple[Path, str]]) -> None:
    for path, text in reversed(undo):
        path.write_text(text)


SELF_TEST_CASES = [
    Injection(
        name="a second prefix field returns under a new name (`yank_prefix`)",
        relative=Path("src/tui/document_session.rs"),
        old="    pub pending_prefix: EditorPrefix,\n",
        new=(
            "    pub pending_prefix: EditorPrefix,\n"
            "    pub yank_prefix: EditorPrefix,\n"
        ),
        expected="declares 2 prefix field(s)",
    ),
    Injection(
        name="the only prefix field is moved out, leaving prefix state nowhere",
        relative=Path("src/tui/document_session.rs"),
        old="    pub pending_prefix: EditorPrefix,\n",
        new="",
        expected="declares 0 prefix field(s)",
    ),
    Injection(
        name="the deleted `pending_command` mechanism is restored as a field",
        relative=Path("src/tui/document_session.rs"),
        old="    pub mode: EditorMode,\n",
        new="    pub mode: EditorMode,\n    pub pending_command: String,\n",
        expected="references deleted `pending_command`",
    ),
    Injection(
        name="a test module resurrects a deleted helper name",
        relative=Path("src/tui/commands/editor.rs"),
        old="#[cfg(test)]\nmod tests {\n    use super::*;\n",
        new=(
            "#[cfg(test)]\nmod tests {\n    use super::*;\n"
            "\n    fn pending_command() -> usize {\n        0\n    }\n"
        ),
        expected="references deleted `pending_command`",
    ),
    Injection(
        name="the mismatch policy is collapsed to one answer for every prefix",
        relative=Path("src/tui/editor.rs"),
        old=(
            "        self == EditorPrefix::Goto || self == EditorPrefix::View "
            "|| self == EditorPrefix::Register\n"
        ),
        new="        true\n",
        expected="answers for every prefix instead of Goto, Register, View",
    ),
    Injection(
        name="the policy loses the view-mode arm",
        relative=Path("src/tui/editor.rs"),
        old=" || self == EditorPrefix::View",
        new="",
        expected="answers for Goto, Register instead of Goto, Register, View",
    ),
    Injection(
        name="the policy loses the register-name arm",
        relative=Path("src/tui/editor.rs"),
        old=" || self == EditorPrefix::Register",
        new="",
        expected="answers for Goto, View instead of Goto, Register, View",
    ),
    Injection(
        name="the dispatch stops asking the prefix for the policy",
        relative=Path("src/tui/commands/editor.rs"),
        old="    if pending.swallows_unmatched() {\n",
        new="    if true {\n",
        expected="expected declaration `pending.swallows_unmatched()` is absent",
    ),
    Injection(
        name="the leaf table accessor is renamed away",
        relative=Path("src/tui/editor.rs"),
        old="    pub fn leaves(self) -> &'static [PrefixLeaf] {\n",
        new="    pub fn leaf_choices(self) -> &'static [PrefixLeaf] {\n",
        expected="expected declaration `pub fn leaves(` is absent",
    ),
]


def run_self_test() -> int:
    """Copy the tree, inject each regression, and require the guard to reject it.

    A case that stops being caught is a failure, not a quiet shrink: the guard
    would otherwise be evidence only of its own ability to print a passing
    line.
    """
    failures: list[str] = []

    if len(SELF_TEST_CASES) != EXPECTED_INJECTIONS:
        failures.append(
            f"the self-test declares {len(SELF_TEST_CASES)} injected defects, "
            f"the guard documents {EXPECTED_INJECTIONS}; a deleted case is a "
            "vacuous self-test"
        )

    with tempfile.TemporaryDirectory(prefix="codegg-prefix-state-") as raw:
        tree = Path(raw) / "tree"
        copy_guard_tree(REPO_ROOT, tree)

        try:
            baseline = check(tree)
        except ParseError as error:
            failures.append(f"the unmodified copy could not be checked: {error}")
            baseline = None
        if baseline is None:
            pass
        elif baseline.violations:
            failures.append(
                "the guard reports violations on an unmodified copy, so its "
                f"checks are not measuring what they claim: {baseline.violations}"
            )
        else:
            print("  ok: the unmodified copy passes")

        for case in SELF_TEST_CASES:
            try:
                undo = [mutate(tree, case.relative, case.old, case.new)]
            except AssertionError as error:
                failures.append(f"{case.name}: fixture could not be injected: {error}")
                continue
            try:
                report = check(tree)
            except ParseError as error:
                failures.append(
                    f"{case.name}: the guard could not resolve the mutated copy: {error}"
                )
                continue
            finally:
                restore(undo)
            if not any(case.expected in violation for violation in report.violations):
                failures.append(
                    f"{case.name}: the injected defect was not reported (expected "
                    f"a violation containing {case.expected!r}, got "
                    f"{report.violations})"
                )
            else:
                print(f"  ok: rejected {case.name}")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        "tui-editor-prefix-state self-test passed: the unmodified copy is "
        f"accepted and all {len(SELF_TEST_CASES)} injected defects are rejected"
    )
    return 0


def main(argv: list[str]) -> int:
    unknown = [argument for argument in argv if argument != "--self-test"]
    if unknown:
        print(f"unknown argument(s): {unknown}", file=sys.stderr)
        return 2
    if "--self-test" in argv:
        return run_self_test()

    try:
        report = check(REPO_ROOT)
    except ParseError as error:
        print(
            f"TUI editor prefix-state guard could not resolve a shape: {error}\n",
            file=sys.stderr,
        )
        return 2

    if report.violations:
        print(
            "TUI editor prefix-state violations found.\n"
            "The editor must have exactly one pending-prefix mechanism. It had two,\n"
            "and each new Helix grammar wanted a third; M002 replaces both with the\n"
            "`EditorPrefix` state machine. A legacy field left behind rather than\n"
            "deleted satisfies nothing.\n"
        )
        for violation in report.violations:
            print(f"  {violation}")
        print(f"\n{len(report.violations)} violation(s) found.")
        return 1

    print(report.summary)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
