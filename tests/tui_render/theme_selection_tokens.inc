//! The two selection *foregrounds* are derived, and every bundled theme must
//! survive that derivation with a legible result.
//!
//! ## Why these tokens exist at all
//!
//! A selected row is a filled surface. Painting text on it needs a foreground
//! that reads against **that fill**, which is a different question from "what
//! colour is the theme's body text". Eight render paths answered it with
//! `Color::White` — a constant that is unreadable on a light theme and blind to
//! the theme entirely — and `Theme::selection_style` answered it with
//! `fg(primary)`, which paints the accent as foreground *on a different fill*.
//!
//! ## Why the token is derived rather than read from Halloy
//!
//! The Halloy format declares `buttons.primary.foreground_selected`. Sourcing
//! from it would be the obvious implementation and it would be wrong:
//! **`foreground_selected` is defined by 0 of the 50 bundled themes**, while
//! `background_selected` and `buffer.selection` are defined by all 50. A read
//! of that slot returns `None` for every real theme and every theme authored
//! against Halloy upstream, so the token would look correct in review and be
//! empty in the product.
//!
//! [`no_bundled_theme_defines_foreground_selected`] is the regression guard for
//! that. It exists because the "simplification" is tempting, local, and
//! compiles: swapping the derivation for the declared slot removes code and
//! changes no test that does not happen to be this one.
//!
//! ## What "no new required key" is proven by
//!
//! [`every_bundled_theme_still_ingests_with_no_new_required_key`] parses all 50
//! bundled themes and checks each one's `id` still derives from its file stem.
//! A new *required* Halloy key would break the parse outright; one that silently
//! changed id derivation would break this. Both are the failure this milestone's
//! hard constraint forbids, and neither is covered by "the sweep passed".

use codegg::theme::color::Rgb;
use codegg::theme::halloy::parse_halloy_theme;
use codegg::theme::registry::{builtin_fallback, BUILTIN_THEME_FILES};
use codegg::theme::schema::{derive_text_on, MIN_DERIVED_TEXT_RATIO};

/// The bar a derived foreground must clear against the fill it is drawn on.
/// Same value as `validate.rs`'s `SELECTION_TEXT_RATIO`, restated here so a
/// change to either one without the other fails this test rather than silently
/// weakening the contract.
const REQUIRED_RATIO: f64 = 3.0;

/// Parse one bundled theme the way the registry does.
fn bundled(entry: &codegg::theme::registry::BundledTheme) -> codegg::theme::schema::SemanticTheme {
    let fallback = builtin_fallback();
    let file = format!("{}.toml", entry.id);
    let (theme, diagnostics) =
        parse_halloy_theme(entry.content, Some(std::path::Path::new(&file)), &fallback)
            .unwrap_or_else(|e| panic!("bundled theme {} failed to parse: {e}", entry.id));
    for d in &diagnostics {
        assert!(
            d.level != codegg::theme::validate::ThemeDiagnosticLevel::Error,
            "bundled theme {} produced an error diagnostic: {:?}",
            entry.id,
            d.message
        );
    }
    theme
}

/// The sweep: every bundled theme, both derived tokens, against the surface each
/// is actually painted on.
#[test]
fn every_bundled_theme_derives_a_legible_selection_foreground() {
    assert!(
        BUILTIN_THEME_FILES.len() >= 50,
        "the sweep is only meaningful over the full bundled set; found {}",
        BUILTIN_THEME_FILES.len()
    );
    let mut worst_selection = f64::INFINITY;
    let mut worst_accent = f64::INFINITY;
    let mut worst_selection_at = "";
    let mut worst_accent_at = "";

    for entry in BUILTIN_THEME_FILES {
        let theme = bundled(entry);

        let selection_ratio = theme.ui.selection_text.contrast_ratio(theme.ui.selection);
        let accent_ratio = theme.ui.accent_text.contrast_ratio(theme.ui.accent_primary);

        assert!(
            selection_ratio >= REQUIRED_RATIO,
            "{}: selection_text {} reads {:.2}:1 on its selection fill {}, \
             below the {:.1}:1 bar — a selected row in this theme would be \
             unreadable",
            entry.id,
            theme.ui.selection_text,
            selection_ratio,
            theme.ui.selection,
            REQUIRED_RATIO
        );
        assert!(
            accent_ratio >= REQUIRED_RATIO,
            "{}: accent_text {} reads {:.2}:1 on its accent fill {}, below the \
             {:.1}:1 bar",
            entry.id,
            theme.ui.accent_text,
            accent_ratio,
            theme.ui.accent_primary,
            REQUIRED_RATIO
        );

        if selection_ratio < worst_selection {
            worst_selection = selection_ratio;
            worst_selection_at = entry.id;
        }
        if accent_ratio < worst_accent {
            worst_accent = accent_ratio;
            worst_accent_at = entry.id;
        }
    }

    println!(
        "worst selection_text {worst_selection:.2}:1 on {worst_selection_at}; \
         worst accent_text {worst_accent:.2}:1 on {worst_accent_at}"
    );
}

