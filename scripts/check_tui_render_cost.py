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
* the frame buffer is retained only while a text selection is live, so a
  frame nobody can drag over does not copy itself;
* the sidebar projection is populated outside the per-placement panel loop,
  so two open panels do not pay for two populations;
* the frame cadence, the spinner and the render's ``catch_unwind`` are the
  ones the architecture document makes contracts;
* the file tree's row vocabulary has **one** producer: the pane and the
  ``/files`` box read the cached slice, and neither re-flattens or filters the
  listing on the render path (M002);
* the chat row model has **one** producer: both chat surfaces read the
  browser's cached rows, neither flattens the window or wraps a body itself,
  and the cache's key still names every input the model depends on (M003);
* the change review **borrows** its parsed hunks: its render path assigns no
  hunk list, so a frame cannot copy the whole patch (M003);
* the streaming tail's fast path — the branch that says the rows are already
  rendered — **serves the rows it assembled**, so a rebuild that did not extend
  the tail copies nothing (M004);
* a tool call's body, and the payload parse that builds it, is built **only
  under an `expanded` test**, so the disclosure model's collapsed default is the
  cheap case (M004);
* an editor row is read as far as its **column window**, not as far as its
  line, and the character walk stops at the window's end (M005);
* the viewport's horizontal clamp is **memoized** on the document revision, the
  first visible line, the visible line count and the drawn width, so an
  unchanged frame measures no row twice (M005);
* the editor's semantic-token scan is a **partition point plus the row's run**,
  not a filter over every token in the document for every row (M005);
* a segment's syntax style is resolved by a **sweep** over the ascending runs,
  not a `find` from index 0 for every segment (M005);
* the highlight cache's scan key is **compared** against the request before it
  is built, so a hit allocates nothing (M005);
* the highlight cache's prefix check **starts at the edit** the presentation
  placed, and falls back to line 0 when nothing placed it (M005);
* the styled wrap's span lookup **advances one cursor** over the source's span
  list rather than rescanning it from the front for every word, and the
  per-word split that allocated a `Vec` is gone (M006);
* the escape parser and `ESC` are **unchanged**: a control sequence is still
  one atomic token in `tokenize`, and the OSC arm still consumes the whole
  sequence, so the OSC-8 hyperlink the wrap keeps whole cannot be re-derived
  by a cost change (M006).

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
APP_MOD_RS = "src/tui/app/mod.rs"
EVENT_LOOP_RS = "src/tui/runtime/event_loop.rs"
FILE_TREE_STATE_RS = "src/tui/app/state/file_tree.rs"
FILE_TREE_PANE_RS = "src/tui/components/file_tree.rs"
FILES_DIALOG_RS = "src/tui/components/dialogs/files.rs"
CHAT_BROWSER_RS = "src/tui/chat_browser.rs"
CHAT_PANEL_RS = "src/tui/panels/chat.rs"
CHAT_DIALOG_RS = "src/tui/components/dialogs/chat.rs"
CHANGE_REVIEW_RS = "src/tui/components/change_review.rs"
DIFF_RS = "src/tui/components/diff.rs"
EDITOR_WIDGET_RS = "src/tui/components/editor.rs"
EDITOR_MODEL_RS = "src/tui/editor.rs"
EDITOR_HIGHLIGHT_RS = "src/tui/editor_highlight.rs"
WRAP_RS = "src/tui/wrap.rs"
# The styled wrap's span lookup. One cursor over the span list is what makes
# the lookup `O(spans + words)`; a lookup that rescans from the front for every
# word is `O(words x spans)`, and `split_at_spans` is the shape that did it.
SPAN_CURSOR_STRUCT = "struct SpanCursor {"
SPAN_CURSOR_ADVANCE = "self.next += 1;"
SPAN_CURSOR_SEEK = "span_cursor.seek(&source.spans"
SPAN_CURSOR_STYLE = "span_cursor.style_at(&source.spans"
SPAN_FLAT_SCAN = "for &(span_start, span_end, style) in spans"
SPAN_SPLIT = "split_at_spans("
# The escape parser and `ESC`, which a cost change may not touch: `ESC` is the
# constant the composer's visual-row geometry shares, the OSC arm is what keeps
# an OSC-8 hyperlink's target out of the measurement, and `tokenize` is where a
# sequence becomes one atomic token.
ESCAPE_CONSTANT = "pub(crate) const ESC: u8 = 0x1b;"
OSC_ARM = "b']' => scan_to_terminator(bytes, 2),"
ESCAPE_TOKEN = "advance_past_escape(text, idx)"
TEST_GATE = "#[cfg(test)]"

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
# The unconditional whole-frame clone `finish_render` used to make.
UNCONDITIONAL_FRAME_CLONE = "frame.buffer_mut().clone()"
# The per-placement panel loop the sidebar population must stay out of.
PANEL_LOOP = "for (placement, rect) in regions.occupied() {"
# The allocating identity key the tasks overlay's subagent lookup used.
TASK_LOOKUP_KEY = 'format!("task:{task_id}")'
# The file tree's two row producers. The cache is the only one, so the render
# path may name neither: `filter_rows` narrows a row list and `visible_rows`
# flattens the tree, and either on the render path is a per-frame rebuild.
FILTER_ROWS = "filter_rows("
FLATTEN_ROWS = "visible_rows("
# The chat row model's producers. The cache calls both; a surface that names
# either is rebuilding the model beside it, and the second producer is what
# M003 exists to remove.
CHAT_BUILD_ROWS = "build_rows("
CHAT_WRAP = "wrap_to_strings("
# The one accessor both chat surfaces read. It borrows, so a hit allocates
# nothing; `selected_row_end` is folded into the cached model for the same
# reason.
CHAT_CACHED_ROWS = "browser.rows("
CHAT_SELECTED_SCAN = "selected_row_end("
# The review's borrowed render path, and the assignment that would copy the
# whole parsed patch again.
REVIEW_BORROWED = "render_hunks_in("
REVIEW_HUNK_ASSIGNMENT = ".hunks ="
# The streaming tail's fast path. The branch says the rows are already
# rendered, so it must serve the assembly the cache holds — not assemble the
# blocks again, which deep-copied every row of the whole response on every
# rebuild (M004).
TAIL_CACHE_IMPL = "impl StreamingTailCache {"
TAIL_CACHE_STRUCT = "struct StreamingTailCache {"
TAIL_FAST_PATH = "if self.epoch == epoch && self.text_len == text.len() && !self.blocks.is_empty() {"
TAIL_ASSEMBLED_FIELD = "assembled: Vec<Line<'static>>,"
TAIL_ASSEMBLED_ACCESS = "&self.assembled"
ASSEMBLER = "assemble_markdown_blocks("
# A tool call's displayed body. The disclosure model makes collapsed the
# default, so the body — and the payload parse that builds it — belongs behind
# an `expanded` test; building it unconditionally paid for a JSON parse and a
# tree of formatted rows that a collapsed call drops (M004).
TOOL_BODY_BUILD = "let output_body: Option<Vec<String>> ="
OUTLINE_BUILD = "render_structured_output_outline("
EXPANDED_TEST = "*expanded"
# The editor row's bounded read and walk. The row's bytes come from the window
# helper; a `read_bytes(start, end)` in `line_spans` is the whole line, which is
# what M005 removed. The walk's early return is the other half: without it the
# row is drawn from a window and walked to the line's end (M005).
ROW_WINDOW_READ = "row_window("
WHOLE_ROW_READ = "read_bytes(start, end)"
WINDOW_EARLY_EXIT = "if started && display >= limit {"
# The horizontal clamp's memo. Without the key the view clamps against another
# document's width; without the memo every frame measures every visible row
# before drawing it (M005).
CLAMP_CACHE_STRUCT = "struct ClampCache {"
CLAMP_KEY_FIELDS = (
    "self.revision == revision",
    "self.first_line == first_line",
    "self.visible_lines == visible_lines",
    "self.width == width",
)
CLAMP_CONSULTED = "cached.covers("
# The semantic-token scan. The vector is sorted by line, so a partition point
# finds the row's run; a filter over the whole vector is every token in the
# document, once per visible row (M005).
TOKEN_PARTITION = "semantic_tokens.partition_point("
TOKEN_LINEAR_FILTER = "filter(|token| token.line"
# The per-segment style lookup. Both runs and the segments ascend, so one
# cursor per run is a single sweep; a `find` from index 0 is
# `segments x ranges` on a dense line (M005).
STYLE_SWEEP = "covering_style("
STYLE_LINEAR_SCAN = ".find(|(token_range"
# The scan key is compared before it is built, so a hit allocates nothing. The
# key is re-derived once per visible row, and two `String`s per row for an
# identity that did not change is the cost this removes (M005).
KEY_COMPARED = "key.path == path && key.code_theme == code_theme"
# The prefix check's seed. `rebase` must consume the line an edit placed; a
# literal 0 puts the whole scan back on every keystroke (M005).
PREFIX_SEED_TAKEN = "self.pending_edit_from.take()"


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


