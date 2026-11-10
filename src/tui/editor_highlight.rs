//! Bounded, server-free syntax highlighting for the TUI editor.
//!
//! The editor's *authoritative* highlighting is LSP semantic tokens
//! (`crate::tui::editor_lsp`), which know the real grammar of the open
//! project. But a language server is optional: with none installed, every
//! file rendered as flat text, so "syntax highlighting" was really a
//! property of the machine rather than of the editor.
//!
//! This module is the always-available floor *underneath* those tokens. It
//! is a plain `syntect` scan of the same document — no server, no network,
//! no daemon round trip — and the renderer prefers an LSP token whenever one
//! covers the same bytes.
//!
//! ## Why the scan is sequential, and why that stays bounded
//!
//! `syntect`'s parser is a state machine: line N's colours depend on whether
//! line N-1 opened a block comment or an unterminated string. Colouring a
//! line in isolation would paint the body of a multi-line comment as code,
//! which is worse than being right on 99% of lines and briefly odd on the
//! rest.
//!
//! So the machine advances **forward only** and caches the spans of the
//! lines it has already consumed. A viewport that scrolls backwards, or
//! jumps, resets the machine and re-scans from the top — bounded by
//! [`MAX_HIGHLIGHT_SCAN_LINES`], past which highlighting disables itself
//! and reports that, rather than stalling a frame on a file nobody can
//! realistically edit in one sitting.
//!
//! ## Bounded work (C002, unchanged)
//!
//! This module adds no budget of its own to the render path: it is asked
//! for one visible row at a time and returns that row's spans. The frame
//! budget that governs how many *bytes* are read lives in the renderer
//! (`MAX_FRAME_SOURCE_BYTES`), and the two compose — a row the frame budget
//! has already refused is never offered here.

use std::collections::BTreeMap;
use std::fmt;
use std::hash::Hasher;
use std::ops::Range;

use crate::tui::render_cost;
use codegg_document::DocumentSnapshot;
use once_cell::sync::Lazy;
use ratatui::style::Color;
use std::collections::hash_map::DefaultHasher;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Largest document, in lines, this scanner will colour from the top.
///
/// Reaching line 5000 of a file by scrolling is routine; reaching it by
/// jumping (`G`) is the case that would force a full re-scan on one frame.
/// Past the bound the scanner refuses and the banner says highlighting is
/// off, because a visibly-unhighlighted file is a far better outcome than a
/// frame that takes a second to draw.
pub const MAX_HIGHLIGHT_SCAN_LINES: usize = 5_000;

/// Cached lines retained. Sized well above any plausible viewport so
/// scrolling within a page never re-scans, and small enough that the map
/// cannot grow without bound across a long editing session.
pub const MAX_HIGHLIGHT_CACHED_LINES: usize = 512;

/// One coloured run within a line, as a byte range relative to the line
/// start plus the foreground to paint it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Byte range relative to the start of the line.
    pub range: Range<usize>,
    /// Theme-resolved foreground for this run.
    pub color: Color,
}

/// Identity the cached spans were produced for.
///
/// Any difference invalidates the cache: a new path is a different grammar
/// and a new code theme is a different palette.
///
/// The **revision** is deliberately not part of the key. A local edit bumps
/// the revision, and keying on it meant every keystroke threw the whole
/// cache away — on the one path the cache was built for. The document's
/// *content* decides instead: the per-line hashes say **where** the
/// document changed, so a keystroke in the middle of a file keeps the lines
/// above it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanKey {
    path: String,
    code_theme: String,
}

