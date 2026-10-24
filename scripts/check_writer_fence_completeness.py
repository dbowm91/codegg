#!/usr/bin/env python3
"""Guard: the writer fence must be complete on every path that can write.

M005 corrective C009 found three ways a write could reach the shared checkout
without the authority the fence exists to enforce:

  1. The `commit` tool was registered without the fail-closed `unrooted(...)`
     wrapper every other workspace tool takes. With no resolved workspace root
     it kept the sentinel workdir `\\0codegg:no-workspace-root`, which
     `canonicalize_existing_prefix` resolves *under* the checkout root — so the
     fence's containment check passed, the tool took the checkout's writer
     permit, and the failure it then produced parked the checkout.
  2. The daemon built its writer fence with the local-principal fallback
     asserted as a literal `true`, so the "a daemon with a team policy refuses
     an unattributed shared-checkout write" rule was unreachable: every shared
     write was attributed to the synthetic `codegg-writer-fence` owner.
  3. The `task` tool's in-process nested dispatch dropped the fence, and the
     worker substituted a containment-only fence that can never take the
     writer permit — so a grandchild wrote into the shared checkout while a
     group sibling held the permit.

This script enforces the three corresponding invariants statically, plus the
two the follow-up corrective added: the production permit authority must
consult the daemon's authorization service, and a delegated child must carry
the dispatching turn's attribution instead of writing anonymously. It
self-tests: `--self-test` feeds synthetic sources through the same detectors
used against the real tree and fails if a planted violation is not reported.
A guard that cannot be shown to fail on an injected violation is not accepted
as evidence.

Usage:
    python3 scripts/check_writer_fence_completeness.py            # scan the tree
    python3 scripts/check_writer_fence_completeness.py --self-test
Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# ── Scanned surfaces ─────────────────────────────────────────────────

REGISTRY_FILE = "src/tool/mod.rs"
DAEMON_CONSTRUCT_FILE = "src/core/daemon_construct.rs"
WORKER_FILE = "src/agent/worker.rs"
TASK_TOOL_FILE = "src/tool/task.rs"
DAEMON_FENCE_FILE = "src/core/writer_fence.rs"
RUNTIME_DEPS_FILE = "src/core/runtime_deps.rs"
EXECUTORS_FILE = "src/scheduler/executors.rs"
TOOL_BATCH_FILE = "src/agent/tool_batch.rs"

SCANNED_FILES: list[str] = [
    REGISTRY_FILE,
    DAEMON_CONSTRUCT_FILE,
    WORKER_FILE,
    TASK_TOOL_FILE,
    DAEMON_FENCE_FILE,
    RUNTIME_DEPS_FILE,
    EXECUTORS_FILE,
    TOOL_BATCH_FILE,
]

# ── Rule A: mutating workspace tools fail closed ─────────────────────
#
# The workspace tools listed here are the mutating ones: they change the
# checkout, so a registry built without a resolved workspace root must REFUSE
# them rather than default. The tool names mirror the `Write` / `GitMutation`
# risk classes in `src/tool/risk.rs`, plus the process-capable `terminal`.
#
# `bash` is deliberately absent: `BashTool` is fail-closed by a different
# mechanism (its default root is the `unresolved_workspace_root()` sentinel,
# which `validate_path` rejects), and it carries its own separate workspace
# ceiling. Listing it here would be a false positive, not a stricter guard.
MUTATING_TOOL_CONSTRUCTORS: dict[str, str] = {
    "crate::tool::edit::EditTool": "edit",
    "crate::tool::write::WriteTool": "write",
    "crate::tool::replace::ReplaceTool": "replace",
    "crate::tool::apply_patch::ApplyPatchTool": "apply_patch",
    "crate::tool::git::GitTool": "git",
    "crate::tool::commit::CommitTool": "commit",
    "crate::tool::terminal::TerminalTool": "terminal",
}

# The fail-closed wrapper. `ToolRegistry::with_options` documents that its
# `None` arms REFUSE rather than defaulting; `unrooted` is how they do it.
UNROOTED_MARKER = "unrooted("

REGISTER_RE = re.compile(r"registry\s*\.\s*register(?:_boxed)?\s*\(")
LET_BINDING_RE = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\b")
FOR_LOOP_RE = re.compile(r"\bfor\s+([A-Za-z_][A-Za-z0-9_]*)\b")


@dataclass(frozen=True)
class Finding:
    file: str
    line_no: int
    line_text: str
    rule: str


def _line_no(source: str, index: int) -> int:
    return source.count("\n", 0, index) + 1


def _line_text(source: str, index: int) -> str:
    start = source.rfind("\n", 0, index) + 1
    end = source.find("\n", index)
    if end == -1:
        end = len(source)
    return source[start:end].strip()


def _scan_string_and_char(source: str, index: int) -> int:
    """Return the index just past the literal that starts at `index`.

    Rust lifetimes (`'_`, `'a`) are not literals. A `'` that is followed by
    an identifier character is treated as a lifetime so that a generic
    lifetime argument does not desynchronise the bracket scan.
    """
    quote = source[index]
    i = index + 1
    if quote == "'":
        if i < len(source) and (source[i] == "_" or source[i].isalpha()):
            return index + 1
    while i < len(source):
        ch = source[i]
        if ch == "\\":
            i += 2
            continue
        if ch == quote:
            return i + 1
        if quote == '"' and ch == "\n":
            return i
        i += 1
    return len(source)


def _balanced(source: str, open_index: int) -> tuple[int, int]:
    """Return `(start, end)` of the balanced bracket group at `open_index`.

    Every bracket kind is tracked, not just the one that opened the group:
    `registry.register(match root.as_ref() { ... })` closes its inner paren
    long before its outer one, and a single-kind counter would end the
    statement at the wrong place.
    """
    pairs = {")": "(", "]": "[", "}": "{"}
    stack: list[str] = []
    i = open_index
    while i < len(source):
        ch = source[i]
        if ch in "\"'":
            i = _scan_string_and_char(source, i)
            continue
        if ch in "([{":
            stack.append(ch)
        elif ch in ")]}":
            if not stack or pairs[ch] != stack[-1]:
                return (open_index, i + 1)
            stack.pop()
            if not stack:
                return (open_index, i + 1)
        i += 1
    return (open_index, len(source))


def _statement_text(source: str, open_paren: int) -> str:
    """Return the full registration statement text starting at `open_paren`."""
    start = source.rfind("\n", 0, open_paren) + 1
    _, end = _balanced(source, open_paren)
    tail = source[end : end + 4]
    semi = tail.find(";")
    if semi != -1:
        end += semi + 1
    return source[start:end]


def _enclosing_function_start(source: str, index: int) -> int:
    """Return the index of the nearest preceding `fn ` item."""
    matches = list(re.finditer(r"\n(?:pub(?:\([^\)]*\))?\s+)?(?:async\s+)?fn\s+\w+", source[:index]))
    if not matches:
        return 0
    return matches[-1].start()


def _statement_end(source: str, start: int) -> int:
    """Return the index just past the `;` that terminates the statement."""
    i = start
    depth = 0
    while i < len(source):
        ch = source[i]
        if ch in "\"'":
            i = _scan_string_and_char(source, i)
            continue
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth = max(0, depth - 1)
        elif ch == ";" and depth == 0:
            return i + 1
        i += 1
    return len(source)


def _only_separators(gap: str) -> bool:
    """True when `gap` holds nothing but whitespace, comments and `;`."""
    stripped = re.sub(r"//[^\n]*", "", gap).replace(";", "")
    return not stripped.strip()


def _construction_region(
    source: str,
    identifier: str,
    registration_index: int,
    function_start: int,
) -> str:
    """Return the source that constructs the tool bound to `identifier`.

    A registration can name its tool directly (`registry.register_boxed(
    match ... { ... })`) or through a binding (`let x = ...; registry.register(x)`).
    For the second form the whole chain of `let x = ...` statements belongs to
    the construction, so the region reaches back across them.

    A registration inside a `for <identifier> in ...` loop names the loop
    variable, not a binding: reaching back from it would drag an unrelated
    tool's construction into the region.
    """
    for_loop = None
    for candidate in FOR_LOOP_RE.finditer(source, function_start, registration_index):
        if candidate.group(1) == identifier:
            for_loop = candidate
    if for_loop is not None:
        shadowed = any(
            candidate.group(1) == identifier
            for candidate in LET_BINDING_RE.finditer(
                source, for_loop.end(), registration_index
            )
        )
        if not shadowed:
            return ""

    bindings = [
        candidate
        for candidate in LET_BINDING_RE.finditer(
            source, function_start, registration_index
        )
        if candidate.group(1) == identifier
    ]
    if not bindings:
        return ""
    region_start = bindings[0].start()
    for previous, following in zip(bindings, bindings[1:]):
        gap = source[_statement_end(source, previous.start()) : following.start()]
        if _only_separators(gap):
            # Chained bindings extend the construction *backwards*; the
            # earliest connected binding wins.
            region_start = min(region_start, previous.start())
    return source[region_start:registration_index]


def _registration_regions(source: str) -> list[tuple[int, str]]:
    """Return `(index, construction_region)` for every registry registration."""
    regions: list[tuple[int, str]] = []
    function_start = 0
    for match in REGISTER_RE.finditer(source):
        if match.start() < function_start:
            function_start = _enclosing_function_start(source, match.start())
        statement = _statement_text(source, match.end() - 1)
        open_paren = match.end() - 1
        _, close = _balanced(source, open_paren)
        argument = source[open_paren + 1 : close - 1].strip()
        region = statement
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", argument):
            # The tool was bound earlier: `let <name> = ...;` then
            # `registry.register(<name>);`. The binding is part of the
            # construction, so the region has to reach back to it.
            region = _construction_region(
                source, argument, match.start(), function_start
            ) or statement
        regions.append((match.start(), region))
    return regions


def scan_registry_fail_closed(sources: dict[str, str]) -> list[Finding]:
    """Every mutating workspace tool must be registered behind `unrooted`."""
    source = sources.get(REGISTRY_FILE, "")
    findings: list[Finding] = []
    for index, region in _registration_regions(source):
        constructors = [
            tool
            for constructor, tool in MUTATING_TOOL_CONSTRUCTORS.items()
            if constructor in region
        ]
        if not constructors:
            continue
        if UNROOTED_MARKER in region:
            continue
        for tool in sorted(constructors):
            findings.append(
                Finding(
                    REGISTRY_FILE,
                    _line_no(source, index),
                    f"mutating tool '{tool}' is registered without the fail-closed "
                    f"unrooted(...) wrapper",
                    "registry-fail-closed",
                )
            )
    return findings


def scan_local_principal_fallback(sources: dict[str, str]) -> list[Finding]:
    """The daemon's local-principal fallback must follow its team state."""
    source = sources.get(DAEMON_CONSTRUCT_FILE, "")
    findings: list[Finding] = []
    for match in re.finditer(r"DaemonWriterFenceFactory::new\s*\(", source):
        open_paren = match.end() - 1
        _, close = _balanced(source, open_paren)
        arguments = source[open_paren + 1 : close - 1]
        if re.search(r"\btrue\b", arguments):
            findings.append(
                Finding(
                    DAEMON_CONSTRUCT_FILE,
                    _line_no(source, match.start()),
                    "local_principal_fallback is asserted as a literal true; it must "
                    "follow the daemon's durable team state",
                    "local-principal-fallback",
                )
            )
    if "DaemonWriterFenceFactory::with_team_state(" not in source:
        findings.append(
            Finding(
                DAEMON_CONSTRUCT_FILE,
                1,
                "the production writer-fence factory is not built from the daemon's "
                "durable team state (DaemonWriterFenceFactory::with_team_state)",
                "local-principal-fallback",
            )
        )
    return findings