def _production_source(text: str) -> str:
    """The shipped source: everything before the module's test gate."""
    cut = text.find(TEST_GATE)
    return text if cut < 0 else text[:cut]


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


def _brace_block(text: str, anchor: str) -> str | None:
    """The brace-balanced block that starts at `anchor`, or None."""
    start = text.find(anchor)
    if start < 0:
        return None
    open_index = text.find("{", start)
    if open_index < 0:
        return None
    depth = 0
    for index in range(open_index, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[open_index : index + 1]
    return None


def _function_block(text: str, name: str) -> str | None:
    """The brace-balanced body of ``fn name(``, or of ``fn name<...>(``."""
    for anchor in (f"fn {name}(", f"fn {name}<"):
        block = _brace_block(text, anchor)
        if block is not None:
            return block
    return None


def _statement_block(text: str, start: int) -> str | None:
    """The `let` statement at `start`: from there to its own terminating `;`.

    An `if`/`else` initializer is two sibling brace blocks, so brace-matching
    from the condition's `{` stops at the end of the `then` branch. The
    statement ends at the first `;` seen outside every brace, which is where the
    `let` ends — the `;` of a call inside a branch is one level in, and is
    skipped.
    """
    depth = 0
    for index in range(start, len(text)):
        character = text[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
        elif character == ";" and depth == 0:
            return text[start : index + 1]
    return None


def check_frame_buffer_is_retained_only_for_a_selection(root: Path) -> list[Violation]:
    """The frame's terminal buffer is copied only while a selection is live."""
    text = _read(root, RENDER_RS)
    sites = _call_sites(text, UNCONDITIONAL_FRAME_CLONE)
    if sites:
        return [
            Violation(
                "frame-buffer-retention",
                RENDER_RS,
                f"the frame retains its whole terminal buffer on line(s) {sites} "
                f"with no selection to resolve — every frame copies width x height "
                f"cells for a reader that returns early",
            )
        ]
    return []


def check_sidebar_population_is_not_in_the_panel_loop(root: Path) -> list[Violation]:
    """`populate_session_panel` is not called once per occupied panel."""
    text = _read(root, RENDER_RS)
    block = _brace_block(text, PANEL_LOOP)
    if block is None:
        return [
            Violation(
                "sidebar-population-per-panel",
                RENDER_RS,
                "the per-placement panel loop cannot be found, so this check "
                "cannot verify that the population stays out of it",
            )
        ]
    if "populate_session_panel" in block:
        return [
            Violation(
                "sidebar-population-per-panel",
                RENDER_RS,
                "the sidebar population runs inside the per-placement panel "
                "loop: two open panels pay for two populations, and a panel "
                "that is not the Session panel still pays",
            )
        ]
    return []


def check_task_lookup_does_not_allocate_a_key(root: Path) -> list[Violation]:
    """`App::task_rows` does not build an identity key per comparison."""
    text = _read(root, APP_MOD_RS)
    sites = _call_sites(text, TASK_LOOKUP_KEY)
    if sites:
        return [
            Violation(
                "task-lookup-key",
                APP_MOD_RS,
                f"the tasks overlay's subagent lookup builds {TASK_LOOKUP_KEY} "
                f"on line(s) {sites}: one String allocation per candidate "
                f"compared, inside a per-frame row build",
            )
        ]
    return []



def check_file_tree_rows_are_cached(root: Path) -> list[Violation]:
    """The file tree's rows are served from a cache, by borrow."""
    text = _read(root, FILE_TREE_STATE_RS)
    violations: list[Violation] = []
    if "struct RowCache" not in text:
        violations.append(
            Violation(
                "file-tree-row-cache",
                FILE_TREE_STATE_RS,
                "the file tree has no row cache: every consumer would flatten "
                "and filter the whole listing itself",
            )
        )
    for accessor in ("pub fn rows(&self) -> &[TreeRow]", "pub fn all_rows(&self) -> &[TreeRow]"):
        if accessor not in text:
            violations.append(
                Violation(
                    "file-tree-row-cache",
                    FILE_TREE_STATE_RS,
                    f"the row accessor {accessor!r} is gone: a clone (or a "
                    f"rebuild) per read is what the cache exists to remove",
                )
            )
    return violations


def check_file_tree_row_cache_key(root: Path) -> list[Violation]:
    """The row cache is keyed on the revision **and** the filter."""
    text = _read(root, FILE_TREE_STATE_RS)
    body = _function_block(text, "rebuild_rows_if_stale")
    if body is None:
        return [
            Violation(
                "file-tree-row-cache-key",
                FILE_TREE_STATE_RS,
                "the row cache's rebuild step cannot be found, so this check "
                "cannot verify what invalidates the cache",
            )
        ]
    violations: list[Violation] = []
    for half in (
        "self.row_cache.revision == self.revision",
        "self.row_cache.filter == self.filter",
    ):
        if half not in body:
            violations.append(
                Violation(
                    "file-tree-row-cache-key",
                    FILE_TREE_STATE_RS,
                    f"the row cache is not keyed on {half!r}: dropping either "
                    f"half serves the previous listing to the next read, which "
                    f"displays a path that is gone",
                )
            )
    return violations


def check_file_tree_render_path_reads_the_cache(root: Path) -> list[Violation]:
    """Neither file-tree surface flattens or filters the listing while drawing."""
    violations: list[Violation] = []
    pane = _read(root, FILE_TREE_PANE_RS)
    for needle, what in ((FILTER_ROWS, "filters"), (FLATTEN_ROWS, "flattens")):
        sites = _call_sites(pane, needle)
        if sites:
            violations.append(
                Violation(
                    "file-tree-render-path-cache",
                    FILE_TREE_PANE_RS,
                    f"the Ctrl+T pane {what} the listing on the render path, on "
                    f"line(s) {sites}: it must read the authority's cached slice",
                )
            )
    dialog = _read(root, FILES_DIALOG_RS)
    sites = _call_sites(dialog, FILTER_ROWS)
    body = _function_block(dialog, "refresh_rows")
    if body is None:
        violations.append(
            Violation(
                "file-tree-render-path-cache",
                FILES_DIALOG_RS,
                "the box has no refresh_rows: there is no single producer of "
                "its filtered rows",
            )
        )
    elif sites:
        inside = _call_sites(body, FILTER_ROWS)
        if len(inside) != len(sites):
            violations.append(
                Violation(
                    "file-tree-render-path-cache",
                    FILES_DIALOG_RS,
                    f"the box filters the listing outside its cache rebuild: "
                    f"{len(sites)} call(s), {len(inside)} inside refresh_rows",
                )
            )
    return violations


def check_chat_rows_are_cached(root: Path) -> list[Violation]:
    """The chat row model is served from a cache, by borrow."""
    text = _read(root, CHAT_BROWSER_RS)
    violations: list[Violation] = []
    if "struct ChatRowCache" not in text:
        violations.append(
            Violation(
                "chat-row-cache",
                CHAT_BROWSER_RS,
                "the chat has no row cache: every surface would wrap the whole "
                "retained channel on every draw",
            )
        )
    if "pub struct ChatRowModel" not in text:
        violations.append(
            Violation(
                "chat-row-cache",
                CHAT_BROWSER_RS,
                "the cached row accessor no longer hands back a model: a "
                "`Vec<ChatRow>` return is a rebuild (or a clone) per read",
            )
        )
    if "pub fn rows<'a>(" not in text:
        violations.append(
            Violation(
                "chat-row-cache",
                CHAT_BROWSER_RS,
                "the one producer of chat rows `ChatBrowser::rows` is gone",
            )
        )
    return violations


def check_chat_row_cache_key(root: Path) -> list[Violation]:
    """The row cache's key names every input the row model depends on."""
    text = _read(root, CHAT_BROWSER_RS)
    body = _function_block(text, "covers")
    if body is None:
        return [
            Violation(
                "chat-row-cache-key",
                CHAT_BROWSER_RS,
                "the row cache's key comparison cannot be found, so this check "
                "cannot verify what invalidates the cache",
            )
        ]
    violations: list[Violation] = []
    for half, why in (
        (
            "self.sequence == request.sequence",
            "a redaction or an edit applies to the messages and never reaches "
            "the screen",
        ),
        (
            "self.unread == view.unread",
            "the unread divider stays where the old read marker put it",
        ),
        (
            "self.channel_id.as_deref() == view.active_channel_id",
            "one channel's rows are drawn under another channel's name",
        ),
        ("self.width == request.width", "a resize serves last width's wrapping"),
        ("self.cursor == cursor", "the selection marker stays on the old message"),
        (
            "self.theme.as_ref() == request.theme.as_ref()",
            "the rows keep the previous palette",
        ),
    ):
        if half not in body:
            violations.append(
                Violation(
                    "chat-row-cache-key",
                    CHAT_BROWSER_RS,
                    f"the row cache is not keyed on {half!r}: {why}",
                )
            )
    return violations


def check_chat_render_path_reads_the_cache(root: Path) -> list[Violation]:
    """Neither chat surface builds or wraps rows while drawing."""
    violations: list[Violation] = []
    for relative, surface in (
        (CHAT_PANEL_RS, "the docked chat panel"),
        (CHAT_DIALOG_RS, "the /chat box"),
    ):
        text = _read(root, relative)
        for needle, what in (
            (CHAT_BUILD_ROWS, "flattens the message window"),
            (CHAT_WRAP, "wraps a message body"),
        ):
            sites = _call_sites(text, needle)
            if sites:
                violations.append(
                    Violation(
                        "chat-render-path-cache",
                        relative,
                        f"{surface} {what} on the render path, on line(s) "
                        f"{sites}: it must read the browser's cached rows",
                    )
                )
        if CHAT_CACHED_ROWS not in text:
            violations.append(
                Violation(
                    "chat-render-path-cache",
                    relative,
                    f"{surface} does not read the cached row model through "
                    f"{CHAT_CACHED_ROWS!r}, so it has its own producer",
                )
            )
        scans = _call_sites(text, CHAT_SELECTED_SCAN)
        if scans:
            violations.append(
                Violation(
                    "chat-render-path-cache",
                    relative,
                    f"{surface} reverse-scans the row list for the viewport "
                    f"anchor on line(s) {scans}: the cached model carries it",
                )
            )
    return violations


def check_review_hunks_are_borrowed(root: Path) -> list[Violation]:
    """The change review's render path assigns no hunk list."""
    text = _read(root, CHANGE_REVIEW_RS)
    body = _function_block(text, "render")
    violations: list[Violation] = []
    if body is None:
        return [
            Violation(
                "review-borrowed-hunks",
                CHANGE_REVIEW_RS,
                "the review's render cannot be found, so this check cannot "
                "verify that it borrows its hunks",
            )
        ]
    assignments = _call_sites(body, REVIEW_HUNK_ASSIGNMENT)
    if assignments:
        violations.append(
            Violation(
                "review-borrowed-hunks",
                CHANGE_REVIEW_RS,
                f"the review assigns a hunk list on render line(s) "
                f"{assignments}: that is a whole-patch copy per frame, bounded "
                f"only by MAX_REVIEW_PATCH_BYTES",
            )
        )
    if REVIEW_BORROWED not in body:
        violations.append(
            Violation(
                "review-borrowed-hunks",
                CHANGE_REVIEW_RS,
                "the review does not render through the borrowed path "
                "`DiffViewer::render_hunks_in`",
            )
        )
    if "pub fn render_hunks_in(" not in _read(root, DIFF_RS):
        violations.append(
            Violation(
                "review-borrowed-hunks",
                DIFF_RS,
                "`DiffViewer::render_hunks_in` is gone: there is no borrowed "
                "render path for a diff the caller already owns",
            )
        )
    return violations


# The authority's row inputs. A write to any of these outside the authority's
# own module bypasses `bump`, so the row cache describes the previous listing —
# a stale path, which is the one defect M002 exists to prevent.
ROW_INPUT_FIELDS = ("nodes", "expanded", "filter")
# The in-place mutators a caller could reach for. The authority's own verbs
# (`filter_push`, `filter_pop`, `expand_and_select`, `collapse`) carry no dot,
# so they never match.
ROW_INPUT_MUTATORS = ("insert(", "remove(", "clear(", "extend(", "retain(", "push(", "pop(")


def _row_input_writes(text: str, receiver: str) -> list[tuple[int, str]]:
    """The lines that *write* a row input through `receiver`."""
    writes: list[tuple[int, str]] = []
    for number, line in enumerate(text.splitlines(), start=1):
        if line.strip().startswith("//"):
            continue
        for field in ROW_INPUT_FIELDS:
            head = f"{receiver}{field}"
            start = line.find(head)
            while start >= 0:
                rest = line[start + len(head) :]
                if rest.startswith(" =") or any(
                    rest.startswith("." + name) for name in ROW_INPUT_MUTATORS
                ):
                    writes.append((number, field))
                    break
                start = line.find(head, start + len(head))
    return writes


def _rust_sources(root: Path, *subtrees: str) -> list[Path]:
    """Every Rust source under the given subtrees, `.inc` members included."""
    files: list[Path] = []
    for subtree in subtrees:
        for path in (root / subtree).rglob("*"):
            if path.is_file() and path.suffix in (".rs", ".inc"):
                files.append(path)
    return sorted(files)


def check_file_tree_row_inputs_are_written_through_verbs(root: Path) -> list[Violation]:
    """`FileTreeState`'s row inputs are written only by its own module."""
    violations: list[Violation] = []
    for path in _rust_sources(root, "src/tui", "tests/tui_render"):
        relative = path.relative_to(root).as_posix()
        if relative == FILE_TREE_STATE_RS:
            continue
        for number, field in _row_input_writes(
            path.read_text(encoding="utf-8"), "file_tree_state."
        ):
            violations.append(
                Violation(
                    "file-tree-row-input-write",
                    relative,
                    f"writes `file_tree_state.{field}` directly on line {number}: "
                    f"the authority rebuilds its row cache inside `bump`, so a "
                    f"write that bypasses a verb serves the previous listing to "
                    f"the next read — which displays a path that is gone",
                )
            )
    return violations


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


def check_streaming_tail_fast_path_serves_the_cached_rows(root: Path) -> list[Violation]:
    """The tail's unchanged fast path serves the rows it already assembled."""
    text = _read(root, MESSAGES_RS)
    violations: list[Violation] = []
    cache = _brace_block(text, TAIL_CACHE_IMPL)
    if cache is None:
        return [
            Violation(
                "tail-fast-path-assembly",
                MESSAGES_RS,
                "the streaming tail's cache cannot be found, so this check cannot "
                "verify that its fast path serves the rows it assembled",
            )
        ]
    cache_struct = _brace_block(text, TAIL_CACHE_STRUCT)
    if cache_struct is None or TAIL_ASSEMBLED_FIELD not in cache_struct:
        violations.append(
            Violation(
                "tail-fast-path-assembly",
                MESSAGES_RS,
                "the tail's cache does not hold its assembled rows: the fast "
                "path has nothing to serve and re-assembles every block's every "
                "row, which is a deep copy of the whole response per rebuild",
            )
        )
    fast_path = _brace_block(cache, TAIL_FAST_PATH)
    if fast_path is None:
        violations.append(
            Violation(
                "tail-fast-path-assembly",
                MESSAGES_RS,
                "the fast path — the tail's text did not move — cannot be found, "
                "so this check cannot verify what it returns",
            )
        )
        return violations
    if ASSEMBLER in fast_path:
        violations.append(
            Violation(
                "tail-fast-path-assembly",
                MESSAGES_RS,
                "the fast path assembles the blocks again: it says the rows are "
                "already rendered and then re-renders them, so every rebuild that "
                "did not extend the tail pays a full row copy",
            )
        )
    if TAIL_ASSEMBLED_ACCESS not in fast_path:
        violations.append(
            Violation(
                "tail-fast-path-assembly",
                MESSAGES_RS,
                f"the fast path does not hand back {TAIL_ASSEMBLED_ACCESS!r}: the "
                f"cached assembly is the only value that costs no copy",
            )
        )
    return violations


def check_tool_output_body_is_built_under_the_expanded_gate(root: Path) -> list[Violation]:
    """A tool call's body — and its payload parse — is built only when expanded."""
    text = _read(root, MESSAGES_RS)
    parts = _function_block(text, "build_assistant_parts_lines")
    if parts is None:
        return [
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                "the assistant's part builder cannot be found, so this check "
                "cannot verify when a tool call's body is built",
            )
        ]
    start = parts.find(TOOL_BODY_BUILD)
    if start < 0:
        return [
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                f"the tool call's body build {TOOL_BODY_BUILD!r} cannot be found: "
                f"there is no one place to check that it is gated",
            )
        ]
    build = _statement_block(parts, start)
    if build is None:
        return [
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                "the body build's statement cannot be read, so this check cannot "
                "verify that the call is expanded before the payload is parsed",
            )
        ]
    violations: list[Violation] = []
    outline_at = build.find(OUTLINE_BUILD)
    expanded_at = build.find(EXPANDED_TEST)
    if outline_at < 0:
        violations.append(
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                f"the body build no longer builds the structured outline "
                f"({OUTLINE_BUILD}): this check is pinned to the parse it must keep "
                f"gated",
            )
        )
    if expanded_at < 0:
        violations.append(
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                "the tool call's body is built whether or not the call is "
                "expanded: the default (collapsed) case pays for a payload parse "
                "and a tree of formatted rows that it never draws",
            )
        )
    elif outline_at >= 0 and expanded_at > outline_at:
        violations.append(
            Violation(
                "tool-body-expanded-gate",
                MESSAGES_RS,
                "the body build tests `expanded` only after it has built the "
                "outline: the gate has to come first",
            )
        )
    return violations


