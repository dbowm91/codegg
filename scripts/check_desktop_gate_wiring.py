#!/usr/bin/env python3
"""Guard: the desktop tree must stay inside an automated gate (M007-A).

The desktop is a separate Cargo workspace (`apps/desktop/src-tauri/Cargo.toml`
declares its own `[workspace]`), and the root workspace excludes it. The root
`cargo check` in `scripts/verify.sh quick` therefore never compiles
`apps/desktop/src-tauri/src/**`. The desktop tree is compiled, linted and
qualified by exactly one automated gate: the hosted `desktop-e2e` workflow.

That gate was paused on 2026-10-23 while it was red, and while it was paused
the desktop Rust tree was compiled by **no** automated gate at all — a whole
subsystem could fall out of CI without anything turning red. This guard makes
that impossible to repeat: it fails closed when the desktop tree is not in a
gate, and it is wired into both automatic lanes (`scripts/verify.sh quick` and
`.github/workflows/ci.yml`) so it runs whether or not anybody remembers it.

The controls this guard enforces:

- **G1 — the hosted workflow exists and is not manual-dispatch-only.** The
  workflow file must exist and be readable, and its `on:` block must carry
  both a `push:` and a `pull_request:` trigger. A qualification that only
  runs when a human dispatches it never runs on a pull request.
- **G2 — those triggers carry path filters that cover the desktop.** Both
  triggers must carry a `paths:` filter, and the filter must cover
  `apps/desktop/**` and the shared-client, protocol and daemon paths the
  desktop consumes. A trigger whose filter no longer names the desktop tree
  is a desktop tree that no longer compiles in CI.
- **G3 — the hosted lane lints the desktop host.** The host had never been
  linted at all, and a tree that is compiled but never linted is still
  unguarded. Two shapes put the lint in the hosted lane's executed path, and
  both are accepted: the workflow runs `cargo clippy` against
  `apps/desktop/src-tauri/Cargo.toml` itself, or the local entry point owns
  the lint in one of its phases and the workflow calls that phase (or calls
  the entry point with no selector, which runs every phase). The shipped lane
  uses the second shape: the entry point owns the step list so the hosted
  lane and the maintainer's machine cannot drift, and the workflow calls the
  phases of it. A workflow that calls a phase which does not reach the lint
  is a finding — the lint is present in the script but absent from the lane.
- **G4 — the local entry point exists and the workflow calls it.**
  `apps/desktop/scripts/verify-desktop.sh` must exist, be executable, and be
  referenced by a `run:` line in the workflow. The hosted lane and the
  maintainer's machine must be the same checks; the workflow has to call the
  script so the two cannot drift.
- **G5 — this guard is itself wired.** The guard must be named by
  `scripts/verify.sh`'s `run_guards()` and by `.github/workflows/ci.yml`. A
  guard that is not run automatically protects nothing.

Fail-closed is the contract. A missing workflow, an unreadable workflow, a
dispatch-only trigger, a filter that no longer covers the desktop, a missing
clippy step or a missing entry point are all findings. There is no state in
which this guard passes vacuously.

Three strictness choices are deliberate, so that they are not mistaken for
accidents:

- a trigger with **no** `paths:` filter is a finding, not a pass. The paused
  workflow's own header preserved the filter list, and restoring the trigger
  without it is a decision somebody has to make on purpose;
- the lint must be in the hosted lane's **executed path**. It may be written
  in the workflow or live in the entry point the workflow calls, but a
  workflow that calls a phase which does not reach the lint does not lint
  the host, and that is a finding;
- the required path list is a recorded set. A broader filter entry covers a
  narrower path (`src/core/**` covers `src/core/daemon.rs`), but narrowing or
  dropping one of the required paths is a deliberate edit to this file.

Modes:
    python3 scripts/check_desktop_gate_wiring.py             # static guard
    python3 scripts/check_desktop_gate_wiring.py --self-test # non-vacuity

`--self-test` builds a compliant fixture tree in a temporary directory,
requires the guard to accept it, then injects each defect class in turn and
requires the guard to reject every one of them. The fixture is the guard's
own: the self-test proves the guard's discrimination, while the ordinary run
is the verdict on the repository's own gate. The self-test never mutates the
repository's files.

A note on YAML: PyYAML is deliberately not used. It is not a dependency of the
repository's CI image, and YAML 1.1 (which PyYAML implements) resolves the
workflow key `on` to the boolean `True` — the one key this guard has to
read. The parser below therefore implements the subset of YAML a GitHub
Actions workflow actually uses: mappings, block sequences, flow sequences,
block scalars and plain scalars.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The hosted qualification that owns the desktop tree.
WORKFLOW_REL = Path(".github/workflows/desktop-e2e.yml")

# The one local entry point the hosted lane and the maintainer share.
ENTRY_POINT_REL = Path("apps/desktop/scripts/verify-desktop.sh")

# The desktop host's own Cargo manifest — a separate workspace, excluded from
# the root one, so nothing else compiles it.
# The desktop host is the repository's only `src-tauri/Cargo.toml`; a
# `--manifest-path` ending in it names the desktop host.
DESKTOP_MANIFEST_SUFFIX = "src-tauri/Cargo.toml"
DESKTOP_MANIFEST = "apps/desktop/src-tauri/Cargo.toml"

# The lanes this guard has to run in.
VERIFY_SH_REL = Path("scripts/verify.sh")
CI_WORKFLOW_REL = Path(".github/workflows/ci.yml")
GUARD_REL = "scripts/check_desktop_gate_wiring.py"

# The paths the desktop genuinely consumes, and therefore the paths its
# hosted qualification must be triggered by. This is the filter list the
# paused workflow's own header preserved. Narrowing or dropping one of them
# is a decision somebody has to make on purpose, so the guard holds the list
# here: changing it is a deliberate edit to this file.
REQUIRED_TRIGGER_PATHS = (
    "apps/desktop/**",
    "crates/codegg-client/**",
    "crates/codegg-protocol/**",
    "crates/codegg-core/src/projection_replay/**",
    "crates/codegg-core/src/storage/**",
    "src/core/transport/**",
    "src/core/daemon_turns.rs",
    "src/core/daemon_projects.rs",
    "src/core/daemon.rs",
)

# The triggers the hosted qualification must carry. `workflow_dispatch` alone
# is a manual lane.
REQUIRED_TRIGGERS = ("push", "pull_request")

# `python3 scripts/check_desktop_gate_wiring.py` — how both lanes invoke it.
GUARD_INVOCATION = re.compile(r"python3\s+" + re.escape(GUARD_REL))

# A script path inside a `run:` line.
SCRIPT_TOKEN = re.compile(r"[^\s'\";|&]*verify-desktop\.sh")

# A GitHub Actions expression. It is not part of a path.
WORKFLOW_EXPRESSION = re.compile(r"\$\{\{.*?\}\}")


# ── A YAML subset parser ──────────────────────────────────────────────


def _indent_of(line: str) -> int:
    """The number of leading spaces on `line`."""
    return len(line) - len(line.lstrip(" "))


def _strip_comment(line: str) -> str:
    """Drop a trailing `#` comment that sits outside quotes."""
    quote = None
    for index, ch in enumerate(line):
        if quote is not None:
            if ch == quote:
                quote = None
            continue
        if ch in "'\"":
            quote = ch
            continue
        if ch == "#" and (index == 0 or line[index - 1] in " \t"):
            return line[:index]
    return line


class Workflow:
    """The parsed form of one workflow file.

    Only the constructs a GitHub Actions workflow uses are supported: nested
    mappings, block sequences, flow sequences (`[main]`), block scalars
    (`run: |`) and plain scalars. Anything else is read as a plain scalar,
    which is enough for the questions this guard asks.
    """

    def __init__(self, text: str) -> None:
        self.lines = text.splitlines()
        self.pos = 0

    # -- reading ---------------------------------------------------------

    def parse(self) -> dict:
        """The workflow as nested dicts, lists and strings."""
        self._skip_ignorable()
        if self.pos >= len(self.lines):
            return {}
        value = self._parse_node(_indent_of(self.lines[self.pos]))
        return value if isinstance(value, dict) else {}

    def _skip_ignorable(self) -> None:
        """Advance past blank lines and whole-line comments."""
        while self.pos < len(self.lines):
            stripped = self.lines[self.pos].strip()
            if not stripped or stripped.startswith("#"):
                self.pos += 1
                continue
            return

    def _parse_node(self, indent: int):
        """The block at `indent`: a mapping or a sequence."""
        self._skip_ignorable()
        if self.pos >= len(self.lines):
            return None
        content = _strip_comment(self.lines[self.pos]).strip()
        if content.startswith("-"):
            return self._parse_sequence(indent)
        return self._parse_mapping(indent)

    def _parse_mapping(self, indent: int) -> dict:
        result: dict = {}
        while True:
            self._skip_ignorable()
            if self.pos >= len(self.lines):
                break
            line = self.lines[self.pos]
            if _indent_of(line) != indent:
                break
            content = _strip_comment(line).strip()
            if content.startswith("-"):
                break
            key, separator, rest = self._split_key(content)
            if not separator:
                # A line that is not a mapping entry: skip it rather than
                # mis-reading the rest of the document.
                self.pos += 1
                continue
            self.pos += 1
            rest = rest.strip()
            if rest[:1] in ("|", ">"):
                result[key] = self._parse_block_scalar(indent)
            elif rest:
                result[key] = self._parse_scalar(rest)
            else:
                result[key] = self._parse_nested(indent)
        return result

    def _parse_sequence(self, indent: int) -> list:
        items: list = []
        while True:
            self._skip_ignorable()
            if self.pos >= len(self.lines):
                break
            line = self.lines[self.pos]
            if _indent_of(line) != indent:
                break
            content = _strip_comment(line).strip()
            if not content.startswith("-"):
                break
            dash = line.index("-")
            after = line[dash + 1 :]
            spaces = len(after) - len(after.lstrip(" "))
            rest = content[1:].strip()
            if not rest:
                self.pos += 1
                items.append(self._parse_nested(indent))
                continue
            column = dash + 1 + spaces
            key, separator, _ = self._split_key(rest)
            if separator and not rest.startswith(("'", '"')):
                # `- name: something` opens a mapping whose first key sits at
                # the column the content starts in. The dash is rewritten as
                # spaces so the mapping continues on the following lines.
                self.lines[self.pos] = " " * column + rest
                items.append(self._parse_mapping(column))
                continue
            self.pos += 1
            items.append(self._parse_scalar(rest))
        return items

    def _parse_nested(self, parent_indent: int):
        """The block nested under a key, or `None` when there is none."""
        saved = self.pos
        self._skip_ignorable()
        if self.pos >= len(self.lines):
            self.pos = saved
            return None
        indent = _indent_of(self.lines[self.pos])
        content = _strip_comment(self.lines[self.pos]).strip()
        if indent == parent_indent and content.startswith("-"):
            # A block sequence may sit at the same column as the key that
            # owns it.
            return self._parse_sequence(indent)
        if indent <= parent_indent:
            self.pos = saved
            return None
        return self._parse_node(indent)

    def _parse_block_scalar(self, parent_indent: int) -> str:
        """The body of a `|` block scalar, dedented to its own margin."""
        body: list[str] = []
        margin: int | None = None
        while self.pos < len(self.lines):
            line = self.lines[self.pos]
            if not line.strip():
                body.append("")
                self.pos += 1
                continue
            if _indent_of(line) <= parent_indent:
                break
            if margin is None:
                margin = _indent_of(line)
            body.append(line[margin:])
            self.pos += 1
        while body and not body[-1].strip():
            body.pop()
        return "\n".join(body)

    @staticmethod
    def _split_key(content: str) -> tuple[str, str, str]:
        """`key: value` split into its three parts."""
        quote = None
        for index, ch in enumerate(content):
            if quote is not None:
                if ch == quote:
                    quote = None
                continue
            if ch in "'\"":
                quote = ch
                continue
            if ch == ":":
                rest = content[index + 1 :]
                if not rest or rest[0] in " \t":
                    key = content[:index].strip()
                    if len(key) >= 2 and key[0] == key[-1] and key[0] in "'\"":
                        key = key[1:-1]
                    return key, ":", rest
        return content, "", ""

    @staticmethod
    def _parse_scalar(text: str) -> object:
        """A plain scalar or a flow sequence."""
        text = text.strip()
        if text.startswith("[") and text.endswith("]"):
            inner = text[1:-1].strip()
            if not inner:
                return []
            return [Workflow._parse_scalar(part) for part in inner.split(",")]
        if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
            return text[1:-1]
        return text


# ── Reading the workflow ──────────────────────────────────────────────


def _read(path: Path) -> str | None:
    """The file's text, or `None` when it cannot be read."""
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return None


