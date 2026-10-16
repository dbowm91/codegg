//! Rendering and view-model preparation for the TUI.
//!
//! This module owns the synchronous presentation pass. It may read cached
//! application state and prepare ratatui widgets, but it must not perform
//! filesystem, daemon, network, process, or other blocking work.

use super::*;
use crate::tui::app::state::subagents::SubagentRow;
use crate::tui::app::state::tasks::{bounded as bounded_task_rows, TaskRow};
use crate::tui::panels::{PanelPlacement, SidePanelId};
use ratatui::style::Color;

/// One overlay row, ellipsized to the strip width.
///
/// Wrapping is deliberately avoided: the overlay's height is computed once in
/// `tasks_strip_height`, so a wrapped line would paint into the next row and a
/// long job summary would silently corrupt the block below it.
fn strip_line<'a>(
    theme: &'a crate::tui::theme::Theme,
    width: u16,
    lead: &str,
    text: &str,
    colour: Color,
) -> Line<'a> {
    let lead_width = unicode_width::UnicodeWidthStr::width(lead);
    let budget = (width as usize).saturating_sub(3 + lead_width);
    let body = crate::tui::wrap::ellipsize_to_width(text, budget);
    Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled(lead.to_string(), Style::default().fg(theme.primary)),
        Span::styled(body, Style::default().fg(colour)),
    ])
}

/// Maximum display width of the agent name shown beside the input-mode badge.
const MAX_AGENT_BADGE_WIDTH: usize = 20;