def check_editor_row_read_is_windowed(root: Path) -> list[Violation]:
    """An editor row is read and walked only as far as its column window."""
    text = _read(root, EDITOR_WIDGET_RS)
    violations: list[Violation] = []
    row = _function_block(text, "line_spans")
    if row is None:
        return [
            Violation(
                "editor-row-window",
                EDITOR_WIDGET_RS,
                "the editor's row builder cannot be found, so this check cannot "
                "verify what it reads",
            )
        ]
    if ROW_WINDOW_READ not in row:
        violations.append(
            Violation(
                "editor-row-window",
                EDITOR_WIDGET_RS,
                "the row does not read through the window helper: a row of a "
                "minified line pays the line's bytes for the columns it draws",
            )
        )
    if WHOLE_ROW_READ in row:
        violations.append(
            Violation(
                "editor-row-window",
                EDITOR_WIDGET_RS,
                f"the row reads the whole line ({WHOLE_ROW_READ!r}) before "
                f"deciding which columns to draw",
            )
        )
    window = _function_block(text, "visible_window")
    if window is None:
        violations.append(
            Violation(
                "editor-row-window",
                EDITOR_WIDGET_RS,
                "the window walk cannot be found, so this check cannot verify "
                "that it stops at the window's end",
            )
        )
    elif WINDOW_EARLY_EXIT not in window:
        violations.append(
            Violation(
                "editor-row-window",
                EDITOR_WIDGET_RS,
                "the character walk has no early exit: the row is drawn from a "
                "window and then decoded to the end of its line anyway",
            )
        )
    return violations