def _steps(workflow: dict) -> list[dict]:
    """Every step of every job in the workflow."""
    jobs = workflow.get("jobs")
    if not isinstance(jobs, dict):
        return []
    steps: list[dict] = []
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        job_steps = job.get("steps")
        if isinstance(job_steps, list):
            steps.extend(step for step in job_steps if isinstance(step, dict))
    return steps


def _run_text(step: dict) -> str:
    """The `run:` body of one step."""
    run = step.get("run")
    return run if isinstance(run, str) else ""


def _logical_lines(text: str) -> list[str]:
    """`text` split into shell commands, with `\\` continuations joined.

    A whole-line shell comment is dropped: a commented-out command is not a
    command, and a gate that counted one would accept a step that lints
    nothing.
    """
    commands: list[str] = []
    current = ""
    for raw in text.splitlines():
        stripped = raw.strip()
        if not current and stripped.startswith("#"):
            continue
        current = f"{current} {stripped}" if current else stripped
        if current.endswith("\\"):
            current = current[:-1].rstrip()
            continue
        commands.append(current)
        current = ""
    if current:
        commands.append(current)
    return commands


def _is_desktop_clippy(command: str) -> bool:
    """Whether one shell command lints the desktop host.

    The desktop host is the repository's only `src-tauri/Cargo.toml`, so a
    `--manifest-path` that ends in it names the desktop host whether the
    path is written out in full or held in a shell variable.
    """
    if "cargo clippy" not in command:
        return False
    if DESKTOP_MANIFEST in command:
        return True
    # `cd apps/desktop/src-tauri && cargo clippy …` lints the same host.
    if "apps/desktop/src-tauri" in command:
        return True
    for match in re.finditer(r"--manifest-path[= ]+(\S+)", command):
        value = match.group(1).strip("\"'")
        if value.endswith(DESKTOP_MANIFEST_SUFFIX):
            return True
    return False


