#!/usr/bin/env python3
"""The TUI render path derives each expensive result once.

A cost milestone is only worth anything if the cost cannot come back. This
guard pins the shape of the render path that C006 established:

* the composer has **one** producer of its wrapped rows, and that producer
  sits behind the layout cache, so a frame wraps the buffer once and a frame
  that changed nothing wraps it not at all;
* the transcript has **one** producer of its Markdown, so a body of Markdown
  is never parsed by two code paths that could drift apart;
* the render path does not clone the whole composer buffer to answer a
  question about its first byte;
* the frame cadence, the spinner and the render's ``catch_unwind`` are the
  ones the architecture document makes contracts.

The guard is a *shape* check, not a benchmark: it looks at the source, so a
regression that re-introduces a second producer is caught before it ships.
The cost itself is measured by the render counters in
``src/tui/render_cost.rs`` and asserted by the tests.

Usage::

    python3 scripts/check_tui_render_cost.py            # check the tree
    python3 scripts/check_tui_render_cost.py --self-test # prove the check bites
"""

from __future__ import annotations

import argparse
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

PROMPT_RS = "src/tui/components/prompt.rs"
MESSAGES_RS = "src/tui/components/messages.rs"
RENDER_RS = "src/tui/app/render.rs"
EVENT_LOOP_RS = "src/tui/runtime/event_loop.rs"

# The composer's wrap primitive. One call site means one producer of the
# rows, and one producer means one answer to "what rows does this text
# make".
COMPOSER_WRAP = "wrap_styled_spans_by_word_rows"
# The Markdown parser. One call site means one producer of the blocks.
MARKDOWN_PARSE = "parse_document"
# The whole-buffer clone the render path used to make.
COMPOSER_CLONE = "get_text()"
# The catch_unwind around the render, and the streaming frame interval.
CATCH_UNWIND = "std::panic::catch_unwind"
STREAM_INTERVAL = "const STREAM_RENDER_INTERVAL: Duration = Duration::from_millis(16);"


@dataclass(frozen=True)
class Violation:
    """One thing the guard found."""

    check: str
    path: str
    detail: str

    def render(self) -> str:
        return f"{self.check}: {self.path}: {self.detail}"


def _read(root: Path, relative: str) -> str:
    return (root / relative).read_text(encoding="utf-8")


def _call_sites(text: str, needle: str) -> list[int]:
    """The 1-based lines that carry `needle` outside a comment or an import."""
    sites: list[int] = []
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("//") or stripped.startswith("use "):
            continue
        if needle in line:
            sites.append(number)
    return sites


def check_composer_has_one_wrap_producer(root: Path) -> list[Violation]:
    """The composer wraps its buffer in exactly one place."""
    text = _read(root, PROMPT_RS)
    sites = _call_sites(text, COMPOSER_WRAP)
    if len(sites) != 1:
        return [
            Violation(
                "composer-wrap-producer",
                PROMPT_RS,
                f"expected exactly one call to {COMPOSER_WRAP}, found {len(sites)} "
                f"on lines {sites}",
            )
        ]
    return []


def check_composer_layout_is_cached(root: Path) -> list[Violation]:
    """The composer's layout is cached, and the cache is consulted."""
    text = _read(root, PROMPT_RS)
    violations: list[Violation] = []
    if "layout_cache" not in text:
        violations.append(
            Violation(
                "composer-layout-cache",
                PROMPT_RS,
                "the composer has no layout cache: its wrapped layout would be "
                "rebuilt by every consumer that wants it",
            )
        )
    if "fn layout_at" not in text:
        violations.append(
            Violation(
                "composer-layout-cache",
                PROMPT_RS,
                "the composer has no layout_at: there is no single producer of "
                "the wrapped layout",
            )
        )
    return violations


def check_markdown_has_one_parse_producer(root: Path) -> list[Violation]:
    """A body of Markdown is parsed in exactly one place."""
    text = _read(root, MESSAGES_RS)
    sites = _call_sites(text, MARKDOWN_PARSE)
    if len(sites) != 1:
        return [
            Violation(
                "markdown-parse-producer",
                MESSAGES_RS,
                f"expected exactly one call to {MARKDOWN_PARSE}, found {len(sites)} "
                f"on lines {sites}",
            )
        ]
    return []


def check_streaming_tail_is_cached(root: Path) -> list[Violation]:
    """The streaming tail's Markdown is parsed through its cache."""
    text = _read(root, MESSAGES_RS)
    violations: list[Violation] = []
    if "StreamingTailCache" not in text:
        violations.append(
            Violation(
                "streaming-tail-cache",
                MESSAGES_RS,
                "the streaming tail has no cache: the whole response would be "
                "re-parsed as Markdown on every token",
            )
        )
    return violations


def check_render_path_does_not_clone_the_composer(root: Path) -> list[Violation]:
    """The render path does not copy the whole composer buffer."""
    text = _read(root, RENDER_RS)
    sites = _call_sites(text, COMPOSER_CLONE)
    if sites:
        return [
            Violation(
                "composer-whole-buffer-clone",
                RENDER_RS,
                f"the render path clones the whole composer buffer on line(s) "
                f"{sites}",
            )
        ]
    return []