def check_editor_clamp_is_cached(root: Path) -> list[Violation]:
    """The viewport's horizontal clamp is memoized on what it depends on."""
    text = _read(root, EDITOR_MODEL_RS)
    violations: list[Violation] = []
    if CLAMP_CACHE_STRUCT not in text:
        return [
            Violation(
                "editor-clamp-cache",
                EDITOR_MODEL_RS,
                "the clamp has no memo: every frame measures every visible row "
                "immediately before the draw reads the same rows",
            )
        ]
    body = _function_block(text, "covers")
    if body is None:
        return [
            Violation(
                "editor-clamp-cache",
                EDITOR_MODEL_RS,
                "the memo's key comparison cannot be found, so this check "
                "cannot verify what invalidates it",
            )
        ]
    for field in CLAMP_KEY_FIELDS:
        if field not in body:
            violations.append(
                Violation(
                    "editor-clamp-cache",
                    EDITOR_MODEL_RS,
                    f"the memo is not keyed on {field!r}: the horizontal clamp "
                    f"is then answered for a document or a window it does not "
                    f"describe, and the view scrolls past content or cannot "
                    f"reach its end",
                )
            )
    viewport = _function_block(text, "resolve_viewport")
    if viewport is None or CLAMP_CONSULTED not in viewport:
        violations.append(
            Violation(
                "editor-clamp-cache",
                EDITOR_MODEL_RS,
                "`resolve_viewport` does not consult the memo, so the "
                "measurement runs on every frame regardless",
            )
        )
    return violations