def scan_worker_fence_fallback(sources: dict[str, str]) -> list[Finding]:
    """The containment-only fallback must be guarded by a managed-checkout probe."""
    source = sources.get(WORKER_FILE, "")
    findings: list[Finding] = []
    for match in re.finditer(r"WorkspaceWriterFence::new\s*\(", source):
        # The region is the enclosing `let` statement: the fallback is part of
        # it, and so is the probe that has to guard it.
        binding = None
        for candidate in re.finditer(r"\n[ \t]*let\b", source[: match.start()]):
            binding = candidate
        region = source[binding.start() : match.start()] if binding else source[: match.start()]
        if "request_holds_managed_checkout" not in region:
            findings.append(
                Finding(
                    WORKER_FILE,
                    _line_no(source, match.start()),
                    "the containment-only fence is substituted without a managed-checkout "
                    "probe; a run holding a managed checkout must receive a real fence "
                    "or be refused",
                    "worker-fence-fallback",
                )
            )
    if re.search(r"request\s*\.\s*writer_fence\s*\.\s*clone\s*\(\s*\)\s*\.\s*or_else\s*\(", source):
        findings.append(
            Finding(
                WORKER_FILE,
                1,
                "the worker substitutes the containment-only fence unconditionally "
                "(request.writer_fence.clone().or_else(...))",
                "worker-fence-fallback",
            )
        )
    return findings


