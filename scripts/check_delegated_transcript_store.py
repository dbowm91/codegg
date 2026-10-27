#!/usr/bin/env python3
"""Guard: the delegated transcript store's declared bounds and redaction are live.

WHAT THIS GUARD DOES
    ADR-0051 option B gives a delegated run a durable transcript, keyed by the
    run's `AgentRunId`. The store is only worth what it enforces, and the
    things it enforces are exactly the things that are easy to declare and
    never apply:

    1. **The declared bounds are used.** `MAX_TRANSCRIPT_ROW_BYTES`,
       `MAX_TRANSCRIPT_RUN_BYTES` and `MAX_TRANSCRIPT_READ_LIMIT` are
       promises about size. A constant that is declared and never read
       promises nothing, and the row, the run and the read would all be
       unbounded while the source still *looked* bounded.

    2. **The redaction entry point is reached.** `redact_transcript_body` is
       the single door through which a body reaches the store. A body that
       reaches the store by another door was never redacted, so the secret is
       already durable — which is the exact failure the milestone exists to
       prevent. The door must be reached from the shared preparation helper,
       and that helper must be reached from **both** store implementations: a
       redaction the in-memory store skips is a redaction the tests never see.

    3. **Every declared field is written.** The transcript row's shape is
       declared once, in `AgentRunTranscriptEntry`. A field the production
       INSERT never writes is a field the store reads as a default forever.

    4. **The migration creates the table.** A store whose table the schema
       never creates is a store that fails on first write.

    5. **The store is wired into production.** The capture path must name the
       store trait, so a capture path that stopped writing transcripts cannot
       pass unnoticed.

    6. **The capture path does not change what the events vector holds.** The
       child's events are what the parent's turn transcript is built from: a
       tool call the provider emitted with no matching result is stored as
       `Pending`. A capture path that appended a synthetic
       `ChatEvent::ToolResult` would turn that call into a completed one and
       claim an outcome that never happened — and it would do it to make the
       delegated transcript look complete. The capture path reads the events;
       it never writes to them.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove that the bounds are the *right* bounds, nor that the
    redaction catches every secret. Those are the Rust tests next to the
    store's own proof.

    It reads one module and one migration on purpose: the guard is about the
    store's own declared surface, and a wider scan would make it depend on
    unrelated churn.

USAGE
    python3 scripts/check_delegated_transcript_store.py            # check the tree
    python3 scripts/check_delegated_transcript_store.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


ROOT = Path(__file__).resolve().parent.parent

# The store module and the migration that creates its table.
STORE_MODULE = "crates/codegg-core/src/agent_run_transcript.rs"
SCHEMA_MODULE = "crates/codegg-core/src/session/schema.rs"

# The production capture path.
CAPTURE_MODULE = "src/agent/worker.rs"

# The module that owns the in-execution capture seams. The transcript is only
# useful while the child is still running, so the capture has to happen inside
# the run rather than after it.
LOOP_MODULE = "src/agent/loop.rs"

# The in-execution capture seams, as the call sites that reach them. Each is
# `self.<helper>(`, so the helper's own definition does not satisfy the rule:
# the capture has to be *called* from the loop, not merely declared there.
IN_EXECUTION_CAPTURE_CALLS: Tuple[str, ...] = (
    "self.capture_assistant_message(",
    "self.capture_accepted_tool_calls(",
    "self.capture_tool_result(",
)

# The markers of the post-run batch reconstruction this replaced. A batch
# reconstruction can only ever describe a child that had already finished, so
# its return means the capture is no longer in execution.
POST_RUN_CAPTURE_MARKERS: Tuple[str, ...] = (
    "calls_by_id",
    "captured_call_ids",
)

# The row whose declared shape the store must write.
ROW_STRUCT = "AgentRunTranscriptEntry"

# The table the store writes to.
TABLE = "agent_run_transcript"

# The single redaction door.
REDACTION_ENTRY = "redact_transcript_body"

# The declared bounds. Each must be read somewhere other than its own
# declaration.
BOUNDS: Tuple[str, ...] = (
    "MAX_TRANSCRIPT_ROW_BYTES",
    "MAX_TRANSCRIPT_RUN_BYTES",
    "MAX_TRANSCRIPT_READ_LIMIT",
)

# The two store implementations. Both must reach the redaction door.
STORE_IMPLS: Tuple[str, ...] = (
    "SqliteAgentRunTranscriptStore",
    "InMemoryAgentRunTranscriptStore",
)

# The trait the production capture path must name.
STORE_TRAIT = "AgentRunTranscriptStore"

# The event the capture path must never construct: it is the parent's
# transcript's marker for "this tool call has an outcome".
TOOL_RESULT_EVENT = "ChatEvent::ToolResult"

# The calls that would write into the child's event vector.
EVENTS_MUTATORS: Tuple[str, ...] = (
    "events.push",
    "events.extend",
    "events.insert",
    "events.remove",
    "events.clear",
    "events.retain",
    "events.append",
)

_CONST_RE = re.compile(
    r"(?m)^[ \t]*pub[ \t]+const[ \t]+(MAX_TRANSCRIPT_\w+)[ \t]*:[ \t]*usize"
)
_FIELD_RE = re.compile(r"(?m)^[ \t]*pub[ \t]+(\w+)[ \t]*:")
_STRUCT_RE = re.compile(r"pub[ \t]+struct[ \t]+" + ROW_STRUCT + r"[ \t]*\{")
_FUNCTION_RE = re.compile(
    r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?"
    r"(?:const[ \t]+)?(?:async[ \t]+)?(?:unsafe[ \t]+)?"
    r"(?:extern[ \t]+r?\"[^\"]*\"[ \t]+)?fn[ \t]+(\w+)"
)
_CALL_RE = re.compile(r"\b(\w+)[ \t]*\(")
_MASK_RE = re.compile(
    r"r(?P<hashes>#+)\"(?:[^\"]|\"(?!(?P=hashes)))*\"(?P=hashes)"
    r"|r\"[^\"]*\""
    r"|//[^\n]*"
    r"|/\*.*?\*/"
    r"|\"(?:\\.|[^\"\\\n])*\""
    r"|'(?:\\.|[^\\'\n])'",
    re.DOTALL,
)


@dataclass
class Finding:
    rule: str
    file: str
    line: int
    message: str


@dataclass
class FunctionSpan:
    name: str
    start: int
    end: int


def _mask_non_code(text: str) -> str:
    """Blank out comments and literal contents, preserving every offset."""
    return _MASK_RE.sub(lambda match: re.sub(r"[^\n]", " ", match.group(0)), text)


def _brace_matches(masked: str) -> Dict[int, int]:
    """Map each `{` offset to the offset of the `}` that closes it."""
    matches: Dict[int, int] = {}
    stack: List[int] = []
    for index, char in enumerate(masked):
        if char == "{":
            stack.append(index)
        elif char == "}" and stack:
            matches[stack.pop()] = index
    return matches


def _function_spans(masked: str, braces: Dict[int, int]) -> List[FunctionSpan]:
    """Every function in the file, with the extent of its body."""
    spans: List[FunctionSpan] = []
    for match in _FUNCTION_RE.finditer(masked):
        open_at = masked.find("{", match.end())
        if open_at == -1:
            continue
        end = braces.get(open_at)
        if end is None:
            continue
        spans.append(FunctionSpan(name=match.group(1), start=match.start(), end=end))
    return spans


def _enclosing_function(spans: List[FunctionSpan], offset: int) -> Optional[FunctionSpan]:
    """The innermost function whose body contains `offset`."""
    best: Optional[FunctionSpan] = None
    for span in spans:
        if span.start <= offset <= span.end:
            if best is None or span.start > best.start:
                best = span
    return best


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _balanced_block(text: str, open_at: int) -> Optional[str]:
    """The text between the `(` at `open_at` and the `)` that closes it."""
    depth = 0
    for index in range(open_at, len(text)):
        char = text[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return text[open_at + 1 : index]
    return None


def _declared_fields(masked: str) -> List[str]:
    """The declared field names of the transcript row."""
    match = _STRUCT_RE.search(masked)
    if match is None:
        return []
    open_at = masked.find("{", match.end() - 1)
    if open_at == -1:
        return []
    depth = 0
    end = -1
    for index in range(open_at, len(masked)):
        char = masked[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index
                break
    if end == -1:
        return []
    return _FIELD_RE.findall(masked[open_at:end])


def _insert_columns(text: str) -> List[str]:
    """The column list of the production INSERT into the transcript table."""
    match = re.search(r"INSERT\s+INTO\s+" + TABLE + r"\b", text)
    if match is None:
        return []
    open_at = text.find("(", match.end())
    if open_at == -1:
        return []
    block = _balanced_block(text, open_at)
    if block is None:
        return []
    columns = []
    for part in block.split(","):
        name = part.strip().split()
        if name:
            columns.append(name[0])
    return columns


def _migration_columns(text: str) -> List[str]:
    """The column list the migration declares for the transcript table."""
    match = re.search(
        r"CREATE\s+TABLE\s+IF\s+NOT\s+EXISTS\s+" + TABLE + r"\s*\(", text, re.IGNORECASE
    )
    if match is None:
        return []
    block = _balanced_block(text, match.end() - 1)
    if block is None:
        return []
    columns = []
    for part in block.split(","):
        name = part.strip().split()
        if name:
            columns.append(name[0])
    return columns


def _impl_block(masked: str, type_name: str) -> Optional[str]:
    """The body of `impl AgentRunTranscriptStore for <type_name>`."""
    pattern = re.compile(
        r"impl(?:<[^>]*>)?[ \t]+AgentRunTranscriptStore[ \t]+for[ \t]+" + type_name + r"\b"
    )
    match = pattern.search(masked)
    if match is None:
        return None
    open_at = masked.find("{", match.end())
    if open_at == -1:
        return None
    depth = 0
    for index in range(open_at, len(masked)):
        char = masked[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return masked[open_at : index + 1]
    return None


def _append_body(masked: str, type_name: str) -> Optional[str]:
    """The body of the store implementation's `append` method."""
    block = _impl_block(masked, type_name)
    if block is None:
        return None
    match = re.search(r"(?m)^[ \t]*(?:async[ \t]+)?fn[ \t]+append[ \t]*\(", block)
    if match is None:
        return None
    open_at = block.find("{", match.end())
    if open_at == -1:
        return None
    depth = 0
    for index in range(open_at, len(block)):
        char = block[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return block[open_at : index + 1]
    return None


def check_store(root: Path) -> List[Finding]:
    """Check the delegated transcript store's declared surface."""
    findings: List[Finding] = []
    store_path = root / STORE_MODULE
    if not store_path.is_file():
        return [
            Finding(
                rule="delegated-transcript-store-missing",
                file=STORE_MODULE,
                line=1,
                message="the delegated transcript store module is missing",
            )
        ]
    try:
        text = store_path.read_text(encoding="utf-8")
    except OSError as error:
        return [
            Finding(
                rule="delegated-transcript-store-unreadable",
                file=STORE_MODULE,
                line=1,
                message=f"could not read the store module: {error}",
            )
        ]

    masked = _mask_non_code(text)
    spans = _function_spans(masked, _brace_matches(masked))

    # 1. Every declared bound must be read somewhere other than its own
    #    declaration.
    for match in _CONST_RE.finditer(masked):
        name = match.group(1)
        if name not in BOUNDS:
            continue
        uses = [
            candidate
            for candidate in re.finditer(r"\b" + re.escape(name) + r"\b", masked)
            if not (match.start() <= candidate.start() < match.end())
        ]
        if not uses:
            findings.append(
                Finding(
                    rule="delegated-transcript-bound-unused",
                    file=STORE_MODULE,
                    line=_line_of(text, match.start()),
                    message=(
                        f"{name} is declared but never read: the bound it names is not "
                        "enforced anywhere"
                    ),
                )
            )

    # 2. The redaction door must be reached, and reached from the shared
    #    preparation helper that both store implementations call.
    callers: List[str] = []
    for match in re.finditer(r"\b" + REDACTION_ENTRY + r"[ \t]*\(", masked):
        prefix = masked[max(0, match.start() - 32) : match.start()]
        if re.search(r"\bfn[ \t]+$", prefix):
            continue
        scope = _enclosing_function(spans, match.start())
        if scope is not None:
            callers.append(scope.name)
    if not callers:
        findings.append(
            Finding(
                rule="delegated-transcript-redaction-unreachable",
                file=STORE_MODULE,
                line=1,
                message=(
                    f"{REDACTION_ENTRY} is never called: a body can reach the store "
                    "without being redacted"
                ),
            )
        )
    else:
        for type_name in STORE_IMPLS:
            body = _append_body(masked, type_name)
            if body is None:
                findings.append(
                    Finding(
                        rule="delegated-transcript-impl-missing",
                        file=STORE_MODULE,
                        line=1,
                        message=f"{type_name} has no append method to check",
                    )
                )
                continue
            if not any(f"{caller}(" in body for caller in set(callers)):
                findings.append(
                    Finding(
                        rule="delegated-transcript-redaction-unreachable",
                        file=STORE_MODULE,
                        line=1,
                        message=(
                            f"{type_name}::append never reaches {REDACTION_ENTRY}: this "
                            "store writes bodies that were never redacted"
                        ),
                    )
                )

    # 3. Every declared row field must be written by the production INSERT.
    declared = _declared_fields(masked)
    if not declared:
        findings.append(
            Finding(
                rule="delegated-transcript-row-undeclared",
                file=STORE_MODULE,
                line=1,
                message=f"the transcript row {ROW_STRUCT} declares no readable field",
            )
        )
    written = _insert_columns(text)
    if not written:
        findings.append(
            Finding(
                rule="delegated-transcript-insert-missing",
                file=STORE_MODULE,
                line=1,
                message=f"no production INSERT into {TABLE} was found",
            )
        )
    for field in declared:
        if field not in written:
            findings.append(
                Finding(
                    rule="delegated-transcript-field-unwritten",
                    file=STORE_MODULE,
                    line=1,
                    message=(
                        f"the declared transcript field `{field}` is never written by the "
                        f"production INSERT into {TABLE}"
                    ),
                )
            )

    # 4. The migration must create the table with those columns.
    schema_path = root / SCHEMA_MODULE
    if not schema_path.is_file():
        findings.append(
            Finding(
                rule="delegated-transcript-migration-missing",
                file=SCHEMA_MODULE,
                line=1,
                message="the schema module is missing; the transcript table may not exist",
            )
        )
    else:
        schema_text = schema_path.read_text(encoding="utf-8")
        migrated = _migration_columns(schema_text)
        if not migrated:
            findings.append(
                Finding(
                    rule="delegated-transcript-migration-missing",
                    file=SCHEMA_MODULE,
                    line=1,
                    message=(
                        f"no migration creates {TABLE}: the store would fail on its "
                        "first write"
                    ),
                )
            )
        for field in declared:
            if field not in migrated:
                findings.append(
                    Finding(
                        rule="delegated-transcript-migration-missing",
                        file=SCHEMA_MODULE,
                        line=1,
                        message=(
                            f"the migration does not declare the transcript field "
                            f"`{field}` on {TABLE}"
                        ),
                    )
                )
    return findings


def check_production_use(root: Path) -> List[Finding]:
    """The production capture path must name the store trait."""
    findings: List[Finding] = []
    source_root = root / "src"
    if not source_root.is_dir():
        return [
            Finding(
                rule="delegated-transcript-store-unused",
                file="src",
                line=1,
                message="the source tree is missing; the capture path cannot be verified",
            )
        ]
    for path in sorted(source_root.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        if STORE_TRAIT not in text:
            continue
        masked = _mask_non_code(text)
        if re.search(r"\b" + STORE_TRAIT + r"\b", masked) and re.search(
            r"\.\s*append[ \t]*\(", masked
        ):
            return findings
        findings.append(
            Finding(
                rule="delegated-transcript-store-unused",
                file=str(path.relative_to(root)),
                line=1,
                message=(
                    f"{STORE_TRAIT} is named but never appended to: the delegated "
                    "transcript is never written"
                ),
            )
        )
    if not findings:
        findings.append(
            Finding(
                rule="delegated-transcript-store-unused",
                file="src",
                line=1,
                message=(
                    f"no production file names {STORE_TRAIT}: the delegated transcript "
                    "store is never wired into the capture path"
                ),
            )
        )
    return findings


def check_capture_path(root: Path) -> List[Finding]:
    """The capture path must read the child's events, never write to them.

    The child's events are what the parent's turn transcript is built from: a
    tool call the provider emitted with no matching result is stored as
    `Pending`. A capture path that appended a synthetic `ChatEvent::ToolResult`
    would turn that call into a completed one and claim an outcome that never
    happened.
    """
    findings: List[Finding] = []
    capture_path = root / CAPTURE_MODULE
    if not capture_path.is_file():
        findings.append(
            Finding(
                rule="delegated-transcript-capture-missing",
                file=CAPTURE_MODULE,
                line=1,
                message="the capture path module is missing; the transcript is never written",
            )
        )
        return findings
    text = capture_path.read_text(encoding="utf-8")
    masked = _mask_non_code(text)
    at = masked.find(TOOL_RESULT_EVENT)
    if at != -1:
        findings.append(
            Finding(
                rule="delegated-transcript-tool-result-constructed",
                file=CAPTURE_MODULE,
                line=_line_of(masked, at),
                message=(
                    f"the capture path names {TOOL_RESULT_EVENT}: the delegated transcript "
                    "must carry the tool outcomes the child executed, never a synthetic "
                    "event that would change what the parent's transcript records"
                ),
            )
        )
    for mutator in EVENTS_MUTATORS:
        at = masked.find(mutator)
        if at == -1:
            continue
        findings.append(
            Finding(
                rule="delegated-transcript-events-mutated",
                file=CAPTURE_MODULE,
                line=_line_of(masked, at),
                message=(
                    f"the capture path calls `{mutator}`: the child's event vector is "
                    "what the parent's transcript is built from, so the capture path "
                    "reads it and never writes to it"
                ),
            )
        )
    return findings


def check_in_execution_capture(root: Path) -> List[Finding]:
    """The capture must happen inside the run, not after it.

    A post-run batch can only describe a child that had already finished. The
    whole point of the live transcript is that a parent can read what the
    child has done *so far*, so the seams have to be reached from the loop
    while it runs, and the batch reconstruction must be gone.
    """
    findings: List[Finding] = []
    loop_path = root / LOOP_MODULE
    if not loop_path.is_file():
        findings.append(
            Finding(
                rule="delegated-transcript-capture-not-in-execution",
                file=LOOP_MODULE,
                line=1,
                message=(
                    "the agent loop module is missing; the delegated transcript cannot be "
                    "captured while the child runs"
                ),
            )
        )
        return findings
    loop_masked = _mask_non_code(loop_path.read_text(encoding="utf-8"))
    for call in IN_EXECUTION_CAPTURE_CALLS:
        at = loop_masked.find(call)
        if at == -1:
            findings.append(
                Finding(
                    rule="delegated-transcript-capture-not-in-execution",
                    file=LOOP_MODULE,
                    line=1,
                    message=(
                        f"the agent loop never calls `{call}`: the delegated transcript is "
                        "captured outside the run, so it can only describe a child that had "
                        "already finished"
                    ),
                )
            )

    capture_path = root / CAPTURE_MODULE
    if not capture_path.is_file():
        findings.append(
            Finding(
                rule="delegated-transcript-capture-not-in-execution",
                file=CAPTURE_MODULE,
                line=1,
                message="the capture path module is missing; the transcript is never written",
            )
        )
        return findings
    capture_masked = _mask_non_code(capture_path.read_text(encoding="utf-8"))
    if "set_transcript_capture(" not in capture_masked:
        findings.append(
            Finding(
                rule="delegated-transcript-capture-not-in-execution",
                file=CAPTURE_MODULE,
                line=1,
                message=(
                    "the capture path never installs the transcript capture on the loop: the "
                    "in-execution seams have no sink to reach"
                ),
            )
        )
    for marker in POST_RUN_CAPTURE_MARKERS:
        at = capture_masked.find(marker)
        if at == -1:
            continue
        findings.append(
            Finding(
                rule="delegated-transcript-capture-not-in-execution",
                file=CAPTURE_MODULE,
                line=_line_of(capture_masked, at),
                message=(
                    f"the capture path reconstructs the transcript after the run (`{marker}`): "
                    "a batch reconstruction can only describe a child that had already "
                    "finished, so the capture belongs in the run"
                ),
            )
        )
    return findings


def check_tree(root: Path) -> List[Finding]:
    """Check the whole tree for a live delegated transcript store."""
    findings: List[Finding] = []
    findings.extend(check_store(root))
    findings.extend(check_production_use(root))
    findings.extend(check_capture_path(root))
    findings.extend(check_in_execution_capture(root))
    return findings


def _copy_tree(root: Path, target: Path) -> None:
    """Copy the files this guard reads into a scratch tree."""
    for relative in (STORE_MODULE, SCHEMA_MODULE, CAPTURE_MODULE, LOOP_MODULE):
        source = root / relative
        if not source.is_file():
            continue
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")


def _self_test_cases() -> List[Tuple[str, str, str, str, str]]:
    """The injected defects, each with the rule that must reject it.

    The patterns are regular expressions so that a defect which has to
    remove *every* use of a declared bound can be injected in one step.
    """
    return [
        (
            "the per-row bound is declared but never read",
            r"(?<!pub const )MAX_TRANSCRIPT_ROW_BYTES",
            "8192",
            "delegated-transcript-bound-unused",
            STORE_MODULE,
        ),
        (
            "the redaction door is never called",
            re.escape("let redacted = redact_transcript_body(kind, scan_prefix(body));"),
            "let redacted = scan_prefix(body).to_owned();",
            "delegated-transcript-redaction-unreachable",
            STORE_MODULE,
        ),
        (
            "a declared transcript field is never written",
            re.escape(
                "(run_id, ordinal, role, kind, redacted_body, byte_len, created_at) \\"
            ),
            "(run_id, ordinal, role, kind, redacted_body, created_at) \\",
            "delegated-transcript-field-unwritten",
            STORE_MODULE,
        ),
        (
            "the migration does not create the transcript table",
            re.escape("CREATE TABLE IF NOT EXISTS agent_run_transcript ("),
            "CREATE TABLE IF NOT EXISTS agent_run_transcript_renamed (",
            "delegated-transcript-migration-missing",
            SCHEMA_MODULE,
        ),
        (
            "the capture path writes into the child's event vector",
            re.escape("let events = events?;"),
            "let events = events?;\n    events.push(crate::provider::ChatEvent::ToolCall(",
            "delegated-transcript-events-mutated",
            CAPTURE_MODULE,
        ),
        (
            "the capture path constructs a synthetic tool result",
            re.escape("let events = events?;"),
            "let events = events?;\n    let _synthetic = crate::provider::ChatEvent::ToolResult {",
            "delegated-transcript-tool-result-constructed",
            CAPTURE_MODULE,
        ),
        (
            "the loop never records the model's assistant message",
            re.escape("self.capture_assistant_message(processor.text()).await;"),
            "let _assistant_text = processor.text();",
            "delegated-transcript-capture-not-in-execution",
            LOOP_MODULE,
        ),
        (
            "the loop never records the tool calls it accepted",
            re.escape("self.capture_accepted_tool_calls(&tool_calls).await;"),
            "let _accepted_tool_calls = &tool_calls;",
            "delegated-transcript-capture-not-in-execution",
            LOOP_MODULE,
        ),
        (
            "the loop never records a delivered tool result",
            re.escape("self.capture_tool_result("),
            "let _delivered_tool_result = (",
            "delegated-transcript-capture-not-in-execution",
            LOOP_MODULE,
        ),
        (
            "the capture path reinstalls a post-run batch reconstruction",
            re.escape("let report = serde_json::from_str::<SubAgentReport>(&output).ok();"),
            "let calls_by_id = HashMap::new();\n    let captured_call_ids = HashSet::new();\n"
            "    let report = serde_json::from_str::<SubAgentReport>(&output).ok();",
            "delegated-transcript-capture-not-in-execution",
            CAPTURE_MODULE,
        ),
    ]


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: List[str] = []
    cases = 0

    with tempfile.TemporaryDirectory() as scratch:
        scratch_root = Path(scratch)

        baseline_root = scratch_root / "baseline"
        _copy_tree(ROOT, baseline_root)
        baseline = check_tree(baseline_root)
        if baseline:
            failures.append(
                "the guard reports findings on the unmodified store, so its checks are not "
                f"measuring what they claim: {baseline}"
            )
        else:
            print("  ok   the unmodified delegated transcript store produced no finding")

        for description, pattern, replacement, expected_rule, module in _self_test_cases():
            cases += 1
            work = scratch_root / re.sub(r"[^a-z0-9]+", "_", description.lower()).strip("_")
            _copy_tree(ROOT, work)
            target = work / module
            text = target.read_text(encoding="utf-8")
            # A callable replacement is literal: the injected text contains
            # backslashes that a replacement template would read as escapes.
            injected, count = re.subn(pattern, lambda _match: replacement, text)
            if count == 0:
                failures.append(f"{description}: fixture could not be injected: target not found")
                continue
            target.write_text(injected, encoding="utf-8")

            reported = check_tree(work)
            if not reported:
                failures.append(f"{description}: the guard did not reject the injected defect")
                continue
            if not any(finding.rule == expected_rule for finding in reported):
                failures.append(
                    f"{description}: the guard rejected the injected defect, but not with the "
                    f"check that owns it (no finding had rule {expected_rule!r}): {reported}"
                )
                continue
            print(f"  ok   {description} (rejected by {expected_rule})")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"delegated-transcript-store self-test passed: {cases} injected defects were each "
        "rejected by the guard, and the unmodified store produced no finding"
    )
    return 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects the drift it claims to catch",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    findings = check_tree(ROOT)
    if findings:
        print("delegated-transcript-store: FAILED", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding.file}:{finding.line}: {finding.message}", file=sys.stderr)
        return 1

    print(
        "delegated-transcript-store check ok: the declared bounds and the redaction "
        "entry point are live, and every declared field is written"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