def check_editor_token_scan_is_bounded(root: Path) -> list[Violation]:
    """A row's semantic-token scan visits the row's tokens, not all of them."""
    text = _read(root, EDITOR_WIDGET_RS)
    row = _function_block(text, "line_spans")
    if row is None:
        return [
            Violation(
                "editor-token-scan",
                EDITOR_WIDGET_RS,
                "the editor's row builder cannot be found, so this check cannot "
                "verify how it finds the row's tokens",
            )
        ]
    violations: list[Violation] = []
    if TOKEN_PARTITION not in row:
        violations.append(
            Violation(
                "editor-token-scan",
                EDITOR_WIDGET_RS,
                "the row does not locate its tokens with a partition point: a "
                "sorted vector searched linearly costs every token in the "
                "document for every visible row",
            )
        )
    if TOKEN_LINEAR_FILTER in row:
        violations.append(
            Violation(
                "editor-token-scan",
                EDITOR_WIDGET_RS,
                f"the row filters the whole token vector ({TOKEN_LINEAR_FILTER!r}) "
                f"instead of partitioning it",
            )
        )
    return violations


def check_editor_style_lookup_is_a_sweep(root: Path) -> list[Violation]:
    """A row's per-segment style lookup is one sweep, not a `find` per segment."""
    text = _read(root, EDITOR_WIDGET_RS)
    row = _function_block(text, "line_spans")
    if row is None:
        return [
            Violation(
                "editor-style-sweep",
                EDITOR_WIDGET_RS,
                "the editor's row builder cannot be found, so this check cannot "
                "verify how a segment's style is found",
            )
        ]
    violations: list[Violation] = []
    if STYLE_SWEEP not in row:
        violations.append(
            Violation(
                "editor-style-sweep",
                EDITOR_WIDGET_RS,
                "the row does not resolve a segment's style through the sweep: "
                "the runs and the segments both ascend, so one cursor per run "
                "is enough",
            )
        )
    if STYLE_LINEAR_SCAN in row:
        violations.append(
            Violation(
                "editor-style-sweep",
                EDITOR_WIDGET_RS,
                f"the row scans the runs from index 0 for every segment "
                f"({STYLE_LINEAR_SCAN!r}), which is `segments x ranges` on a "
                f"dense line",
            )
        )
    return violations