/// The measurement that justifies deriving instead of reading.
///
/// If this ever starts failing, the Halloy ecosystem has grown a convention and
/// the derivation can be reconsidered — but that is a *decision to re-read the
/// format*, not a cleanup. Until then, a `foreground_selected` read is dead code
/// that returns `None` for every real theme.
#[test]
fn no_bundled_theme_defines_foreground_selected() {
    let mut with_background_selected = 0;
    let mut with_selection = 0;
    let mut with_foreground_selected = 0;

    for entry in BUILTIN_THEME_FILES {
        if entry.content.contains("foreground_selected") {
            with_foreground_selected += 1;
        }
        if entry.content.contains("background_selected") {
            with_background_selected += 1;
        }
        if entry.content.contains("selection") {
            with_selection += 1;
        }
    }

    let total = BUILTIN_THEME_FILES.len();
    println!(
        "of {total} bundled themes: background_selected {with_background_selected}, \
         selection {with_selection}, foreground_selected {with_foreground_selected}"
    );
    assert_eq!(
        with_background_selected, total,
        "background_selected is the slot every bundled theme does set; if it stops \
         being universal the accent/selection fallbacks need re-measuring too"
    );
    assert_eq!(
        with_foreground_selected, 0,
        "a bundled theme now sets `foreground_selected`. Do NOT 'simplify' the \
         derivation into a read of that slot on the strength of one theme — \
         re-measure how many set it, and decide whether reading it (with the \
         derivation as fallback) beats deriving unconditionally. \
         {with_foreground_selected}/{total} set it now."
    );
}

/// The hard constraint, in parse form: a theme that ingests today ingests after
/// this milestone, unchanged.
#[test]
fn every_bundled_theme_still_ingests_with_no_new_required_key() {
    for entry in BUILTIN_THEME_FILES {
        let theme = bundled(entry);
        assert_eq!(
            theme.id, entry.id,
            "{} no longer derives its id from its file stem — id derivation is \
             load-bearing for the registry key",
            entry.id
        );
        assert_eq!(
            theme.name, entry.id,
            "{} changed its display name",
            entry.id
        );
    }
}

/// The derivation rule itself, on the four cases that separate it from a guess.
#[test]
fn the_derivation_picks_whichever_body_colour_reads_on_the_surface() {
    let white = Rgb::new(255, 255, 255);
    let black = Rgb::new(0, 0, 0);

    // Dark theme: a dark selection fill takes the light body colour.
    let dark_bg = Rgb::new(15, 15, 20);
    let dark_fg = Rgb::new(220, 220, 225);
    let dark_selection = Rgb::new(40, 60, 90);
    assert_eq!(
        derive_text_on(dark_fg, dark_bg, dark_selection),
        dark_fg,
        "on a dark fill the light body colour is the readable one"
    );

    // Light theme: a pale selection fill takes the dark body colour — which is
    // the case `Color::White` got wrong.
    let light_bg = Rgb::new(250, 250, 255);
    let light_fg = Rgb::new(30, 30, 35);
    let light_selection = Rgb::new(200, 220, 250);
    assert_eq!(
        derive_text_on(light_fg, light_bg, light_selection),
        light_fg,
        "on a pale fill the dark body colour is the readable one"
    );

    // A selection colour close to its background still has to produce something
    // that reads — this is the degenerate-input case, and it must not panic.
    let near = Rgb::new(20, 20, 20);
    let derived = derive_text_on(near, near, near);
    assert!(
        derived.contrast_ratio(near) >= MIN_DERIVED_TEXT_RATIO,
        "a surface identical to both body colours still needs a legible \
         foreground; got {} at {:.2}:1",
        derived,
        derived.contrast_ratio(near)
    );

    // Every grey surface must clear the bar. With both body colours identical
    // to each other there is no palette answer to prefer, so this is the pure
    // fallback path: it must never produce a surface-coloured foreground, which
    // is the failure the hardcoded `Color::White` literals represented on light
    // themes.
    for luma in 0..=255u16 {
        let surface = Rgb::new(luma as u8, luma as u8, luma as u8);
        let derived = derive_text_on(near, near, surface);
        assert!(
            derived.contrast_ratio(surface) >= MIN_DERIVED_TEXT_RATIO,
            "grey {luma} produced {derived} at {:.2}:1, below the bar",
            derived.contrast_ratio(surface)
        );
        if near.contrast_ratio(surface) < MIN_DERIVED_TEXT_RATIO {
            assert!(
                derived == black || derived == white,
                "grey {luma}: no body colour cleared the bar, so the answer must be \
                 an absolute one — got {derived}"
            );
        }
    }
}