/// Incremental, bounded syntax highlighting state for one document.
///
/// Holds no document text: the snapshot is borrowed per call and dropped
/// with it. Every cache entry is a list of byte ranges and colors.
#[derive(Default)]
pub struct EditorHighlight {
    key: Option<ScanKey>,
    /// Forward-only parser state, valid once `scanned_to > 0`. The `'static`
    /// comes from the lazily-loaded built-in theme set, so the scanner owns
    /// no theme and cannot outlive one.
    state: Option<HighlightLines<'static>>,
    /// Number of lines consumed so far, and therefore the index of the next
    /// line the machine has not yet seen.
    scanned_to: usize,
    cache: BTreeMap<usize, Vec<HighlightSpan>>,
    /// The hash of the bytes each scanned line was produced from.
    ///
    /// The hashes say **where** the document changed rather than only that
    /// it changed, so a local edit keeps the lines above it. They are kept
    /// for every scanned line, not only for the cached ones: a line whose
    /// spans were evicted still has to be comparable.
    line_hashes: BTreeMap<usize, u64>,
    /// The revision the scan was made against.
    scanned_revision: u64,
    /// The first line an accepted edit could have touched, pending the next
    /// prefix check.
    ///
    /// The check's job is to find the **first** line whose content differs
    /// from the scan the cache holds. Starting at line 0 answers that
    /// question by re-reading every scanned line, which is the whole file on
    /// every keystroke. An edit knows where it landed: nothing above it
    /// changed, so the check can start there and stop at the first
    /// difference.
    ///
    /// `None` is the honest fallback — a change the presentation did not
    /// make (a daemon-side edit, a reload) is not placed by anything here,
    /// and a check seeded above a real change would colour rows from a
    /// context that no longer exists. A missed change is a wrong colour,
    /// which is a correctness defect: this cache is the only source of a
    /// row's spans.
    pending_edit_from: Option<usize>,
    /// Set once the document exceeds [`MAX_HIGHLIGHT_SCAN_LINES`].
    disabled: bool,
}

