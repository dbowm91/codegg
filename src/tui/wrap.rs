//! Shared display-width-aware word wrapping for TUI text surfaces.
//!
//! `ratatui::widgets::Paragraph::line_count` is gated behind the unstable
//! `rendered-line-info` feature, so row-accurate wrapped scrolling cannot be
//! delegated to the widget. Both the chat viewport and the read-only log
//! dialog therefore pre-wrap their content with these helpers: the same
//! algorithm decides both how many rows a string occupies and which rows to
//! draw, which is what keeps scroll offsets aligned with what is on screen.

/// Return the number of visual lines produced by [`wrap_to_strings`]. Keeping
/// estimation on the rendering path prevents scroll/layout drift when display
/// width differs from the number of Unicode scalar values.
pub fn wrap_count(s: &str, width: u16) -> usize {
    wrap_to_strings(s, width).len()
}

/// Split a string into wrapped lines of at most `width` display columns each.
/// Returns one entry per visual line (preserves explicit newlines as line
/// breaks). Matches the line-counting semantics of [`wrap_count`].
///
/// Word-wrap is greedy: words that fit on the current line stay there; a
/// word that would overflow the current line starts a new line. Words
/// longer than `width` (URLs, long paths) are hard-broken at character
/// boundaries to avoid overflowing the render area. Trailing whitespace is
/// trimmed from each output line so the wrap doesn't double-space between
/// words or leave stray spaces at line ends.
pub fn wrap_to_strings(s: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    if width == 0 || s.is_empty() {
        return vec![s.to_string()];
    }
    fn hard_break_word(word: &str, width: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut chunk = String::new();
        let mut chunk_width = 0;
        for ch in word.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch_width > 0 && chunk_width + ch_width > width && !chunk.is_empty() {
                chunks.push(std::mem::take(&mut chunk));
                chunk_width = 0;
            }
            chunk.push(ch);
            chunk_width += ch_width;
        }
        if !chunk.is_empty() || chunks.is_empty() {
            chunks.push(chunk);
        }
        chunks
    }

    fn wrap_logical_line(line: &str, width: usize) -> Vec<String> {
        let mut wrapped = Vec::new();
        let mut current = String::new();
        let mut current_width = 0;
        let mut word = String::new();

        let flush_word = |word: &mut String,
                          current: &mut String,
                          current_width: &mut usize,
                          wrapped: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            let chunks = hard_break_word(word, width);
            let word_width = chunks
                .first()
                .map(|chunk| {
                    chunk
                        .chars()
                        .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                        .sum::<usize>()
                })
                .unwrap_or(0);
            let is_hard_break = chunks.len() > 1;

            if current.is_empty() {
                // Start a new line below. The final chunk remains current so
                // a following word can still share its line.
            } else if !is_hard_break && *current_width + 1 + word_width <= width {
                current.push(' ');
                *current_width += 1;
            } else {
                wrapped.push(std::mem::take(current));
                *current_width = 0;
            }

            for (idx, chunk) in chunks.iter().enumerate() {
                if idx + 1 == chunks.len() {
                    current.push_str(chunk);
                    *current_width += chunk
                        .chars()
                        .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                        .sum::<usize>();
                } else {
                    wrapped.push(chunk.clone());
                }
            }
            word.clear();
        };

        for ch in line.chars() {
            if ch.is_whitespace() {
                flush_word(&mut word, &mut current, &mut current_width, &mut wrapped);
            } else {
                word.push(ch);
            }
        }
        flush_word(&mut word, &mut current, &mut current_width, &mut wrapped);
        wrapped.push(current);
        wrapped
    }

    let mut lines = Vec::new();
    let logical_lines = s.strip_suffix('\n').unwrap_or(s).split('\n');
    for logical_line in logical_lines {
        lines.extend(wrap_logical_line(logical_line, width));
    }
    lines
}

/// Shorten `s` to at most `width` display columns, appending a horizontal
/// ellipsis when anything was removed.
///
/// Display width (not char count) is what matters in a terminal, so a CJK or
/// emoji-heavy task title is measured by the columns it actually occupies.
/// Returns the input unchanged when it already fits or `width` is 0.
pub fn ellipsize_to_width(s: &str, width: usize) -> String {
    if width == 0 || s.is_empty() {
        return s.to_string();
    }
    let current = unicode_width::UnicodeWidthStr::width(s);
    if current <= width {
        return s.to_string();
    }
    const ELLIPSIS: char = '\u{2026}';
    let budget = width.saturating_sub(ELLIPSIS.len_utf8());
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push(ELLIPSIS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_line_is_one_row() {
        assert_eq!(wrap_count("hello world", 80), 1);
        assert_eq!(wrap_count("", 80), 1);
    }

    #[test]
    fn zero_width_is_passed_through_unwrapped() {
        // Ratatui treats a zero-width area as nothing to draw; returning the
        // input unchanged keeps callers free of special cases.
        let long = "x".repeat(500);
        assert_eq!(wrap_to_strings(&long, 0), vec![long]);
    }

    #[test]
    fn count_matches_wrapping_output() {
        for w in [1u16, 5, 12, 20, 80] {
            let s = "the quick brown fox jumps over the lazy dog 0123456789";
            assert_eq!(wrap_to_strings(s, w).len(), wrap_count(s, w));
        }
    }

    #[test]
    fn no_wrapped_row_exceeds_the_width() {
        let s = "Investigate request-direction stream event capture semantics end to end";
        for w in [8u16, 17, 40] {
            for row in wrap_to_strings(s, w) {
                let width = unicode_width::UnicodeWidthStr::width(row.as_str());
                assert!(
                    width <= w as usize,
                    "row of display width {width} exceeded width {w}: {row:?}"
                );
            }
        }
    }

    #[test]
    fn ellipsize_respects_display_width() {
        assert_eq!(ellipsize_to_width("short", 10), "short");
        // Wide glyphs are measured by columns, so three of them already
        // exceed a four-column budget and get cut, not kept whole.
        for w in [4usize, 6, 10] {
            let out = ellipsize_to_width("\u{4f60}\u{597d}\u{4e16}\u{754c}", w);
            assert!(
                unicode_width::UnicodeWidthStr::width(out.as_str()) <= w,
                "ellipsized output of width {w} overflowed: {out:?}"
            );
        }
        assert!(ellipsize_to_width("abcdefghij", 5).ends_with('\u{2026}'));
    }
}
