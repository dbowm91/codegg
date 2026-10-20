//! Byte-level pair scanning for the TUI editor's match mode and text objects.
//!
//! Owned by the M009 implementation plan
//! (`plans/implementation/tui-editor-helix-parity/009-match-mode-and-text-objects.md`),
//! work package W1.
//!
//! This module is intentionally **dependency-free**: it holds the pair table
//! and the depth-counting scanner as pure functions over a string plus byte
//! offsets, so it can be unit-tested in isolation from the editor's snapshot
//! type and can land without touching shared files.
//!
//! # What it provides
//!
//! - **The pair table** ([`PAIRS`]) and the lookups [`pair_for`],
//!   [`pair_mate`] and [`is_pair_char`].
//! - **The depth-counting scanner** — [`pair_from_open`], [`pair_from_close`]
//!   and [`enclosing_pair`], each bounded by a byte budget and each reporting
//!   one of three outcomes ([`MatchOutcome`]).
//!
//! # The nesting rule
//!
//! For a pair whose open and close **differ** (`(`/`)`), the scanner counts
//! depth and matches the **outermost** enclosing occurrence of the same
//! delimiter. On `(a(b))` the pair belonging to the outer `(` is the whole
//! `(a(b))`, not the inner `(b)`: surround-replace therefore replaces the
//! outer pair and leaves the inner one intact, so `(a(b))` with `mr([` becomes
//! `[a(b)]` and not `[a(b])`.
//!
//! For a **quote pair** (open == close: `'`, `"`, `` ` ``) there is no depth:
//! the pair is the **next unescaped** occurrence of the same character, and an
//! escaped quote (`\"`) is skipped.
//!
//! # Bounding
//!
//! Every scan takes a `max_bytes` budget and **never scans the whole buffer**.
//! The scan walks characters until the pair closes, the text ends, or the
//! budget runs out — in that order — and reports which of the three happened.
//! A scan that runs out of budget reports [`MatchOutcome::BudgetExhausted`]
//! rather than guessing. This mirrors `MAX_SET_VERB_READ_BYTES` and
//! `bounded_regex_window` in `src/tui/editor.rs`.
//!
//! # Deviation from the plan (§6.3 / §6.5)
//!
//! The plan names the crate helpers `leading_byte` / `next_char_boundary` as
//! the byte-stepping primitives. Those operate on `DocumentSnapshot`, and
//! using them here would make this module depend on the editor's document
//! type — which this work package forbids, because the module has to stay
//! dependency-free to be unit-testable in isolation and to land in parallel
//! with the other work packages. This module therefore does its own UTF-8
//! character-boundary stepping: forward with `str::chars()`, and backward with
//! the UTF-8 continuation-byte rule (`0b10xxxxxx`). The two agree — both land
//! on the same character boundaries — so a call site may use either. The
//! call-site concern is recorded here so it is not rediscovered.

use std::ops::Range;

// ── The pair table ───────────────────────────────────────────────────────────

/// One delimiter pair.
///
/// A pair's open and close may be the **same** character (the quote pairs).
/// The scanner treats that case differently — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    /// The opening delimiter.
    pub open: char,
    /// The closing delimiter. Equal to [`Pair::open`] for a quote pair.
    pub close: char,
}

impl Pair {
    /// A quote pair: the same character opens and closes it.
    pub const fn quote(ch: char) -> Self {
        Self {
            open: ch,
            close: ch,
        }
    }

    /// Whether the two delimiters are the same character.
    pub const fn is_quote(&self) -> bool {
        self.open == self.close
    }

    /// The byte range **between** the two delimiters of a pair found at
    /// `open`/`close` — the interior a text object selects.
    pub fn content_range(&self, open: usize, close: usize) -> Range<usize> {
        open + self.open.len_utf8()..close
    }

    /// The byte range covering the whole pair, delimiters included.
    pub fn full_range(&self, open: usize, close: usize) -> Range<usize> {
        open..close + self.close.len_utf8()
    }
}

/// The delimiters the editor knows, as pairs.
///
/// The set is Helix's and is the floor: a smaller set is defensible, a larger
/// one is scope creep. The last three are quote pairs — same character on both
/// sides.
pub const PAIRS: [Pair; 7] = [
    Pair {
        open: '(',
        close: ')',
    },
    Pair {
        open: '[',
        close: ']',
    },
    Pair {
        open: '{',
        close: '}',
    },
    Pair {
        open: '<',
        close: '>',
    },
    Pair::quote('\''),
    Pair::quote('"'),
    Pair::quote('`'),
];

/// The pair whose delimiter is `ch`, if `ch` is a known pair character.
///
/// A character that is both an opening and a closing delimiter (a quote)
/// resolves to its quote pair.
pub fn pair_for(ch: char) -> Option<Pair> {
    PAIRS
        .iter()
        .copied()
        .find(|pair| pair.open == ch || pair.close == ch)
}