/// Hand-written because `syntect`'s parser state has no `Debug` and a derived
/// one would dump a scope stack on every failing assertion. What matters
/// when this is printed is *how far the scan got*, not what is inside it.
impl fmt::Debug for EditorHighlight {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EditorHighlight")
            .field("path", &self.key.as_ref().map(|key| key.path.as_str()))
            .field("scanned_to", &self.scanned_to)
            .field("cached_lines", &self.cache.len())
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl EditorHighlight {
    /// Forget everything about the current document.
    ///
    /// Called when the attachment closes or switches documents, so a new
    /// file cannot inherit the previous grammar's cache.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether highlighting has switched itself off for this document.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Note that an accepted edit landed on `line`.
    ///
    /// The caller is the edit funnel: the transaction's edits are sorted, so
    /// its **first** edit's line is a line above which nothing in that
    /// transaction changed. Several edits between two frames combine by
    /// `min`, because the earliest of them is the first line the next check
    /// has to verify.
    ///
    /// `line` must be resolved against the **post-edit** snapshot. That is
    /// not a detail: an edit that inserts or deletes lines above itself
    /// moves every later line index, and a seed recorded from the pre-edit
    /// numbering would name a line the change is no longer on.
    pub fn note_edit_from(&mut self, line: usize) {
        self.pending_edit_from = Some(match self.pending_edit_from {
            Some(existing) => existing.min(line),
            None => line,
        });
    }

    /// The line the next prefix check will start from, or `None` when it
    /// will fall back to the top.
    ///
    /// Read by the tests that prove an edit reaches the cache, and by
    /// nothing on the render path — hence the `cfg`: a shipped build has no
    /// use for it, and an unused accessor is dead code.
    #[cfg(test)]
    pub(crate) fn pending_edit_from(&self) -> Option<usize> {
        self.pending_edit_from
    }

    /// Coloured runs for `line`, or an empty slice when this line cannot be
    /// highlighted.
    ///
    /// `snapshot` is borrowed for the call and dropped with it; nothing
    /// here retains document text.
    pub fn spans_for_line(
        &mut self,
        snapshot: &DocumentSnapshot,
        path: &str,
        code_theme: &str,
        line: usize,
    ) -> &[HighlightSpan] {
        // Compare before allocating. `spans_for_line` runs once per visible
        // row per frame, so building the key first meant two `String`s per
        // row to re-derive an identity that had not changed.
        let same_document = self
            .key
            .as_ref()
            .is_some_and(|key| key.path == path && key.code_theme == code_theme);
        if !same_document {
            self.reset();
            self.key = Some(ScanKey {
                path: path.to_string(),
                code_theme: code_theme.to_string(),
            });
        }
        if self.disabled || line >= MAX_HIGHLIGHT_SCAN_LINES {
            self.disabled = true;
            return &[];
        }
        // The document changed under the cache. The revision says *that* it
        // changed; the per-line hashes say *where*, so the lines the edit did
        // not touch keep their spans.
        let revision = snapshot.revision().get();
        if self.state.is_some() && self.scanned_revision != revision {
            self.rebase(snapshot, path, code_theme, revision);
        }
        // Serve from cache when this row and every line before it are done.
        // The `scanned_to` check matters as much as the lookup: a cached row
        // whose predecessors have been evicted cannot be trusted, because
        // the parser state that produced it is gone.
        if self.state.is_some() && line < self.scanned_to && self.cache.contains_key(&line) {
            return &self.cache[&line];
        }
        self.rescan_to(snapshot, path, code_theme, line);
        self.cache.get(&line).map_or(&[], Vec::as_slice)
    }

    /// Rebuild the machine after the document changed under it.
    ///
    /// A local edit changes the lines it touches and none of the others, so
    /// the cache is **not** thrown away. The lines before the change keep
    /// their spans; the machine is replayed to the change, because the
    /// parser is forward-only and the lines after the change have to be
    /// coloured from the context they would have been coloured from.
    fn rebase(&mut self, snapshot: &DocumentSnapshot, path: &str, code_theme: &str, revision: u64) {
        self.scanned_revision = revision;
        // Where the check starts. An edit the presentation placed says
        // nothing above it changed, so verifying from there is complete; an
        // edit it did not place (a daemon-side change, a reload) has to be
        // found from the top, because assuming "nothing above" is exactly
        // how a row ends up with the previous content's colours.
        let from = self.pending_edit_from.take().unwrap_or(0);
        let Some(changed) = self.first_changed_line(snapshot, from) else {
            // The scanned lines are unchanged: the edit is below them, and
            // the machine's state is still the state of those lines.
            return;
        };
        self.cache.retain(|index, _| *index < changed);
        self.line_hashes.retain(|index, _| *index < changed);
        self.replay_to(snapshot, path, code_theme, changed);
    }

    /// The first line at or after `from` whose content differs from the scan
    /// the cache holds.
    ///
    /// `from` is a **lower bound on the answer**, not a hint to be
    /// second-guessed: the caller only passes a line it knows nothing above
    /// changed. The walk stops at the first difference, so an edit on line
    /// 4,900 of a 5,000-line document costs one line rather than 4,900.
    fn first_changed_line(&self, snapshot: &DocumentSnapshot, from: usize) -> Option<usize> {
        let mut examined = 0u64;
        let mut changed = None;
        for index in from..self.scanned_to {
            examined += 1;
            let Some(cached) = self.line_hashes.get(&index) else {
                // The line's bytes are not known, so nothing above it can be
                // trusted either.
                changed = Some(index);
                break;
            };
            if self.line_hash(snapshot, index) != Some(*cached) {
                changed = Some(index);
                break;
            }
        }
        render_cost::count_highlight_prefix_lines(examined);
        changed
    }

    /// The hash of a line's bytes.
    ///
    /// Hashed from the rope's borrowed chunks, so a line is never
    /// materialized as a `String` to be measured. The hasher is fed the
    /// line's byte stream in pieces; SipHash (which `DefaultHasher` is) is a
    /// streaming hash, so the value depends on the bytes and not on where the
    /// rope happens to keep its chunk boundaries — a line whose text is
    /// unchanged hashes the same in two documents built from the same text.
    fn line_hash(&self, snapshot: &DocumentSnapshot, index: usize) -> Option<u64> {
        let range = snapshot.line_range(index).ok()?;
        let mut hasher = DefaultHasher::new();
        for chunk in snapshot.byte_chunks(range.start, range.end).ok()? {
            hasher.write(chunk);
        }
        Some(hasher.finish())
    }

    /// Replay the machine up to `line` without keeping the spans.
    ///
    /// The spans of the lines before `line` are already cached and are not
    /// rewritten: the machine is being rebuilt, not the colours.
    fn replay_to(
        &mut self,
        snapshot: &DocumentSnapshot,
        path: &str,
        code_theme: &str,
        line: usize,
    ) {
        let Some(syntax) = syntax_for(path) else {
            self.state = None;
            self.cache.clear();
            return;
        };
        let Some(theme) = THEME_SET.themes.get(code_theme) else {
            self.state = None;
            self.cache.clear();
            return;
        };
        self.state = Some(HighlightLines::new(syntax, theme));
        self.scanned_to = 0;
        self.scanned_revision = snapshot.revision().get();
        for index in 0..line {
            if !self.advance(snapshot, index) {
                break;
            }
        }
    }

    /// Advance the machine over `index` without keeping its spans.
    fn advance(&mut self, snapshot: &DocumentSnapshot, index: usize) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        let Some(range) = snapshot.line_range(index).ok() else {
            return false;
        };
        let Ok(raw) = snapshot.read_bytes(range.start, range.end) else {
            return false;
        };
        if state.highlight_line(&raw, &SYNTAX_SET).is_err() {
            return false;
        }
        self.scanned_to = index + 1;
        true
    }