def scan_nested_dispatch_fence(sources: dict[str, str]) -> list[Finding]:
    """The nested subagent dispatch must carry the parent's fence."""
    source = sources.get(TASK_TOOL_FILE, "")
    findings: list[Finding] = []
    for match in re.finditer(r"SubAgentRequest\s*\{", source):
        open_brace = source.index("{", match.start())
        _, close = _balanced(source, open_brace)
        block = source[open_brace:close]
        if re.search(r"writer_fence\s*:\s*None", block):
            findings.append(
                Finding(
                    TASK_TOOL_FILE,
                    _line_no(source, match.start()),
                    "the nested dispatch drops the parent's writer fence "
                    "(writer_fence: None)",
                    "nested-dispatch-fence",
                )
            )
    return findings


def scan_fail_closed_authority_reads(sources: dict[str, str]) -> list[Finding]:
    """A durable authority read must never be answered with "absent".

    The fence and the containment-only probe each answer one question with a
    durable read: *does this run hold a managed checkout, and is it a shared
    one?* An **unreadable** store is not an absent record. When the read's
    error is discarded — `.ok()`, `.ok().flatten()`, `if let Ok(..)`,
    `unwrap_or(..)` — the error becomes a permissive answer: no fence at all,
    a private checkout, or a run that looks unmanaged. The refusal is the
    only safe direction, so these reads must surface their errors.
    """
    findings: list[Finding] = []

    authority_reads: tuple[tuple[str, str, tuple[str, ...]], ...] = (
        (
            DAEMON_FENCE_FILE,
            "fence_for",
            ("get_run(",),
        ),
        (
            DAEMON_FENCE_FILE,
            "shared_group",
            ("get_run(", "groups_for_member(", ".worktrees", "worktrees.get("),
        ),
        (
            DAEMON_FENCE_FILE,
            "worktree_id",
            ("get_run(",),
        ),
    )
    for rel, function, markers in authority_reads:
        source = sources.get(rel, "")
        body = _function_body(source, function)
        if body is None:
            findings.append(
                Finding(
                    rel,
                    1,
                    f"the `{function}` authority read could not be located, so the "
                    "guard cannot prove it fails closed",
                    "fail-closed-authority-read",
                )
            )
            continue
        start, end = body
        text = source[start:end]
        for statement in _swallowed_read_statements(text, markers):
            findings.append(
                Finding(
                    rel,
                    _line_no(source, start + text.index(statement)),
                    f"the `{function}` authority read discards its error "
                    f"({statement}); an unreadable store is not an absent record, "
                    "and the refusal is the only safe direction",
                    "fail-closed-authority-read",
                )
            )

    # The managed-checkout probe must not read a store error as "this run is
    # not managed": that answers "unmanaged" and downgrades the fence to a
    # containment-only one that can never take the writer permit.
    worker = sources.get(WORKER_FILE, "")
    body = _function_body(worker, "request_holds_managed_checkout")
    if body is None:
        findings.append(
            Finding(
                WORKER_FILE,
                1,
                "the managed-checkout probe could not be located, so the guard "
                "cannot prove it fails closed",
                "fail-closed-authority-read",
            )
        )
    else:
        start, end = body
        text = worker[start:end]
        for arm in _error_arm_bodies(text):
            if _arm_yields_false(arm):
                findings.append(
                    Finding(
                        WORKER_FILE,
                        _line_no(worker, start + text.index(arm)),
                        "the managed-checkout probe answers 'unmanaged' when the "
                        "run store cannot be read; an unreadable store is not an "
                        "absent record, and the refusal is the only safe direction",
                        "fail-closed-authority-read",
                    )
                )
    return findings