/// The mate of `ch` — the other delimiter of its pair.
///
/// For a quote pair the mate is the character itself.
pub fn pair_mate(ch: char) -> Option<char> {
    PAIRS.iter().copied().find_map(|pair| {
        if pair.open == ch {
            Some(pair.close)
        } else if pair.close == ch {
            Some(pair.open)
        } else {
            None
        }
    })
}

/// Whether `ch` is a known pair character.
pub fn is_pair_char(ch: char) -> bool {
    pair_for(ch).is_some()
}

// ── The scanner ──────────────────────────────────────────────────────────────

/// What a pair scan concluded.
///
/// Three outcomes, not two: a caller has to be able to tell "there is no mate"
/// from "I was not allowed to look that far", because they refuse
/// differently. Unbalanced input is a **normal** state for a buffer being
/// typed into, so it is a refusal with its own reason, not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    /// The pair was found.
    Found {
        /// Byte offset of the opening delimiter.
        open: usize,
        /// Byte offset of the closing delimiter.
        close: usize,
    },
    /// The pair does not balance: the text ended without a matching
    /// delimiter, the delimiter under the cursor has no mate, or the anchor
    /// offset did not hold the expected delimiter.
    Unbalanced,
    /// The byte budget ran out before the scan could tell.
    BudgetExhausted,
}

impl MatchOutcome {
    /// Whether the scan found the pair.
    pub fn is_found(self) -> bool {
        matches!(self, MatchOutcome::Found { .. })
    }

    /// The two delimiter offsets, when the pair was found.
    pub fn offsets(self) -> Option<(usize, usize)> {
        match self {
            MatchOutcome::Found { open, close } => Some((open, close)),
            MatchOutcome::Unbalanced | MatchOutcome::BudgetExhausted => None,
        }
    }
}

/// The pair that starts at the opening delimiter at `open_offset`.
///
/// Walks forward counting depth, so the close it reports is the one that
/// balances the **outermost** run of the same delimiter opened at
/// `open_offset` — on `(a(b))` from offset 0 that is the final `)`, not the
/// inner one.
///
/// `max_bytes` bounds the bytes the scan may walk **beyond** the anchor
/// delimiter; the anchor is the caller's, and is not charged. A scan that runs
/// out of budget reports [`MatchOutcome::BudgetExhausted`].
pub fn pair_from_open(
    text: &str,
    open_offset: usize,
    pair: Pair,
    max_bytes: usize,
) -> MatchOutcome {
    let mut budget = Budget::new(max_bytes);
    scan_forward(text, open_offset, pair, &mut budget)
}

/// The pair that ends at the closing delimiter at `close_offset`.
///
/// Walks backward counting depth, so the open it reports is the one that
/// balances this close — on `((a))` from the final `)` that is the first `(`.
///
/// `max_bytes` bounds the bytes the scan may walk **beyond** the anchor
/// delimiter; the anchor is the caller's, and is not charged. A scan that runs
/// out of budget reports [`MatchOutcome::BudgetExhausted`].
pub fn pair_from_close(
    text: &str,
    close_offset: usize,
    pair: Pair,
    max_bytes: usize,
) -> MatchOutcome {
    let mut budget = Budget::new(max_bytes);
    scan_backward(text, close_offset, pair, &mut budget)
}

/// The **outermost** pair enclosing `cursor`.
///
/// The cursor must be strictly **inside** the pair: the search looks for an
/// opening delimiter before the cursor and a closing delimiter after it. A
/// cursor sitting on a delimiter is the caller's business — use
/// [`pair_from_open`] or [`pair_from_close`] for that.
///
/// For a pair whose open and close differ, the search walks backward and
/// keeps the **last** opening delimiter it meets at depth zero, which is the
/// outermost one that still encloses the cursor. On `(a(b))` with the cursor
/// on the `b` that is the outer `(a(b))` — the rule that makes surround
/// replace preserve nesting.
///
/// For a quote pair there is no nesting, so the enclosing pair is the
/// **nearest unescaped quote before the cursor** and the next unescaped quote
/// after it. A parity count of the unescaped quotes before the cursor decides
/// whether the cursor is inside a quoted run at all; a cursor that is not
/// inside one has no enclosing quote pair.
///
/// `max_bytes` bounds the bytes the scan may walk in total, across the
/// backward and the forward walk. A scan that runs out of budget reports
/// [`MatchOutcome::BudgetExhausted`].
pub fn enclosing_pair(text: &str, cursor: usize, pair: Pair, max_bytes: usize) -> MatchOutcome {
    let mut budget = Budget::new(max_bytes);
    let cursor = clamp_boundary(text, cursor);

    let open_offset = if pair.is_quote() {
        let Some((nearest, inside)) = nearest_quote_before(text, cursor, pair.open, &mut budget)
        else {
            return if budget.exhausted {
                MatchOutcome::BudgetExhausted
            } else {
                MatchOutcome::Unbalanced
            };
        };
        if !inside {
            return MatchOutcome::Unbalanced;
        }
        nearest
    } else {
        let Some(open) = outermost_open_before(text, cursor, pair, &mut budget) else {
            return if budget.exhausted {
                MatchOutcome::BudgetExhausted
            } else {
                MatchOutcome::Unbalanced
            };
        };
        open
    };

    match scan_forward(text, open_offset, pair, &mut budget) {
        MatchOutcome::Found { open, close } if close > cursor => {
            MatchOutcome::Found { open, close }
        }
        MatchOutcome::Found { .. } => MatchOutcome::Unbalanced,
        other => other,
    }
}

