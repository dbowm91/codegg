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

use codegg_document::DocumentSnapshot;
use crate::tui::render_cost;
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
        let key = ScanKey {
            path: path.to_string(),
            code_theme: code_theme.to_string(),
        };
        if self.key.as_ref() != Some(&key) {
            self.reset();
            self.key = Some(key);
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
    fn rebase(
        &mut self,
        snapshot: &DocumentSnapshot,
        path: &str,
        code_theme: &str,
        revision: u64,
    ) {
        self.scanned_revision = revision;
        let Some(changed) = self.first_changed_line(snapshot) else {
            // The scanned lines are unchanged: the edit is below them, and
            // the machine's state is still the state of those lines.
            return;
        };
        self.cache.retain(|index, _| *index < changed);
        self.line_hashes.retain(|index, _| *index < changed);
        self.replay_to(snapshot, path, code_theme, changed);
    }

    /// The first line whose content differs from the scan the cache holds.
    fn first_changed_line(&self, snapshot: &DocumentSnapshot) -> Option<usize> {
        for index in 0..self.scanned_to {
            let Some(cached) = self.line_hashes.get(&index) else {
                // The line's bytes are not known, so nothing above it can be
                // trusted either.
                return Some(index);
            };
            if self.line_hash(snapshot, index) != Some(*cached) {
                return Some(index);
            }
        }
        None
    }

    /// The hash of a line's bytes.
    fn line_hash(&self, snapshot: &DocumentSnapshot, index: usize) -> Option<u64> {
        let range = snapshot.line_range(index).ok()?;
        let raw = snapshot.read_bytes(range.start, range.end).ok()?;
        Some(hash_bytes(raw.as_bytes()))
    }

    /// Replay the machine up to `line` without keeping the spans.
    ///
    /// The spans of the lines before `line` are already cached and are not
    /// rewritten: the machine is being rebuilt, not the colours.
    fn replay_to(&mut self, snapshot: &DocumentSnapshot, path: &str, code_theme: &str, line: usize) {
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
    use codegg_document::DocumentBuffer;

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