def _function_body(source: str, name: str) -> tuple[int, int] | None:
    """The `(start, end)` span of a named function's body.

    Handles free functions and inherent/impl methods alike: the signature is
    located by name, and the body is the brace group that follows it.
    """
    match = re.search(rf"\bfn\s+{re.escape(name)}\s*(?:<[^>]*>)?\s*\(", source)
    if match is None:
        return None
    open_paren = source.index("(", match.start())
    _, close_paren = _balanced(source, open_paren)
    brace = source.find("{", close_paren)
    if brace < 0:
        return None
    _, close = _balanced(source, brace)
    return brace, close


def _swallowed_read_statements(body: str, markers: tuple[str, ...]) -> list[str]:
    """The statements that read durable state and discard its error."""
    offenders: list[str] = []
    seen: set[str] = set()
    for marker in markers:
        for match in re.finditer(re.escape(marker), body):
            start = body.rfind("\n", 0, match.start()) + 1
            end = _statement_end(body, start)
            statement = body[start:end].strip()
            if statement in seen:
                continue
            if not re.search(r"\.ok\s*\(\s*\)", statement) and not re.search(
                r"if\s+let\s+Ok\s*\(", statement
            ) and not re.search(r"unwrap_or(?:_default)?\s*\(", statement):
                continue
            seen.add(statement)
            offenders.append(statement)
    return offenders


def _error_arm_bodies(body: str) -> list[str]:
    """The bodies of the `Err(..) => ..` arms in a match."""
    arms: list[str] = []
    for match in re.finditer(r"\bErr\s*\(", body):
        arrow = body.find("=>", match.end())
        if arrow < 0:
            continue
        brace = body.find("{", arrow)
        if brace < 0 or brace - arrow > 16:
            continue
        _, close = _balanced(body, brace)
        arms.append(body[brace:close])
    return arms


def _arm_yields_false(arm: str) -> bool:
    """True when a match arm's value is the literal `false`.

    The arm's value is the block's trailing expression, so the test is on the
    arm's **tail**, not on the whole arm: an arm that logs and then answers
    `false` yields `false` just as an arm that simply answers `false` does.
    """
    body = re.sub(r"//[^\n]*", "", arm)
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.DOTALL)
    body = body.strip().strip("{}").strip()
    body = body.rstrip(";").strip()
    return bool(re.search(r"\bfalse\b\s*$", body))