def check_editor_scan_key_is_compared_before_built(root: Path) -> list[Violation]:
    """The highlight key is compared against the request, not rebuilt to compare."""
    text = _read(root, EDITOR_HIGHLIGHT_RS)
    body = _function_block(text, "spans_for_line")
    if body is None:
        return [
            Violation(
                "editor-scan-key-alloc",
                EDITOR_HIGHLIGHT_RS,
                "the per-row entry point cannot be found, so this check cannot "
                "verify what it allocates",
            )
        ]
    if KEY_COMPARED not in body:
        return [
            Violation(
                "editor-scan-key-alloc",
                EDITOR_HIGHLIGHT_RS,
                "the key is not compared against the request before it is built: "
                "every visible row allocates two `String`s to re-derive an "
                "identity that did not change",
            )
        ]
    return []


def check_editor_prefix_check_is_seeded(root: Path) -> list[Violation]:
    """The highlight cache's prefix check starts where an edit placed it."""
    text = _read(root, EDITOR_HIGHLIGHT_RS)
    violations: list[Violation] = []
    if PREFIX_SEED_TAKEN not in text:
        if "pending_edit_from" not in text:
            violations.append(
                Violation(
                    "editor-prefix-seed",
                    EDITOR_HIGHLIGHT_RS,
                    "the cache has no edit seed at all: the prefix check re-reads "
                    "and re-hashes every scanned line on every keystroke",
                )
            )
        else:
            violations.append(
                Violation(
                    "editor-prefix-seed",
                    EDITOR_HIGHLIGHT_RS,
                    "the prefix check does not consume the line an edit placed "
                    f"({PREFIX_SEED_TAKEN!r}): the seed is recorded and ignored",
                )
            )
    rebase = _function_block(text, "rebase")
    if rebase is None or "first_changed_line(snapshot, from)" not in rebase:
        violations.append(
            Violation(
                "editor-prefix-seed",
                EDITOR_HIGHLIGHT_RS,
                "the check does not start from the seed, so an edit near the end "
                "of a long file still verifies every scanned line above it",
            )
        )
    return violations


def check_wrap_span_lookup_advances(root: Path) -> list[Violation]:
    """The styled wrap's span lookup advances once over the span list."""
    text = _read(root, WRAP_RS)
    production = _production_source(text)
    violations: list[Violation] = []
    if SPAN_CURSOR_STRUCT not in production:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                "the styled wrap has no span cursor: every word would resolve "
                "its parts by rescanning the span list from the front",
            )
        )
    if SPAN_CURSOR_ADVANCE not in production:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                f"the span cursor does not advance ({SPAN_CURSOR_ADVANCE!r}): a "
                f"lookup that never moves is the front rescan by another name",
            )
        )
    if SPAN_SPLIT in production:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                f"the wrap names {SPAN_SPLIT!r}: the per-word split allocates a "
                f"`Vec` and rescans the span list for every word, which is "
                f"`O(words x spans)`",
            )
        )
    if SPAN_FLAT_SCAN in production:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                f"the wrap scans the span list from index 0 ({SPAN_FLAT_SCAN!r}): "
                f"the lookup must advance the cursor instead",
            )
        )
    build = _function_block(production, "build_items")
    if build is None:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                "the styled item builder cannot be found, so this check cannot "
                "verify how it resolves a word's spans",
            )
        )
        return violations
    if SPAN_CURSOR_SEEK not in build:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                "the item builder resolves a word's span parts without the "
                "cursor, so every word works from the front of the span list",
            )
        )
    if SPAN_CURSOR_STYLE not in build:
        violations.append(
            Violation(
                "wrap-span-cursor",
                WRAP_RS,
                "the item builder resolves a separator's style without the "
                "cursor, so every space rescans the span list from the front",
            )
        )
    return violations


