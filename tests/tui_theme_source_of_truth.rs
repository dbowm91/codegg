//! A docked box's colours must be a function of the theme in force **at render
//! time**, not the theme that happened to be current when the box was opened.
//!
//! ## The defect
//!
//! `set_theme` was called when a box was **opened** and never again. Seven
//! sites reassign the live theme on a theme change (`src/tui/app/input.rs`,
//! `src/tui/app/mod.rs`) and none of them re-pushes it into an open box, so any
//! render body reading `self.theme` froze its colours at open time while the
//! rest of the UI followed the new theme. A box reading the cache in some spans
//! and the live theme in others repainted *incoherently*, which is worse than
//! being uniformly stale.
//!
//! ## Why this asserts on the buffer
//!
//! "The body does not read `self.theme`" is not checkable here: it is a source
//! property, and a source guard passes just as happily on a body that reads the
//! cache behind a renamed field. So every claim here is made against the
//! rendered column — construct with theme A, render, then **re-render the same
//! widget with theme B** and inspect the cells.
//!
//! ## The two themes, and why they are chosen to be disjoint
//!
//! `Theme::dark()` and `Theme::light()` share exactly one colour (`link`), which
//! none of these boxes paints, so a colour seen after the switch cannot have
//! come from theme A by accident. The assertion is therefore one-directional
//! and mechanical: **no interior foreground after a theme change may be a colour
//! the old theme defined**.
//!
//! The host's own border is excluded from the inspection. `render_hosted` paints
//! it from the theme it is handed, so it follows the switch today and would mask
//! a stale widget underneath it. Only the interior — the rect the widget's own
//! body fills — is the widget's to get right.
//!
//! ## Which of these fail at the baseline, and the two that do not
//!
//! Measured against the baseline, **four** fail: `review`, `plugin`,
//! `security_review` and `diff`.
//!
//! Two widgets read `self.theme` heavily and still pass: `mcp` (46 cache
//! reads) and `research` (47). Both re-assign `self.theme = Arc::clone(theme)`
//! *inside their own render body* — `McpDialog::render`/`render_in_panel` and
//! `ResearchBrowserDialog::render_body` — so the cache is overwritten on the
//! frame that reads it and never goes stale. They are correct by accident of a
//! line that looks like the defect it is meant to be, which is why the counts
//! below are not the verdicts.
//!
//! `keybind`, `tree`, `plan` and the theme picker read the seam's theme already.
//! The picker's column legitimately paints each *candidate* theme's palette as
//! a preview swatch, so it paints colours that are not the active theme's by
//! design; it is excluded from the leak check for that reason and said so
//! rather than quietly dropped.

use std::sync::Arc;

use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::diff::DiffDialog;
use codegg::tui::components::dialogs::keybind::KeybindDialog;
use codegg::tui::components::dialogs::mcp::{McpDialog, McpServerInfo, McpToolInfo};
use codegg::tui::components::dialogs::plan::PlanDialog;
use codegg::tui::components::dialogs::plugin::PluginDialog;
use codegg::tui::components::dialogs::research::ResearchBrowserDialog;
use codegg::tui::components::dialogs::review::{ReviewDialog, ReviewItem, ReviewMode};
use codegg::tui::components::dialogs::security_review::SecurityReviewDialog;
use codegg::tui::components::dialogs::task_view::TaskViewDialog;
use codegg::tui::components::dialogs::theme::ThemePickerDialog;
use codegg::tui::components::dialogs::tree::{TreeDialog, TreeNode};
use codegg::tui::panels::{render_hosted, SidePanelId};
use codegg::tui::theme::Theme;

fn dark() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

fn light() -> Arc<Theme> {
    Arc::new(Theme::light())
}

/// Every colour `Theme::dark()` defines.
///
/// `link` is deliberately excluded: it is the one value `dark` and `light`
/// share (`#3c8cf0`), so treating it as dark-only would fail a box that painted
/// a link, which none of these widgets does and none of this milestone changes.
///
/// `ratatui::style::Color` is not `Ord`, so these are plain `Vec`s with linear
/// `contains` — the sets are a dozen entries at most.
fn dark_palette() -> Vec<ratatui::style::Color> {
    let d = Theme::dark();
    [
        d.background,
        d.foreground,
        d.primary,
        d.secondary,
        d.success,
        d.warning,
        d.error,
        d.muted,
        d.border,
        d.selection,
        d.selection_dim,
        d.selection_text,
        d.accent_text,
        d.alternate_bg,
        d.input_bg,
    ]
    .to_vec()
}