/// Pick a foreground that stays legible on `background`.
///
/// Theme palettes are authored independently, so a slot that reads well
/// against the window background can be invisible against a different fill —
/// a completion row's `selection` background is the common case, because
/// themes set it from a widget-specific slot rather than deriving it from
/// the background. Walk `candidates` (most-preferred first) and return the
/// first whose WCAG contrast against `background` clears the legibility bar,
/// falling back to the highest-contrast candidate so the result is always
/// the best available rather than an arbitrary first pick.
fn readable_on(
    preferred: Color,
    background: Color,
    candidates: impl IntoIterator<Item = Color>,
) -> Color {
    const MIN_CONTRAST: f64 = 3.0;
    let to_rgb = |c: Color| match c {
        Color::Rgb(r, g, b) => crate::theme::Rgb::new(r, g, b),
        // Indexed / named / reset colors carry no measurable luminance here;
        // treat them as opaque so we never reject a palette we cannot
        // actually evaluate.
        _ => crate::theme::Rgb::new(128, 128, 128),
    };
    let bg = to_rgb(background);

    let mut best: Option<(Color, f64)> = None;
    for candidate in candidates {
        let ratio = to_rgb(candidate).contrast_ratio(bg);
        if ratio >= MIN_CONTRAST {
            return candidate;
        }
        if best.as_ref().is_none_or(|(_, r)| ratio > *r) {
            best = Some((candidate, ratio));
        }
    }
    // `preferred` is what the caller asked for; it is only overridden when
    // every candidate failed the bar, in which case the best-scoring one
    // still beats rendering text that is indistinguishable from the fill.
    let preferred_ratio = to_rgb(preferred).contrast_ratio(bg);
    match best {
        Some((color, ratio)) if ratio > preferred_ratio => color,
        _ => preferred,
    }
}

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        // One rect per *occupied* edge, named rather than positional: the
        // transcript reads `regions.main` whatever the panels do, so adding a
        // second column cannot renumber the thing underneath it.
        let occupied: Vec<PanelPlacement> = PanelPlacement::ALL
            .iter()
            .copied()
            .filter(|placement| self.panel_host.is_visible_at(*placement))
            .collect();
        // Recorded so a resize can convert the bottom panel's percentage into
        // an exact row count the first time a reader drags that edge; a
        // percentage cannot express a one-row step on its own.
        self.ui_state.layout.observe_frame(area);
        let regions = self.ui_state.layout.split(area, &occupied);
        let main_chunks = [regions.main];
        let main_area = main_chunks[0];

        // Reserve a 1-column strip on the left of the main area for the
        // outer left border. Reserving the strip (rather than painting
        // the border on top of the content) keeps the line continuous —
        // the header / viewport / prompt / footer widgets no longer
        // overwrite the leftmost column with their own text.
        let bordered = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(main_area);
        let main_area_inner = bordered[1];
        self.left_border_area = Some(bordered[0]);

        // Reserve the main pane's last row for the outer bottom border
        // *before* the session layout runs. `footer_height` is 1, so
        // anchoring the edge on the footer's own last row painted the
        // status line over: `StatusBar` takes its border-less early path
        // at height 1, and `finish_render` then drew a `Borders::BOTTOM`
        // block across the only row the status line ever had. Carving the
        // row off first gives the footer a row of its own and leaves the
        // edge underneath it.
        let (pane_area, border_area) = if main_area_inner.height > 0 {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(main_area_inner);
            let usable = main_area_inner.width > 0 && split[0].height > 0;
            (split[0], if usable { Some(split[1]) } else { None })
        } else {
            (main_area_inner, None)
        };
        self.bottom_border_area = border_area;

        let todo_rows = self.todo_strip_height();
        let max_prompt_height = (area.height * 40 / 100).max(3);
        // Reserve the strip's rows *inside* the prompt allocation so the
        // input box keeps the height it asked for and the chat viewport
        // absorbs the difference. Carving them back out of the prompt chunk
        // instead would silently shrink the input by up to
        // `MAX_TODO_STRIP_TASKS` rows every time the list was expanded.
        // The contextual key line sits directly above the input and is
        // reserved from the prompt allocation for the same reason the todo
        // strip is: carving it out afterwards would shrink the input every
        // time a panel took focus.
        let hint_rows = u16::from(self.composer_hint_text().is_some());
        // The working strip sits closest to the transcript, so a turn in
        // flight reads as part of the buffer rather than as chrome. It is
        // reserved from the prompt allocation for the same reason the hint
        // line and todo strip are: carving it afterwards would shrink the
        // input every time the agent started working.
        let working_rows = self.working_strip_rows();
        // The unified tasks overlay reserves from the prompt allocation for
        // the same reason the todo strip does: carving it out afterwards would
        // shrink the input box every time the overlay opened. It is added
        // *after* the todo strip because the overlay renders directly beneath
        // it, so their order in the allocation has to match their order on
        // screen.
        //
        // The reservation is then clamped against the height actually left
        // over once the transcript's floor is honoured. Without the clamp the
        // overlay and the prompt box together can exceed a short pane, and the
        // carve below silently yields — so `Shift+T` looks like a dead key on
        // exactly the terminals where a status overlay would be most wanted.
        // A too-short pane shows no overlay rather than a wrong one.
        let overlay_allowance = self.tasks_overlay_allowance(
            pane_area.height,
            todo_rows,
            hint_rows,
            working_rows,
            max_prompt_height,
        );
        let tasks_rows = self.tasks_strip_height().min(overlay_allowance);
        let prompt_height = self
            .prompt_state
            .prompt
            .needed_height(max_prompt_height)
            .saturating_add(todo_rows)
            .saturating_add(tasks_rows)
            .saturating_add(hint_rows)
            .saturating_add(working_rows);
        let session_chunks = self
            .ui_state
            .layout
            .session_layout(pane_area, Some(prompt_height));

        // Carve the hint line, then the todo strip, then the working strip,
        // off the top of the prompt chunk. Order matters: hints belong closer
        // to the input than the task strip does, and the working strip closer
        // to the transcript than both.
        let mut available = session_chunks[2];
        let mut working_chunk = None;
        if working_rows > 0 && available.height > working_rows {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(working_rows), Constraint::Min(1)])
                .split(available);
            working_chunk = Some(split[0]);
            available = split[1];
        }
        let mut hint_chunk = None;
        if hint_rows > 0 && available.height > hint_rows {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(hint_rows), Constraint::Min(1)])
                .split(available);
            hint_chunk = Some(split[0]);
            available = split[1];
        }
        // Carve top-down in the same order the rows were added to
        // `prompt_height`: working strip, hint line, todo strip, tasks
        // overlay, then the prompt gets whatever is left. The overlay sits
        // directly beneath the todo strip on screen for the same reason.
        let (remaining, todo_chunk) = if todo_rows > 0 && available.height > todo_rows {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(todo_rows), Constraint::Min(1)])
                .split(available);
            (split[1], Some(split[0]))
        } else {
            (available, None)
        };
        let (prompt_chunk, tasks_chunk) = if tasks_rows > 0 && remaining.height > tasks_rows {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(tasks_rows), Constraint::Min(1)])
                .split(remaining);
            (split[1], Some(split[0]))
        } else {
            (remaining, None)
        };
        self.todo_area = todo_chunk;
        self.tasks_area = tasks_chunk;
        self.viewport_area = Some(session_chunks[1]);
        self.prompt_area = Some(prompt_chunk);

        // Outer TUI frame: paint the left border on the reserved strip
        // and the four corner cells that connect it with the header's
        // bottom border (top) and the reserved bottom edge (bottom).
        // The header supplies the top horizontal line via its own
        // `Borders::BOTTOM`; the bottom line is painted in `finish_render`
        // on `border_area`, so the lower corners have to be anchored there
        // too — anchoring them on the footer's last row left the bottom
        // edge's own leftmost cell showing the `│` of the left strip and
        // put a stray `└` up on the status line.
        self.render_outer_borders(
            frame,
            bordered[0],
            pane_area,
            session_chunks[0],
            border_area,
        );

        self.render_header(frame, session_chunks[0]);

        // Viewport (messages) — highest-risk surface for render panics
        let viewport_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                if self.render_panic_injection.messages {
                    panic!("injected messages render panic");
                }
                self.render_viewport(frame, session_chunks[1]);
                // An open subagent buffer replaces the transcript in the
                // same rect rather than opening a modal over it: Esc then has
                // exactly one meaning — return to the main buffer — instead of
                // being ambiguous between "close a dialog" and "go back".
                if self.ui_state.subagents.open.is_some() {
                    self.render_subagent_view(frame, session_chunks[1]);
                }
                Ok(())
            }));
        if let Err(err) = viewport_result {
            let msg = Self::extract_panic_message(&err);
            tracing::error!("Messages render panic: {msg}");
            self.ui_state.last_render_error = Some(msg);
            self.ui_state
                .diagnostics
                .record_component_render_panic("messages");
            self.render_component_fallback(frame, session_chunks[1], "Messages render error");
        }

        self.render_working_strip(frame, working_chunk);
        self.render_composer_hints(frame, hint_chunk);
        self.render_todo_strip(frame, todo_chunk);
        self.render_tasks_strip(frame, tasks_chunk);
        self.render_prompt(frame, prompt_chunk);
        self.render_footer(frame, session_chunks[3]);

        // Panels — dynamic content can trigger panics
        //
        // Each occupied edge paints itself from *its own* slot, so two edges
        // can be up at once. Nothing here changes which edge the app thinks
        // is selected: render is not allowed to move the keyboard, and the
        // gesture dispatcher reads `placement()` after this returns.
        self.sidebar_area = None;
        // Every occupied column records its rect so a later mouse event can
        // resolve a click to a *panel* rather than to nothing. The list is
        // rebuilt each frame alongside `sidebar_area`, so it can never name a
        // panel that is no longer painted — which is what would otherwise let
        // a click focus a column that has moved or closed.
        self.panel_areas.clear();
        for (placement, rect) in regions.occupied() {
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            self.panel_areas.push((placement, rect));
            if self.panel_host.panel_at(placement) == SidePanelId::Session {
                self.sidebar_area = Some(rect);
            }
            let sidebar_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.sidebar {
                        panic!("injected sidebar render panic");
                    }
                    self.render_panel_at(frame, placement, rect);
                    Ok(())
                }));
            if let Err(err) = sidebar_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Sidebar render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("sidebar");
                self.render_component_fallback(frame, rect, "Sidebar unavailable");
            }
        }

        // Dialog — failures close only that dialog
        if !self.focus_manager.is_empty() {
            // The hit-test rect must be the rect the dialog actually
            // draws into. The project picker uses its own bounded
            // geometry rather than the shared 60x50 popup, so deriving
            // the stored rect from the same helper keeps mouse selection
            // aligned with the rendered rows.
            let popup_area = match self.focus_manager.active_dialog_type() {
                DialogType::ProjectPicker => {
                    crate::tui::components::dialogs::project_picker::ProjectPickerDialog::picker_area(
                        area,
                    )
                }
                _ => centered_rect(60, 50, area),
            };
            self.dialog_area = Some(popup_area);
            let dialog_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.dialog {
                        panic!("injected dialog render panic");
                    }
                    self.render_dialog(frame, area);
                    Ok(())
                }));
            if let Err(err) = dialog_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Dialog render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("dialog");
                self.focus_manager.pop();
                self.ui_state.dialog = Dialog::from(self.focus_manager.active_dialog_type());
            }
        } else {
            self.dialog_area = None;
        }

        // Completion overlay — failures hide completions
        if self.prompt_state.show_completions {
            self.completion_area = None;
            self.completion_visible_rows = 0;
            let compl_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.completions {
                        panic!("injected completions render panic");
                    }
                    self.render_completions(frame, session_chunks[2]);
                    Ok(())
                }));
            if let Err(err) = compl_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Completion overlay render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("completions");
                self.prompt_state.show_completions = false;
                self.completion_area = None;
                self.completion_visible_rows = 0;
            }
        } else {
            self.completion_area = None;
            self.completion_visible_rows = 0;
        }

        // Timeline
        if self.ui_state.timeline_visible {
            let tl_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    if self.render_panic_injection.timeline {
                        panic!("injected timeline render panic");
                    }
                    self.render_timeline(frame, area);
                    Ok(())
                }));
            if let Err(err) = tl_result {
                let msg = Self::extract_panic_message(&err);
                tracing::error!("Timeline render panic: {msg}");
                self.ui_state
                    .diagnostics
                    .record_component_render_panic("timeline");
                self.ui_state.timeline_visible = false;
            }
        }

        if !self.messages_state.toasts.is_empty() {
            let toast_area = Rect {
                x: area.width.saturating_sub(60),
                y: 2,
                width: 60.min(area.width),
                height: 10.min(area.height.saturating_sub(4)),
            };
            self.messages_state
                .toasts
                .render(frame, toast_area, &self.ui_state.theme);
        }

        self.finish_render(frame);
    }

    /// Apply the live text selection over the finished frame, then retain
    /// the frame so a drag can later be resolved to real text.
    ///
    /// This runs last, after every overlay (dialogs, completions, toasts),
    /// so a selection reads as a single continuous band over the whole
    /// window rather than being partially overpainted. Mutating the buffer
    /// directly — instead of re-rendering the widgets — is what keeps the
    /// selected text byte-identical to what was already drawn.
    fn finish_render(&mut self, frame: &mut Frame) {
        // Close the bottom edge last so nothing overwrites it.
        if let Some(area) = self.bottom_border_area {
            let bottom_block = Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(self.ui_state.theme.border))
                .style(Style::default().bg(self.ui_state.theme.background));
            frame.render_widget(bottom_block, area);
        }
        if let (Some(anchor), Some(focus)) = (self.selection_anchor, self.selection_focus) {
            let selection = crate::tui::selection::TextSelection::new(anchor, focus);
            let buffer_area = frame.area();
            let highlight = ratatui::style::Style::default()
                .bg(self.ui_state.theme.selection)
                .fg(self.ui_state.theme.foreground)
                .add_modifier(Modifier::REVERSED);
            let buffer = frame.buffer_mut();
            for y in buffer_area.y..buffer_area.y + buffer_area.height {
                for x in buffer_area.x..buffer_area.x + buffer_area.width {
                    if selection.contains((x, y)) {
                        buffer[(x, y)].set_style(highlight);
                    }
                }
            }
            self.last_frame_buffer = Some(buffer.clone());
        } else {
            self.last_frame_buffer = Some(frame.buffer_mut().clone());
        }
    }

    pub fn render_error(&mut self, frame: &mut Frame, error_msg: &str) {
        let area = frame.area();
        let theme = &self.ui_state.theme;

        let block = Block::default()
            .title(" Error ")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.error))
            .style(
                ratatui::style::Style::default()
                    .bg(theme.background)
                    .fg(theme.foreground),
            );

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠ Rendering Error",
                ratatui::style::Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::raw("The UI failed to render properly:")),
            Line::from(""),
            Line::from(Span::styled(
                error_msg,
                ratatui::style::Style::default().fg(theme.warning),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'r' to retry or 'q' to quit",
                ratatui::style::Style::default().fg(theme.muted),
            )),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center);

        let center_area = centered_rect(60, 40, area);
        frame.render_widget(Clear, center_area);
        frame.render_widget(paragraph, center_area);
    }

    /// Render a compact fallback block when a component panics.
    pub(crate) fn render_component_fallback(&self, frame: &mut Frame, area: Rect, title: &str) {
        let theme = &self.ui_state.theme;
        let block = Block::default()
            .title(format!(" {title} "))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.warning))
            .style(
                ratatui::style::Style::default()
                    .bg(theme.background)
                    .fg(theme.muted),
            );
        let paragraph = Paragraph::new("Render failed — see log for details")
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    /// Extract a human-readable message from a panic payload.
    pub(crate) fn extract_panic_message(err: &Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = err.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = err.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown render panic".to_string()
        }
    }

    fn render_header(&mut self, frame: &mut Frame, area: Rect) {
        // Split the header into a 1-row tab strip (bottom) and the
        // header content (top). On narrow terminals (<80 cols) the
        // tab strip is hidden to preserve prompt + label.
        if area.height >= 3 && area.width >= 80 {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(area.height - 1), Constraint::Length(1)])
                .split(area);
            self.render_tab_strip(frame, chunks[1]);
            self.render_header_content(frame, chunks[0]);
        } else {
            self.render_header_content(frame, area);
        }
    }

    fn render_header_content(&mut self, frame: &mut Frame, area: Rect) {
        // `render_header` is one of the few render surfaces without a
        // `catch_unwind` net, so this index must not be able to panic.
        let agent_name = self
            .agent_state
            .agents
            .get(self.agent_state.current_agent)
            .map(|a| a.name.as_str())
            .unwrap_or("unknown");
        let model_short = self
            .agent_state
            .current_model
            .split('/')
            .next_back()
            .unwrap_or(&self.agent_state.current_model);
        let mode_indicator = if self.agent_state.plan_mode {
            format!(
                "[PLAN: {}]  ",
                self.agent_state.plan_topic.as_deref().unwrap_or("general")
            )
        } else {
            String::new()
        };
        let context_indicator = self.active_context_indicator();
        let sess_title = if let Some(ref session) = self.session_state.session {
            format!("[{}]  ", clean_inline_text(&session.title, 48))
        } else {
            String::new()
        };
        // Presence M002: daemon-owned collaborator count for the active
        // project. `None` when unavailable/loading/unauthorized so hidden
        // projects never leak through the header.
        let presence_summary = self
            .active_project_id()
            .and_then(|pid| self.presence.header_summary(pid))
            .unwrap_or_default();
        // Presence M003: explicit read-only observer banner. `None` when
        // not observing. Carries only the opaque session locator + coarse
        // status — never prompts, content, or secrets.
        let observer_banner = self.observer.banner_line().unwrap_or_default();
        let title = match self.ui_state.routes.current() {
            Route::Home => Line::from(vec![
                Span::styled(
                    " codegg ",
                    Style::default()
                        .fg(self.ui_state.theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                if context_indicator.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(
                        context_indicator.clone(),
                        Style::default()
                            .fg(self.ui_state.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    )
                },
                Span::styled(
                    format!("{mode_indicator}{sess_title}  "),
                    Style::default()
                        .fg(self.ui_state.theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("agent:{agent_name}  model:{model_short}"),
                    Style::default().fg(self.ui_state.theme.muted),
                ),
                // Project Work Orders M003: composer submission mode in
                // the same semantic area as agent/model context. Session
                // mode (the default) shows nothing extra; Task mode is
                // explicit so a scheduled submit is never a surprise.
                if self.prompt_state.composer_mode.is_task() {
                    Span::styled(
                        "  composer:task",
                        Style::default()
                            .fg(self.ui_state.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw("")
                },
                if presence_summary.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(
                        format!("  {presence_summary}"),
                        Style::default().fg(self.ui_state.theme.secondary),
                    )
                },
                if observer_banner.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(
                        format!("  {observer_banner}"),
                        Style::default()
                            .fg(self.ui_state.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    )
                },
            ]),
            Route::Session(_) => Line::from(""),
            Route::Workspace => {
                let selected = self
                    .dialog_state
                    .workspace_dashboard
                    .as_ref()
                    .and_then(|dashboard| dashboard.selected())
                    .map(|row| row.summary.display_name.clone())
                    .unwrap_or_default();
                let focus = match self.workspace_focus {
                    crate::tui::app::state::WorkspaceFocus::Chat => "chat",
                    crate::tui::app::state::WorkspaceFocus::Composer => "composer",
                };
                Line::from(vec![
                    Span::styled(
                        " workspace ",
                        Style::default()
                            .fg(self.ui_state.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" selected:{selected}  focus:{focus}"),
                        Style::default().fg(self.ui_state.theme.muted),
                    ),
                    if self.prompt_state.composer_mode.is_task() {
                        Span::styled(
                            "  composer:task",
                            Style::default()
                                .fg(self.ui_state.theme.warning)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("")
                    },
                ])
            }
            Route::Editor => {
                let path = self.editor_state.path.clone().unwrap_or_default();
                let focus = if self.editor_state.buffer_focused() {
                    "buffer"
                } else {
                    "composer"
                };
                Line::from(vec![
                    Span::styled(
                        " editor ",
                        Style::default()
                            .fg(self.ui_state.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {path}  focus:{focus}"),
                        Style::default().fg(self.ui_state.theme.muted),
                    ),
                ])
            }
        };
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .style(Style::default().bg(self.ui_state.theme.background));
        let paragraph = Paragraph::new(title).block(block);
        frame.render_widget(paragraph, area);
    }

    /// Draw the left border on the reserved 1-column strip and the
    /// four corner cells that connect it with the header's bottom
    /// border (top edge) and the reserved bottom edge (bottom edge).
    /// The header supplies the top horizontal line via its own
    /// `Borders::BOTTOM`, and the bottom horizontal line is painted
    /// later by `finish_render` — the corners here are what make the
    /// three sides look like a single connected frame instead of three
    /// disjoint segments.
    ///
    /// `bottom_edge_area` is the row carved off for that bottom line,
    /// so the lower corners are anchored on the edge itself. They used
    /// to be placed on the footer's last row instead, which is one row
    /// higher: the status line got a `└`/`┘` it never asked for and the
    /// real bottom-left corner was left showing the left border's `│`.
    fn render_outer_borders(
        &mut self,
        frame: &mut Frame,
        left_area: Rect,
        content_area: Rect,
        header_area: Rect,
        bottom_edge_area: Option<Rect>,
    ) {
        // Left border: paint a `Borders::LEFT` block on the reserved
        // strip. Drawing on a dedicated strip keeps the line continuous
        // since the content widgets never write into column 0.
        let border_style = Style::default().fg(self.ui_state.theme.border);
        let bg_style = Style::default().bg(self.ui_state.theme.background);
        let left_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style)
            .style(bg_style);
        frame.render_widget(left_block, left_area);

        // The bottom edge lives on its own reserved row (see `render`),
        // painted in `finish_render`. Without it the main pane read as an
        // open-ended window next to the sidebar, which draws a full
        // `Borders::ALL`. This function only owns the corner glyphs that
        // connect that edge to the side edges.
        let Some(bottom_edge_area) = bottom_edge_area else {
            return;
        };
        // Corner cells: top-left, top-right, bottom-left, bottom-right.
        // Drawn after the header widget (which supplies the top line) so
        // the corner glyphs overwrite the line glyphs at the intersections
        // and visually connect them.
        if header_area.height > 0 && content_area.width > 0 {
            let header_bottom_y = header_area.y + header_area.height - 1;
            let bottom_y = bottom_edge_area.y + bottom_edge_area.height.saturating_sub(1);
            let right_x = content_area.x + content_area.width - 1;
            self.render_corner(frame, left_area.x, header_bottom_y, '┌', border_style);
            self.render_corner(frame, right_x, header_bottom_y, '┐', border_style);
            self.render_corner(frame, left_area.x, bottom_y, '└', border_style);
            self.render_corner(frame, right_x, bottom_y, '┘', border_style);
        }
    }

    /// Paint a single corner glyph at `(x, y)` in the given style.
    /// Uses a 1x1 [`Paragraph`] so the glyph goes through ratatui's
    /// normal styling pipeline and respects the active background.
    fn render_corner(&self, frame: &mut Frame, x: u16, y: u16, glyph: char, style: Style) {
        let cell = Paragraph::new(glyph.to_string()).style(style);
        frame.render_widget(cell, Rect::new(x, y, 1, 1));
    }

    fn active_context_indicator(&self) -> String {
        let dialog_type = self.focus_manager.active_dialog_type();
        if dialog_type != DialogType::None {
            return format!("[DIALOG: {:?}]  ", dialog_type);
        }
        if self.ui_state.dialog.is_open() {
            return format!("[DIALOG: {:?}]  ", self.ui_state.dialog);
        }
        if self.ui_state.command_mode {
            return "[CMD]  ".to_string();
        }
        if self.messages_state.messages.search_visible {
            return "[SEARCH]  ".to_string();
        }
        if self.agent_state.plan_mode {
            return "[PLAN]  ".to_string();
        }
        if self.session_state.permission_pending {
            return "[PERMISSION]  ".to_string();
        }
        let subagent_count = self.session_state.subagent_count;
        if subagent_count > 0 {
            return format!("[...{}]  ", subagent_count);
        }
        // Turn state deliberately does NOT appear here. This prefix used to
        // paint a second spinner here, driven by a private widget whose cadence
        // disagreed with the working strip's — two indicators, two speeds, one
        // of them on a row too small to read. The working strip is the single
        // status surface now; keeping a token here would only re-create the
        // ambiguity it replaced.
        String::new()
    }

    fn render_viewport(&mut self, frame: &mut Frame, area: Rect) {
        // M006-E: an open review takes over the viewport. It is a modal
        // decision surface — the user must accept or reject — so it is the one
        // place M006 does not stay non-modal, and it is reachable only via an
        // explicit `/review`.
        if self.change_review_state.is_open() {
            crate::tui::components::change_review::ChangeReviewView::new(
                &self.change_review_state,
                Arc::clone(&self.ui_state.theme),
            )
            .render(frame, area);
            return;
        }
        match self.ui_state.routes.current() {
            Route::Home => self.render_home(frame, area),
            Route::Session(_) => self.render_session(frame, area),
            Route::Workspace => self.render_workspace_view(frame, area),
            Route::Editor => self.render_editor_view(frame, area),
        }
    }

    /// M006-A: non-modal document editor primary view. Reads the text
    /// from a scoped borrow of the controller replica and drops it with
    /// this call, so the frame holds no document buffer. The bottom
    /// composer and the sidebar are rendered separately and are
    /// unaffected.
    fn render_editor_view(&mut self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.ui_state.theme);
        // M006-D: when the tree pane is visible the viewport splits. The tree
        // is rendered first and borrows only its own state, so the editor's
        // controller replica is still borrowed and dropped inside its own
        // render below — the frame holds no document buffer either way.
        let (tree_area, editor_area) =
            crate::tui::components::file_tree::split_pane(area, self.file_tree_state.visible);
        let shell = |frame: &mut Frame, message: String, style: Style, area: Rect| {
            let block = Block::default()
                .title(" Editor (/open <workspace-relative path> · Esc leaves) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background));
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(Paragraph::new(message).style(style), inner);
        };
        let Some(session) = self.editor_state.session.as_ref() else {
            // The tree is a navigator, not a precondition: it renders even
            // with no document open, because browsing a workspace is useful
            // before choosing what to open.
            self.render_file_tree_pane(frame, tree_area);
            shell(
                frame,
                "No document open — use /open <path>".to_string(),
                Style::default().fg(theme.muted),
                editor_area,
            );
            return;
        };
        let Some(snapshot) = session.try_snapshot() else {
            self.render_file_tree_pane(frame, tree_area);
            shell(
                frame,
                self.editor_state.status.label().to_string(),
                Style::default().fg(theme.warning),
                editor_area,
            );
            return;
        };
        let presentation = session.presentation();
        let cursor = presentation.cursor_byte;
        let mode = presentation.mode;
        let focus = presentation.focus;
        let pending = presentation
            .pending_command
            .get(..crate::tui::editor::MAX_EDITOR_PENDING_COMMAND)
            .unwrap_or("")
            .to_string();
        let command_active = presentation.command_active;
        let command_line = presentation.command_line.clone();
        let command_selection = presentation.command_selection;
        let scope = self.editor_state.scope;
        let path = self
            .editor_state
            .path
            .clone()
            .unwrap_or_else(|| "(unknown)".to_string());
        let status = self.editor_state.status;
        let notice = self
            .editor_state
            .notice
            .as_ref()
            .map(|notice| notice.message());
        let writable = self.editor_state.is_writer();
        let busy = self.editor_state.is_busy();
        self.render_file_tree_pane(frame, tree_area);
        if let Some(session) = self.editor_state.session.as_mut() {
            crate::tui::components::editor::EditorWidget {
                path: &path,
                status: status.label(),
                notice: notice.as_deref(),
                mode,
                focus,
                pending_command: &pending,
                command_active,
                command_line: &command_line,
                command_selection,
                scope,
                cursor_byte: cursor,
                writable,
                busy,
                snapshot: &snapshot,
                lsp: &self.editor_state.lsp,
                completion: &self.editor_state.completion,
                presentation: session.presentation_mut(),
                theme,
            }
            .render(frame, editor_area);
        }
    }

    /// M006-D: render the project-scoped file tree beside the editor.
    ///
    /// Borrows only `file_tree_state` and the theme, and is a no-op when the
    /// pane is hidden or did not fit, so the M006-A render path is unchanged
    /// in the default configuration.
    fn render_file_tree_pane(&self, frame: &mut Frame, area: Option<Rect>) {
        let Some(area) = area else { return };
        if !self.file_tree_state.visible {
            return;
        }
        crate::tui::components::file_tree::FileTreeView::new(
            &self.file_tree_state,
            self.ui_state.tree_focused,
            Arc::clone(&self.ui_state.theme),
        )
        .render(frame, area);
    }

    /// M005: non-modal Workspace primary view. Reads the bounded
    /// `WorkspaceDashboardState` projection; performs no I/O. The bottom
    /// composer stays editable (rendered separately) and the sidebar
    /// shows project chat for the same selection.
    fn render_workspace_view(&mut self, frame: &mut Frame, area: Rect) {
        use crate::tui::app::state::MAX_DASHBOARD_VISIBLE_ROWS;
        let theme = Arc::clone(&self.ui_state.theme);
        let block = Block::default()
            .title(" Workspace (Esc leaves · Enter descends when prompt empty · Space expands) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(dashboard) = self.dialog_state.workspace_dashboard.as_ref() else {
            let paragraph = Paragraph::new("Workspace closed — /workspace to open")
                .style(Style::default().fg(theme.muted));
            frame.render_widget(paragraph, inner);
            return;
        };
        let mut lines: Vec<Line> = Vec::new();
        if dashboard.loading {
            lines.push(Line::from(Span::styled(
                "Loading workspace…",
                Style::default().fg(theme.muted),
            )));
        }
        if let Some(error) = dashboard.error.as_ref() {
            lines.push(Line::from(Span::styled(
                format!("Error: {error}"),
                Style::default().fg(theme.error),
            )));
        }
        if !dashboard.query.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Filter: {}", dashboard.query),
                Style::default().fg(theme.secondary),
            )));
        }
        let indices = dashboard.filtered_indices();
        if indices.is_empty() && !dashboard.loading {
            lines.push(Line::from(Span::styled(
                "No projects match — adjust the filter",
                Style::default().fg(theme.muted),
            )));
        }
        for (visible_pos, row_idx) in indices.iter().take(MAX_DASHBOARD_VISIBLE_ROWS).enumerate() {
            let Some(row) = dashboard.rows.get(*row_idx) else {
                continue;
            };
            let is_selected = visible_pos == dashboard.selected_row;
            let badge = crate::tui::app::state::dashboard_row_badge(row);
            let stale = if row.stale { " stale" } else { "" };
            let expanded = if dashboard.expanded_project_id.as_deref()
                == Some(row.summary.project_id.as_str())
            {
                " [expanded]"
            } else {
                ""
            };
            let text = format!(
                "{} {} — {badge}{stale}{expanded}",
                row.summary.display_name, row.summary.project_id
            );
            let style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(text, style),
            ]));
            // Inline expansion for the selected project only.
            if dashboard.expanded_project_id.as_deref() == Some(row.summary.project_id.as_str()) {
                if dashboard.expanded_loading {
                    lines.push(Line::from(Span::styled(
                        "    Loading tasks…",
                        Style::default().fg(theme.muted),
                    )));
                } else if let Some(error) = dashboard.expanded_error.as_ref() {
                    lines.push(Line::from(Span::styled(
                        format!("    Task detail: {error}"),
                        Style::default().fg(theme.error),
                    )));
                } else if dashboard.expanded_tasks.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    No running tasks",
                        Style::default().fg(theme.muted),
                    )));
                } else {
                    for task in dashboard.expanded_tasks.iter() {
                        let title = task.title.as_deref().unwrap_or(&task.work_order_id);
                        lines.push(Line::from(Span::styled(
                            format!("    · {} [{}]", title, task.state),
                            Style::default().fg(theme.foreground),
                        )));
                    }
                }
            }
        }
        if dashboard.truncated {
            lines.push(Line::from(Span::styled(
                "More projects exist — refine the filter",
                Style::default().fg(theme.warning),
            )));
        }
        if dashboard.dirty {
            lines.push(Line::from(Span::styled(
                "Stale — Ctrl+R refreshes",
                Style::default().fg(theme.warning),
            )));
        }
        let focus_hint = match self.workspace_focus {
            crate::tui::app::state::WorkspaceFocus::Chat => {
                "Chat panel focused — Enter sends, Esc returns to composer"
            }
            crate::tui::app::state::WorkspaceFocus::Composer => {
                "Composer focused — type Session/Task input; Space expands, / focuses slash"
            }
        };
        lines.push(Line::from(Span::styled(
            focus_hint,
            Style::default().fg(theme.muted),
        )));
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }

    fn render_home(&mut self, frame: &mut Frame, area: Rect) {
        // Render a background block first to ensure the viewport isn't transparent
        let block = Block::default().style(Style::default().bg(self.ui_state.theme.background));
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  codegg ",
                Style::default()
                    .fg(self.ui_state.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Type a prompt to begin",
                Style::default().fg(self.ui_state.theme.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Ctrl+N  new session    Ctrl+L  change model    ?  help",
                Style::default().fg(self.ui_state.theme.muted),
            )),
        ];
        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn render_session(&mut self, frame: &mut Frame, area: Rect) {
        self.messages_state.messages.set_theme(&self.ui_state.theme);
        self.messages_state
            .messages
            .set_display_options(self.ui_state.show_thinking, self.ui_state.show_timestamps);

        let (content_area, scrollbar_area) = self.ui_state.layout.viewport_with_scrollbar(area);
        self.messages_state
            .messages
            .set_visible_height(content_area.height as usize);
        self.messages_state.messages.set_width(content_area.width);
        self.scrollbar_area = if scrollbar_area.width == 0 {
            None
        } else {
            Some(scrollbar_area)
        };

        // Render a background block first to ensure the viewport isn't transparent
        let block = Block::default().style(Style::default().bg(self.ui_state.theme.background));
        frame.render_widget(block, area);

        frame.render_widget(&self.messages_state.messages, content_area);

        if self.scrollbar_area.is_some() {
            let mut state = self
                .messages_state
                .messages
                .scrollbar_state(scrollbar_area.height as usize);
            frame.render_stateful_widget(
                Scrollbar::default()
                    .orientation(ScrollbarOrientation::VerticalRight)
                    .thumb_style(Style::default().fg(self.ui_state.theme.foreground))
                    .track_style(Style::default().fg(self.ui_state.theme.border))
                    .begin_symbol(None)
                    .end_symbol(None),
                scrollbar_area,
                &mut state,
            );
        }
    }

    /// Draw the todo strip directly above the input box.
    ///
    /// Compact form is a single `n/m completed` progress line. Expanded form
    /// adds one status-bubbled row per task, bounded by
    /// `MAX_TODO_STRIP_TASKS`; task titles are ellipsized to the available
    /// width rather than wrapped, so a long title can never push the input
    /// off screen or change the layout height.
    fn render_todo_strip(&mut self, frame: &mut Frame, area: Option<Rect>) {
        let Some(area) = area else { return };
        if area.width == 0 || area.height == 0 || self.todos.is_empty() {
            return;
        }

        let total = self.todos.len();
        let done = self.todos_completed();
        let remaining = total.saturating_sub(done);
        let theme = self.ui_state.theme.clone();

        let mut lines: Vec<Line> = Vec::new();
        if self.todo_expanded {
            lines.push(Line::from(Span::styled(
                format!(" Todo list {done}/{total} · {remaining} remaining · Ctrl+T compact "),
                Style::default().fg(theme.muted),
            )));
            for todo in self
                .todos
                .iter()
                .take(crate::tui::app::MAX_TODO_STRIP_TASKS)
            {
                let bubble = format!(" {} ", Self::todo_bubble(&todo.status));
                // 3 columns of leading indent, minus the bubble and its spaces.
                let budget = (area.width as usize)
                    .saturating_sub(3 + unicode_width::UnicodeWidthStr::width(bubble.as_str()));
                let title = crate::tui::wrap::ellipsize_to_width(&todo.content, budget);
                lines.push(Line::from(vec![
                    Span::styled("   ", Style::default()),
                    Span::styled(bubble, Style::default().fg(theme.primary)),
                    Span::styled(title, Style::default().fg(theme.foreground)),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                format!(" Todo list: {done}/{total} completed · Ctrl+T expand "),
                Style::default().fg(theme.muted),
            )));
        }

        let shown = (area.height as usize).min(lines.len());
        frame.render_widget(Paragraph::new(lines[..shown].to_vec()), area);
    }

    /// How many rows the tasks overlay may reserve on a pane of `pane_height`.
    ///
    /// Returns the space left after honouring every other claim on the pane:
    /// the header and footer, the transcript's `MIN_TRANSCRIPT` floor, the
    /// prompt box at its own maximum, and the working strip, hint line, and
    /// todo strip above it.
    ///
    /// The floor is the load-bearing part. `session_layout` splits the pane
    /// into `Min(10)` for the transcript and a fixed height for the prompt, so
    /// an overlay that overshoots does not merely render short — it makes the
    /// carve below yield, and `Shift+T` stops doing anything at all. Clamping
    /// here makes a short pane drop the overlay instead, which is the honest
    /// outcome: a truncated transcript is worse than no overlay.
    fn tasks_overlay_allowance(
        &self,
        pane_height: u16,
        todo_rows: u16,
        hint_rows: u16,
        working_rows: u16,
        max_prompt_height: u16,
    ) -> u16 {
        const MIN_TRANSCRIPT: u16 = 10;
        let config = &self.ui_state.layout.config;
        let claimed = config
            .header_height
            .saturating_add(config.footer_height)
            .saturating_add(MIN_TRANSCRIPT)
            .saturating_add(max_prompt_height)
            .saturating_add(todo_rows)
            .saturating_add(hint_rows)
            .saturating_add(working_rows);
        pane_height.saturating_sub(claimed)
    }

    /// Draw the unified tasks overlay (`Shift+T`) directly above the input.
    ///
    /// Renders three projection-backed groups — todos, in-flight subagents,
    /// in-flight jobs — each preceded by its own header, with empty groups
    /// omitted entirely. Every line is ellipsized to the available width
    /// rather than wrapped, so a long description can never change the
    /// overlay's height and reflow the transcript underneath it.
    fn render_tasks_strip(&mut self, frame: &mut Frame, area: Option<Rect>) {
        let Some(area) = area else { return };
        if area.width == 0 || area.height == 0 {
            return;
        }

        let theme = self.ui_state.theme.clone();
        let all_rows = self.task_rows();
        let rows = bounded_task_rows(all_rows.clone());

        let mut lines: Vec<Line> = Vec::new();
        if rows.is_empty() {
            // The overlay is open and there is genuinely nothing to wait on.
            // Say so rather than rendering an empty block, so the user learns
            // the key works instead of concluding it is broken.
            lines.push(Line::from(Span::styled(
                " Nothing pending · Shift+T close ".to_string(),
                Style::default().fg(theme.muted),
            )));
        } else {
            for row in &rows {
                match row {
                    TaskRow::Header { group } => {
                        lines.push(Line::from(Span::styled(
                            format!(" ── {} ──", group.title()),
                            Style::default().fg(theme.muted),
                        )));
                    }
                    TaskRow::Todo { bubble, content } => {
                        lines.push(strip_line(
                            &theme,
                            area.width,
                            &format!(" {bubble} "),
                            content,
                            theme.foreground,
                        ));
                    }
                    TaskRow::Subagent {
                        label,
                        agent,
                        description,
                        status,
                    } => {
                        // Name and agent type together: the name is what the
                        // user looks for later, the type is what it actually is.
                        let lead = if label == agent {
                            format!(" {label} ")
                        } else {
                            format!(" {label} ({agent}) ")
                        };
                        let text = if description.trim().is_empty() {
                            lead.clone()
                        } else {
                            format!("{lead}· {status} — {description}")
                        };
                        lines.push(strip_line(&theme, area.width, " ", &text, theme.foreground));
                    }
                    TaskRow::Job {
                        kind,
                        summary,
                        state,
                        error_class,
                    } => {
                        let text = if summary.trim().is_empty() {
                            format!(" {kind} · {state}")
                        } else {
                            format!(" {kind} · {state} — {summary}")
                        };
                        let colour = if error_class.is_some() {
                            theme.error
                        } else {
                            theme.foreground
                        };
                        lines.push(strip_line(&theme, area.width, " ", &text, colour));
                    }
                }
            }
            lines.push(Line::from(Span::styled(
                " Shift+T close ".to_string(),
                Style::default().fg(theme.muted),
            )));
        }

        let shown = (area.height as usize).min(lines.len());
        frame.render_widget(Paragraph::new(lines[..shown].to_vec()), area);
    }

    /// Render the open subagent buffer over the transcript.
    ///
    /// Shows only what the projection actually vouches for — the agent, its
    /// task description, the lifecycle status, and the canonical ids needed
    /// to reach durable run detail. It deliberately does not synthesize
    /// progress text: the projection adapter hardcodes `progress: None`, so
    /// an invented "working on step 3" line would be a lie the user acts on.
    fn render_subagent_view(&mut self, frame: &mut Frame, area: Rect) {
        let Some(view) = self.ui_state.subagents.open.clone() else {
            return;
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        let theme = self.ui_state.theme.clone();
        let status_style = if view.row.is_running() {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.muted)
        };

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!(" Subagent · {}", view.row.agent),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(" {}", view.row.status),
            status_style,
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " Task",
            Style::default().fg(theme.muted),
        )));
        lines.push(Line::from(Span::styled(
            format!("   {}", view.row.description),
            Style::default().fg(theme.foreground),
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " Identity",
            Style::default().fg(theme.muted),
        )));
        lines.push(Line::from(Span::styled(
            format!("   task {}", view.row.task_id),
            Style::default().fg(theme.foreground),
        )));
        if let Some(run_id) = &view.row.run_id {
            lines.push(Line::from(Span::styled(
                format!("   run  {run_id}"),
                Style::default().fg(theme.foreground),
            )));
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " Esc returns to the main buffer".to_string(),
            Style::default().fg(theme.muted),
        )));

        let width = area.width.saturating_sub(1);
        let paragraph = Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((view.scroll, 0));
        frame.render_widget(paragraph, Rect { width, ..area });
    }

    /// Rows the working strip needs.
    ///
    /// Row 0 is **unconditionally reserved**. It used to be gated on "there is
    /// something to say", but this height is subtracted from the transcript
    /// (`session_layout` splits `prompt_height` against `Min(10)` for the
    /// buffer), so every row appearing or vanishing reflowed the transcript and
    /// moved whatever the user was reading. Gating it on turn state meant the
    /// buffer jumped on every submit and again on every completion.
    ///
    /// Reserving it permanently turns that off for good: the strip's *content*
    /// can now change freely — idle, working, stopped, failed — without the
    /// transcript viewport ever changing height.
    ///
    /// Subagent rows remain content-driven, because N subagents genuinely
    /// need N rows. That variation is real information rather than a state
    /// transition, and it only moves while a turn is in flight.
    fn working_strip_rows(&self) -> u16 {
        let total = self.ui_state.subagents.rows.len();
        let shown = total.min(MAX_WORKING_STRIP_SUBAGENTS);
        // Reserve a row for the overflow marker when the list is truncated.
        // Without the reservation the marker has nowhere to draw — the rows
        // exactly fill the strip — so a wide fan-out would silently drop
        // subagents with no indication that any were hidden.
        let overflow = u16::from(total > shown);
        1 + shown as u16 + overflow
    }

    /// Draw the working strip: the single status surface for the turn.
    ///
    /// Row 0 is always present and always answers "is the model healthy, stuck,
    /// or broken?" — the question a long pause raises and the strip used to be
    /// silent about. It then draws one clickable row per advertised subagent.
    ///
    /// Hit rects are recorded as they are drawn so a click resolves to the
    /// row the user actually saw rather than a recomputed layout.
    fn render_working_strip(&mut self, frame: &mut Frame, chunk: Option<Rect>) {
        // Expire a lapsed stop confirmation here rather than only on the next
        // keypress: the pump runs on every animation tick, so a prompt the
        // user walked away from clears itself instead of waiting to be
        // overwritten by whatever they type next.
        crate::tui::commands::turn_control::expire_pending_stop_armed(self);
        let Some(area) = chunk else {
            self.ui_state.working_area = None;
            self.ui_state.subagents.hits.clear();
            return;
        };
        if area.width == 0 || area.height == 0 {
            self.ui_state.working_area = None;
            self.ui_state.subagents.hits.clear();
            return;
        }

        self.ui_state.working_area = Some(area);
        // Rebuilt from scratch each frame: a stale rect from a previous
        // layout would let a click land on a row that has since moved.
        self.ui_state.subagents.hits.clear();
        let theme = self.ui_state.theme.clone();
        let mut lines: Vec<Line> = Vec::new();

        // Row 0 is the whole status surface. Its right-hand side is the reason a
        // long pause reads as progress rather than as a disconnect: it names
        // the one key that changes the situation. An indicator that only
        // reports the current phase still leaves a user staring at a frozen
        // screen with no way out.
        let is_working = matches!(self.session_state.session_status, SessionStatus::Working);
        let key = crate::tui::commands::turn_control::STOP_KEY_LABEL;
        let stop_hint = if self.session_state.stopped_turn {
            Some(format!("press {key} or /start"))
        } else if is_working {
            // A stop waits for a confirming second press, so while the arm is
            // live the line must say exactly which press completes it —
            // otherwise the confirmation is invisible and the key looks dead.
            Some(if crate::tui::commands::turn_control::stop_armed(self) {
                format!("press {key} again to stop agent")
            } else {
                format!("{key} to stop")
            })
        } else {
            None
        };

        // Precedence is a debugging aid, not a prettiness ranking: a latched
        // failure outranks every live phase because it is the one thing that
        // explains why nothing is progressing, and by the time it is readable
        // here the toast that also said so has expired.
        let (body, color) = if let Some(error) = self.session_state.last_error.as_deref() {
            // A static glyph: nothing is running, so an animated one would lie
            // about it. The full text stays in the toast and in `/logs`; this
            // row is the durable one-line reminder.
            (format!("✗ {error}"), theme.error)
        } else if self.session_state.stopped_turn {
            // Stopped is not a phase of a running turn, so it does not get a
            // spinner either.
            ("■ stopped".to_string(), theme.warning)
        } else if is_working {
            let line = self
                .messages_state
                .messages
                .activity
                .as_ref()
                .map(|activity| activity.line())
                .unwrap_or_else(|| self.working_elapsed_fallback());
            (line, theme.primary)
        } else {
            // Idle is an answer, not an absence. Naming it is what lets row 0
            // be reserved permanently without reading as a broken widget.
            ("idle".to_string(), theme.muted)
        };

        let mut head = body;
        if let Some(hint) = stop_hint {
            head.push_str(" · ");
            head.push_str(&hint);
        }
        let running_subs = self.ui_state.subagents.running_count();
        if running_subs > 0 && !self.session_state.stopped_turn {
            head.push_str(&format!(" · {running_subs} subagent"));
            if running_subs != 1 {
                head.push('s');
            }
        }
        lines.push(Line::from(Span::styled(
            format!(" {head}"),
            Style::default().fg(color),
        )));

        for (index, sub) in self
            .ui_state
            .subagents
            .rows
            .iter()
            .take(MAX_WORKING_STRIP_SUBAGENTS)
            .enumerate()
        {
            let summary = sub.summary_line(area.width as usize);
            lines.push(Line::from(Span::styled(
                summary,
                Style::default().fg(if sub.is_running() {
                    theme.foreground
                } else {
                    theme.muted
                }),
            )));
            // Row 1 is the throbber, so subagent row `index` sits one line
            // below the top of the strip.
            let y = area.y + 1 + index as u16;
            if y < area.y + area.height {
                self.ui_state
                    .subagents
                    .hits
                    .push((index, Rect::new(area.x, y, area.width, 1)));
            }
        }

        // Rows that did not fit are summarized rather than silently dropped:
        // a user who cannot see how many subagents are left has no way to
        // tell the difference between "all done" and "off screen".
        let hidden = self
            .ui_state
            .subagents
            .rows
            .len()
            .saturating_sub(MAX_WORKING_STRIP_SUBAGENTS);
        if hidden > 0 && lines.len() < area.height as usize {
            lines.push(Line::from(Span::styled(
                format!(" … +{hidden} more"),
                Style::default().fg(theme.muted),
            )));
        }

        let shown = (area.height as usize).min(lines.len());
        frame.render_widget(Paragraph::new(lines[..shown].to_vec()), area);
    }

    /// A phase line for a working turn that has no `ActivityIndicator` yet.
    ///
    /// The strip must not blink out during the window between the turn being
    /// marked `Working` and the first event that names a phase, so this
    /// falls back to the turn's own elapsed time.
    ///
    /// It used to hardcode the first frame of the table, so the one interval
    /// that most needs to read as alive — the long silence after a tool
    /// returns, while the model decides what to do next — was the only one
    /// that could not animate. It now animates on the shared clock like every
    /// other phase.
    fn working_elapsed_fallback(&self) -> String {
        let since = self.session_state.working_since;
        let secs = since.map(|s| s.elapsed().as_secs()).unwrap_or(0);
        let glyph = match since {
            Some(started) => crate::tui::components::spinner::spinner_frame_since(started),
            None => {
                crate::tui::components::spinner::spinner_frame(std::time::Duration::from_secs(0))
            }
        };
        format!("{glyph} working {secs}s")
    }

    /// The contextual key line for whatever currently owns the keyboard.
    ///
    /// Rendered from the panel's own `composer_hints`, so a panel advertises
    /// its verbs once and they appear next to the input they modify. Kept out
    /// of each panel's own chrome: inside the panel they described the
    /// panel, while the keys actually operate on the shared composer.
    fn render_composer_hints(&mut self, frame: &mut Frame, chunk: Option<Rect>) {
        let Some(area) = chunk else {
            return;
        };
        if area.width < 4 || area.height == 0 {
            return;
        }
        let Some(text) = self.composer_hint_text() else {
            return;
        };
        let budget = usize::from(area.width).saturating_sub(4);
        let theme = &self.ui_state.theme;
        let line = Line::from(vec![
            Span::styled(
                " \u{258c} ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                crate::tui::wrap::ellipsize_to_width(&text, budget),
                Style::default().fg(theme.muted),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_prompt(&mut self, frame: &mut Frame, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(self.ui_state.theme.input_bg));
        frame.render_widget(bg_block, area);

        // The active agent rides alongside the input-mode badge (`[INS]
        // [build]`) so the mode a prompt will actually run under is visible
        // without opening the agent picker. Bounded so a long agent name
        // cannot crowd out the prompt's own prefix row.
        //
        // The destination badge names the surface that currently has the
        // keyboard — `[LLM]`, `[CHAT]`, `[SESSIONS]` — rather than listing
        // every reachable destination. Showing both send targets at once was
        // meant to answer "where am I about to send this?", but it rendered
        // `[CHAT]` unconditionally and put a second `[BUILD]` beside the agent
        // name, so the row read as three badges about two axes and answered
        // the question worse than a single accurate one does.
        let target = self.input_target();
        // One destination badge, naming the surface that currently has the
        // keyboard. See `App::composer_destination_title`.
        let destination_title = self.composer_destination_title();
        let destination_style = if target == crate::tui::panels::InputTarget::Chat {
            Style::default()
                .fg(self.ui_state.theme.warning)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.ui_state.theme.primary)
                .add_modifier(Modifier::BOLD)
        };
        // The agent only runs a prompt; naming it while chat is armed would
        // imply a delivery that is not going to happen.
        let agent_badge = match target {
            crate::tui::panels::InputTarget::Chat => String::new(),
            crate::tui::panels::InputTarget::Build => self
                .agent_state
                .agents
                .get(self.agent_state.current_agent)
                .map(|agent| {
                    let name =
                        crate::tui::wrap::ellipsize_to_width(&agent.name, MAX_AGENT_BADGE_WIDTH);
                    format!(" [{name}]")
                })
                .unwrap_or_default(),
        };
        let mode_span_style = match self.ui_state.input_mode {
            InputMode::Insert => Style::default()
                .fg(self.ui_state.theme.secondary)
                .add_modifier(Modifier::BOLD),
            InputMode::Normal => Style::default()
                .fg(self.ui_state.theme.primary)
                .add_modifier(Modifier::BOLD),
        };
        let mode_label = match self.ui_state.input_mode {
            InputMode::Insert => "[INS]",
            InputMode::Normal => "[NOR]",
        };
        // An armed chat reply/edit is a *third* state neither badge covers:
        // `[CHAT]` says the destination but not that Enter will thread or
        // replace a specific message, which is exactly what a user needs to
        // be reminded of before pressing it.
        let context_badge = self
            .prompt_state
            .chat_seed
            .as_ref()
            .filter(|seed| seed.is_contextual())
            .map(|seed| {
                Span::styled(
                    format!("[{}] ", seed.label().to_uppercase()),
                    Style::default()
                        .fg(self.ui_state.theme.warning)
                        .add_modifier(Modifier::BOLD),
                )
            });
        // `[NOR] [agent] [LLM]` — one row carrying the mode, the agent that
        // will run the prompt, and the single surface this text is aimed at,
        // plus the armed chat context when there is one.
        let mut badge_spans = vec![
            Span::styled(mode_label.to_string(), mode_span_style),
            Span::styled(agent_badge.clone(), mode_span_style),
            Span::styled(format!("[{}] ", destination_title), destination_style),
        ];
        badge_spans.extend(context_badge);
        let mode_indicator = Line::from(badge_spans);
        let session_prefix = match &self.session_state.session_status {
            SessionStatus::Working => Span::styled(
                " ● ",
                Style::default()
                    .fg(self.ui_state.theme.warning)
                    .add_modifier(Modifier::BOLD),
            ),
            SessionStatus::Error => Span::styled(
                " ✗ ",
                Style::default()
                    .fg(self.ui_state.theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            SessionStatus::Idle => Span::styled(
                " ❯ ",
                Style::default()
                    .fg(self.ui_state.theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        };
        self.prompt_state.prompt.set_theme(&self.ui_state.theme);
        self.prompt_state.prompt.set_mode_indicator(mode_indicator);
        self.prompt_state.prompt.set_prefix(session_prefix);

        let prompt_text = self.prompt_state.prompt.get_text();
        let placeholder = if target == crate::tui::panels::InputTarget::Chat {
            "Message the project chat…".to_string()
        } else if prompt_text.starts_with('!') {
            "shell: run locally; not included in model context".to_string()
        } else if self.prompt_state.composer_mode.is_task() {
            "Describe the task to schedule… (Enter opens sheet, Tab for session)".to_string()
        } else {
            "Ask anything…".to_string()
        };
        self.prompt_state.prompt.set_placeholder(placeholder);
        let visible_lines = area.height.saturating_sub(2) as usize;
        self.prompt_state
            .prompt
            .ensure_cursor_visible_with_width(visible_lines, area.width as usize);
        frame.render_widget(&self.prompt_state.prompt, area);

        // Command mode deliberately does NOT draw `dialog_state.command_palette`
        // over the prompt any more. That widget is a second, independent
        // slash-command popup: 50 columns instead of 110, a raw
        // `selection`-on-`primary` highlight that themes render as an
        // unreadable solid block, and its own fuzzy ordering. Because
        // `update_completions` used to leave `show_completions` false in
        // command mode, typing `/logs` showed that box, while backspacing to
        // a bare `/` cleared command mode and showed `render_completions`
        // instead — one interaction, two boxes. `render_completions` is now
        // the only popup, and it is driven by `show_completions` in every
        // mode.
    }

    fn render_tab_strip(&mut self, frame: &mut Frame, area: Rect) {
        use crate::tui::app::state::project_picker::disambiguate_label;

        let labels = self.project_tabs.display_labels();
        let active_id = self.project_tabs.active_tab_id().cloned();
        let total = labels.len();
        if total == 0 {
            return;
        }

        // Sliding visible window around the active tab, max 7 tabs.
        const MAX_VISIBLE: usize = 7;
        let active_idx = active_id
            .as_ref()
            .and_then(|id| labels.iter().position(|(tid, _)| tid == id))
            .unwrap_or(0);
        let window_start = active_idx.saturating_sub(MAX_VISIBLE / 2);
        let window_end = (window_start + MAX_VISIBLE).min(total);
        let window_start = window_end.saturating_sub(MAX_VISIBLE);

        let mut spans: Vec<Span> = Vec::new();
        if window_start > 0 {
            spans.push(Span::styled(
                "‹ ",
                Style::default().fg(self.ui_state.theme.muted),
            ));
        }
        for (_i, (tid, label)) in labels
            .iter()
            .enumerate()
            .take(window_end)
            .skip(window_start)
        {
            let is_active = active_id.as_ref() == Some(tid);
            let suffix_source = tid.as_str();
            let label_str: &str = label.as_str();
            let display = if labels
                .iter()
                .filter(|(_, l)| l.as_str() == label_str)
                .count()
                > 1
            {
                disambiguate_label(label_str, suffix_source)
            } else {
                label.clone()
            };
            let display = crate::tui::app::state::project_picker::truncate_tab_label(&display);

            let prefix = if is_active { "▸" } else { " " };
            let (fg, bg) = if is_active {
                (self.ui_state.theme.primary, self.ui_state.theme.selection)
            } else {
                (
                    self.ui_state.theme.foreground,
                    self.ui_state.theme.background,
                )
            };
            spans.push(Span::styled(
                format!("{} {} ", prefix, display),
                Style::default().fg(fg).bg(bg),
            ));
        }
        if window_end < total {
            spans.push(Span::styled(
                "›",
                Style::default().fg(self.ui_state.theme.muted),
            ));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }

    fn render_footer(&mut self, frame: &mut Frame, area: Rect) {
        let mut summary = self.build_status_summary();

        let token_str = format_token_line(
            self.session_state.token_in,
            self.session_state.token_out,
            self.session_state.live_output_tokens,
            self.session_state.context_tokens as u64,
            self.session_state.context_limit as u64,
        );
        summary.secondary = Some(token_str);

        self.status_bar.set_theme(&self.ui_state.theme);
        self.status_bar.apply_summary(&summary);
        self.status_bar
            .set_project_context(self.active_project_display_context());

        // M007: daemon-resolved effective policy line (cached from the
        // last snapshot; cleared until the first snapshot lands).
        self.status_bar
            .set_policy_status(self.policy_ui.status_line());

        // The LSP status is a cache: computing it awaits the live LSP
        // service, which cannot happen here (the render pass runs inside
        // the event loop's runtime context, where `Handle::block_on`
        // panics). `maybe_refresh_lsp_status` refreshes it off the render
        // path and `LspStatusRefreshed` fills this cache.
        self.status_bar
            .set_lsp_status(self.lsp_status_cache.clone());

        frame.render_widget(&self.status_bar, area);
    }

    pub fn build_status_summary(&self) -> TuiStatusSummary {
        let mut primary = String::from("idle");
        let mut activity: Vec<String> = Vec::new();

        if let Some(ref err) = self.ui_state.last_render_error {
            primary = format!("degraded: {err}");
        } else if self.session_state.permission_pending {
            primary = "permission pending".to_string();
        } else if self.dialog_state.question_dialog.is_some() {
            primary = "question pending".to_string();
        } else if self.security_review_running.is_some() {
            primary = "security review".to_string();
        } else if self.session_state.session_status == SessionStatus::Working {
            primary = "working".to_string();
        } else if !self.shell_handles.is_empty() {
            primary = "shell running".to_string();
        } else if self.task_registry.active_count() > 0 {
            primary = format!("bg:{}", self.task_registry.active_count());
        } else if self.session_state.session_status == SessionStatus::Error {
            primary = "error".to_string();
        }

        let undo_message = if self.undo_session_id.is_some() {
            if let Some(until) = self.undo_until {
                if Instant::now() < until {
                    Some("Session deleted — press U to undo".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let agent_name = &self.agent_state.agents[self.agent_state.current_agent].name;
        activity.push(format!("agent:{agent_name}"));

        if self.session_state.subagent_count > 0 {
            activity.push(format!("subagents:{}", self.session_state.subagent_count));
        }

        if self.dialog_state.session_reload_request.is_loading() {
            activity.push("reloading".to_string());
        }

        if self.dialog_state.import_request.is_loading() {
            activity.push("importing".to_string());
        }

        if self.dialog_state.research_request.is_loading() {
            activity.push("research".to_string());
        }

        if self.dialog_state.session_messages_request.is_loading() {
            activity.push("messages".to_string());
        }

        if self.dialog_state.session_mutation_request.is_loading() {
            activity.push("mutating".to_string());
        }

        if self.dialog_state.task_list_request.is_loading() {
            activity.push("tasks".to_string());
        }

        if self.dialog_state.worktree_list_request.is_loading() {
            activity.push("worktrees".to_string());
        }

        if self.dialog_state.template_create_request.is_loading() {
            activity.push("template".to_string());
        }

        let memory_tasks = self
            .task_registry
            .iter()
            .filter(|(_, r)| r.kind == crate::tui::task_lifecycle::TuiTaskKind::Memory)
            .count();
        if memory_tasks > 0 {
            activity.push(format!("mem:{memory_tasks}"));
        }

        let active_tasks = self.task_registry.active_count();
        if active_tasks > 0 {
            activity.push(format!("tasks:{active_tasks}"));
        }

        if !self.shell_handles.is_empty() {
            activity.push(format!("shell:{}", self.shell_handles.len()));
        }

        // Tool programs from projection snapshot
        if let Some((_, snapshot)) = self.projection_client.active_snapshot() {
            let active_programs = snapshot
                .tool_programs
                .iter()
                .filter(|p| p.state != "completed" && p.state != "failed")
                .count();
            if active_programs > 0 {
                activity.push(format!("programs:{active_programs}"));
            }
            let active_agent_runs = snapshot
                .agent_runs
                .iter()
                .filter(|run| {
                    !matches!(
                        run.status.as_str(),
                        "completed" | "failed" | "interrupted" | "cancelled"
                    )
                })
                .count();
            if active_agent_runs > 0 {
                activity.push(format!("agent-runs:{active_agent_runs}"));
            }
            let attention_runs = snapshot
                .agent_runs
                .iter()
                .filter(|run| run.attention_required)
                .count();
            if attention_runs > 0 {
                activity.push(format!("attention:{attention_runs}"));
            }
        }

        let pending_diffs = self
            .session_state
            .changed_files
            .iter()
            .filter(|f| {
                matches!(
                    f.diff_state,
                    crate::tui::app::state::session::DiffStatsState::Pending { .. }
                )
            })
            .count();
        if pending_diffs > 0 {
            activity.push(format!("diff:{pending_diffs}"));
        }

        if self.security_review_running.is_some() {
            activity.push("security".to_string());
        }

        // M004: shared-session controller indicator (cached lease for
        // the active session; display only, refreshed via `/control`).
        let active_session = self.active_session_id().map(str::to_string);
        if let Some(indicator) =
            crate::tui::commands::control::control_indicator_for(self, active_session.as_deref())
        {
            activity.push(indicator);
        }

        if let Some(ref goal) = self.active_goal {
            activity.push(format!("goal:{}", format_goal_status_line(goal)));
        }

        TuiStatusSummary {
            primary,
            secondary: None,
            activity,
            undo_message,
        }
    }

    /// Paint the panel docked to `placement`.
    ///
    /// Split from [`render_sidebar`](Self::render_sidebar) so the two differ in
    /// exactly one thing: which slot's panel they read, and whether that slot is
    /// the one holding the keyboard. Keeping them together is what would let the
    /// focus flag drift between "the edge I am painting" and "the edge I am
    /// driving", which shows up as a panel that renders unfocused while the badge
    /// claims it is focused.
    fn render_panel_at(&mut self, frame: &mut Frame, placement: PanelPlacement, area: Rect) {
        self.populate_session_panel();
        let mut ctx = crate::tui::panels::PanelCtx {
            theme: Arc::clone(&self.ui_state.theme),
            // `now_ms` is display-only (composing expiry); no I/O.
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            session: &mut self.sidebar,
            chat: &mut self.chat,
            terminal: &mut self.interactive_terminals,
            vim_mode: self.ui_state.vim_mode,
            chat_project: self.chat_panel_project.as_deref(),
            area,
            focused: self.panel_host.is_focused_at(placement),
            input_mode: self.ui_state.input_mode,
        };
        // `Clear` first: the previous panel's pixels must not show through
        // a narrower panel (the chat panel is shorter than the sidebar).
        frame.render_widget(Clear, area);
        self.panel_host
            .render_at(placement, &mut ctx, frame.buffer_mut(), area);
    }

    /// Push session projection into the Session panel's widget.
    ///
    /// Split out of `render_panel_at` so the sidebar's field population is
    /// readable on its own; it performs no I/O and reads only cached state.
    fn populate_session_panel(&mut self) {
        // The Workspace route no longer forces this panel. It selects the
        // Chat panel through the host instead (see
        // `workspace_dashboard::open_workspace`), which keeps the previous
        // behaviour while leaving the choice switchable on every route.
        let project_scope = self.active_project_id().map(str::to_string);
        self.sidebar.set_project_scope(project_scope.as_deref());
        if let Some(ref sess) = self.session_state.session {
            self.sidebar.set_session(sess);
        }
        self.sidebar
            .set_agent(&self.agent_state.agents[self.agent_state.current_agent].name);
        self.sidebar.set_model(&self.agent_state.current_model);
        let provider = self
            .agent_state
            .current_model
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        self.sidebar.set_provider(&provider);
        self.sidebar
            .set_mcp_servers(self.session_state.mcp_servers.clone());
        self.sidebar.set_file_changes(
            self.session_state
                .changed_files
                .iter()
                .map(|file| crate::tui::components::sidebar::SidebarFileChange {
                    path: file.path.to_string_lossy().into_owned(),
                    action: file.action.clone(),
                    diff_preview: file.diff_preview.clone(),
                    diff_state: file.diff_state.clone(),
                })
                .collect(),
        );

        if let Some(ref sess) = self.session_state.session {
            let cached = &self.session_state.git_sidebar;
            let display_root = cached
                .root
                .clone()
                .or_else(|| Some(sess.project_id.clone()));
            self.sidebar
                .set_git_status(cached.loading, cached.error.clone());
            self.sidebar.set_git_info(GitSidebarInfo {
                root: display_root,
                branch: cached.branch.clone(),
                dirty: cached.dirty,
                staged_count: cached.staged_count,
                unstaged_count: cached.unstaged_count,
                untracked_count: cached.untracked_count,
                conflicted_count: cached.conflicted_count,
                ahead: cached.ahead,
                behind: cached.behind,
                operation_state_label: cached.operation_state_label.clone(),
                available_actions: cached.available_actions.clone(),
                conflicted_paths: cached.conflicted_paths.clone(),
            });
        } else {
            self.sidebar.set_git_status(false, None);
            self.sidebar.set_git_info(GitSidebarInfo::default());
        }

        let derived = &self.session_state_derived;
        self.sidebar.set_goal(derived.goal.clone());
        self.sidebar.set_plan(derived.plan.clone());

        // Derive the subagent rows first and let the snapshot borrow end
        // here: the sidebar writes below all need `&mut self`, so holding the
        // snapshot across them would conflict.
        let subagent_rows = self
            .projection_client
            .active_snapshot()
            .and_then(|(_, snapshot)| Self::sync_subagents_from_snapshot(snapshot));

        // Populate tool programs from projection snapshot
        if let Some((_, snapshot)) = self.projection_client.active_snapshot() {
            use crate::tui::components::sidebar::{
                SidebarAgentRun, SidebarConvergence, SidebarToolProgram,
            };
            self.sidebar.set_tool_programs(
                snapshot
                    .tool_programs
                    .iter()
                    .map(|p| SidebarToolProgram {
                        program_id: p.program_id.clone(),
                        state: p.state.clone(),
                        language: p.language.clone(),
                        calls_completed: p.calls_completed,
                        summary: p.last_progress.clone(),
                    })
                    .collect(),
            );
            self.sidebar.set_agent_runs(
                snapshot
                    .agent_runs
                    .iter()
                    .map(|run| SidebarAgentRun {
                        run_id: run.run_id.clone(),
                        task_id: Some(run.task_id.clone()),
                        agent: run.agent.clone(),
                        status: run.status.clone(),
                        worktree: run.worktree_id.clone(),
                        branch: run.branch.clone(),
                        result_commit: run.result_commit.clone(),
                        attention_required: run.attention_required,
                        progress: run.progress.clone(),
                    })
                    .collect(),
            );
            self.sidebar
                .set_run_groups(snapshot.run_groups.iter().cloned().collect());
            self.sidebar.set_agent_tree(
                snapshot
                    .active_turn
                    .as_ref()
                    .map(|turn| turn.agent_tree.iter().cloned().collect())
                    .or_else(|| {
                        snapshot
                            .recent_turns
                            .front()
                            .map(|turn| turn.agent_tree.iter().cloned().collect())
                    })
                    .unwrap_or_default(),
            );
            self.sidebar.set_convergences(
                snapshot
                    .convergences
                    .iter()
                    .map(|convergence| SidebarConvergence {
                        convergence_id: convergence.convergence_id.clone(),
                        status: convergence.status.clone(),
                        cycle_ordinal: convergence.cycle_ordinal,
                        max_cycles: convergence.max_cycles,
                        remaining_cycles: convergence.remaining_cycles,
                        producer_completed: convergence.producer_completed,
                        producer_active: convergence.producer_active,
                        verifier_run_id: convergence.verifier_run_id.clone(),
                        verdict_kind: convergence.verdict_kind.clone(),
                        awaiting_decision: convergence.awaiting_decision,
                        selected_run_id: convergence.selected_run_id.clone(),
                        selected_result_commit: convergence.selected_result_commit.clone(),
                        last_finding_count: convergence.last_finding_count,
                    })
                    .collect(),
            );
        } else {
            self.sidebar.set_tool_programs(Vec::new());
            self.sidebar.set_agent_runs(Vec::new());
            self.sidebar.set_agent_tree(Vec::new());
            self.sidebar.set_convergences(Vec::new());
        }

        // Only a snapshot that *arrives* replaces the rows. A missing
        // snapshot is not evidence that no subagents exist — it means the
        // projection stream has not spoken yet or just hiccupped — so
        // clearing on it would blank the strip mid-turn and hide work that
        // is genuinely in flight. An arriving snapshot with no tree is
        // authoritative emptiness and does clear.
        if let Some(rows) = subagent_rows {
            self.ui_state.subagents.set_rows(rows);
        }
    }

    /// Project the snapshot's agent tree into the main buffer's subagent rows.
    ///
    /// Reads the same `active_turn` (falling back to the newest
    /// `recent_turns`) tree the sidebar renders, so the two surfaces cannot
    /// disagree about which delegated runs exist. The durable `run_id` and the
    /// human-readable `display_name` are read straight off the node, having
    /// been carried there by the event; a task with no matching run still gets
    /// a row rather than being hidden, because the agent tree is the
    /// authoritative record that the task was dispatched.
    ///
    /// Returns `None` when there is no snapshot to read at all, which is not
    /// the same as an empty tree: the caller keeps the rows it already has
    /// rather than blanking the strip mid-turn.
    fn sync_subagents_from_snapshot(
        snapshot: &crate::protocol::projection::SessionProjectionSnapshot,
    ) -> Option<Vec<SubagentRow>> {
        use crate::protocol::projection::dto::AgentTreeStatus;

        let tree = snapshot
            .active_turn
            .as_ref()
            .or_else(|| snapshot.recent_turns.front());

        let Some(tree) = tree else {
            // The snapshot arrived and carries no agent tree at all: that is
            // authoritative, so it clears rather than keeps the last set.
            return Some(Vec::new());
        };

        Some(
            tree.agent_tree
                .iter()
                .map(|node| {
                    let task_id = node.task_id.to_string();
                    SubagentRow {
                        // The durable run id now arrives **in the event**
                        // (`CoreEvent::SubagentStarted::run_id`) and is copied
                        // onto the node verbatim. It used to be recovered by
                        // joining this node's `task_id` against
                        // `AgentRunSummary::task_id`, which could never match:
                        // the node's is a daemon-local `u64` rendered in
                        // decimal, the summary's is a UUID string. The field
                        // was therefore permanently `None`.
                        run_id: node.run_id.clone(),
                        task_id,
                        display_name: node.display_name.clone(),
                        agent: node.agent.clone(),
                        description: node.description.clone(),
                        status: match node.status {
                            AgentTreeStatus::Running => "running",
                            AgentTreeStatus::Completed => "completed",
                            AgentTreeStatus::Failed => "failed",
                        }
                        .to_string(),
                    }
                })
                .collect(),
        )
    }

    fn render_dialog(&mut self, frame: &mut Frame, area: Rect) {
        if self.focus_manager.is_empty() {
            return;
        }

        // The project picker renders directly from `App::dialog_state.project_picker`
        // because the picker state is owned by the App, not the
        // focus manager.
        if self.focus_manager.active_dialog_type() == DialogType::ProjectPicker {
            self.render_project_picker(frame, area);
            return;
        }

        let popup_area = centered_rect(60, 50, area);
        frame.render_widget(Clear, popup_area);

        if !self.focus_manager.is_empty() {
            self.focus_manager
                .render(frame, popup_area, &self.ui_state.theme);
        }
    }

    fn render_project_picker(&mut self, frame: &mut Frame, area: Rect) {
        use crate::tui::components::dialogs::project_picker::{
            render_picker_body, ProjectPickerDialog,
        };

        let dialog_area = ProjectPickerDialog::picker_area(area);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .title(" Project Picker ");
        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        if let Some(picker) = self.dialog_state.project_picker.as_ref() {
            let filtered = picker.filtered_indices(&self.project_catalog.entries);
            render_picker_body(
                frame,
                picker,
                &filtered,
                &self.project_catalog.entries,
                self.project_catalog.capability_supported,
                inner,
                &self.ui_state.theme,
            );
        }
    }

    fn render_completions(&mut self, frame: &mut Frame, prompt_area: Rect) {
        let rows = self.completion_rows();
        let sel = self.prompt_state.completion_sel;
        // Popup inner width, so descriptions get an ellipsis instead of
        // being cut mid-word by the widget's hard clip.
        //
        // The popup used to be pinned to 40 columns, which clipped almost
        // every command description ("Show the aggregated token/cost usage
        // breakdown for this session" became "Show the aggregated …"). Size
        // it against the prompt area instead, capped so it still reads as a
        // popup anchored to the composer rather than a full-screen overlay.
        const MAX_COMPLETION_WIDTH: u16 = 110;
        // Rendered on the popup's top border row, so it costs no content
        // height. The retired command palette advertised its keys this way and
        // the single popup now needs to stand on its own.
        const COMPLETION_HINT: &str =
            " \u{2191}\u{2193} navigate \u{b7} Enter run \u{b7} Esc cancel ";
        let compl_w = MAX_COMPLETION_WIDTH.min(prompt_area.width.saturating_sub(2));
        let inner_w = (compl_w as usize).saturating_sub(2);
        // The selected row is painted with an explicit background, so its
        // colors have to be resolved against *that* background rather than
        // the popup's. Themes are free to pick a `selection` and an accent
        // that sit at the same luminance, which renders the highlighted row
        // as a solid block of one color — unreadable, and indistinguishable
        // from the surrounding background. Resolve both foregrounds against
        // the selection fill by contrast so the highlighted row is legible
        // under every theme, including the 50 bundled Halloy imports.
        let sel_bg = self.ui_state.theme.selection;
        let sel_label_fg = readable_on(
            self.ui_state.theme.primary,
            sel_bg,
            [
                self.ui_state.theme.primary,
                self.ui_state.theme.foreground,
                self.ui_state.theme.secondary,
                self.ui_state.theme.link,
            ],
        );
        let sel_desc_fg = readable_on(
            self.ui_state.theme.muted,
            sel_bg,
            [
                self.ui_state.theme.foreground,
                self.ui_state.theme.primary,
                self.ui_state.theme.secondary,
                self.ui_state.theme.muted,
            ],
        );
        let items: Vec<ListItem> = match self.prompt_state.completion_type {
            CompletionType::Slash => rows
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_label_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.foreground)
                    };
                    // The description shares the label's background on the
                    // selected row; without this it kept the popup fill and
                    // the highlight bar visibly stopped mid-line.
                    let desc_style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_desc_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.muted)
                    };
                    let content = if let Some(ref desc) = c.description {
                        let budget = inner_w.saturating_sub(c.label.chars().count() + 1);
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("{} ", c.label), style),
                            Span::styled(
                                crate::tui::components::sidebar::clean_inline_text(desc, budget),
                                desc_style,
                            ),
                        ])])
                    } else {
                        Text::from(Span::styled(&c.label, style))
                    };
                    ListItem::new(content)
                })
                .collect(),
            CompletionType::File => rows
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_label_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.foreground)
                    };
                    let desc_style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_desc_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.muted)
                    };
                    let content = if let Some(ref desc) = c.description {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("{} ", c.icon()), style),
                            Span::styled(format!("{} ", c.label), style),
                            Span::styled(desc, desc_style),
                        ])])
                    } else {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("{} ", c.icon()), style),
                            Span::styled(&c.label, style),
                        ])])
                    };
                    ListItem::new(content)
                })
                .collect(),
            CompletionType::Agent => rows
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_label_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.foreground)
                    };
                    let desc_style = if i == sel {
                        Style::default().bg(sel_bg).fg(sel_desc_fg)
                    } else {
                        Style::default().fg(self.ui_state.theme.muted)
                    };
                    let content = if let Some(ref desc) = c.description {
                        Text::from(vec![Line::from(vec![
                            Span::styled(format!("@{} ", c.label), style),
                            Span::styled(desc, desc_style),
                        ])])
                    } else {
                        Text::from(Span::styled(format!("@{}", c.label), style))
                    };
                    ListItem::new(content)
                })
                .collect(),
        };
        if items.is_empty() {
            // A trigger is active but nothing matched. Say so instead of
            // leaving the user looking at an unchanged screen; the old
            // command-palette popup rendered a " No results " row here and
            // this is the one popup now serving that path.
            let compl_h = 3u16;
            let empty_area = Rect {
                x: prompt_area.x + 1,
                y: prompt_area.y.saturating_sub(compl_h),
                width: compl_w,
                height: compl_h,
            };
            frame.render_widget(Clear, empty_area);
            let block = Block::default()
                .borders(Borders::ALL)
                .title(COMPLETION_HINT)
                .title_style(Style::default().fg(self.ui_state.theme.muted))
                .border_style(Style::default().fg(self.ui_state.theme.border))
                .style(Style::default().bg(self.ui_state.theme.background));
            frame.render_widget(
                Paragraph::new(" No results ")
                    .block(block)
                    .alignment(ratatui::layout::Alignment::Center),
                empty_area,
            );
            self.completion_area = None;
            self.completion_visible_rows = 0;
            self.completion_scroll_offset = 0;
            return;
        }
        let max_h = 8.min(items.len() as u16);
        let compl_h = max_h + 2;
        // Scroll the list so the selection is always on screen. Without this
        // the popup was a fixed 8-row window over the whole result set, so
        // arrowing past row 8 highlighted an entry that was not drawn — the
        // selection silently vanished. `completion_sel` is reset to 0 on
        // every filter change (`update_completions`), so anchoring the
        // window so the selection sits on its last row keeps navigation
        // monotonic without carrying a second piece of scroll state that
        // could drift out of sync with the filter.
        let visible = max_h as usize;
        let total = items.len();
        let sel = sel.min(total.saturating_sub(1));
        let start = sel.saturating_sub(visible.saturating_sub(1));
        let compl_area = Rect {
            x: prompt_area.x + 1,
            y: prompt_area.y.saturating_sub(compl_h),
            width: compl_w,
            height: compl_h,
        };
        frame.render_widget(Clear, compl_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(COMPLETION_HINT)
            .title_style(Style::default().fg(self.ui_state.theme.muted))
            .border_style(Style::default().fg(self.ui_state.theme.border))
            .style(Style::default().bg(self.ui_state.theme.background));
        let list = List::new(items[start..(start + visible).min(total)].to_vec()).block(block);
        frame.render_widget(list, compl_area);
        // The visible rows are the *filtered* set, which the popup sizes
        // itself from. Click hit-testing must use the same count, offset by
        // the scroll window or every click would select the wrong row.
        self.completion_area = Some(compl_area);
        self.completion_visible_rows = visible;
        self.completion_scroll_offset = start;
    }
}