/// The **innermost** pair of `pair`'s kind enclosing `cursor`.
///
/// The mirror of [`enclosing_pair`], which finds the **outermost** one. The
/// difference matters whenever the same delimiter nests: on `(a(b))` with the
/// cursor on the `b`, the outermost paren pair is the whole `(a(b))` and the
/// innermost is `(b)` — and "the pair around the cursor" means the inner one.
///
/// The cursor must be strictly **inside** the pair; a cursor sitting on a
/// delimiter is the caller's business, as for [`enclosing_pair`].
///
/// `max_bytes` bounds the bytes the scan may walk in total, across the
/// backward and the forward walk. A scan that runs out of budget reports
/// [`MatchOutcome::BudgetExhausted`].
pub fn innermost_pair(text: &str, cursor: usize, pair: Pair, max_bytes: usize) -> MatchOutcome {
    let mut budget = Budget::new(max_bytes);
    innermost_pair_with(text, cursor, pair, &mut budget)
}

/// [`innermost_pair`] against a budget the caller shares.
fn innermost_pair_with(text: &str, cursor: usize, pair: Pair, budget: &mut Budget) -> MatchOutcome {
    let cursor = clamp_boundary(text, cursor);

    if pair.is_quote() {
        // A quote does not nest, so the innermost quote pair around the cursor
        // is the run the cursor is inside — and the parity of the unescaped
        // quotes before it is what says whether it is inside one at all.
        let Some((nearest, inside)) = nearest_quote_before(text, cursor, pair.open, budget) else {
            return if budget.exhausted {
                MatchOutcome::BudgetExhausted
            } else {
                MatchOutcome::Unbalanced
            };
        };
        if !inside {
            return MatchOutcome::Unbalanced;
        }
        return match scan_forward(text, nearest, pair, budget) {
            MatchOutcome::Found { open, close } if close > cursor => {
                MatchOutcome::Found { open, close }
            }
            MatchOutcome::Found { .. } => MatchOutcome::Unbalanced,
            other => other,
        };
    }

    // Walk backward and keep the **first** opening delimiter met at depth
    // zero: that is the nearest one, and so the innermost that can enclose the
    // cursor. A closing delimiter met on the way is counted, not matched —
    // it belongs to a pair that ends before the cursor.
    let mut depth = 0usize;
    let mut offset = cursor;
    while let Some(start) = prev_char_start(text, offset) {
        let len = offset - start;
        if !budget.charge(len) {
            return MatchOutcome::BudgetExhausted;
        }
        let Some(ch) = text
            .get(start..offset)
            .and_then(|slice| slice.chars().next())
        else {
            break;
        };
        if ch == pair.close {
            depth += 1;
        } else if ch == pair.open {
            if depth == 0 {
                return match scan_forward(text, start, pair, budget) {
                    MatchOutcome::Found { open, close } if close > cursor => {
                        MatchOutcome::Found { open, close }
                    }
                    MatchOutcome::Found { .. } => MatchOutcome::Unbalanced,
                    other => other,
                };
            }
            depth -= 1;
        }
        offset = start;
    }
    MatchOutcome::Unbalanced
}

/// The **innermost** pair of any kind enclosing `cursor`.
///
/// Helix's "closest pair" object. Every pair type is resolved on its own and
/// the **tightest** result wins, which is the innermost whatever the
/// delimiters happen to be: on `(a[b])` with the cursor on the `b`, the
/// bracket pair is the tighter one and is the answer.
///
/// The budget is **shared** across the types, so one call cannot cost more
/// than `max_bytes` in total however many pair types the table holds. A scan
/// that runs out of budget reports [`MatchOutcome::BudgetExhausted`].
pub fn closest_pair(text: &str, cursor: usize, max_bytes: usize) -> MatchOutcome {
    let mut budget = Budget::new(max_bytes);
    let cursor = clamp_boundary(text, cursor);
    let mut best: Option<(usize, usize)> = None;
    for pair in PAIRS {
        let MatchOutcome::Found { open, close } =
            innermost_pair_with(text, cursor, pair, &mut budget)
        else {
            continue;
        };
        let tighter = match best {
            None => true,
            Some((best_open, best_close)) => (close - open) < (best_close - best_open),
        };
        if tighter {
            best = Some((open, close));
        }
    }
    if let Some((open, close)) = best {
        return MatchOutcome::Found { open, close };
    }
    if budget.exhausted {
        MatchOutcome::BudgetExhausted
    } else {
        MatchOutcome::Unbalanced
    }
}