    /// Reset the machine and replay lines `0..=line` from the top.
    ///
    /// The machine's state is valid up to `scanned_to`, so a line past it
    /// continues the scan instead of starting it over: the previous version
    /// cleared the cache and replayed from the top for **every** line past
    /// the viewport, which made a frame cost the square of the viewport's
    /// height.
    fn rescan_to(
        &mut self,
        snapshot: &DocumentSnapshot,
        path: &str,
        code_theme: &str,
        line: usize,
    ) {
        let Some(syntax) = syntax_for(path) else {
            // An extension with no bundled grammar is not an error — it just
            // means this file has no built-in colouring. Leave the machine
            // unset so the cheap path stays cheap.
            self.state = None;
            self.cache.clear();
            return;
        };
        let Some(theme) = THEME_SET.themes.get(code_theme) else {
            self.state = None;
            self.cache.clear();
            return;
        };
        if self.state.is_some() && line >= self.scanned_to {
            // The machine already consumed the lines before `scanned_to`, so
            // the scan continues from there and the cached lines stay.
            for index in self.scanned_to..=line {
                if !self.consume(snapshot, index) {
                    break;
                }
            }
            self.evict();
            return;
        }
        self.cache.clear();
        self.state = Some(HighlightLines::new(syntax, theme));
        self.scanned_to = 0;
        self.scanned_revision = snapshot.revision().get();
        for index in 0..=line {
            if !self.consume(snapshot, index) {
                break;
            }
        }
        self.evict();
    }

    /// Highlight one line into the cache, advancing the machine.
    ///
    /// Returns `false` when the line could not be read or parsed, which
    /// ends the scan: every later line's state would be built on a guess.
    fn consume(&mut self, snapshot: &DocumentSnapshot, index: usize) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        let Some(range) = snapshot.line_range(index).ok() else {
            return false;
        };
        let Ok(raw) = snapshot.read_bytes(range.start, range.end) else {
            return false;
        };
        let Ok(runs) = state.highlight_line(&raw, &SYNTAX_SET) else {
            return false;
        };
        let mut spans = Vec::with_capacity(runs.len());
        // `highlight_line` hands back borrowed slices of the input, so a
        // running offset is enough to recover each run's byte range within the
        // line without a second pass to measure it.
        let mut offset = 0usize;
        for (style, piece) in runs {
            let width = piece.len();
            if width == 0 {
                continue;
            }
            // The plain-text grammar (and some single-line grammars) carry no
            // foreground; leaving those runs out keeps the renderer's base
            // style — which owns the current-line tint — instead of pinning
            // an arbitrary terminal default.
            if style.foreground.a != 0 {
                spans.push(HighlightSpan {
                    range: offset..offset + width,
                    color: Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                });
            }
            offset += width;
        }
        render_cost::count_highlight_scan();
        self.cache.insert(index, spans);
        // The bytes this line was coloured from, so a later edit can say
        // whether this line changed.
        self.line_hashes.insert(index, hash_bytes(raw.as_bytes()));
        self.scanned_to = index + 1;
        true
    }

    /// Drop the oldest cached lines once the map outgrows its bound.
    ///
    /// Only lines below `scanned_to` are evicted, and never a line the
    /// machine still has state for, so eviction can never silently
    /// invalidate the scan order.
    fn evict(&mut self) {
        while self.cache.len() > MAX_HIGHLIGHT_CACHED_LINES {
            let Some(&oldest) = self.cache.keys().next() else {
                return;
            };
            if oldest >= self.scanned_to {
                return;
            }
            self.cache.remove(&oldest);
        }
    }
}