def scan_permit_authorization_seam(sources: dict[str, str]) -> list[Finding]:
    """The production permit authority must consult the authorization service.

    M005 corrective C009 (U2). The permit authority decides *which* principal
    may write a managed checkout. Without an `AuthorizationService` it decides
    no policy at all: every writer that reached it held the same blanket
    authority the local owner has, so a team member with no membership in the
    checkout's project wrote it exactly as the owner does. The seam is the
    only thing that narrows that to the writer's own `file.modify` grant.
    """
    source = sources.get(RUNTIME_DEPS_FILE, "")
    findings: list[Finding] = []
    matches = list(re.finditer(r"WorktreePermitAuthority::new\s*\(", source))
    if not matches:
        findings.append(
            Finding(
                RUNTIME_DEPS_FILE,
                1,
                "the production permit authority is not built here; the writer "
                "permit would resolve no policy at all",
                "permit-authorization-seam",
            )
        )
        return findings
    for match in matches:
        # The whole builder chain belongs to one `let` statement, so the
        # statement — not the `new(...)` call — is where a seam would be
        # wired.
        binding = None
        for candidate in re.finditer(r"\n[ \t]*let\b", source[: match.start()]):
            binding = candidate
        if binding is None:
            continue
        end = _statement_end(source, binding.start())
        statement = source[binding.start() : end]
        if ".with_authorization(" not in statement:
            findings.append(
                Finding(
                    RUNTIME_DEPS_FILE,
                    _line_no(source, match.start()),
                    "the production permit authority is built without "
                    "`.with_authorization(...)`; the writer's `file.modify` "
                    "grant for the checkout's project is never checked",
                    "permit-authorization-seam",
                )
            )
    return findings


def scan_delegated_child_attribution(sources: dict[str, str]) -> list[Finding]:
    """A delegated child must carry the dispatching turn's attribution.

    M005 corrective C009 (U1). A delegated child has no daemon-owned turn of
    its own, so it resolves no actor of its own: without a carried
    attribution its write on a shared checkout is refused as anonymous, or —
    worse — attributed to the daemon's synthetic fallback. The dispatch site
    carries the dispatching turn's already-admitted attribution in, the worker
    installs it, and the tool batch reads it.
    """
    findings: list[Finding] = []

    for rel in (EXECUTORS_FILE, TASK_TOOL_FILE):
        source = sources.get(rel, "")
        for match in re.finditer(r"SubAgentRequest\s*\{", source):
            open_brace = source.index("{", match.start())
            _, close = _balanced(source, open_brace)
            block = source[open_brace:close]
            if "execution_audit" not in block:
                findings.append(
                    Finding(
                        rel,
                        _line_no(source, match.start()),
                        "the dispatch carries no attribution at all; the child's "
                        "write resolves no actor",
                        "delegated-child-attribution",
                    )
                )
            elif re.search(r"execution_audit\s*:\s*None", block):
                findings.append(
                    Finding(
                        rel,
                        _line_no(source, match.start()),
                        "the dispatch drops the dispatching turn's attribution "
                        "(execution_audit: None)",
                        "delegated-child-attribution",
                    )
                )

    worker = sources.get(WORKER_FILE, "")
    if "set_inherited_execution_audit(" not in worker:
        findings.append(
            Finding(
                WORKER_FILE,
                1,
                "the worker never installs the carried attribution; the child's "
                "loop resolves no actor",
                "delegated-child-attribution",
            )
        )

    batch = sources.get(TOOL_BATCH_FILE, "")
    if "inherited_execution_audit" not in batch:
        findings.append(
            Finding(
                TOOL_BATCH_FILE,
                1,
                "the tool batch never reads the carried attribution; the child's "
                "write stays unattributed",
                "delegated-child-attribution",
            )
        )
    return findings


CHECKS = (
    ("registry-fail-closed", scan_registry_fail_closed),
    ("local-principal-fallback", scan_local_principal_fallback),
    ("worker-fence-fallback", scan_worker_fence_fallback),
    ("nested-dispatch-fence", scan_nested_dispatch_fence),
    ("fail-closed-authority-read", scan_fail_closed_authority_reads),
    ("permit-authorization-seam", scan_permit_authorization_seam),
    ("delegated-child-attribution", scan_delegated_child_attribution),
)


def _sources(overrides: dict[str, str]) -> dict[str, str]:
    sources: dict[str, str] = {}
    for rel in SCANNED_FILES:
        path = REPO_ROOT / rel
        sources[rel] = path.read_text() if path.is_file() else ""
    sources.update(overrides)
    return sources


def collect_findings() -> list[Finding]:
    sources = _sources({})
    findings: list[Finding] = []
    for _, check in CHECKS:
        findings.extend(check(sources))
    return findings


# ── Self-test ────────────────────────────────────────────────────────
#
# Exercises the exact detectors used above. Each case states what it proves.