// ── The budget ───────────────────────────────────────────────────────────────
/// A byte budget a scan draws down as it walks.
///
/// The budget is the bound the roadmap's exit condition asks for: no object
/// resolution may scan more than a bounded window, and a document larger than
/// the budget is **refused**, not scanned. `exhausted` records that the budget
/// ran out, so a search that finds nothing can still say whether it looked
/// everywhere it was allowed to look.
struct Budget {
    remaining: usize,
    exhausted: bool,
}

impl Budget {
    fn new(max_bytes: usize) -> Self {
        Self {
            remaining: max_bytes,
            exhausted: false,
        }
    }

    /// Charge `len` bytes. `false` means the budget could not cover them.
    fn charge(&mut self, len: usize) -> bool {
        if len > self.remaining {
            self.exhausted = true;
            return false;
        }
        self.remaining -= len;
        true
    }
}

// ── Character stepping ───────────────────────────────────────────────────────

/// The character starting at `offset`, with its byte length.
///
/// `offset` must be a character boundary; an offset that is not one yields
/// `None` rather than a panic, so a caller mistake cannot take the editor
/// down.
fn char_at(text: &str, offset: usize) -> Option<(char, usize)> {
    text.get(offset..)?
        .chars()
        .next()
        .map(|ch| (ch, ch.len_utf8()))
}

/// The start of the character that ends at `offset`.
///
/// Walks back over UTF-8 continuation bytes (`0b10xxxxxx`), so it lands on a
/// character boundary in at most three steps for valid UTF-8. This is the
/// backward twin of the forward step: **neither ever splits a character**.
fn prev_char_start(text: &str, offset: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let mut start = offset.checked_sub(1)?;
    while start > 0 && (bytes[start] & 0xC0) == 0x80 {
        start -= 1;
    }
    if !text.is_char_boundary(start) {
        return None;
    }
    Some(start)
}