def check_render_path_does_not_clone_the_task_list(root: Path) -> list[Violation]:
    """The render path does not copy the whole task list before truncating."""
    text = _read(root, RENDER_RS)
    sites = _call_sites(text, "all_rows.clone()")
    if sites:
        return [
            Violation(
                "tasks-whole-list-clone",
                RENDER_RS,
                f"the render path clones the whole task list on line(s) {sites} "
                f"before the overlay truncates it to a dozen rows",
            )
        ]
    return []


def check_frame_cadence_is_unchanged(root: Path) -> list[Violation]:
    """The render keeps its `catch_unwind` and its streaming interval."""
    text = _read(root, EVENT_LOOP_RS)
    violations: list[Violation] = []
    if CATCH_UNWIND not in text:
        violations.append(
            Violation(
                "frame-cadence",
                EVENT_LOOP_RS,
                "the render is no longer wrapped in a catch_unwind",
            )
        )
    if STREAM_INTERVAL not in text:
        violations.append(
            Violation(
                "frame-cadence",
                EVENT_LOOP_RS,
                "the streaming render interval is no longer 16ms",
            )
        )
    return violations


CHECKS = (
    check_composer_has_one_wrap_producer,
    check_composer_layout_is_cached,
    check_markdown_has_one_parse_producer,
    check_streaming_tail_is_cached,
    check_render_path_does_not_clone_the_composer,
    check_render_path_does_not_clone_the_task_list,
    check_frame_cadence_is_unchanged,
)


def run_checks(root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for check in CHECKS:
        violations.extend(check(root))
    return violations


# ── Self-test ────────────────────────────────────────────────────────────
#
# Each case injects one violation into a copy of the tree and proves the
# matching check reports it. A guard that cannot fail is not a guard.

INJECTIONS = (
    (
        "composer-wrap-producer",
        "a second wrap producer in the composer",
        PROMPT_RS,
        None,
        "    fn wrapped_rows(&self, width: u16) -> Vec<WrappedRow> {\n"
        "        let extra = wrap_styled_spans_by_word_rows(&[], width);\n"
        "        extra\n"
        "    }\n",
    ),
    (
        "composer-layout-cache",
        "a composer with no layout cache",
        PROMPT_RS,
        "layout_cache",
        "wrapped_rows_cache",
    ),
    (
        "markdown-parse-producer",
        "a second Markdown parse producer",
        MESSAGES_RS,
        None,
        "fn another_parse(text: &str) -> usize {\n"
        "    let arena = Arena::new();\n"
        "    parse_document(&arena, text, &MARKDOWN_OPTIONS).children().count()\n"
        "}\n",
    ),
    (
        "streaming-tail-cache",
        "a streaming tail with no cache",
        MESSAGES_RS,
        "StreamingTailCache",
        "TailRenderState",
    ),
    (
        "composer-whole-buffer-clone",
        "a render path that clones the whole composer buffer",
        RENDER_RS,
        None,
        "        let prompt_text = self.prompt_state.prompt.get_text();\n",
    ),
    (
        "tasks-whole-list-clone",
        "a render path that clones the whole task list",
        RENDER_RS,
        "let rows = bounded_task_rows(&all_rows);",
        "let rows = bounded_task_rows(all_rows.clone());",
    ),
    (
        "frame-cadence",
        "a render without its catch_unwind",
        EVENT_LOOP_RS,
        "std::panic::catch_unwind",
        "std::panic::resume_unwind",
    ),
)


def _copy_tree(destination: Path) -> None:
    """Copy the tree under test, without the build output.

    The build directory is the largest thing in a checkout and none of the
    checks read it, so copying it would make the self-test slow enough that
    people stop running it.
    """
    shutil.copytree(
        REPO_ROOT,
        destination,
        symlinks=True,
        ignore=shutil.ignore_patterns("target", ".git", "node_modules"),
    )


def _self_test() -> int:
    failures: list[str] = []
    for expected_check, description, relative, find, replacement in INJECTIONS:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "tree"
            _copy_tree(root)
            target = root / relative
            original = target.read_text(encoding="utf-8")
            if find is None:
                mutated = original + "\n" + replacement
            else:
                if find not in original:
                    failures.append(
                        f"{description}: the injection target {find!r} is not in {relative}"
                    )
                    continue
                mutated = original.replace(find, replacement)
            target.write_text(mutated, encoding="utf-8")
            violations = run_checks(root)
            matching = [v for v in violations if v.check == expected_check]
            if not matching:
                failures.append(
                    f"{description}: injecting it produced no {expected_check} violation"
                )
            else:
                print(f"  rejected: {description}")
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1
    print(f"self-test: {len(INJECTIONS)} injections, all rejected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="inject each violation into a copy of the tree and prove it is caught",
    )
    arguments = parser.parse_args()
    if arguments.self_test:
        return _self_test()
    violations = run_checks(REPO_ROOT)
    if violations:
        for violation in violations:
            print(violation.render(), file=sys.stderr)
        return 1
    print("check_tui_render_cost: the render path derives each result once")
    return 0


if __name__ == "__main__":
    sys.exit(main())