def check_wrap_escape_parser_is_untouched(root: Path) -> list[Violation]:
    """The escape parser and `ESC` are the ones the geometry shares."""
    text = _read(root, WRAP_RS)
    production = _production_source(text)
    violations: list[Violation] = []
    if ESCAPE_CONSTANT not in production:
        violations.append(
            Violation(
                "wrap-escape-parser",
                WRAP_RS,
                f"`ESC` is not the shared constant ({ESCAPE_CONSTANT!r}): the "
                f"composer's visual-row geometry skips a sequence with this "
                f"parser, and a second answer to `how wide is this sequence` is "
                f"what it exists to prevent",
            )
        )
    parser = _function_block(production, "escape_sequence_len")
    if parser is None:
        violations.append(
            Violation(
                "wrap-escape-parser",
                WRAP_RS,
                "the escape parser cannot be found, so this check cannot verify "
                "that a sequence is still measured as atomic",
            )
        )
    elif OSC_ARM not in parser:
        violations.append(
            Violation(
                "wrap-escape-parser",
                WRAP_RS,
                f"the OSC arm no longer consumes the unsupported sequence "
                f"({OSC_ARM!r}): an OSC-8 hyperlink would be split at its first "
                f"byte and its target would measure as visible text",
            )
        )
    tokenize = _function_block(production, "tokenize")
    if tokenize is None or ESCAPE_TOKEN not in tokenize:
        violations.append(
            Violation(
                "wrap-escape-parser",
                WRAP_RS,
                f"`tokenize` no longer takes a control sequence as one token "
                f"({ESCAPE_TOKEN!r}): the word around an OSC-8 hyperlink would "
                f"be split through the sequence",
            )
        )
    return violations