/// The nearest character boundary at or before `offset`.
fn clamp_boundary(text: &str, offset: usize) -> usize {
    let mut candidate = offset.min(text.len());
    while candidate > 0 && !text.is_char_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Whether the character at `offset` is escaped by a preceding backslash.
///
/// An odd number of backslashes immediately before the character escapes it;
/// an even number does not (`\\"` is a literal backslash followed by a
/// delimiter).
fn is_escaped(text: &str, offset: usize) -> bool {
    let bytes = text.as_bytes();
    let mut backslashes = 0usize;
    let mut index = offset;
    while index > 0 && bytes[index - 1] == b'\\' {
        backslashes += 1;
        index -= 1;
    }
    backslashes % 2 == 1
}

// ── The scans ────────────────────────────────────────────────────────────────

/// Forward scan from an opening delimiter.
fn scan_forward(text: &str, open_offset: usize, pair: Pair, budget: &mut Budget) -> MatchOutcome {
    let Some((open_char, open_len)) = char_at(text, open_offset) else {
        return MatchOutcome::Unbalanced;
    };
    if open_char != pair.open {
        return MatchOutcome::Unbalanced;
    }

    let mut offset = open_offset + open_len;
    if pair.is_quote() {
        // A quote pair is the next matching quote, not a nesting depth.
        loop {
            let Some((ch, len)) = char_at(text, offset) else {
                return MatchOutcome::Unbalanced;
            };
            if !budget.charge(len) {
                return MatchOutcome::BudgetExhausted;
            }
            if ch == pair.open && !is_escaped(text, offset) {
                return MatchOutcome::Found {
                    open: open_offset,
                    close: offset,
                };
            }
            offset += len;
        }
    }

    let mut depth = 0usize;
    loop {
        let Some((ch, len)) = char_at(text, offset) else {
            return MatchOutcome::Unbalanced;
        };
        if !budget.charge(len) {
            return MatchOutcome::BudgetExhausted;
        }
        if ch == pair.open {
            depth += 1;
        } else if ch == pair.close {
            if depth == 0 {
                return MatchOutcome::Found {
                    open: open_offset,
                    close: offset,
                };
            }
            depth -= 1;
        }
        offset += len;
    }
}

/// Backward scan from a closing delimiter.
fn scan_backward(text: &str, close_offset: usize, pair: Pair, budget: &mut Budget) -> MatchOutcome {
    let Some((close_char, _)) = char_at(text, close_offset) else {
        return MatchOutcome::Unbalanced;
    };
    if close_char != pair.close {
        return MatchOutcome::Unbalanced;
    }

    let mut offset = close_offset;
    if pair.is_quote() {
        // A quote pair is the previous matching quote, not a nesting depth.
        loop {
            let Some(start) = prev_char_start(text, offset) else {
                return MatchOutcome::Unbalanced;
            };
            let len = offset - start;
            if !budget.charge(len) {
                return MatchOutcome::BudgetExhausted;
            }
            let Some(ch) = text
                .get(start..offset)
                .and_then(|slice| slice.chars().next())
            else {
                return MatchOutcome::Unbalanced;
            };
            if ch == pair.close && !is_escaped(text, start) {
                return MatchOutcome::Found {
                    open: start,
                    close: close_offset,
                };
            }
            offset = start;
        }
    }

    let mut depth = 0usize;
    loop {
        let Some(start) = prev_char_start(text, offset) else {
            return MatchOutcome::Unbalanced;
        };
        let len = offset - start;
        if !budget.charge(len) {
            return MatchOutcome::BudgetExhausted;
        }
        let Some(ch) = text
            .get(start..offset)
            .and_then(|slice| slice.chars().next())
        else {
            return MatchOutcome::Unbalanced;
        };
        if ch == pair.close {
            depth += 1;
        } else if ch == pair.open {
            if depth == 0 {
                return MatchOutcome::Found {
                    open: start,
                    close: close_offset,
                };
            }
            depth -= 1;
        }
        offset = start;
    }
}

/// The outermost opening delimiter that encloses `cursor`.
///
/// Walks backward, counting the closing delimiters it passes, and keeps the
/// **last** opening delimiter it meets at depth zero — the outermost one that
/// still encloses the cursor. A closing delimiter met at depth zero would
/// mean the cursor is outside that pair, so it is counted rather than
/// matched.
fn outermost_open_before(
    text: &str,
    cursor: usize,
    pair: Pair,
    budget: &mut Budget,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut outermost: Option<usize> = None;
    let mut offset = cursor;
    while let Some(start) = prev_char_start(text, offset) {
        let len = offset - start;
        if !budget.charge(len) {
            return None;
        }
        let Some(ch) = text
            .get(start..offset)
            .and_then(|slice| slice.chars().next())
        else {
            break;
        };
        if ch == pair.close {
            depth += 1;
        } else if ch == pair.open {
            if depth == 0 {
                outermost = Some(start);
            } else {
                depth -= 1;
            }
        }
        offset = start;
    }
    outermost
}

/// The nearest unescaped quote before `cursor`, and whether the cursor is
/// inside a quoted run.
///
/// The quotes alternate open/close, so the parity of the unescaped quotes
/// before the cursor decides whether the nearest one **opened** the run the
/// cursor is in. A cursor that is not inside a quoted run has no enclosing
/// quote pair.
fn nearest_quote_before(
    text: &str,
    cursor: usize,
    quote: char,
    budget: &mut Budget,
) -> Option<(usize, bool)> {
    let mut nearest: Option<usize> = None;
    let mut count = 0usize;
    let mut offset = cursor;
    while let Some(start) = prev_char_start(text, offset) {
        let len = offset - start;
        if !budget.charge(len) {
            return None;
        }
        let Some(ch) = text
            .get(start..offset)
            .and_then(|slice| slice.chars().next())
        else {
            break;
        };
        if ch == quote && !is_escaped(text, start) {
            nearest = Some(start);
            count += 1;
        }
        offset = start;
    }
    nearest.map(|offset| (offset, count % 2 == 1))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PAREN: Pair = Pair {
        open: '(',
        close: ')',
    };
    const BRACKET: Pair = Pair {
        open: '[',
        close: ']',
    };
    const DOUBLE_QUOTE: Pair = Pair::quote('"');

    /// A budget large enough that no test here is accidentally bounded.
    const PLENTY: usize = 4096;

    // ── the pair table ──

    #[test]
    fn the_table_holds_seven_pairs() {
        assert_eq!(PAIRS.len(), 7);
    }

    #[test]
    fn the_table_answers_what_the_mate_of_a_character_is() {
        assert_eq!(pair_mate('('), Some(')'));
        assert_eq!(pair_mate(')'), Some('('));
        assert_eq!(pair_mate('['), Some(']'));
        assert_eq!(pair_mate(']'), Some('['));
        assert_eq!(pair_mate('{'), Some('}'));
        assert_eq!(pair_mate('}'), Some('{'));
        assert_eq!(pair_mate('<'), Some('>'));
        assert_eq!(pair_mate('>'), Some('<'));
        assert_eq!(pair_mate('\''), Some('\''));
        assert_eq!(pair_mate('"'), Some('"'));
        assert_eq!(pair_mate('`'), Some('`'));
    }

    #[test]
    fn the_table_answers_whether_a_character_is_a_pair_character() {
        for pair in PAIRS {
            assert!(is_pair_char(pair.open));
            assert!(is_pair_char(pair.close));
        }
        assert!(!is_pair_char('a'));
        assert!(!is_pair_char(' '));
        assert!(!is_pair_char('\n'));
        assert!(!is_pair_char('§'));
    }

    #[test]
    fn a_character_outside_the_table_has_no_mate() {
        assert_eq!(pair_mate('a'), None);
        assert_eq!(pair_mate(' '), None);
        assert_eq!(pair_for('a'), None);
    }

    #[test]
    fn the_table_resolves_a_character_to_its_pair() {
        assert_eq!(pair_for('('), Some(PAREN));
        assert_eq!(pair_for(')'), Some(PAREN));
        assert_eq!(pair_for('['), Some(BRACKET));
        assert_eq!(pair_for('"'), Some(DOUBLE_QUOTE));
    }

    #[test]
    fn a_quote_pair_opens_and_closes_with_the_same_character() {
        assert!(DOUBLE_QUOTE.is_quote());
        assert!(!PAREN.is_quote());
        assert_eq!(Pair::quote('`').open, Pair::quote('`').close);
    }

    // ── empty input ──

    #[test]
    fn an_empty_text_has_no_pair() {
        assert_eq!(
            pair_from_open("", 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            pair_from_close("", 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            enclosing_pair("", 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            enclosing_pair("", 0, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    // ── a balanced pair ──

    #[test]
    fn a_balanced_pair_is_found_in_both_directions() {
        let text = "(abc)";
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
        assert_eq!(
            pair_from_close(text, 4, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
    }

    #[test]
    fn a_balanced_pair_reports_its_content_range() {
        let text = "(abc)";
        let (open, close) = pair_from_open(text, 0, PAREN, PLENTY).offsets().unwrap();
        assert_eq!(PAREN.content_range(open, close), 1..4);
        assert_eq!(PAREN.full_range(open, close), 0..5);
        assert_eq!(&text[PAREN.content_range(open, close)], "abc");
    }

    #[test]
    fn a_pair_nested_in_another_pair_keeps_its_own_mate() {
        let text = "(a(b)c)";
        // The inner pair, found from the inner delimiter.
        assert_eq!(
            pair_from_open(text, 2, PAREN, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
        // The outer pair, found from the outer delimiter.
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 6 }
        );
        // And backwards from the outer close.
        assert_eq!(
            pair_from_close(text, 6, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 6 }
        );
    }

    // ── unbalanced input ──

    #[test]
    fn an_open_with_no_close_is_unbalanced() {
        let text = "(abc";
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            enclosing_pair(text, 2, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn a_close_with_no_open_is_unbalanced() {
        let text = "abc)";
        assert_eq!(
            pair_from_close(text, 3, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            enclosing_pair(text, 1, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn an_unbalanced_pair_is_a_refusal_not_an_error() {
        let text = "(abc";
        let outcome = pair_from_open(text, 0, PAREN, PLENTY);
        assert!(!outcome.is_found());
        assert_eq!(outcome.offsets(), None);
    }

    // ── multi-byte input ──

    #[test]
    fn a_pair_around_non_ascii_text_is_found() {
        let text = "(日本語)";
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 10 }
        );
        assert_eq!(
            pair_from_close(text, 10, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 10 }
        );
    }

    #[test]
    fn a_multi_byte_delimiter_is_matched_without_splitting_it() {
        // `«` and `»` are two bytes each: a scanner that stepped one byte at
        // a time would land in the middle of them.
        let text = "«日本語»";
        let pair = Pair {
            open: '«',
            close: '»',
        };
        // `«` is two bytes, `日本語` is nine, `»` is two: the close is at 11.
        assert_eq!(
            pair_from_open(text, 0, pair, PLENTY),
            MatchOutcome::Found { open: 0, close: 11 }
        );
        assert_eq!(
            pair_from_close(text, 11, pair, PLENTY),
            MatchOutcome::Found { open: 0, close: 11 }
        );
        assert_eq!(pair.content_range(0, 11), 2..11);
        assert_eq!(&text[pair.content_range(0, 11)], "日本語");
    }

    #[test]
    fn a_multi_byte_quote_pair_is_matched() {
        // The bytes are: `«` `日本語` `»` ` ` `«` `文字` `»`.
        let text = "«日本語» «文字»";
        // A quote pair matches the **next occurrence of the same character**,
        // so the `«` pair runs from the first `«` to the second one.
        let open_quote = Pair::quote('«');
        assert_eq!(
            pair_from_open(text, 0, open_quote, PLENTY),
            MatchOutcome::Found { open: 0, close: 14 }
        );
        assert_eq!(
            pair_from_close(text, 14, open_quote, PLENTY),
            MatchOutcome::Found { open: 0, close: 14 }
        );
        // And the `»` pair runs from the first `»` to the second one.
        let close_quote = Pair::quote('»');
        assert_eq!(
            pair_from_open(text, 11, close_quote, PLENTY),
            MatchOutcome::Found {
                open: 11,
                close: 22
            }
        );
    }

    // ── the outermost-pair rule ──

    #[test]
    fn the_outermost_pair_is_the_match() {
        // `(a(b))` with the cursor on the `b`: the enclosing pair is the
        // outer one, so surround-replace keeps the inner pair intact and
        // yields `[a(b)]` rather than `[a(b])`.
        let text = "(a(b))";
        let outcome = enclosing_pair(text, 3, PAREN, PLENTY);
        assert_eq!(outcome, MatchOutcome::Found { open: 0, close: 5 });
        let (open, close) = outcome.offsets().unwrap();
        assert_eq!(&text[PAREN.content_range(open, close)], "a(b)");
        // The inner pair is still there, untouched.
        assert_eq!(
            pair_from_open(text, 2, PAREN, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
    }

    #[test]
    fn the_outermost_pair_is_the_match_from_any_inner_cursor() {
        let text = "(a(b))";
        for cursor in 1..=4 {
            assert_eq!(
                enclosing_pair(text, cursor, PAREN, PLENTY),
                MatchOutcome::Found { open: 0, close: 5 },
                "cursor {cursor}"
            );
        }
    }

    #[test]
    fn the_outermost_pair_is_found_from_the_outer_delimiter_too() {
        let text = "(a(b))";
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 5 }
        );
    }

    #[test]
    fn a_cursor_on_a_delimiter_is_not_inside_a_pair() {
        // The cursor must be strictly inside: the delimiter it sits on
        // belongs to the caller to resolve.
        let text = "(a(b))";
        assert_eq!(
            enclosing_pair(text, 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    // ── the quote rule ──

    #[test]
    fn a_quote_pair_is_the_next_unescaped_quote() {
        let text = "\"abc\" def";
        assert_eq!(
            pair_from_open(text, 0, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
        assert_eq!(
            pair_from_close(text, 4, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
    }

    #[test]
    fn an_escaped_quote_is_skipped() {
        // The bytes are: `"` `a` `\` `"` `b` `"` ` ` `c`.
        let text = "\"a\\\"b\" c";
        // The quote at offset 3 is escaped, so the pair closes at the
        // unescaped quote at offset 5.
        assert_eq!(
            pair_from_open(text, 0, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 5 }
        );
        assert_eq!(
            pair_from_close(text, 5, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 5 }
        );
    }

    #[test]
    fn a_quote_pair_does_not_nest() {
        // Two quoted runs: the first pair closes at the second quote, it
        // does not count depth the way a bracket pair does.
        let text = "\"a\" \"b\"";
        assert_eq!(
            pair_from_open(text, 0, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 2 }
        );
        assert_eq!(
            pair_from_open(text, 4, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 4, close: 6 }
        );
    }

    #[test]
    fn a_cursor_inside_a_quoted_run_has_that_run_as_its_pair() {
        let text = "\"abc\" def";
        assert_eq!(
            enclosing_pair(text, 2, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
    }

    #[test]
    fn a_cursor_outside_every_quoted_run_has_no_quote_pair() {
        let text = "\"abc\" def";
        assert_eq!(
            enclosing_pair(text, 6, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    // ── the budget rule ──

    #[test]
    fn a_match_beyond_the_budget_is_budget_exhausted_not_found() {
        let text = "(aaaaaaaaaaaaaaaaaaaa)";
        // The close is 21 bytes away; a budget of 4 cannot reach it.
        assert_eq!(
            pair_from_open(text, 0, PAREN, 4),
            MatchOutcome::BudgetExhausted
        );
        // The same text with a budget that reaches the close is found.
        assert_eq!(
            pair_from_open(text, 0, PAREN, 21),
            MatchOutcome::Found { open: 0, close: 21 }
        );
    }

    #[test]
    fn a_backward_match_beyond_the_budget_is_budget_exhausted() {
        let text = "(aaaaaaaaaaaaaaaaaaaa)";
        assert_eq!(
            pair_from_close(text, 21, PAREN, 4),
            MatchOutcome::BudgetExhausted
        );
    }

    #[test]
    fn an_enclosing_pair_beyond_the_budget_is_budget_exhausted() {
        let text = "(aaaaaaaaaaaaaaaaaaaa)";
        assert_eq!(
            enclosing_pair(text, 10, PAREN, 4),
            MatchOutcome::BudgetExhausted
        );
    }

    #[test]
    fn a_zero_budget_finds_nothing() {
        let text = "(abc)";
        assert_eq!(
            pair_from_open(text, 0, PAREN, 0),
            MatchOutcome::BudgetExhausted
        );
        assert_eq!(
            pair_from_close(text, 4, PAREN, 0),
            MatchOutcome::BudgetExhausted
        );
    }

    #[test]
    fn an_unbalanced_pair_inside_the_budget_is_still_unbalanced() {
        // The budget is not reached here, so the refusal is about the
        // delimiters, not about the window.
        let text = "(abc";
        assert_eq!(
            pair_from_open(text, 0, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    // ── misuses ──

    #[test]
    fn an_anchor_that_does_not_hold_the_delimiter_is_unbalanced() {
        let text = "(abc)";
        assert_eq!(
            pair_from_open(text, 1, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            pair_from_close(text, 1, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn an_offset_past_the_end_is_unbalanced_rather_than_a_panic() {
        let text = "(abc)";
        assert_eq!(
            pair_from_open(text, 99, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            pair_from_close(text, 99, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
        assert_eq!(
            enclosing_pair(text, 99, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn a_cursor_inside_a_character_is_clamped_to_a_boundary() {
        // Offset 2 is inside `日` (bytes 1..=3). It must not panic and must
        // not split the character.
        let text = "(日本語)";
        let outcome = enclosing_pair(text, 2, PAREN, PLENTY);
        assert_eq!(outcome, MatchOutcome::Found { open: 0, close: 10 });
    }

    // ── the innermost pair ──

    #[test]
    fn the_innermost_pair_is_the_nearest_one_that_encloses_the_cursor() {
        // `(a(b))` — the outer pair is `(a(b))` and the inner one is `(b)`.
        // The cursor is on the `b`, so the inner one is the answer.
        let text = "(a(b))";
        assert_eq!(
            innermost_pair(text, 3, PAREN, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
        // And the **outermost** scan still says what it always said, because
        // the two rules are different questions.
        assert_eq!(
            enclosing_pair(text, 3, PAREN, PLENTY),
            MatchOutcome::Found { open: 0, close: 5 }
        );
    }

    #[test]
    fn the_innermost_pair_of_a_type_the_cursor_is_outside_of_is_unbalanced() {
        let text = "f(a)b";
        // The `b` is after the closed pair, so no paren pair encloses it.
        assert_eq!(
            innermost_pair(text, 5, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn the_innermost_pair_of_an_unclosed_delimiter_is_unbalanced() {
        let text = "(abc";
        assert_eq!(
            innermost_pair(text, 1, PAREN, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    #[test]
    fn the_innermost_pair_of_a_quote_needs_an_odd_number_of_quotes_before_it() {
        let text = "\"abc\"";
        // Inside the run: the opening quote is the opener.
        assert_eq!(
            innermost_pair(text, 2, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Found { open: 0, close: 4 }
        );
        // Outside it: the quotes before the cursor balance, so there is no
        // enclosing run.
        assert_eq!(
            innermost_pair(text, 6, DOUBLE_QUOTE, PLENTY),
            MatchOutcome::Unbalanced
        );
    }

    // ── the closest pair ──

    #[test]
    fn the_closest_pair_is_the_tightest_one_of_any_kind() {
        // `(a[b])` with the cursor on the `b`: the bracket pair is the
        // tighter one, and it is the answer.
        let text = "(a[b])";
        assert_eq!(
            closest_pair(text, 3, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
    }

    #[test]
    fn the_closest_pair_is_the_innermost_one_of_a_single_kind() {
        let text = "(a(b))";
        assert_eq!(
            closest_pair(text, 3, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
    }

    #[test]
    fn the_closest_pair_of_text_with_no_pair_around_the_cursor_is_unbalanced() {
        let text = "abc";
        assert_eq!(closest_pair(text, 1, PLENTY), MatchOutcome::Unbalanced);
    }

    #[test]
    fn the_closest_pair_shares_one_budget_across_every_type() {
        // Seven pair types, one budget: the whole call may not cost more
        // than the budget says, however many types it tried.
        let text = "(a[b])";
        assert_eq!(
            closest_pair(text, 3, PLENTY),
            MatchOutcome::Found { open: 2, close: 4 }
        );
        // A budget too small to walk the text reports the exhaustion rather
        // than a wrong answer.
        assert_eq!(closest_pair(text, 3, 1), MatchOutcome::BudgetExhausted);
    }
}