def _substitute_script_variables(text: str) -> str:
    """Resolve simple shell assignments so a variable-held path is readable.

    `desktop_manifest="$app_root/src-tauri/Cargo.toml"` is the desktop
    host's manifest; a textual scan that cannot see through the variable
    would report the lane as unlinted when it is linted. Only plain
    assignments are resolved — a value containing a command substitution is
    left as it is, and the suffix rule still recognises it.
    """
    assignments: dict[str, str] = {}
    for line in text.split("\n"):
        match = re.match(r"^\s*([A-Za-z_][A-Za-z0-9_]*)=\s*\"([^\"]*)\"", line)
        if match:
            assignments[match.group(1)] = match.group(2)
    if not assignments:
        return text
    resolved = text
    for _ in range(4):
        previous = resolved
        for name, value in assignments.items():
            resolved = resolved.replace("${" + name + "}", value)
            resolved = re.sub(r"\$" + re.escape(name) + r"\b", value, resolved)
        if resolved == previous:
            break
    return resolved


def _filter_covers(entry: str, required: str) -> bool:
    """Whether one `paths:` filter entry covers `required`.

    A filter entry is a glob, so a broader entry covers a narrower path:
    `src/core/**` covers `src/core/daemon.rs`, and `apps/desktop/**` covers
    `apps/desktop/src-tauri/Cargo.toml`. A plain path covers only itself.
    """
    entry = entry.strip().strip("'\"")
    if not entry:
        return False
    if entry == required:
        return True
    if entry.endswith("/**"):
        prefix = entry[:-3].rstrip("/")
        return required == prefix or required.startswith(f"{prefix}/")
    if entry.endswith("/*"):
        prefix = entry[:-2].rstrip("/")
        parent = required.rsplit("/", 1)[0] if "/" in required else ""
        return parent == prefix
    return False