/// The foreground colours of the cells the **widget** painted: the interior of
/// the host's box, skipping blank cells.
///
/// A blank cell carries whatever style the surrounding `Block` left on it, so
/// counting it would report chrome rather than content. A widget whose body
/// paints no visible glyph has nothing to be stale about, so the emptiness is
/// asserted separately — see [`assert_follows_the_live_theme`].
fn interior_fgs(
    widget: &mut dyn Component,
    id: SidePanelId,
    theme: &Arc<Theme>,
) -> Vec<ratatui::style::Color> {
    let area = ratatui::layout::Rect::new(0, 0, 40, 16);
    let title = format!(" {} ", id.label());
    let mut buf = ratatui::buffer::Buffer::empty(area);
    render_hosted(widget, &mut buf, area, theme, &title);

    let mut out: Vec<ratatui::style::Color> = Vec::new();
    for y in 1..area.height.saturating_sub(1) {
        for x in 1..area.width.saturating_sub(1) {
            let Some(cell) = buf.cell((x, y)) else {
                continue;
            };
            if cell.symbol().trim().is_empty() {
                continue;
            }
            if let Some(fg) = cell.style().fg {
                if !out.contains(&fg) {
                    out.push(fg);
                }
            }
        }
    }
    out
}

/// The assertion: after a theme change, nothing in the column may still be a
/// colour the **old** theme defined.
fn assert_follows_the_live_theme(
    widget: &mut dyn Component,
    id: SidePanelId,
    who: &str,
    before: &[ratatui::style::Color],
) {
    let new_theme = light();
    let after = interior_fgs(widget, id, &new_theme);

    assert!(
        !after.is_empty(),
        "{who}: the box painted no foreground at all under either theme, so this \
         check cannot see its colours. Give it content."
    );
    assert!(
        !before.is_empty(),
        "{who}: the box painted no foreground under the first theme either."
    );

    let old_palette = dark_palette();
    let leaked: Vec<_> = after.iter().filter(|c| old_palette.contains(c)).collect();
    assert!(
        leaked.is_empty(),
        "{who}: after the theme changed to light the column still paints {} \
         colour(s) that belong to the theme it was *opened* with: {:?}. A render \
         path is reading a cached theme instead of the one the panel host \
         passes it.",
        leaked.len(),
        leaked
    );
}

/// Render once with the old theme, then again with the new one, and assert the
/// column follows.
fn check(build: impl FnOnce(&Arc<Theme>) -> Box<dyn Component>, id: SidePanelId, who: &str) {
    let mut widget = build(&dark());
    let before = interior_fgs(&mut *widget, id, &dark());
    assert_follows_the_live_theme(&mut *widget, id, who, &before);
}

// ------------------------------------------------------------------ fixtures

fn mcp_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut d = McpDialog::new();
    d.set_servers(vec![McpServerInfo {
        name: "files".to_string(),
        status: "connected".to_string(),
        status_error: None,
        server_type: "stdio".to_string(),
        tools: vec![McpToolInfo {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
        }],
        resources: Vec::new(),
        has_oauth: false,
    }]);
    Box::new(d)
}

fn research_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    use codegg::research::types::{ResearchDepth, ResearchMode, ResearchRunCounts, RunStatus};

    let mut d = ResearchBrowserDialog::new();
    d.set_runs(vec![codegg::research::service::ResearchRunSummary {
        run_id: "run-1".to_string(),
        status: RunStatus::Completed,
        question: "does the render path read the live theme?".to_string(),
        mode: ResearchMode::Landscape,
        depth: ResearchDepth::Medium,
        started_at: chrono::Utc::now(),
        finished_at: None,
        artifact_dir: std::path::PathBuf::from("/tmp/run-1"),
        counts: ResearchRunCounts {
            sources: 3,
            evidence_spans: 9,
            claims: 4,
            contradictions: 0,
        },
    }]);
    Box::new(d)
}