/// A native theme may pin the token, and pinning it is an override rather than a
/// requirement — a file that omits the key still gets a derived value.
#[test]
fn a_native_theme_may_pin_the_tokens_but_never_has_to() {
    let derived_only = codegg::theme::native::parse_native_theme(
        r###"
        [meta]
        id = "derived-only"
        [base]
        background = "#101010"
        foreground = "#eeeeee"
        [ui]
        selection = "#283858"
        "###,
        codegg::theme::schema::ThemeSource::Inline,
        &builtin_fallback(),
    )
    .expect("a minimal native theme parses");

    assert!(
        derived_only
            .ui
            .selection_text
            .contrast_ratio(derived_only.ui.selection)
            >= MIN_DERIVED_TEXT_RATIO,
        "an omitted selection_text must derive, not inherit the fallback's token"
    );

    let pinned = codegg::theme::native::parse_native_theme(
        r###"
        [meta]
        id = "pinned"
        [base]
        background = "#101010"
        foreground = "#eeeeee"
        [ui]
        selection = "#283858"
        selection_text = "#ff00ff"
        "###,
        codegg::theme::schema::ThemeSource::Inline,
        &builtin_fallback(),
    )
    .expect("a pinned native theme parses");
    assert_eq!(
        pinned.ui.selection_text,
        Rgb::new(0xff, 0x00, 0xff),
        "an explicit selection_text must win over the derivation"
    );
}

/// A malformed override is dropped, not fatal: the derivation is already a
/// usable answer, so failing the whole file over an optional key would be the
/// wrong trade.
#[test]
fn a_malformed_selection_override_falls_back_to_the_derivation() {
    let theme = codegg::theme::native::parse_native_theme(
        r###"
        [meta]
        id = "bad-override"
        [base]
        background = "#101010"
        foreground = "#eeeeee"
        [ui]
        selection = "#283858"
        selection_text = "not-a-color"
        "###,
        codegg::theme::schema::ThemeSource::Inline,
        &builtin_fallback(),
    )
    .expect("a malformed optional key must not fail the parse");
    assert!(
        theme.ui.selection_text.contrast_ratio(theme.ui.selection) >= MIN_DERIVED_TEXT_RATIO,
        "the derived value must survive a malformed override"
    );
}

/// The same rule reaches the legacy static palette list, which has no importer
/// to derive it: all 32 built-in `ThemeData` palettes get both tokens.
#[test]
fn every_static_palette_derives_a_legible_selection_foreground() {
    let themes = codegg::tui::theme::all_themes();
    // 31, not the 32 the M003 plan recorded. Verified against the literals:
    // `grep -c '^    ThemeData {' src/tui/theme.rs` is 31, and there are 31
    // `THEMES[n]` accessors. The plan's count was off by one; it changed no
    // work here because the tokens are derived rather than authored per
    // literal, but the number is recorded so the next reader does not re-derive
    // it.
    assert_eq!(themes.len(), 31, "the static palette list changed size");
    for t in &themes {
        assert!(
            rgb_ratio(t.selection_text, t.selection) >= REQUIRED_RATIO,
            "{}: selection_text {:?} reads {:.2}:1 on {:?}",
            t.id,
            t.selection_text,
            rgb_ratio(t.selection_text, t.selection),
            t.selection
        );
        assert!(
            rgb_ratio(t.accent_text, t.primary) >= REQUIRED_RATIO,
            "{}: accent_text {:?} reads {:.2}:1 on {:?}",
            t.id,
            t.accent_text,
            rgb_ratio(t.accent_text, t.primary),
            t.primary
        );
    }
}

/// `selection_style` paints a fill, so its foreground must be the token derived
/// for that fill — not the accent, which is a colour and not a surface.
#[test]
fn selection_style_paints_the_derived_foreground_not_the_accent() {
    for t in codegg::tui::theme::all_themes() {
        assert_eq!(
            t.selection_style().fg,
            Some(t.selection_text),
            "{}: selection_style still paints the accent as a foreground over the \
             selection fill",
            t.id
        );
        assert_eq!(t.highlight_style().fg, Some(t.selection_text));
    }
}

/// WCAG ratio between two `ratatui` colours, for the static-palette checks.
fn rgb_ratio(a: ratatui::style::Color, b: ratatui::style::Color) -> f64 {
    let to_rgb = |c: ratatui::style::Color| match c {
        ratatui::style::Color::Rgb(r, g, b) => Rgb::new(r, g, b),
        other => panic!("expected an Rgb colour, got {other:?}"),
    };
    to_rgb(a).contrast_ratio(to_rgb(b))
}

/// A spot check that the sweep is measuring real per-theme variation rather than
/// one constant token reused everywhere.
#[test]
fn the_tokens_actually_vary_by_theme() {
    let themes = codegg::tui::theme::all_themes();
    let mut distinct: Vec<_> = themes.iter().map(|t| t.selection_text).collect();
    distinct.sort_by_key(|c| format!("{c:?}"));
    distinct.dedup();
    assert!(
        distinct.len() > 1,
        "every static palette derived the same selection_text, which means the \
         derivation is not reading the palette"
    );
}