def _resolve_step_path(step: dict, token: str) -> str:
    """The repository-relative path a `run:` token refers to.

    A `${{ … }}` expression is not part of the path: `${{ github.workspace
    }}/apps/desktop/scripts/verify-desktop.sh` names the entry point, and a
    workspace-rooted path is relative to the repository root.
    """
    token = WORKFLOW_EXPRESSION.sub("", token)
    working = step.get("working-directory")
    parts = (
        [part for part in working.split("/") if part and part != "."]
        if isinstance(working, str)
        else []
    )
    token = token.removeprefix("./")
    if token.startswith("/"):
        # A workspace-rooted path (`${{ github.workspace }}/…`) is relative to
        # the repository root.
        parts = []
        token = token.lstrip("/")
    parts.append(token)
    return "/".join(parts)


# ── The controls ──────────────────────────────────────────────────────


def check_triggers(workflow: dict) -> list[str]:
    """G1 — the workflow exists, and its triggers are not dispatch-only."""
    findings: list[str] = []
    on_block = workflow.get("on")
    if on_block is None:
        findings.append(
            f"{WORKFLOW_REL}: the workflow has no `on:` block, so it runs on no "
            "trigger at all and the desktop tree is compiled by no automated gate."
        )
        return findings
    if not isinstance(on_block, dict):
        findings.append(
            f"{WORKFLOW_REL}: the `on:` block is not a mapping of triggers, so "
            "the guard cannot prove which events run the hosted qualification."
        )
        return findings
    for trigger in REQUIRED_TRIGGERS:
        if trigger not in on_block:
            findings.append(
                f"{WORKFLOW_REL}: the `on:` block has no `{trigger}:` trigger. A "
                "manual-dispatch-only qualification never runs on a pull "
                "request, so a change to the desktop tree compiles in no "
                "automated gate."
            )
    return findings