def _synthetic_fence_source(fence_body: str, group_body: str) -> str:
    """A synthetic `src/core/writer_fence.rs` carrying both authority reads.

    The guard reports a missing function as "cannot prove it fails closed",
    so a self-test source has to carry **both** of them; otherwise the case
    would be measuring the scaffold rather than the detector.
    """
    return (
        "impl WriterFenceFactory for DaemonWriterFenceFactory {\n"
        "    async fn fence_for(&self, request: WriterScopeRequest) -> Option<Fence> {\n"
        f"{fence_body}"
        "    }\n"
        "}\n"
        "impl DaemonWriterFence {\n"
        "    async fn shared_group(&self) -> Option<AgentRunGroupId> {\n"
        f"{group_body}"
        "    }\n"
        "    async fn worktree_id(&self) -> Option<WorktreeId> {\n"
        "        let run = self.run_id.clone()?;\n"
        "        let record = self\n"
        "            .runs\n"
        "            .get_run(&run)\n"
        "            .await\n"
        "            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
        "            .and_then(|record| record.worktree_id.clone());\n"
        "        record\n"
        "    }\n"
        "}\n"
    )


SELF_TEST_CASES: list[tuple[str, str, dict[str, str], int]] = [
    (
        "a mutating tool registered without the unrooted wrapper is caught",
        "registry-fail-closed",
        {
            REGISTRY_FILE: (
                "fn with_options() {\n"
                "    let commit_tool = crate::tool::commit::CommitTool::new();\n"
                "    registry.register(commit_tool);\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "the same mutating tool behind the unrooted wrapper is not a violation",
        "registry-fail-closed",
        {
            REGISTRY_FILE: (
                "fn with_options() {\n"
                "    let commit_tool = if let Some(root) = workspace_root.as_ref() {\n"
                "        crate::tool::commit::CommitTool::new().with_workdir(root.clone())\n"
                "    } else {\n"
                "        crate::tool::commit::CommitTool::new()\n"
                "    };\n"
                "    let commit_tool: Box<dyn Tool> = match workspace_root.as_ref() {\n"
                "        Some(_) => Box::new(commit_tool),\n"
                "        None => unrooted(Box::new(commit_tool)),\n"
                "    };\n"
                "    registry.register_boxed(commit_tool);\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a read-only workspace tool registered without the wrapper is not a violation",
        "registry-fail-closed",
        {
            REGISTRY_FILE: (
                "fn with_options() {\n"
                "    let read_tool = crate::tool::read::ReadTool::default();\n"
                "    registry.register(read_tool);\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "the inline match form of a mutating registration is accepted",
        "registry-fail-closed",
        {
            REGISTRY_FILE: (
                "fn with_options() {\n"
                "    registry.register_boxed(match workspace_root.as_ref() {\n"
                "        Some(root) => Box::new(crate::tool::write::WriteTool::default()\n"
                "            .with_allowed_root(root.clone())) as Box<dyn Tool>,\n"
                "        None => unrooted(Box::new(crate::tool::write::WriteTool::default())),\n"
                "    });\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a literal true local-principal fallback at the production site is caught",
        "local-principal-fallback",
        {
            DAEMON_CONSTRUCT_FILE: (
                "fn build() {\n"
                "    let factory = DaemonWriterFenceFactory::new(\n"
                "        permit,\n"
                "        worktrees,\n"
                "        runs,\n"
                "        groups,\n"
                "        true,\n"
                "    );\n"
                "}\n"
            )
        },
        2,
    ),
    (
        "a production site that derives the fallback from the team state is accepted",
        "local-principal-fallback",
        {
            DAEMON_CONSTRUCT_FILE: (
                "fn build() {\n"
                "    let factory = DaemonWriterFenceFactory::with_team_state(\n"
                "        permit,\n"
                "        worktrees,\n"
                "        runs,\n"
                "        groups,\n"
                "        team_store,\n"
                "    );\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a production site that asserts the fallback but never consults the team "
        "state is caught",
        "local-principal-fallback",
        {
            DAEMON_CONSTRUCT_FILE: (
                "fn build() {\n"
                "    let factory = DaemonWriterFenceFactory::new(\n"
                "        permit,\n"
                "        worktrees,\n"
                "        runs,\n"
                "        groups,\n"
                "        false,\n"
                "    );\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "the unconditional containment-only substitution is caught",
        "worker-fence-fallback",
        {
            WORKER_FILE: (
                "fn build() {\n"
                "    let writer_fence: Option<SharedWriterFence> =\n"
                "        request.writer_fence.clone().or_else(|| {\n"
                "            Some(std::sync::Arc::new(\n"
                "                crate::tool::writer_fence::WorkspaceWriterFence::new(root.clone()),\n"
                "            ))\n"
                "        });\n"
                "}\n"
            )
        },
        2,
    ),
    (
        "a containment-only fallback guarded by the managed-checkout probe is accepted",
        "worker-fence-fallback",
        {
            WORKER_FILE: (
                "fn build() {\n"
                "    let writer_fence: Option<SharedWriterFence> =\n"
                "        match request.writer_fence.clone() {\n"
                "            Some(fence) => Some(fence),\n"
                "            None => {\n"
                "                if request_holds_managed_checkout(&pool, &request).await {\n"
                "                    return Err(\"managed checkout\".into());\n"
                "                }\n"
                "                Some(std::sync::Arc::new(\n"
                "                    crate::tool::writer_fence::WorkspaceWriterFence::new(root.clone()),\n"
                "                ))\n"
                "            }\n"
                "        };\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a nested dispatch that drops the parent's fence is caught",
        "nested-dispatch-fence",
        {
            TASK_TOOL_FILE: (
                "fn build() {\n"
                "    let req = SubAgentRequest {\n"
                "        task_id,\n"
                "        writer_fence: None,\n"
                "    };\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "a nested dispatch that forwards the parent's fence is accepted",
        "nested-dispatch-fence",
        {
            TASK_TOOL_FILE: (
                "fn build() {\n"
                "    let req = SubAgentRequest {\n"
                "        task_id,\n"
                "        writer_fence: self.inherited_writer_fence(ctx),\n"
                "    };\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "the fence factory discarding its run read is caught",
        "fail-closed-authority-read",
        {
            DAEMON_FENCE_FILE: _synthetic_fence_source(
                "        let worktree_id = match request.agent_run_id.as_ref() {\n"
                "            Some(run) => self\n"
                "                .runs\n"
                "                .get_run(run)\n"
                "                .await\n"
                "                .ok()\n"
                "                .flatten()\n"
                "                .and_then(|record| record.worktree_id),\n"
                "            None => None,\n"
                "        };\n"
                "        let _ = worktree_id;\n"
                "        None\n",
                "        let member_groups = self.groups.as_ref()?.groups_for_member(&run).await?;\n"
                "        let _ = member_groups;\n"
                "        None\n",
            )
        },
        1,
    ),
    (
        "the fence factory surfacing its run read is accepted",
        "fail-closed-authority-read",
        {
            DAEMON_FENCE_FILE: _synthetic_fence_source(
                "        let worktree_id = match request.agent_run_id.as_ref() {\n"
                "            Some(run) => self\n"
                "                .runs\n"
                "                .get_run(run)\n"
                "                .await\n"
                "                .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
                "                .and_then(|record| record.worktree_id),\n"
                "            None => None,\n"
                "        };\n"
                "        let _ = worktree_id;\n"
                "        None\n",
                "        let member_groups = self\n"
                "            .groups\n"
                "            .as_ref()?\n"
                "            .groups_for_member(&run)\n"
                "            .await\n"
                "            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;\n"
                "        let _ = member_groups;\n"
                "        None\n",
            )
        },
        0,
    ),
    (
        "the shared-group derivation discarding its group read is caught",
        "fail-closed-authority-read",
        {
            DAEMON_FENCE_FILE: _synthetic_fence_source(
                "        let worktree_id = match request.agent_run_id.as_ref() {\n"
                "            Some(run) => self\n"
                "                .runs\n"
                "                .get_run(run)\n"
                "                .await\n"
                "                .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
                "                .and_then(|record| record.worktree_id),\n"
                "            None => None,\n"
                "        };\n"
                "        let _ = worktree_id;\n"
                "        None\n",
                "        let member_groups = self.groups.as_ref()?.groups_for_member(&run).await.ok()?;\n"
                "        let _ = member_groups;\n"
                "        None\n",
            )
        },
        1,
    ),
    (
        "the shared-group derivation surfacing its group read is accepted",
        "fail-closed-authority-read",
        {
            DAEMON_FENCE_FILE: _synthetic_fence_source(
                "        let worktree_id = match request.agent_run_id.as_ref() {\n"
                "            Some(run) => self\n"
                "                .runs\n"
                "                .get_run(run)\n"
                "                .await\n"
                "                .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
                "                .and_then(|record| record.worktree_id),\n"
                "            None => None,\n"
                "        };\n"
                "        let _ = worktree_id;\n"
                "        None\n",
                "        let member_groups = self\n"
                "            .groups\n"
                "            .as_ref()?\n"
                "            .groups_for_member(&run)\n"
                "            .await\n"
                "            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;\n"
                "        let _ = member_groups;\n"
                "        None\n",
            )
        },
        0,
    ),
    (
        "the fence's worktree-scope read discarding its error is caught",
        "fail-closed-authority-read",
        {
            DAEMON_FENCE_FILE: _synthetic_fence_source(
                "        let worktree_id = match request.agent_run_id.as_ref() {\n"
                "            Some(run) => self\n"
                "                .runs\n"
                "                .get_run(run)\n"
                "                .await\n"
                "                .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
                "                .and_then(|record| record.worktree_id),\n"
                "            None => None,\n"
                "        };\n"
                "        let _ = worktree_id;\n"
                "        None\n",
                "        let member_groups = self\n"
                "            .groups\n"
                "            .as_ref()?\n"
                "            .groups_for_member(&run)\n"
                "            .await\n"
                "            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;\n"
                "        let _ = member_groups;\n"
                "        None\n",
            ).replace(
                "            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?\n"
                "            .and_then(|record| record.worktree_id.clone());",
                "            .ok()?;\n"
                "        record",
            )
        },
        1,
    ),
    (
        "the managed-checkout probe answering 'unmanaged' on a store error is caught",
        "fail-closed-authority-read",
        {
            WORKER_FILE: (
                "async fn request_holds_managed_checkout(store: Option<Arc<dyn Store>>, request: &Req) -> bool {\n"
                "    let Some(store) = store else {\n"
                "        return false;\n"
                "    };\n"
                "    let Some(run_id) = request.run_id.as_ref().or(request.parent_run_id.as_ref()) else {\n"
                "        return false;\n"
                "    };\n"
                "    match store.get_run(run_id).await {\n"
                "        Ok(Some(run)) => run.worktree_id.is_some(),\n"
                "        Ok(None) => false,\n"
                "        Err(error) => {\n"
                "            tracing::warn!(error = %error, \"probe failed\");\n"
                "            false\n"
                "        }\n"
                "    }\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "the managed-checkout probe refusing on a store error is accepted",
        "fail-closed-authority-read",
        {
            WORKER_FILE: (
                "async fn request_holds_managed_checkout(store: Option<Arc<dyn Store>>, request: &Req) -> bool {\n"
                "    let Some(store) = store else {\n"
                "        return false;\n"
                "    };\n"
                "    let Some(run_id) = request.run_id.as_ref().or(request.parent_run_id.as_ref()) else {\n"
                "        return false;\n"
                "    };\n"
                "    match store.get_run(run_id).await {\n"
                "        Ok(Some(run)) => run.worktree_id.is_some(),\n"
                "        Ok(None) => false,\n"
                "        Err(error) => {\n"
                "            tracing::warn!(error = %error, \"probe failed\");\n"
                "            true\n"
                "        }\n"
                "    }\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a production permit authority built without the authorization seam is caught",
        "permit-authorization-seam",
        {
            RUNTIME_DEPS_FILE: (
                "fn build() {\n"
                "    let permit = WriterPermitService::new(\n"
                "        store,\n"
                "        WorktreePermitAuthority::new(worktrees)\n"
                "            .with_groups(groups),\n"
                "    );\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "a production permit authority wired with the authorization seam is accepted",
        "permit-authorization-seam",
        {
            RUNTIME_DEPS_FILE: (
                "fn build() {\n"
                "    let permit = WriterPermitService::new(\n"
                "        store,\n"
                "        WorktreePermitAuthority::new(worktrees)\n"
                "            .with_groups(groups)\n"
                "            .with_authorization(authz),\n"
                "    );\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a dispatch that drops the dispatching turn's attribution is caught",
        "delegated-child-attribution",
        {
            TASK_TOOL_FILE: (
                "fn build() {\n"
                "    let req = SubAgentRequest {\n"
                "        task_id,\n"
                "        writer_fence: self.inherited_writer_fence(ctx),\n"
                "        execution_audit: None,\n"
                "    };\n"
                "}\n"
            )
        },
        1,
    ),
    (
        "a dispatch that carries the dispatching turn's attribution is accepted",
        "delegated-child-attribution",
        {
            TASK_TOOL_FILE: (
                "fn build() {\n"
                "    let req = SubAgentRequest {\n"
                "        task_id,\n"
                "        writer_fence: self.inherited_writer_fence(ctx),\n"
                "        execution_audit: ctx.and_then(|c| c.execution_audit().cloned()),\n"
                "    };\n"
                "}\n"
            )
        },
        0,
    ),
    (
        "a worker that never installs the carried attribution is caught",
        "delegated-child-attribution",
        {
            WORKER_FILE: (
                "fn build() {\n"
                "    let mut agent_loop = AgentLoop::new();\n"
                "    agent_loop.set_session_id(id);\n"
                "}\n"
            )
        },
        1,
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, rule, overrides, expected in SELF_TEST_CASES:
        check = dict(CHECKS)[rule]
        sources = _sources(overrides)
        found = [f for f in check(sources) if f.rule == rule]
        actual = len(found)
        if actual == expected:
            print(f"  ok   {description}")
        else:
            failures += 1
            print(f"  FAIL {description}: expected {expected}, found {actual}")
            for f in found:
                print(f"         {f.file}:{f.line_no}: {f.line_text}")

    if failures:
        print(f"\nself-test FAILED: {failures} case(s) — the detector does not discriminate.")
        return 2

    print(f"\nself-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return run_self_test()

    findings = collect_findings()

    if findings:
        print(
            "writer-fence completeness violations.\n"
            "A mutating tool must be registered behind the fail-closed `unrooted(...)`\n"
            "wrapper; the daemon's local-principal fallback must follow its durable\n"
            "team state; the containment-only fence must never stand in for a managed\n"
            "checkout; a nested dispatch must carry the parent's fence; the production\n"
            "permit authority must consult the authorization service; and a delegated\n"
            "child must carry the dispatching turn's attribution.\n"
        )
        for f in findings:
            print(f"  [{f.rule}] {f.file}:{f.line_no}: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print("writer-fence completeness check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