fn review_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut d = ReviewDialog::new();
    d.set_items(vec![ReviewItem {
        path: "src/theme/schema.rs".to_string(),
        kind: "Modified".to_string(),
        additions: 40,
        deletions: 2,
    }]);
    d.mode = ReviewMode::FileList;
    Box::new(d)
}

fn security_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    use codegg::security::workflow::{
        SecurityReviewCommandArgs, SecurityReviewOutput, SecurityReviewReceipt,
    };

    let receipt = SecurityReviewReceipt::now(
        "sr-m003".to_string(),
        std::path::PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: Vec::new(),
            findings: Vec::new(),
            review_prompts: Vec::new(),
            preflight_results: Vec::new(),
            notes: vec!["no findings".to_string()],
            hunks: Vec::new(),
        },
        "rendered report".to_string(),
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        0,
    );
    Box::new(SecurityReviewDialog::with_receipt(receipt))
}

fn plugin_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(PluginDialog::new(
        "acme".to_string(),
        "Acme Plugin".to_string(),
        (0..20)
            .map(|i| format!("plugin output line {i:02}"))
            .collect(),
    ))
}

fn tree_dialog(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut d = TreeDialog::new(Arc::clone(theme));
    d.load_nodes(
        vec![TreeNode {
            id: "n1".to_string(),
            session_id: "s1".to_string(),
            label: "main".to_string(),
            time_updated: 1,
            message_count: Some(12),
            is_current: true,
            is_archived: false,
            children: Vec::new(),
            depth: 0,
        }],
        Some("s1".to_string()),
    );
    Box::new(d)
}

fn keybind_dialog(theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(KeybindDialog::new(Arc::clone(theme)))
}

fn plan_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(PlanDialog::new("plan-1".into(), "ship the thing".into()))
}

fn theme_dialog(theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(ThemePickerDialog::new(Arc::clone(theme)))
}

fn diff_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(DiffDialog::new(
        "let old = 1;\nlet a = 2;\n".into(),
        "let new = 1;\nlet a = 3;\n".into(),
        "src/lib.rs".into(),
    ))
}

fn task_view_dialog(_theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(TaskViewDialog::new("proj".into()))
}

// ---------------------------------------------------------------- the widgets

/// `mcp` carried 46 reads of a cached theme and was **not** stale, because
/// `McpDialog::render` and `render_in_panel` both began
/// `self.theme = Arc::clone(theme)` — a cache the render overwrites every frame.
/// The M003 roadmap records this file as both "the exemplar of the correct
/// pattern" and "the single worst instance of the defect"; the first is right,
/// the second is false, and a source count cannot tell them apart. It now reads
/// the theme it is handed, like every other box.
#[test]
fn mcp_follows_a_theme_change() {
    check(mcp_dialog, SidePanelId::Mcp, "McpDialog");
}

/// `research` is the same shape: 47 reads of a cached theme, refreshed once per
/// frame at the top of `render_body`. Also converted, also not stale.
#[test]
fn research_follows_a_theme_change() {
    check(
        research_dialog,
        SidePanelId::Research,
        "ResearchBrowserDialog",
    );
}

#[test]
fn review_follows_a_theme_change() {
    check(review_dialog, SidePanelId::Review, "ReviewDialog");
}

#[test]
fn security_review_follows_a_theme_change() {
    check(
        security_dialog,
        SidePanelId::Security,
        "SecurityReviewDialog",
    );
}

#[test]
fn plugin_follows_a_theme_change() {
    check(plugin_dialog, SidePanelId::Plugins, "PluginDialog");
}

/// The `diff` dialog holds no theme of its own — it delegates to `DiffViewer`,
/// which caches one and is refreshed only by the same open-time `set_theme`.
#[test]
fn diff_follows_a_theme_change() {
    check(diff_dialog, SidePanelId::Diff, "DiffDialog");
}

// ------------------------------------------------------------------ controls