def check_path_filters(workflow: dict) -> list[str]:
    """G2 — both triggers filter on the paths the desktop consumes."""
    findings: list[str] = []
    on_block = workflow.get("on")
    if not isinstance(on_block, dict):
        return findings
    for trigger in REQUIRED_TRIGGERS:
        if trigger not in on_block:
            continue  # Already reported by `check_triggers`.
        config = on_block.get(trigger)
        paths = config.get("paths") if isinstance(config, dict) else None
        if not isinstance(paths, list) or not paths:
            findings.append(
                f"{WORKFLOW_REL}: the `{trigger}:` trigger carries no `paths:` "
                "filter. The restored trigger keeps the path filters the pause "
                "preserved; dropping them is a decision, not a default."
            )
            continue
        entries = [str(entry) for entry in paths]
        for required in REQUIRED_TRIGGER_PATHS:
            if not any(_filter_covers(entry, required) for entry in entries):
                findings.append(
                    f"{WORKFLOW_REL}: the `{trigger}:` path filter does not cover "
                    f"`{required}`. The desktop consumes that path, so a change "
                    "to it would not run the hosted qualification."
                )
    return findings


def entry_point_lint_phase(entry_text: str) -> str | None:
    """The selector of the entry-point phase that lints the desktop host.

    The hosted lane may own the lint in two places: directly in a workflow
    `run:` block, or inside the local entry point the workflow calls. In the
    second form the lint lives in a phase of that script, and the phase's
    name is derived from the shell function that encloses the invocation
    (`phase_host()` yields `host`) rather than hard-coded, so the guard
    tracks the script instead of pinning one spelling of it.

    `None` means the script does not lint the desktop host at all, or the
    invocation cannot be attributed to a phase — both are findings.
    """
    phase: str | None = None
    resolved = _substitute_script_variables(entry_text)
    resolved_lines = resolved.split("\n")
    for index, line in enumerate(entry_text.split("\n")):
        match = re.match(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\(\)\s*\{", line)
        if match:
            name = match.group(1)
            phase = name[len("phase_"):] if name.startswith("phase_") else name
        candidate = resolved_lines[index] if index < len(resolved_lines) else line
        if _is_desktop_clippy(_strip_comment(candidate)):
            return phase
    return None


def entry_point_invocations(workflow: dict) -> list[list[str]]:
    """The argument lists of every entry-point invocation in the workflow.

    An invocation with no argument runs the whole gate; one with a selector
    runs only that phase. The list is empty when the workflow never calls
    the entry point.
    """
    invocations: list[list[str]] = []
    for step in _steps(workflow):
        text = WORKFLOW_EXPRESSION.sub("", _run_text(step))
        for command in _logical_lines(text):
            token_match = SCRIPT_TOKEN.search(command)
            if token_match is None:
                continue
            if _resolve_step_path(step, token_match.group(0)) != ENTRY_POINT_REL.as_posix():
                continue
            rest = command[token_match.end():].strip()
            invocations.append(rest.split() if rest else [])
    return invocations


def check_clippy_step(
    workflow: dict,
    lint_phase: str | None = None,
    invocations: list[list[str]] | None = None,
) -> list[str]:
    """G3 — the hosted lane lints the desktop host.

    Two forms are accepted, and both put the lint in the hosted lane's
    executed path:

    - the workflow runs `cargo clippy` against the desktop manifest itself;
    - the local entry point owns the lint in one of its phases, and the
      workflow calls that phase (or calls the entry point with no selector,
      which runs every phase).

    The second form is the one the shipped lane uses: the entry point owns
    the step list so the hosted lane and the maintainer's machine cannot
    drift, and the workflow calls the phases of it.
    """
    for step in _steps(workflow):
        for command in _logical_lines(_run_text(step)):
            if _is_desktop_clippy(command):
                return []
    if lint_phase is not None:
        for args in invocations or []:
            if not args or lint_phase in args:
                return []
    return [
        f"{WORKFLOW_REL}: the hosted lane never lints the desktop host with "
        f"`cargo clippy` against `{DESKTOP_MANIFEST}`. The desktop host is "
        "compiled by no lint gate, so a lint regression in it is caught by "
        "nothing."
    ]


def check_entry_point(root: Path, workflow: dict) -> list[str]:
    """G4 — the local entry point exists, is executable, and is called."""
    findings: list[str] = []
    entry = root / ENTRY_POINT_REL
    if not entry.is_file():
        findings.append(
            f"{ENTRY_POINT_REL}: the local entry point is missing. The hosted "
            "lane and the maintainer's machine must run the same checks, and "
            "the script is where that list lives."
        )
        return findings
    if not os.access(entry, os.X_OK):
        findings.append(
            f"{ENTRY_POINT_REL}: the local entry point is not executable, so the "
            "documented local invocation does not run."
        )
    referenced = False
    for step in _steps(workflow):
        # A `${{ … }}` expression is not part of a path, so it is removed
        # before the command is read: `${{ github.workspace }}/apps/desktop/
        # scripts/verify-desktop.sh` names the entry point.
        text = WORKFLOW_EXPRESSION.sub("", _run_text(step))
        for command in _logical_lines(text):
            token_match = SCRIPT_TOKEN.search(command)
            if token_match is None:
                continue
            if _resolve_step_path(step, token_match.group(0)) == ENTRY_POINT_REL.as_posix():
                referenced = True
    if not referenced:
        findings.append(
            f"{WORKFLOW_REL}: no step runs `{ENTRY_POINT_REL}`. The hosted lane "
            "and the local entry point must be the same checks, and the "
            "workflow has to call the script so the two cannot drift."
        )
    return findings


def check_guard_wiring(root: Path) -> list[str]:
    """G5 — this guard is wired into both automatic lanes."""
    findings: list[str] = []
    verify_sh = _read(root / VERIFY_SH_REL)
    if verify_sh is None:
        findings.append(
            f"{VERIFY_SH_REL}: the file that wires the guards cannot be read, so "
            "this guard may not be wired at all."
        )
    else:
        body = _run_guards_body(verify_sh)
        if not GUARD_INVOCATION.search(body):
            findings.append(
                f"{VERIFY_SH_REL}: `run_guards()` does not invoke "
                f"`{GUARD_REL}`. A guard that is not run automatically protects "
                "nothing."
            )
    ci_workflow = _read(root / CI_WORKFLOW_REL)
    if ci_workflow is None:
        findings.append(
            f"{CI_WORKFLOW_REL}: the CI workflow cannot be read, so this guard "
            "may not run in CI at all."
        )
    elif not GUARD_INVOCATION.search(ci_workflow):
        findings.append(
            f"{CI_WORKFLOW_REL}: no step invokes `{GUARD_REL}`. A guard that CI "
            "does not run protects nothing."
        )
    return findings


def _run_guards_body(verify_sh: str) -> str:
    """The body of `run_guards()` in `scripts/verify.sh`.

    The guard list lives in that function and must not be duplicated
    elsewhere, so the wiring check reads the function itself.
    """
    lines = verify_sh.splitlines()
    collected: list[str] = []
    inside = False
    for line in lines:
        if not inside:
            if line.startswith("run_guards()"):
                inside = True
            continue
        if line.startswith("}"):
            break
        collected.append(line)
    return "\n".join(collected)


def check_gate(root: Path) -> list[str]:
    """Every control, against the tree under `root`."""
    findings: list[str] = []

    text = _read(root / WORKFLOW_REL)
    if text is None:
        findings.append(
            f"{WORKFLOW_REL}: the hosted desktop qualification workflow is "
            "missing or unreadable. The desktop tree is a separate Cargo "
            "workspace that the root check never compiles, so without this "
            "workflow nothing compiles it."
        )
        workflow: dict = {}
    else:
        workflow = Workflow(text).parse()

    findings.extend(check_triggers(workflow))
    findings.extend(check_path_filters(workflow))
    entry_text = ""
    entry = root / ENTRY_POINT_REL
    if entry.is_file():
        try:
            entry_text = entry.read_text(encoding="utf-8")
        except OSError:
            entry_text = ""
    findings.extend(
        check_clippy_step(
            workflow,
            entry_point_lint_phase(entry_text),
            entry_point_invocations(workflow),
        )
    )
    findings.extend(check_entry_point(root, workflow))
    findings.extend(check_guard_wiring(root))
    return findings


# ── The self-test fixture ─────────────────────────────────────────────

FIXTURE_TRIGGERS = """on:
  push:
    branches: [main]
    paths:
      - 'apps/desktop/**'
      - 'crates/codegg-client/**'
      - 'crates/codegg-protocol/**'
      - 'crates/codegg-core/src/projection_replay/**'
      - 'crates/codegg-core/src/storage/**'
      - 'src/core/transport/**'
      - 'src/core/daemon_turns.rs'
      - 'src/core/daemon_projects.rs'
      - 'src/core/daemon.rs'
      - '.github/workflows/desktop-e2e.yml'
  pull_request:
    paths:
      - 'apps/desktop/**'
      - 'crates/codegg-client/**'
      - 'crates/codegg-protocol/**'
      - 'crates/codegg-core/src/projection_replay/**'
      - 'crates/codegg-core/src/storage/**'
      - 'src/core/transport/**'
      - 'src/core/daemon_turns.rs'
      - 'src/core/daemon_projects.rs'
      - 'src/core/daemon.rs'
      - '.github/workflows/desktop-e2e.yml'
  workflow_dispatch:
"""

# The shipped lane's shape: the entry point owns the step list, the
# workflow calls the phase of it that lints the host. The lint therefore
# lives in the script, and the guard has to follow the call to find it.
FIXTURE_ENTRY_POINT_STEP = """      - name: Desktop host checks
        run: apps/desktop/scripts/verify-desktop.sh host
"""

FIXTURE_WORKFLOW = (
    "name: Desktop E2E\n\n"
    + FIXTURE_TRIGGERS
    + "\njobs:\n  e2e:\n    runs-on: ubuntu-24.04\n    steps:\n"
    + FIXTURE_ENTRY_POINT_STEP
)

FIXTURE_ENTRY_POINT = """#!/usr/bin/env bash
set -euo pipefail

phase_host() {
  cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
}

main() {
  phase_host
}
"""

# The other accepted shape: the workflow runs the lint itself. The guard
# accepts it too, and the self-test proves both are accepted.
FIXTURE_INLINE_CLIPPY_STEP = """      - name: Desktop host checks
        run: |
          rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
"""

FIXTURE_INLINE_WORKFLOW = (
    "name: Desktop E2E\n\n"
    + FIXTURE_TRIGGERS
    + "\njobs:\n  e2e:\n    runs-on: ubuntu-24.04\n    steps:\n"
    + FIXTURE_INLINE_CLIPPY_STEP
    + "\n"
    + FIXTURE_ENTRY_POINT_STEP
)

FIXTURE_VERIFY_SH = """#!/usr/bin/env bash
run_guards() {
    echo "==> python3 scripts/check_desktop_gate_wiring.py"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_gate_wiring.py)
}
"""

FIXTURE_CI_WORKFLOW = """name: CI

on:
  pull_request:

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - name: Desktop gate wiring guard
        run: python3 scripts/check_desktop_gate_wiring.py
"""


def _write_fixture(root: Path) -> None:
    """A compliant gate tree under `root`."""
    workflow = root / WORKFLOW_REL
    workflow.parent.mkdir(parents=True, exist_ok=True)
    workflow.write_text(FIXTURE_WORKFLOW, encoding="utf-8")
    entry = root / ENTRY_POINT_REL
    entry.parent.mkdir(parents=True, exist_ok=True)
    entry.write_text(FIXTURE_ENTRY_POINT, encoding="utf-8")
    entry.chmod(0o755)
    verify_sh = root / VERIFY_SH_REL
    verify_sh.parent.mkdir(parents=True, exist_ok=True)
    verify_sh.write_text(FIXTURE_VERIFY_SH, encoding="utf-8")
    ci = root / CI_WORKFLOW_REL
    ci.parent.mkdir(parents=True, exist_ok=True)
    ci.write_text(FIXTURE_CI_WORKFLOW, encoding="utf-8")


def _replace(path: Path, old: str, new: str) -> None:
    """Replace `old` with `new` in `path`, once."""
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects each defect class it is meant to catch."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="codegg-desktop-gate-") as raw:
        fixture = Path(raw)
        _write_fixture(fixture)

        findings = check_gate(fixture)
        if findings:
            failures.append(
                "the guard did not accept a compliant gate: " + "; ".join(findings)
            )
            _report(failures)
            return 1
        print("  ok: a compliant gate is accepted")

        # The other accepted shape: the workflow runs the lint itself.
        with tempfile.TemporaryDirectory(prefix="codegg-desktop-gate-inline-") as raw_inline:
            inline = Path(raw_inline)
            _write_fixture(inline)
            (inline / WORKFLOW_REL).write_text(FIXTURE_INLINE_WORKFLOW, encoding="utf-8")
            inline_findings = check_gate(inline)
            if inline_findings:
                failures.append(
                    "the guard did not accept a gate whose workflow runs the lint "
                    "itself: " + "; ".join(inline_findings)
                )
                _report(failures)
                return 1
            print("  ok: a gate whose workflow lints the host itself is accepted")

        cases: list[tuple[str, str, str]] = [
            (
                "a dispatch-only trigger",
                "no `push:` trigger",
                "no `pull_request:` trigger",
            ),
            (
                "a path filter that omits the desktop tree",
                "path filter does not cover `apps/desktop/**`",
                "",
            ),
            (
                "a hosted lane that never lints the desktop host",
                "the hosted lane never lints the desktop host",
                "",
            ),
            (
                "an entry point the workflow calls with a phase that does not lint",
                "the hosted lane never lints the desktop host",
                "",
            ),
            (
                "a removed local entry point",
                "the local entry point is missing",
                "",
            ),
            (
                "a local entry point the workflow never calls",
                "no step runs `apps/desktop/scripts/verify-desktop.sh`",
                "",
            ),
            (
                "a guard that CI does not run",
                f"{CI_WORKFLOW_REL}: no step invokes `{GUARD_REL}`",
                "",
            ),
        ]

        for label, needle, second_needle in cases:
            with tempfile.TemporaryDirectory(prefix="codegg-desktop-gate-case-") as raw_case:
                case = Path(raw_case)
                _write_fixture(case)
                workflow = case / WORKFLOW_REL

                if label == "a dispatch-only trigger":
                    text = workflow.read_text(encoding="utf-8")
                    start = text.index("on:\n")
                    end = text.index("jobs:\n")
                    workflow.write_text(
                        text[:start] + "on:\n  workflow_dispatch:\n\n" + text[end:],
                        encoding="utf-8",
                    )
                elif label == "a path filter that omits the desktop tree":
                    text = workflow.read_text(encoding="utf-8")
                    workflow.write_text(
                        text.replace("      - 'apps/desktop/**'\n", ""),
                        encoding="utf-8",
                    )
                elif label == "a hosted lane that never lints the desktop host":
                    # The lint lives in the entry point's host phase; removing
                    # it there leaves the hosted lane with no lint at all.
                    entry = case / ENTRY_POINT_REL
                    text = entry.read_text(encoding="utf-8")
                    entry.write_text(
                        text.replace(
                            "  cargo clippy --manifest-path "
                            "apps/desktop/src-tauri/Cargo.toml "
                            "--all-targets --locked -- -D warnings\n",
                            "",
                        ),
                        encoding="utf-8",
                    )
                elif label == (
                    "an entry point the workflow calls with a phase that does not lint"
                ):
                    # The lint is still in the script, but the workflow calls a
                    # phase that does not reach it.
                    _replace(
                        workflow,
                        "apps/desktop/scripts/verify-desktop.sh host",
                        "apps/desktop/scripts/verify-desktop.sh renderer",
                    )
                elif label == "a removed local entry point":
                    (case / ENTRY_POINT_REL).unlink()
                elif label == "a local entry point the workflow never calls":
                    _replace(workflow, FIXTURE_ENTRY_POINT_STEP, "")
                else:
                    _replace(
                        case / CI_WORKFLOW_REL,
                        "        run: python3 scripts/check_desktop_gate_wiring.py\n",
                        "",
                    )

                findings = check_gate(case)
                if not findings:
                    failures.append(f"the guard did not reject {label}")
                    continue
                if not any(needle in finding for finding in findings):
                    failures.append(
                        f"the guard rejected {label}, but not for the expected "
                        f"reason ({needle!r}): {findings}"
                    )
                    continue
                if second_needle and not any(second_needle in f for f in findings):
                    failures.append(
                        f"the guard rejected {label}, but did not report "
                        f"{second_needle!r}: {findings}"
                    )
                    continue
                print(f"  ok: rejected {label}")

    if failures:
        _report(failures)
        return 1

    # The repository's own gate is the guard's ordinary verdict, not the
    # self-test's: a finding there is a fact about the tree, and this
    # self-test must keep measuring the guard rather than the tree.
    findings = check_gate(ROOT)
    if findings:
        print(f"  note: the repository's own gate has {len(findings)} finding(s):")
        for finding in findings:
            print(f"    {finding}")
    else:
        print("  ok: the repository's own gate is complete")
    print("desktop gate wiring guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it is meant to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    findings = check_gate(ROOT)
    if findings:
        for finding in findings:
            print(f"ERROR: {finding}", file=sys.stderr)
        print(f"{len(findings)} desktop gate wiring violation(s)", file=sys.stderr)
        return 1
    print("desktop gate wiring guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