CHECKS = (
    check_wrap_span_lookup_advances,
    check_wrap_escape_parser_is_untouched,
    check_composer_has_one_wrap_producer,
    check_composer_layout_is_cached,
    check_markdown_has_one_parse_producer,
    check_streaming_tail_is_cached,
    check_render_path_does_not_clone_the_composer,
    check_render_path_does_not_clone_the_task_list,
    check_frame_buffer_is_retained_only_for_a_selection,
    check_sidebar_population_is_not_in_the_panel_loop,
    check_task_lookup_does_not_allocate_a_key,
    check_frame_cadence_is_unchanged,
    check_file_tree_rows_are_cached,
    check_file_tree_row_cache_key,
    check_file_tree_render_path_reads_the_cache,
    check_chat_rows_are_cached,
    check_chat_row_cache_key,
    check_chat_render_path_reads_the_cache,
    check_review_hunks_are_borrowed,
    check_streaming_tail_fast_path_serves_the_cached_rows,
    check_tool_output_body_is_built_under_the_expanded_gate,
    check_editor_row_read_is_windowed,
    check_editor_clamp_is_cached,
    check_editor_token_scan_is_bounded,
    check_editor_style_lookup_is_a_sweep,
    check_editor_scan_key_is_compared_before_built,
    check_editor_prefix_check_is_seeded,
    check_file_tree_row_inputs_are_written_through_verbs,
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
        "let all_rows = built.all.as_slice();",
        # A genuine deep clone: `all_rows` must be the `Vec<TaskRow>` for
        # `all_rows.clone()` to copy rows rather than a slice reference. The
        # earlier form cloned a `&[TaskRow]`, so the injection only carried the
        # token the check greps for and demonstrated nothing.
        "let all_rows = built.all.clone();\n"
        "        let _cloned = all_rows.clone();\n"
        "        let all_rows = all_rows.as_slice();",
    ),
    (
        "frame-buffer-retention",
        "a frame that retains its buffer with no selection",
        RENDER_RS,
        "self.last_frame_buffer = Some(buffer.clone());",
        "self.last_frame_buffer = Some(buffer.clone());\n"
        "        self.last_frame_buffer = Some(frame.buffer_mut().clone());",
    ),
    (
        "sidebar-population-per-panel",
        "a sidebar population inside the per-placement panel loop",
        RENDER_RS,
        "for (placement, rect) in regions.occupied() {",
        "for (placement, rect) in regions.occupied() {\n"
        "            self.populate_session_panel_if_needed();",
    ),
    (
        "task-lookup-key",
        "an allocating identity key in the task-row lookup",
        APP_MOD_RS,
        None,
        "fn another_task_lookup(task_id: &str) -> String {\n"
        '    format!("task:{task_id}")\n'
        "}\n",
    ),
    (
        "file-tree-row-input-write",
        "a direct write to the tree's row inputs from outside the authority",
        "src/tui/app/modal.rs",
        None,
        # The receiver has to be `file_tree_state.` — the check is anchored on
        # it, so an injection that writes through a bare `state` would pass the
        # check by not resembling the defect at all.
        '\n#[cfg(test)]\nfn injected_row_input_write(app: &mut App) {\n'
        '    app.file_tree_state.expanded.insert("injected".to_string());\n'
        '    app.file_tree_state.filter = "x".to_string();\n}\n',
    ),
    (
        "frame-cadence",
        "a render without its catch_unwind",
        EVENT_LOOP_RS,
        "std::panic::catch_unwind",
        "std::panic::resume_unwind",
    ),
    (
        "file-tree-row-cache",
        "a file-tree row accessor that rebuilds per read",
        FILE_TREE_STATE_RS,
        "pub fn rows(&self) -> &[TreeRow]",
        "pub fn rows(&self) -> Vec<TreeRow>",
    ),
    (
        "file-tree-row-cache-key",
        "a row cache keyed without the revision",
        FILE_TREE_STATE_RS,
        "self.row_cache.revision == self.revision && self.row_cache.filter == self.filter",
        "self.row_cache.filter == self.filter",
    ),
    (
        "file-tree-render-path-cache",
        "a pane that filters the listing while drawing",
        FILE_TREE_PANE_RS,
        None,
        "fn another_pane_filter(rows: &[TreeRow], filter: &str) -> Vec<TreeRow> {\n"
        "    filter_rows(rows, filter)\n"
        "}\n",
    ),
    (
        "file-tree-render-path-cache",
        "a box that filters the listing outside its cache rebuild",
        FILES_DIALOG_RS,
        None,
        "impl FilesDialog {\n"
        "    fn stale_rows(&self) -> Vec<TreeRow> {\n"
        "        filter_rows(&self.view.rows, &self.filter)\n"
        "    }\n"
        "}\n",
    ),
    (
        "chat-row-cache",
        "a chat row accessor that rebuilds per read",
        CHAT_BROWSER_RS,
        "pub fn rows<'a>(",
        "pub fn rows_uncached<'a>(",
    ),
    (
        "chat-row-cache-key",
        "a chat row cache keyed without the reducer's sequence",
        CHAT_BROWSER_RS,
        "self.sequence == request.sequence",
        "true",
    ),
    (
        "chat-render-path-cache",
        "a chat panel that rebuilds the window while drawing",
        CHAT_PANEL_RS,
        None,
        "fn another_chat_panel_producer(view: &ChatView<'_>) -> usize {\n"
        "    build_rows(view, 0, 30, &Theme::dark()).len()\n"
        "}\n",
    ),
    (
        "chat-render-path-cache",
        "a chat box that rebuilds the window while drawing",
        CHAT_DIALOG_RS,
        None,
        "fn another_chat_box_producer(view: &ChatView<'_>) -> usize {\n"
        "    build_rows(view, 0, 30, &Theme::dark()).len()\n"
        "}\n",
    ),
    (
        "review-borrowed-hunks",
        "a review that clones its patch into the viewer",
        CHANGE_REVIEW_RS,
        "viewer.render_hunks_in(&patch.hunks, body, frame.buffer_mut(), &theme);",
        "viewer.hunks = patch.hunks.clone();\n"
        "        viewer.render_hunks_in(&patch.hunks, body, frame.buffer_mut(), &theme);",
    ),
    (
        "tail-fast-path-assembly",
        "a streaming tail whose fast path assembles the blocks again",
        MESSAGES_RS,
        TAIL_FAST_PATH,
        TAIL_FAST_PATH
        + "\n            let _stale = assemble_markdown_blocks(&self.blocks);",
    ),
    (
        "tool-body-expanded-gate",
        "a tool-call body built whether or not the call is expanded",
        MESSAGES_RS,
        "if !*expanded || output.is_empty() || read_lang.is_some() {",
        "if output.is_empty() || read_lang.is_some() {",
    ),
    (
        "editor-row-window",
        "an editor row that reads its whole line",
        EDITOR_WIDGET_RS,
        "        let Some(window) = self.row_window(start, end, viewport.column, limit) else {",
        "        let _whole = self.snapshot.read_bytes(start, end);\n"
        "        let Some(window) = self.row_window(start, end, viewport.column, limit) else {",
    ),
    (
        "editor-row-window",
        "an editor row whose walk never stops at the window's end",
        EDITOR_WIDGET_RS,
        WINDOW_EARLY_EXIT,
        "        if false && started && display >= limit {",
    ),
    (
        "editor-clamp-cache",
        "a clamp memo keyed without the document revision",
        EDITOR_MODEL_RS,
        "        self.revision == revision\n            && self.first_line == first_line",
        "        self.first_line == first_line",
    ),
    (
        "editor-clamp-cache",
        "a viewport resolver that never consults the clamp memo",
        EDITOR_MODEL_RS,
        "            Some(cached) if cached.covers(revision, first, visible_lines, text_columns) => {",
        "            Some(cached) if false => {",
    ),
    (
        "editor-token-scan",
        "a row that filters the whole token vector",
        EDITOR_WIDGET_RS,
        "        let lsp_ranges = syntax_ranges.len();",
        "        let _all = self\n            .lsp\n            .semantic_tokens\n"
        "            .iter()\n            .filter(|token| token.line as usize == line_index)\n"
        "            .count();\n"
        "        let lsp_ranges = syntax_ranges.len();",
    ),
    (
        "editor-token-scan",
        "a row that searches the token vector from the front",
        EDITOR_WIDGET_RS,
        "        let mut token_cursor = self.lsp.semantic_tokens.partition_point(|token| {\n"
        "            token_scan_steps += 1;\n"
        "            (token.line as usize) < line_index\n"
        "        });",
        "        let mut token_cursor = 0usize;\n"
        "        let _ = token_scan_steps;",
    ),
    (
        "editor-style-sweep",
        "a row that scans the style runs from index 0 for every segment",
        EDITOR_WIDGET_RS,
        "        // Both runs and the segments are ascending, so each run needs one",
        "        let _first = syntax_ranges\n"
        "            .iter()\n"
        "            .find(|(token_range, _)| token_range.start <= visible_start);\n"
        "        // Both runs and the segments are ascending, so each run needs one",
    ),
    (
        "editor-scan-key-alloc",
        "a scan key that is rebuilt before it is compared",
        EDITOR_HIGHLIGHT_RS,
        "        let same_document = self\n"
        "            .key\n"
        "            .as_ref()\n"
        "            .is_some_and(|key| key.path == path && key.code_theme == code_theme);",
        "        let probe = ScanKey {\n"
        "            path: path.to_string(),\n"
        "            code_theme: code_theme.to_string(),\n"
        "        };\n"
        "        let same_document = self.key.as_ref() == Some(&probe);",
    ),
    (
        "editor-prefix-seed",
        "a prefix check that ignores the edit's line",
        EDITOR_HIGHLIGHT_RS,
        "        let from = self.pending_edit_from.take().unwrap_or(0);",
        "        let from = 0;",
    ),
    (
        "wrap-span-cursor",
        "a per-word split that rescans the span list from the front",
        WRAP_RS,
        "            SegmentKind::Word => {",
        "            SegmentKind::Word => {\n"
        "                let _stale =\n"
        "                    split_at_spans(&source.spans, segment.start, segment.end);",
    ),
    (
        "wrap-span-cursor",
        "a span cursor that never advances",
        WRAP_RS,
        "            self.next += 1;",
        "            self.next += 0;",
    ),
    (
        "wrap-escape-parser",
        "an OSC sequence that is no longer consumed whole",
        WRAP_RS,
        "        b']' => scan_to_terminator(bytes, 2),",
        "        b']' => 2,",
    ),
    (
        "wrap-escape-parser",
        "a tokenizer that no longer takes a sequence as a token",
        WRAP_RS,
        "            let end = advance_past_escape(text, idx);",
        "            let end = idx + 1;",
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