/// The four boxes whose shared body already read the seam's theme pass here
/// *before* any production change. They are the harness's own control: if these
/// failed, the assertion would be measuring something other than staleness.
#[test]
fn the_boxes_that_already_read_the_seam_theme_pass_this_check() {
    check(
        keybind_dialog,
        SidePanelId::Keybinds,
        "KeybindDialog (control)",
    );
    check(tree_dialog, SidePanelId::Tree, "TreeDialog (control)");
    check(plan_dialog, SidePanelId::Plan, "PlanDialog (control)");
    check(
        task_view_dialog,
        SidePanelId::TaskView,
        "TaskViewDialog (control)",
    );
}

/// The picker is the one box that paints colours which are not the active
/// theme's **on purpose**: `push_theme_row` draws each candidate theme's own
/// palette so `/theme` shows what selecting it would give you. A leak check
/// would flag it, so it is checked against its actual contract instead — the
/// swatches must carry the *candidate* theme's colours, which is what makes the
/// picker useful and what a "make everything follow the live theme" refactor
/// would silently destroy.
#[test]
fn the_theme_picker_shows_each_candidate_palette_rather_than_the_active_one() {
    let mut picker = theme_dialog(&dark());
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, 16));
    let area = ratatui::layout::Rect::new(0, 0, 40, 16);
    render_hosted(&mut *picker, &mut buf, area, &dark(), " Themes ");

    let mut painted: Vec<ratatui::style::Color> = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let Some(cell) = buf.cell((x, y)) else {
                continue;
            };
            if cell.symbol().trim().is_empty() {
                continue;
            }
            if let Some(fg) = cell.style().fg {
                if !painted.contains(&fg) {
                    painted.push(fg);
                }
            }
        }
    }

    let catalog = codegg::tui::theme::all_themes();
    let swatches = catalog
        .iter()
        .flat_map(|t| [t.success, t.warning, t.error, t.primary, t.secondary])
        .collect::<Vec<_>>();
    assert!(
        painted.iter().any(|c| swatches.contains(c)),
        "the picker's column shows no candidate-theme swatch at all, so it is not \
         previewing anything — if this fails the exclusion above is unfounded"
    );
}

/// The reverse direction: this check must actually be able to detect a stale
/// widget, or four green tests prove nothing.
///
/// A widget that paints the *old* theme's primary while ignoring the theme it is
/// handed is the defect in miniature, and the check catches it. This is what
/// stops the harness from silently becoming a no-op — e.g. if a future refactor
/// made `interior_fgs` sample nothing, every staleness test would pass.
///
/// The sibling defect — a colour that belongs to *no* theme, which is what the
/// hardcoded `Color::White` literals were — is deliberately **not** checked
/// here: those colours are in neither palette, so this check is blind to them
/// by construction. `tests/theme_selection_tokens.rs` and
/// `scripts/check_no_hardcoded_selection_colours.py` cover that one.
#[test]
fn this_check_can_detect_a_widget_that_reads_a_stale_theme() {
    struct PaintsTheOldPrimary;
    impl Component for PaintsTheOldPrimary {
        fn handle_key(
            &mut self,
            _key: crossterm::event::KeyEvent,
        ) -> Option<codegg::tui::app::TuiMsg> {
            None
        }
        fn update(&mut self, _msg: codegg::tui::app::TuiMsg) -> Option<codegg::tui::app::TuiMsg> {
            None
        }
        fn render(
            &mut self,
            frame: &mut ratatui::Frame,
            area: ratatui::layout::Rect,
            _theme: &Arc<Theme>,
        ) {
            use ratatui::layout::Margin;
            use ratatui::widgets::{Paragraph, Widget};
            // Inset by one row and column, because `interior_fgs` excludes the
            // host's border ring — the same thing `render_in_panel` does before
            // handing a rect to a widget.
            let inner = area.inner(Margin::new(1, 1));
            let style = ratatui::style::Style::default().fg(Theme::dark().primary);
            Paragraph::new("stale")
                .style(style)
                .render(inner, frame.buffer_mut());
        }
        fn dialog_type(&self) -> codegg::tui::components::component::DialogType {
            codegg::tui::components::component::DialogType::None
        }
    }

    let mut stale = PaintsTheOldPrimary;
    let before = interior_fgs(&mut stale, SidePanelId::Files, &dark());
    let after = interior_fgs(&mut stale, SidePanelId::Files, &light());
    assert!(after.iter().any(|c| dark_palette().contains(c)));
    let _ = before;
}