/// The hash of a line's bytes.
///
/// The hash says **whether** the line changed, not what it changed to. A
/// collision would colour a line from a stale context, which is why the
/// hash is a keyed one rather than a checksum.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

/// Resolve the bundled grammar for a workspace-relative path.
///
/// Returns `None` when no bundled grammar claims the extension, which is
/// the honest answer for a `.md` or an unknown suffix: render it plain and
/// let the LSP layer speak if a server is around.
fn syntax_for(path: &str) -> Option<&'static SyntaxReference> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())?;
    // `find_syntax_by_extension` is the whole of the match: it is already
    // keyed by the suffix, and `find_syntax_by_token` expects a language id
    // (`rs`, `py`), which a path does not carry.
    SYNTAX_SET
        .find_syntax_by_extension(extension)
        .or_else(|| SYNTAX_SET.find_syntax_by_name(extension))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render_cost;
    use codegg_document::{DocumentBuffer, DocumentLimits, TextEdit, TextTransaction};

    const PATH: &str = "src/main.rs";
    const THEME: &str = "base16-ocean.dark";

    /// Forty lines of ordinary Rust, so a scan is cheap and every line is
    /// distinguishable by its own content.
    fn source_lines() -> String {
        (0..40)
            .map(|index| format!("let value_{index} = {index};\n"))
            .collect()
    }

    /// The byte offset where `line` starts in `text`.
    fn offset_of(text: &str, line: usize) -> usize {
        text.split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
    }

    /// Apply one zero-line-count edit that changes `line`'s first word.
    ///
    /// `let` becomes `var`, which is not a Rust keyword: the row's colours
    /// genuinely change, so a cache that served the old spans would be
    /// visibly wrong rather than merely stale.
    fn edit_first_word(buffer: &mut DocumentBuffer, text: &str, line: usize) -> DocumentSnapshot {
        let start = offset_of(text, line);
        buffer
            .apply(
                &TextTransaction::new(vec![TextEdit::new(start..start + 3, "var")]),
                DocumentLimits::default(),
            )
            .expect("the fixture's edit applies")
            .snapshot
    }

    /// Every line's spans from one highlighter, requested in document order.
    fn all_spans(
        highlight: &mut EditorHighlight,
        snapshot: &DocumentSnapshot,
        lines: usize,
    ) -> Vec<Vec<HighlightSpan>> {
        (0..lines)
            .map(|line| {
                highlight
                    .spans_for_line(snapshot, PATH, THEME, line)
                    .to_vec()
            })
            .collect()
    }

    /// Every line's spans from a highlighter that has seen nothing.
    fn fresh_spans(snapshot: &DocumentSnapshot, lines: usize) -> Vec<Vec<HighlightSpan>> {
        all_spans(&mut EditorHighlight::default(), snapshot, lines)
    }

    /// The lines the prefix check re-read, for one piece of work.
    fn prefix_lines(work: impl FnOnce()) -> u64 {
        let before = render_cost::snapshot();
        work();
        render_cost::snapshot()
            .since(&before)
            .highlight_prefix_lines
    }

    fn spans_for(text: &str, path: &str, line: usize) -> Vec<HighlightSpan> {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut highlight = EditorHighlight::default();
        highlight
            .spans_for_line(&snapshot, path, "base16-ocean.dark", line)
            .to_vec()
    }

    #[test]
    fn rust_keywords_and_strings_get_distinct_colors() {
        // `let` is a keyword and `"text"` a string; a scanner that returned
        // one flat run for the line would mean the grammar never matched.
        let spans = spans_for("let value = \"text\";\n", "src/main.rs", 0);
        assert!(
            spans.len() >= 2,
            "expected several coloured runs, got {spans:?}"
        );
        assert!(
            spans.windows(2).any(|pair| pair[0].color != pair[1].color),
            "expected at least two distinct colors, got {spans:?}"
        );
    }

    #[test]
    fn the_body_of_a_block_comment_still_reads_as_a_comment() {
        // This is the whole reason the scan is forward-only rather than
        // per-line: `not a keyword` on line 1 is code-coloured if line 0's
        // `/*` is forgotten.
        let text = "/* a block\nlet value = 1;\n*/\nlet after = 2;\n";
        let after_open = spans_for(text, "src/main.rs", 1);
        let after_close = spans_for(text, "src/main.rs", 3);
        assert!(
            !after_open.is_empty(),
            "the comment body must still be coloured"
        );
        assert_ne!(
            after_open[0].color, after_close[0].color,
            "comment body and code after it must not share a color"
        );
    }

    #[test]
    fn an_unknown_extension_is_plain_rather_than_wrong() {
        let spans = spans_for("some words here\n", "notes.unknownext", 0);
        assert!(spans.is_empty(), "unknown suffix must render plain");
    }

    #[test]
    fn a_different_path_is_a_different_grammar_and_resets_the_scanner() {
        let snapshot = DocumentBuffer::new("let a = 1;\n").snapshot();
        let mut highlight = EditorHighlight::default();
        let first = highlight
            .spans_for_line(&snapshot, "src/main.rs", "base16-ocean.dark", 0)
            .to_vec();
        assert!(!first.is_empty());

        // An extension with no bundled grammar cannot resolve a syntax, so a
        // path switch must clear the previous grammar's spans rather than
        // handing Rust colors to a file that has no Rust in it. (A *different
        // real* grammar would legitimately produce spans, so the honest
        // probe for "did the cache reset" is an unresolvable suffix.)
        let other = highlight
            .spans_for_line(&snapshot, "notes.notagrammar", "base16-ocean.dark", 0)
            .to_vec();
        assert!(other.is_empty(), "switching path must reset the scanner");

        // And the original grammar still works afterwards.
        let again = highlight
            .spans_for_line(&snapshot, "src/main.rs", "base16-ocean.dark", 0)
            .to_vec();
        assert_eq!(again, first, "the Rust spans must be reproducible");
    }

    #[test]
    fn two_fresh_attachments_to_different_text_share_a_key() {
        // Both replicas start at revision 0, so a key of
        // (path, theme, revision) alone cannot tell them apart. This is not
        // hypothetical: reopening a path after discarding edits produces a
        // brand-new replica at revision 0 with different bytes.
        let first = DocumentBuffer::new("let a = 1;\n").snapshot();
        let second = DocumentBuffer::new("let ab = 22;\n").snapshot();
        assert_eq!(first.revision(), second.revision());
        let mut highlight = EditorHighlight::default();
        let before = highlight
            .spans_for_line(&first, "src/main.rs", "base16-ocean.dark", 0)
            .to_vec();
        let _stale = highlight.spans_for_line(&second, "src/main.rs", "base16-ocean.dark", 0);
        assert_eq!(
            before,
            highlight.spans_for_line(&first, "src/main.rs", "base16-ocean.dark", 0),
            "a same-keyed request is served from cache by design"
        );

        // The fix is not a cleverer key — `EditorState::detach` resets the
        // scanner, because "a different document" is a fact about the
        // attachment rather than about the bytes.
        highlight.reset();
        assert!(
            highlight.state.is_none() && highlight.cache.is_empty(),
            "reset must drop the parser state and every cached line"
        );
    }

    // ── M005: the prefix check's seed ─────────────────────────────────

    /// An edit **above** the cached scan is detected.
    ///
    /// This is the direction that must never be optimised away: if the seed
    /// let the check skip lines above the edit, the rows above would keep the
    /// previous content's colours, and the cache is the only source of a
    /// row's spans — a wrong colour here is a correctness defect, not a stale
    /// pixel.
    #[test]
    fn an_edit_above_the_cached_scan_is_still_detected() {
        let text = source_lines();
        let mut buffer = DocumentBuffer::new(&text);
        let mut highlight = EditorHighlight::default();
        // The cache has scanned through line 20.
        let _ = highlight.spans_for_line(&buffer.snapshot(), PATH, THEME, 20);

        let edited = edit_first_word(&mut buffer, &text, 3);
        highlight.note_edit_from(3);
        let cached = all_spans(&mut highlight, &edited, 21);

        assert_eq!(
            cached,
            fresh_spans(&edited, 21),
            "the rows after an edit above the scan must equal a fresh scan"
        );
        assert_ne!(
            cached[3], cached[4],
            "the edited row's colours must have changed, or this test proves nothing"
        );
    }

    /// An edit **below** the cached scan is detected and costs no prefix
    /// read at all.
    #[test]
    fn an_edit_below_the_cached_scan_reads_no_prefix() {
        let text = source_lines();
        let mut buffer = DocumentBuffer::new(&text);
        let mut highlight = EditorHighlight::default();
        let _ = highlight.spans_for_line(&buffer.snapshot(), PATH, THEME, 20);
        let before = all_spans(&mut highlight, &buffer.snapshot(), 21);

        let edited = edit_first_word(&mut buffer, &text, 30);
        highlight.note_edit_from(30);
        let cost = prefix_lines(|| {
            let _ = highlight.spans_for_line(&edited, PATH, THEME, 20);
        });

        assert_eq!(
            cost, 0,
            "an edit below the scan leaves the scanned prefix unchanged: {cost} lines checked"
        );
        assert_eq!(
            all_spans(&mut highlight, &edited, 21),
            before,
            "no scanned row changed, so no scanned row is recoloured"
        );
    }

    /// An edit **near the end** starts the check at the edit.
    ///
    /// The check's job is to find the first row whose content differs. From
    /// line 0 that answer costs every scanned line; from the edit it costs
    /// one.
    #[test]
    fn an_edit_near_the_end_starts_the_check_at_the_edit() {
        let text = source_lines();
        let mut buffer = DocumentBuffer::new(&text);
        let mut highlight = EditorHighlight::default();
        let _ = highlight.spans_for_line(&buffer.snapshot(), PATH, THEME, 39);

        let edited = edit_first_word(&mut buffer, &text, 38);
        highlight.note_edit_from(38);
        let cost = prefix_lines(|| {
            let _ = highlight.spans_for_line(&edited, PATH, THEME, 39);
        });

        assert_eq!(
            cost, 1,
            "the check starts at the edit and stops at the first difference: {cost} lines"
        );
        assert_eq!(
            highlight.spans_for_line(&edited, PATH, THEME, 38).to_vec(),
            fresh_spans(&edited, 39)[38],
            "the edited row is coloured from the context it would have had"
        );
    }

    /// An edit the presentation cannot place still scans from the top.
    ///
    /// The count is the distance from line 0 to the change — four lines for
    /// an edit on line 3 — where a seeded check would have examined one. That
    /// is the price of a change nothing placed, and it is the price that must
    /// be paid: assuming "nothing above the scan changed" without an edit to
    /// place is how a row keeps the previous content's colours.
    #[test]
    fn an_unplaced_edit_still_scans_from_the_top() {
        let text = source_lines();
        let mut buffer = DocumentBuffer::new(&text);
        let mut highlight = EditorHighlight::default();
        let _ = highlight.spans_for_line(&buffer.snapshot(), PATH, THEME, 20);

        let edited = edit_first_word(&mut buffer, &text, 3);
        let cost = prefix_lines(|| {
            let _ = highlight.spans_for_line(&edited, PATH, THEME, 20);
        });

        assert_eq!(
            cost, 4,
            "a change nothing placed is found by walking from line 0: {cost} lines"
        );
        assert_eq!(
            all_spans(&mut highlight, &edited, 21),
            fresh_spans(&edited, 21),
            "an unplaced edit is still detected"
        );
    }

    /// Two edits between frames start the check at the **first** of them.
    #[test]
    fn the_seed_keeps_the_earliest_edit() {
        let text = source_lines();
        let mut buffer = DocumentBuffer::new(&text);
        let mut highlight = EditorHighlight::default();
        let _ = highlight.spans_for_line(&buffer.snapshot(), PATH, THEME, 20);

        let once = edit_first_word(&mut buffer, &text, 7);
        highlight.note_edit_from(7);
        let edited = edit_first_word(&mut buffer, &once.to_string(), 12);
        highlight.note_edit_from(12);

        assert_eq!(
            highlight.pending_edit_from(),
            Some(7),
            "the earliest unverified edit is the one the check must start from"
        );
        assert_eq!(
            all_spans(&mut highlight, &edited, 21),
            fresh_spans(&edited, 21),
            "both edits are detected"
        );
    }

    /// A hit does not rebuild the scan key.
    ///
    /// The key is compared before it is built, so asking for a row of the
    /// document already keyed costs no allocation and — the observable half —
    /// does not reset the cache.
    #[test]
    fn a_hit_does_not_reset_the_cache() {
        let snapshot = DocumentBuffer::new(source_lines()).snapshot();
        let mut highlight = EditorHighlight::default();
        let first = highlight.spans_for_line(&snapshot, PATH, THEME, 4).to_vec();
        assert!(!first.is_empty(), "the fixture row must be coloured");
        let scanned = highlight.scanned_to;

        let again = highlight.spans_for_line(&snapshot, PATH, THEME, 4).to_vec();

        assert_eq!(again, first, "the same row is served from the cache");
        assert_eq!(
            highlight.scanned_to, scanned,
            "a hit must not throw the scan away and rebuild it"
        );
        // A different path *is* a different grammar, so it does reset.
        highlight.spans_for_line(&snapshot, "notes.notagrammar", THEME, 4);
        assert_eq!(
            highlight.scanned_to, 0,
            "a different key must reset the scan"
        );
    }

    /// The keyed hash depends on the bytes, not on where the rope keeps its
    /// leaves.
    ///
    /// `consume` hashes the line it already holds as one slice and
    /// `line_hash` feeds the same hasher the rope's chunks, so the two values
    /// are compared against each other on every frame. If the hasher were
    /// sensitive to how the bytes were split, an unchanged line would look
    /// changed whenever the rope's structure did.
    #[test]
    fn the_keyed_hash_is_a_stream_and_not_a_slice() {
        let mut whole = DefaultHasher::new();
        whole.write(b"let value = 1;\n");
        let mut split = DefaultHasher::new();
        for piece in [b"let ".as_slice(), b"value", b" = 1;", b"\n".as_slice()] {
            split.write(piece);
        }
        assert_eq!(
            whole.finish(),
            split.finish(),
            "the keyed hash must depend on the line's bytes, not on the chunking"
        );
    }

    #[test]
    fn a_document_past_the_scan_bound_disables_highlighting_visibly() {
        let lines = (0..MAX_HIGHLIGHT_SCAN_LINES + 10)
            .map(|index| format!("let value_{index} = {index};\n"))
            .collect::<String>();
        let snapshot = DocumentBuffer::new(&lines).snapshot();
        let mut highlight = EditorHighlight::default();
        assert!(!highlight.is_disabled());
        highlight.spans_for_line(
            &snapshot,
            "src/main.rs",
            "base16-ocean.dark",
            MAX_HIGHLIGHT_SCAN_LINES + 5,
        );
        assert!(
            highlight.is_disabled(),
            "a file past the bound must switch highlighting off, not stall"
        );
        // And it stays off, cheaply, for every later request.
        assert!(highlight
            .spans_for_line(
                &snapshot,
                "src/main.rs",
                "base16-ocean.dark",
                MAX_HIGHLIGHT_SCAN_LINES + 6,
            )
            .is_empty());
    }
}
