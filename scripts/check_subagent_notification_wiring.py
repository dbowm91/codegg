#!/usr/bin/env python3
"""Guard: a delegated job must subscribe for its own durable terminal notice.

WHAT THIS GUARD DOES
    A delegated run (a subagent spawned by an orchestrator turn) has no
    durable record of its terminal state unless the job is *subscribed* for a
    durable terminal notice. The `task` tool's durable spawn path is the only
    place that can create that subscription, so this guard asserts, for every
    delegated-job submission path in `src/tool/task.rs`:

    1. **The subscription exists.** The submitting function must reach a
       `subscribe_with_options(` call — directly, or through a local helper
       it calls (the "one admission door" shape).

    2. **The disposition is `NotifyOnly`.** `ResumeOwner` turns the notice
       into a continuation, and `Suppress` drops it: neither is a notice, and
       `ResumeOwner` claims an authority the parent did not ask for.

    3. **The kind token is `"subagent"`.** A notice whose kind is anything
       else does not get the delegated notice format, so a reader of the
       notice cannot tell which child it is about.

WHAT THIS GUARD DELIBERATELY DOES NOT DO
    It does not prove that the subscription is *useful* — that the notice is
    recorded, injected and read. Those are the Rust tests next to it.

    It does not check the *order* of the subscription against the submit.
    The order is not a static property: the fresh-spawn path subscribes after
    its submit, while the retry-after-a-restart path subscribes a job an
    earlier invocation already submitted, before it returns the run's
    status. Both are correct, and a rule that demanded "subscribe after the
    submit" would reject the second. The restart test covers the order.

    It scans `src/tool/task.rs` only. The legacy in-process delegated path
    (`src/agent/worker.rs`) has no durable notice and is out of scope.

USAGE
    python3 scripts/check_subagent_notification_wiring.py            # check the tree
    python3 scripts/check_subagent_notification_wiring.py --self-test  # prove it bites
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

# The file that owns the delegated-job submission.
TASK_TOOL = Path("src") / "tool" / "task.rs"

# The durable notifier that owns the subscription API.
NOTIFICATIONS = Path("src") / "scheduler" / "job_notifications.rs"

# The subscription call this guard looks for.
SUBSCRIBE_CALL = "subscribe_with_options"

# The kind token a delegated notice carries.
DELEGATED_KIND = "subagent"

# The disposition a delegated subscription must request.
REQUIRED_DISPOSITION = "NotifyOnly"

# The dispositions that are not a notice.
FORBIDDEN_DISPOSITIONS = ("ResumeOwner", "Suppress")

# The delegated job kind — a submission of this kind is a delegated run.
DELEGATED_JOB_KIND = "JobKind::Subagent"

_FUNCTION_RE = re.compile(
    r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?"
    r"(?:const[ \t]+)?(?:async[ \t]+)?(?:unsafe[ \t]+)?"
    r"(?:extern[ \t]+r?\"[^\"]*\"[ \t]+)?fn[ \t]+(\w+)"
)
_CALL_RE = re.compile(r"\b(\w+)[ \t]*\(")
_METHOD_CALL_RE = re.compile(r"\.\s*(\w+)[ \t]*\(")
_SUBMIT_CALL_RE = re.compile(r"\.\s*submit[ \t]*\(")
_CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
_CONST_DECL_RE = re.compile(
    r"(?:pub(?:\([^)]*\))?[ \t]+)?const[ \t]+(\w+)[ \t]*:[ \t]*&str[ \t]*=[ \t]*"
)

# The regions that are not code: comments, string literals (including raw
# ones), and character literals. A lifetime is not a character literal —
# `<'a, 'b>` has no closing quote after the `a` — so the character-literal
# form only matches a quoted single character.
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


@dataclass
class ArgSpan:
    start: int
    end: int


def _mask_non_code(text: str) -> str:
    """Blank out comments and literal contents, preserving every offset."""
    return _MASK_RE.sub(lambda match: re.sub(r"[^\n]", " ", match.group(0)), text)


def _mask_test_modules(masked: str, braces: Dict[int, int]) -> str:
    """Blank out `#[cfg(test)]` module bodies.

    A unit test in the same file may build a delegated job to assert on it;
    that is not a production submission path and must not be held to the
    production wiring.
    """
    chars = list(masked)
    for match in _CFG_TEST_RE.finditer(masked):
        open_at = masked.find("{", match.end())
        if open_at == -1:
            continue
        end = braces.get(open_at)
        if end is None:
            continue
        for index in range(match.start(), end + 1):
            if chars[index] != "\n":
                chars[index] = " "
    return "".join(chars)


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


def _call_closing_paren(masked: str, open_at: int) -> Optional[int]:
    """The offset of the `)` that closes the call whose `(` is at `open_at`."""
    depth = 0
    for index in range(open_at, len(masked)):
        char = masked[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index
    return None


def _split_args(masked: str, open_at: int, close_at: int) -> List[ArgSpan]:
    """Split a call's arguments at the top-level commas."""
    args: List[ArgSpan] = []
    depth = 0
    start = open_at + 1
    for index in range(open_at + 1, close_at):
        char = masked[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            args.append(ArgSpan(start=start, end=index))
            start = index + 1
    tail = masked[start:close_at]
    if tail.strip():
        args.append(ArgSpan(start=start, end=close_at))
    return args


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _resolve_string_const(root: Path, name: str) -> Optional[str]:
    """The literal value of a `&str` constant, when the tree defines one.

    The declaration is found in the masked text (so a name that only appears
    inside a comment or a string is not mistaken for a constant), and the
    literal is read from the unmasked text at the same offset.
    """
    source_root = root / "src"
    if not source_root.is_dir():
        source_root = root
    for path in sorted(source_root.rglob("*.rs")):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        masked = _mask_non_code(text)
        for match in _CONST_DECL_RE.finditer(masked):
            if match.group(1) != name:
                continue
            literal = re.search(r"\"([^\"]*)\"", text[match.start() : match.end()])
            if literal is not None:
                return literal.group(1)
    return None


def _kind_is_delegated(root: Path, argument: str) -> bool:
    """True when the kind argument names the delegated kind.

    A literal is compared directly. An identifier is resolved through the
    tree's `&str` constants, so a named constant is accepted only when it
    really is the delegated token; an identifier the tree cannot resolve is
    not accepted.
    """
    candidate = argument.strip()
    literal = re.fullmatch(r"\"([^\"]*)\"", candidate)
    if literal is not None:
        return literal.group(1) == DELEGATED_KIND
    identifier = candidate.rsplit("::", 1)[-1].strip()
    if not re.fullmatch(r"\w+", identifier):
        return False
    resolved = _resolve_string_const(root, identifier)
    return resolved == DELEGATED_KIND


def _disposition_is_notify_only(argument: str) -> bool:
    """True when the disposition argument requests a notice and nothing else."""
    candidate = argument.strip()
    if any(name in candidate for name in FORBIDDEN_DISPOSITIONS):
        return False
    return REQUIRED_DISPOSITION in candidate


def _reachable_subscriptions(
    masked: str,
    spans: List[FunctionSpan],
    scope: FunctionSpan,
) -> List[Tuple[int, str, List[ArgSpan]]]:
    """Every subscription reachable from `scope`, with its anchor offset.

    The anchor is the offset inside `scope` that reaches the subscription: the
    subscription call itself when it is direct, and the local helper's call
    site when it is reached through a helper.
    """
    local_bodies: Dict[str, Tuple[int, str]] = {
        span.name: (span.start, masked[span.start : span.end + 1]) for span in spans
    }
    scope_text = masked[scope.start : scope.end + 1]
    reachable: List[Tuple[int, str, List[ArgSpan]]] = []

    for match in re.finditer(
        r"\b" + SUBSCRIBE_CALL + r"[ \t]*\(", masked[scope.start : scope.end + 1]
    ):
        open_at = scope.start + match.end() - 1
        close_at = _call_closing_paren(masked, open_at)
        if close_at is None:
            continue
        args = _split_args(masked, open_at, close_at)
        reachable.append((open_at, SUBSCRIBE_CALL, args))

    for match in _METHOD_CALL_RE.finditer(scope_text):
        name = match.group(1)
        found = local_bodies.get(name)
        if found is None or name == SUBSCRIBE_CALL:
            continue
        body_start, body = found
        for helper_match in re.finditer(
            r"\b" + SUBSCRIBE_CALL + r"[ \t]*\(", body
        ):
            helper_open = body_start + helper_match.end() - 1
            helper_close = _call_closing_paren(masked, helper_open)
            if helper_close is None:
                continue
            args = _split_args(masked, helper_open, helper_close)
            reachable.append(
                (scope.start + match.start(), f"{name}() -> {SUBSCRIBE_CALL}()", args)
            )
    return reachable


def check_file(path: Path, root: Path) -> List[Finding]:
    """Check the delegated submission path in one Rust file."""
    relative = str(path.relative_to(root))
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        return [
            Finding(
                rule="subagent-notification-unreadable",
                file=relative,
                line=1,
                message=f"could not read the file: {error}",
            )
        ]

    masked = _mask_non_code(text)
    braces = _brace_matches(masked)
    masked = _mask_test_modules(masked, braces)
    spans = _function_spans(masked, braces)
    findings: List[Finding] = []

    for scope in spans:
        scope_text = masked[scope.start : scope.end + 1]
        if DELEGATED_JOB_KIND not in scope_text:
            continue
        submits = [match.start() for match in _SUBMIT_CALL_RE.finditer(scope_text)]
        if not submits:
            continue
        submit_at = scope.start + submits[0]

        reachable = _reachable_subscriptions(masked, spans, scope)
        if not reachable:
            findings.append(
                Finding(
                    rule="subagent-notification-not-subscribed",
                    file=relative,
                    line=_line_of(text, submit_at),
                    message=(
                        f"{scope.name}() submits a delegated job without subscribing it for a "
                        f"durable terminal notice ({SUBSCRIBE_CALL}() is not reached from it)"
                    ),
                )
            )
            continue

        for anchor, label, args in reachable:
            line = _line_of(text, anchor)
            if len(args) < 6:
                findings.append(
                    Finding(
                        rule="subagent-notification-not-subscribed",
                        file=relative,
                        line=line,
                        message=(
                            f"{scope.name}() reaches {label}() with too few arguments to state a "
                            "kind and a disposition"
                        ),
                    )
                )
                continue
            if not _kind_is_delegated(root, masked[args[1].start : args[1].end]):
                findings.append(
                    Finding(
                        rule="subagent-notification-kind",
                        file=relative,
                        line=line,
                        message=(
                            f"{scope.name}() reaches {label}() with kind "
                            f"{masked[args[1].start : args[1].end].strip()!r}, which is not the "
                            f"delegated token {DELEGATED_KIND!r}"
                        ),
                    )
                )
            if not _disposition_is_notify_only(masked[args[5].start : args[5].end]):
                findings.append(
                    Finding(
                        rule="subagent-notification-disposition",
                        file=relative,
                        line=line,
                        message=(
                            f"{scope.name}() reaches {label}() with disposition "
                            f"{masked[args[5].start : args[5].end].strip()!r}, which is not "
                            f"{REQUIRED_DISPOSITION}"
                        ),
                    )
                )
    return findings


def check_tree(root: Path) -> List[Finding]:
    """Check the delegated submission path in the tree."""
    path = root / TASK_TOOL
    if not path.is_file():
        return [
            Finding(
                rule="subagent-notification-source-missing",
                file=str(TASK_TOOL),
                line=1,
                message="the task tool source is missing; the delegated wiring cannot be verified",
            )
        ]
    return check_file(path, root)


def _copy_tree(root: Path, target: Path) -> None:
    """Copy the files this guard reads into a scratch tree."""
    for relative in (TASK_TOOL, NOTIFICATIONS):
        source = root / relative
        if not source.is_file():
            continue
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")


# The synthetic tree the self-test works on. It is deliberately minimal: a
# delegated submission, a local helper that subscribes, and the constant the
# kind resolves through. The self-test must not depend on the tree it ships
# with, or it would prove nothing about the guard's own rules.
CLEAN_TASK_TOOL = """\
use codegg_core::jobs::{JobId, JobKind};
use codegg_core::workspace::WorkspaceId;

impl TaskTool {
    async fn execute_impl(&self) -> Result<(), ToolError> {
        let submitted = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    kind: JobKind::Subagent,
                    workspace_id: workspace_id.clone(),
                    session_id: self.parent_session_id.clone(),
                    turn_id: self.parent_turn_id.clone(),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
                },
            )
            .await?;
        self.subscribe_delegated_notice(
            &submitted.job_id,
            &delegation_key,
            &workspace_id,
            ctx,
        )
        .await?;
        Ok(())
    }

    async fn subscribe_delegated_notice(
        &self,
        job_id: &JobId,
        invocation_key: &str,
        workspace_id: &WorkspaceId,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<(), ToolError> {
        use crate::scheduler::job_notifications::{
            JobCompletionDisposition, DELEGATED_JOB_KIND,
        };
        service
            .subscribe_with_options(
                job_id,
                DELEGATED_JOB_KIND,
                session_id,
                turn_id.as_deref(),
                invocation_key,
                JobCompletionDisposition::NotifyOnly,
                owner_principal.as_deref(),
                Some(workspace_id.as_str()),
                None,
            )
            .await?;
        Ok(())
    }
}
"""

CLEAN_NOTIFICATIONS = """\
pub const DELEGATED_JOB_KIND: &str = "subagent";
"""


def _write_clean_tree(target: Path) -> None:
    """Write the synthetic clean tree the self-test starts from."""
    for relative, body in ((TASK_TOOL, CLEAN_TASK_TOOL), (NOTIFICATIONS, CLEAN_NOTIFICATIONS)):
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(body, encoding="utf-8")


def _first_subscription(root: Path) -> Optional[Tuple[Path, int, List[ArgSpan], str]]:
    """The first subscription call in the tree, with its argument spans."""
    path = root / TASK_TOOL
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8")
    masked = _mask_non_code(text)
    braces = _brace_matches(masked)
    masked = _mask_test_modules(masked, braces)
    spans = _function_spans(masked, braces)
    for scope in spans:
        scope_text = masked[scope.start : scope.end + 1]
        if DELEGATED_JOB_KIND not in scope_text:
            continue
        if not _SUBMIT_CALL_RE.search(scope_text):
            continue
        for anchor, label, args in _reachable_subscriptions(masked, spans, scope):
            return path, anchor, args, label
    return None


def _self_test_cases() -> List[Tuple[str, str, str]]:
    """The injected defects, each with the rule that must reject it."""
    return [
        (
            "the delegated spawn never subscribes for its terminal notice",
            "not-subscribed",
            "subagent-notification-not-subscribed",
        ),
        (
            "the delegated subscription asks for a continuation instead of a notice",
            "disposition",
            "subagent-notification-disposition",
        ),
        (
            "the delegated subscription carries a foreign kind token",
            "kind",
            "subagent-notification-kind",
        ),
    ]


def _inject(root: Path, kind: str) -> Optional[str]:
    """Inject one defect into the scratch tree; return what was changed."""
    found = _first_subscription(root)
    if found is None:
        return None
    path, anchor, args, _label = found
    text = path.read_text(encoding="utf-8")
    if kind == "not-subscribed":
        if not args:
            return None
        # The call name precedes its first argument; the reachable anchor may
        # be the helper's call site rather than the subscription itself.
        name_at = text.rfind(SUBSCRIBE_CALL, 0, args[0].start)
        if name_at == -1:
            return None
        return text[:name_at] + "skip_subscription" + text[name_at + len(SUBSCRIBE_CALL) :]
    if kind == "disposition":
        if len(args) < 6:
            return None
        return (
            text[: args[5].start]
            + "JobCompletionDisposition::ResumeOwner"
            + text[args[5].end :]
        )
    if kind == "kind":
        if len(args) < 2:
            return None
        return text[: args[1].start] + "\"background\"" + text[args[1].end :]
    return None


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: List[str] = []
    cases = 0

    with tempfile.TemporaryDirectory() as scratch:
        scratch_root = Path(scratch)

        # The clean synthetic tree must produce no finding.
        clean_root = scratch_root / "clean"
        _write_clean_tree(clean_root)
        clean = check_tree(clean_root)
        if clean:
            failures.append(
                "the guard reports findings on a clean delegated submission, so its checks are "
                f"not measuring what they claim: {clean}"
            )
        else:
            print("  ok   a clean delegated submission produces no finding")

        for description, kind, expected_rule in _self_test_cases():
            cases += 1
            work = scratch_root / re.sub(r"[^a-z0-9]+", "_", description.lower()).strip("_")
            _write_clean_tree(work)
            injected = _inject(work, kind)
            if injected is None:
                failures.append(f"{description}: the defect could not be injected")
                continue
            target = work / TASK_TOOL
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
        f"subagent-notification-wiring self-test passed: {cases} injected defects were each "
        "rejected by the guard, and a clean delegated submission produced no finding"
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
        print("subagent-notification-wiring: FAILED", file=sys.stderr)
        for finding in findings:
            print(f"  - {finding.file}:{finding.line}: {finding.message}", file=sys.stderr)
        return 1

    print(
        "subagent-notification-wiring check ok: every delegated-job submission path "
        "subscribes for a durable terminal notice"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
