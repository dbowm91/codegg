//! Project Work Orders M003: Task composer, scheduling sheet, and Task view state.
//!
//! Frontend-only projection/controller state for the project Task
//! composer/view. Durable WorkOrder truth stays in daemon/core stores;
//! this module owns no sessions, jobs, schedules, or preferences and
//! performs no I/O. Every async completion that consumes this state must
//! carry project/view/request identity (see `UiRouteToken` +
//! `AsyncUiRequestState`) so stale completions are dropped at apply time.
//!
//! `InputMode::Insert|Normal` remains a separate text-editing/Vim concern:
//! [`ComposerMode`] is the submission/composer mode owned by project UI
//! state.

use crate::protocol::work_order::{
    WorkOrderCreateRequest, WorkOrderDto, WorkOrderGateDto, WorkOrderOccurrenceDto,
};

// ── Composer mode ────────────────────────────────────────────────────

/// Frontend submission mode for the prompt composer. Distinct from
/// `InputMode` (text editing/Vim): `Session` submits a normal turn (or
/// creates a session), `Task` opens the scheduling sheet and creates a
/// daemon-owned WorkOrder on confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposerMode {
    #[default]
    Session,
    Task,
}

impl ComposerMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Session => Self::Task,
            Self::Task => Self::Session,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Task => "task",
        }
    }

    pub const fn is_task(self) -> bool {
        matches!(self, Self::Task)
    }
}

// ── Scheduling sheet draft ───────────────────────────────────────────

/// Bounded scheduling-sheet draft. Field text is validated on confirm;
/// nothing here is durable until `WorkOrderCreate` succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskScheduleDraft {
    /// Delay/wait duration text (default `"0"`). Bounded duration forms
    /// only; ambiguous input fails validation in the modal.
    pub delay_text: String,
    /// Optional not-before timestamp text (empty = none). RFC 3339 with
    /// an explicit offset only; naive datetimes fail as ambiguous.
    pub not_before_text: String,
    /// Finite repeat count text (default `"1"`).
    pub repeat_text: String,
    /// Sequential lane toggle + selected lane (lane id + label preview).
    pub sequential: bool,
    pub lane_id: Option<String>,
    pub lane_label: Option<String>,
    /// `all` or `any` when more than one nontrivial gate is enabled.
    pub gate_join: GateJoin,
    /// External-trigger placeholder. M005 owns the server capability, so
    /// the TUI always renders this disabled and confirm fails closed if
    /// it is somehow enabled.
    pub external_trigger: bool,
    /// Composer model snapshot selected for this Task (stable identity).
    /// Defaults to the last valid Task model when available, else the
    /// normal current/default selection. Never execution authority.
    pub model: Option<String>,
    /// Whether the model above came from the remembered Task preference
    /// (`false` = fell back to the current/default selection).
    pub model_from_task_preference: bool,
    /// Requested approval/sandbox summary snapshot (effective policy at
    /// compose time; re-resolved/narrowed at execution).
    pub requested_approval: Option<String>,
    pub requested_sandbox: Option<String>,
    /// Workspace policy summary (`auto_isolated` default lane).
    pub workspace_policy: Option<String>,
}

/// Explicit All/Any join policy for multiple nontrivial gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GateJoin {
    #[default]
    All,
    Any,
}

impl GateJoin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "ALL gates must release",
            Self::Any => "ANY gate releases",
        }
    }
}

impl Default for TaskScheduleDraft {
    fn default() -> Self {
        Self {
            delay_text: "0".to_string(),
            not_before_text: String::new(),
            repeat_text: "1".to_string(),
            sequential: false,
            lane_id: None,
            lane_label: None,
            gate_join: GateJoin::All,
            external_trigger: false,
            model: None,
            model_from_task_preference: false,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
        }
    }
}

/// Bounds for sheet validation (client-side clamp before send; the
/// daemon re-validates authoritatively).
pub const MAX_DELAY_SECS: i64 = 30 * 24 * 3600;
pub const MAX_REPEAT_COUNT: u32 = 100;
pub const MAX_PROMPT_PREVIEW_CHARS: usize = 120;

/// Validated scheduling values derived from a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSchedule {
    pub delay_secs: i64,
    pub not_before_ms: Option<i64>,
    pub repeat_count: u32,
}

/// Parse a bounded delay duration. Accepts `0`/empty (zero), plain
/// seconds (`90`), and unit forms `s/m/h/d` with `+`-joined combos
/// (`1h30m`, `2d`). Case-insensitive, whitespace-tolerant. Rejects
/// negatives, decimals, unknown units, and values over 30 days.
pub fn parse_delay_duration(raw: &str) -> Result<i64, String> {
    let text = raw.trim().to_lowercase().replace(' ', "");
    if text.is_empty() || text == "0" {
        return Ok(0);
    }
    // Plain integer = seconds.
    if text.chars().all(|c| c.is_ascii_digit()) {
        let secs: i64 = text
            .parse()
            .map_err(|_| format!("Invalid delay '{raw}': expected e.g. 0, 20m, 1h30m"))?;
        return check_delay_bound(secs, raw);
    }
    let mut total: i64 = 0;
    let mut current = String::new();
    let mut saw_component = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        let unit_secs: i64 = match ch {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => {
                return Err(format!(
                    "Invalid delay '{raw}': unknown unit '{ch}' (use s, m, h, d; e.g. 20m)"
                ));
            }
        };
        if current.is_empty() {
            return Err(format!(
                "Invalid delay '{raw}': unit '{ch}' needs a number (e.g. 20m)"
            ));
        }
        let amount: i64 = current
            .parse()
            .map_err(|_| format!("Invalid delay '{raw}': expected e.g. 0, 20m, 1h30m"))?;
        total = total
            .checked_add(
                amount
                    .checked_mul(unit_secs)
                    .ok_or_else(|| format!("Invalid delay '{raw}': value too large (max 30d)"))?,
            )
            .ok_or_else(|| format!("Invalid delay '{raw}': value too large (max 30d)"))?;
        current.clear();
        saw_component = true;
    }
    if !current.is_empty() {
        return Err(format!(
            "Invalid delay '{raw}': trailing number needs a unit (e.g. {current}m)"
        ));
    }
    if !saw_component {
        return Err(format!(
            "Invalid delay '{raw}': expected e.g. 0, 20m, 1h30m"
        ));
    }
    check_delay_bound(total, raw)
}

fn check_delay_bound(secs: i64, raw: &str) -> Result<i64, String> {
    if !(0..=MAX_DELAY_SECS).contains(&secs) {
        return Err(format!(
            "Invalid delay '{raw}': must be 0..=30d (got {secs}s)"
        ));
    }
    Ok(secs)
}

/// Parse an optional not-before timestamp. Empty/blank = `None`.
/// Otherwise RFC 3339 with an explicit offset is required
/// (`2026-09-18T10:00:00Z`, `...+02:00`); naive datetimes fail as
/// ambiguous instead of guessing. Returns absolute UTC millis for
/// storage/submission; callers render local time with offset.
pub fn parse_not_before_ms(raw: &str) -> Result<Option<i64>, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let example = "e.g. 2026-09-18T10:00:00Z";
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(zoned) => Ok(Some(zoned.timestamp_millis())),
        Err(_) => Err(format!(
            "Invalid not-before '{raw}': need an explicit timezone ({example})"
        )),
    }
}

/// Parse a finite repeat count (default 1, max 100).
pub fn parse_repeat_count(raw: &str) -> Result<u32, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Ok(1);
    }
    let count: u32 = text
        .parse()
        .map_err(|_| format!("Invalid repeat '{raw}': need 1..={MAX_REPEAT_COUNT}"))?;
    if count == 0 || count > MAX_REPEAT_COUNT {
        return Err(format!(
            "Invalid repeat '{raw}': need 1..={MAX_REPEAT_COUNT}"
        ));
    }
    Ok(count)
}

/// Validate a sheet draft into daemon-submittable values. Fails closed
/// (with an actionable message) on ambiguous time input, unsupported
/// gates, or out-of-range repeats — never guesses.
pub fn validate_draft(draft: &TaskScheduleDraft) -> Result<ValidatedSchedule, String> {
    if draft.external_trigger {
        return Err(
            "External trigger is unavailable: the task-trigger capability (M005) is not enabled on this server"
                .to_string(),
        );
    }
    if draft.sequential
        && draft
            .lane_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(
            "Sequential lane is on but no lane is selected: pick a lane or turn sequential off"
                .to_string(),
        );
    }
    Ok(ValidatedSchedule {
        delay_secs: parse_delay_duration(&draft.delay_text)?,
        not_before_ms: parse_not_before_ms(&draft.not_before_text)?,
        repeat_count: parse_repeat_count(&draft.repeat_text)?,
    })
}

/// Human duration for summaries (`20m`, `1h30m`, `0`).
pub fn format_delay_secs(secs: i64) -> String {
    if secs <= 0 {
        return "0".to_string();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let rest = secs % 60;
    let mut out = String::new();
    if days > 0 {
        out.push_str(&format!("{days}d"));
    }
    if hours > 0 {
        out.push_str(&format!("{hours}h"));
    }
    if mins > 0 {
        out.push_str(&format!("{mins}m"));
    }
    if rest > 0 || out.is_empty() {
        out.push_str(&format!("{rest}s"));
    }
    out
}

/// Concise natural-language schedule summary, e.g.
/// `Run after task abc123 AND wait 20m · repeat 3x · model foo/bar`.
/// Never exposes raw JSON gate structures.
pub fn describe_schedule(
    validated: &ValidatedSchedule,
    lane_preview: Option<&str>,
    model: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(lane) = lane_preview {
        let short: String = lane.chars().take(12).collect();
        parts.push(format!("Run after task {short}"));
    }
    let mut wait_bits: Vec<String> = Vec::new();
    if validated.delay_secs > 0 {
        wait_bits.push(format!("wait {}", format_delay_secs(validated.delay_secs)));
    }
    if validated.not_before_ms.is_some() {
        wait_bits.push("not-before set".to_string());
    }
    if parts.is_empty() && wait_bits.is_empty() {
        parts.push("Run now".to_string());
    } else {
        parts.extend(wait_bits);
    }
    let mut summary = parts.join(" AND ");
    if validated.repeat_count > 1 {
        summary.push_str(&format!(" · repeat {}x", validated.repeat_count));
    }
    if let Some(model) = model {
        summary.push_str(&format!(" · model {model}"));
    }
    summary
}

/// Build the default zero-delay `WorkOrderCreate` payload: immediate
/// gate, one occurrence, no external trigger. Callers pass the already
/// validated schedule plus the snapshotted model/policy summary.
pub fn build_work_order_create(
    project_id: &str,
    prompt: &str,
    title: Option<&str>,
    draft: &TaskScheduleDraft,
    validated: &ValidatedSchedule,
    idempotency_key: Option<&str>,
) -> WorkOrderCreateRequest {
    let mut gates: Vec<WorkOrderGateDto> = Vec::new();
    if draft.sequential {
        if let Some(lane_id) = draft.lane_id.clone() {
            gates.push(WorkOrderGateDto {
                kind: "sequence_ready".to_string(),
                delay_secs: None,
                not_before_ms: None,
                lane_id: Some(lane_id),
                trigger_ref: None,
            });
        }
    }
    if validated.delay_secs > 0 {
        gates.push(WorkOrderGateDto {
            kind: "delay".to_string(),
            delay_secs: Some(validated.delay_secs),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        });
    }
    if let Some(not_before_ms) = validated.not_before_ms {
        gates.push(WorkOrderGateDto {
            kind: "not_before".to_string(),
            delay_secs: None,
            not_before_ms: Some(not_before_ms),
            lane_id: None,
            trigger_ref: None,
        });
    }
    if gates.is_empty() {
        gates.push(WorkOrderGateDto {
            kind: "immediate".to_string(),
            delay_secs: None,
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        });
    }
    // Count only nontrivial gates for the join decision.
    let nontrivial = gates.iter().filter(|g| g.kind != "immediate").count();
    let gate_join = if nontrivial > 1 {
        Some(draft.gate_join.as_str().to_string())
    } else {
        None
    };
    WorkOrderCreateRequest {
        project_id: project_id.to_string(),
        title: title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        prompt: prompt.to_string(),
        requested_model: draft.model.clone(),
        requested_approval: draft.requested_approval.clone(),
        requested_sandbox: draft.requested_sandbox.clone(),
        workspace_policy: draft.workspace_policy.clone(),
        gates,
        gate_join,
        repeat_count: Some(validated.repeat_count),
        sequence_lane_id: if draft.sequential {
            draft.lane_id.clone()
        } else {
            None
        },
        parent_session_id: None,
        parent_turn_id: None,
        parent_work_order_id: None,
        idempotency_key: idempotency_key.map(str::to_string),
    }
}

/// Bounded prompt preview for the sheet title line.
pub fn prompt_preview(prompt: &str) -> String {
    let flat: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview: String = flat.chars().take(MAX_PROMPT_PREVIEW_CHARS).collect();
    if flat.chars().count() > MAX_PROMPT_PREVIEW_CHARS {
        preview.push('…');
    }
    preview
}

// ── Pending Task create (prompt preservation) ────────────────────────

/// Immutable payload captured when a Task create is in flight. Mirrors
/// `PendingSessionSubmit`: the editable prompt widget is never consulted
/// by the async continuation, and a failed create restores the prompt
/// exactly once for explicit retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTaskCreate {
    pub request_id: u64,
    pub prompt: String,
    pub project_id: String,
    pub route: crate::tui::app::state::UiRouteToken,
    pub context: crate::tui::app::state::ProjectExecutionContext,
}

// ── Project Task view ────────────────────────────────────────────────

/// Maximum cached WorkOrder rows per project view refresh.
pub const MAX_TASK_VIEW_ROWS: usize = 200;
/// Maximum visible rows per section before paging hints.
pub const MAX_SECTION_ROWS: usize = 50;

/// One Task-view section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskViewSection {
    Running,
    Waiting,
    Attention,
    Recent,
}

impl TaskViewSection {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Waiting => "FUTURE / WAITING",
            Self::Attention => "NEEDS ATTENTION",
            Self::Recent => "RECENT",
        }
    }
}

/// Frontend row: a WorkOrder plus optional lazily-fetched occurrence
/// detail (session linkage, attention). The primary list refresh issues
/// one bounded `WorkOrderList` (+ one `WorkOrderSummary`); occurrence
/// detail is fetched only for the selected row, never N+1 per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskViewRow {
    pub work_order: WorkOrderDto,
    pub occurrence: Option<WorkOrderOccurrenceDto>,
}

impl TaskViewRow {
    /// Attention reason for the NEEDS ATTENTION section, if any.
    /// Unavailable model/policy/workspace state is shown as attention,
    /// never silently repaired in the TUI.
    pub fn attention_reason(&self) -> Option<String> {
        if let Some(occurrence) = self.occurrence.as_ref() {
            if occurrence.state == "needs_attention" {
                return Some(attention_label(
                    occurrence.attention_code.as_deref(),
                    occurrence.diagnostic.as_deref(),
                ));
            }
        }
        // WorkOrder-level states that need user action.
        match self.work_order.state.as_str() {
            "paused" => Some("paused — resume to release future occurrences".to_string()),
            _ => None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.occurrence
            .as_ref()
            .is_some_and(|o| o.state == "running" || o.state == "claiming" || o.state == "ready")
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.work_order.state.as_str(),
            "completed" | "cancelled" | "archived"
        )
    }

    pub fn materialized_session_id(&self) -> Option<&str> {
        self.occurrence
            .as_ref()
            .and_then(|o| o.session_id.as_deref())
    }

    pub fn title_or_preview(&self) -> String {
        if let Some(title) = self.work_order.title.as_deref().map(str::trim) {
            if !title.is_empty() {
                return title.chars().take(60).collect();
            }
        }
        prompt_preview(&self.work_order.prompt)
    }
}

/// Stable attention label for an occurrence attention code. Never leaks
/// prompt bodies, secrets, or reasoning; the daemon diagnostic is shown
/// only as a bounded hint.
pub fn attention_label(code: Option<&str>, diagnostic: Option<&str>) -> String {
    // Closed daemon `attention_code` set (M001 CHECK + M002
    // `*_unavailable`-prefixed diagnostics carried in `diagnostic`).
    let base = match code {
        Some("model_unavailable") => "model unavailable",
        Some("materialization_failed") => "materialization failed",
        Some("worktree_conflict") => "workspace/worktree conflict",
        Some("predecessor_failed") => "sequence predecessor failed",
        Some("predecessor_attention") => "sequence predecessor needs attention",
        Some("policy_narrowed") => "policy narrowed — action required",
        Some("trigger_expired") => "external trigger expired",
        Some(other) => other,
        None => "needs attention",
    };
    match diagnostic.map(str::trim).filter(|d| !d.is_empty()) {
        Some(detail) => {
            let hint: String = detail.chars().take(80).collect();
            format!("{base} — {hint}")
        }
        None => base.to_string(),
    }
}

/// Group cached rows into the four view sections. Running = materialized
/// sessions in flight; Waiting = ordered future lane members; Attention
/// = actionable states; Recent = terminal work (bounded, newest first by
/// `updated_at_ms`).
pub fn group_task_rows(rows: &[TaskViewRow]) -> Vec<(TaskViewSection, Vec<usize>)> {
    let mut running = Vec::new();
    let mut waiting = Vec::new();
    let mut attention = Vec::new();
    let mut recent = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        if row.attention_reason().is_some() {
            attention.push(idx);
        } else if row.is_running() {
            running.push(idx);
        } else if row.is_terminal() {
            recent.push(idx);
        } else {
            waiting.push(idx);
        }
    }
    // Recent: newest first.
    recent.sort_by(|&a, &b| {
        rows[b]
            .work_order
            .updated_at_ms
            .cmp(&rows[a].work_order.updated_at_ms)
    });
    vec![
        (TaskViewSection::Running, running),
        (TaskViewSection::Waiting, waiting),
        (TaskViewSection::Attention, attention),
        (TaskViewSection::Recent, recent),
    ]
}

/// Ordered flat navigation over non-empty sections (for j/k + arrows).
pub fn flat_navigation_order(groups: &[(TaskViewSection, Vec<usize>)]) -> Vec<usize> {
    groups
        .iter()
        .flat_map(|(_, idxs)| idxs.iter().copied())
        .collect()
}

/// Project Task view state: bounded cached rows plus selection/scroll.
/// `generation` invalidates late completions (another successful
/// refresh/reorder bumps it; completions carrying an older generation
/// are dropped).
#[derive(Debug, Clone, Default)]
pub struct TaskViewState {
    pub project_id: Option<String>,
    pub rows: Vec<TaskViewRow>,
    pub summary: Option<crate::protocol::work_order::WorkOrderSummaryDto>,
    pub lanes: Vec<crate::protocol::work_order::SequenceLaneDto>,
    pub capabilities_supported: bool,
    pub selected: usize,
    pub scroll_offset: usize,
    pub generation: u64,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl TaskViewState {
    pub fn begin_refresh(&mut self, project_id: &str) -> u64 {
        self.project_id = Some(project_id.to_string());
        self.loading = true;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    pub fn flat_order(&self) -> Vec<usize> {
        flat_navigation_order(&group_task_rows(&self.rows))
    }

    /// Move selection by `delta` rows over the flat order; returns the
    /// newly selected row index (into `rows`), if any.
    pub fn move_selection(&mut self, delta: isize) -> Option<usize> {
        let order = self.flat_order();
        if order.is_empty() {
            self.selected = 0;
            return None;
        }
        let current_pos = order.iter().position(|&i| i == self.selected).unwrap_or(0);
        let next_pos = (current_pos as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        self.selected = order[next_pos];
        Some(self.selected)
    }

    pub fn selected_row(&self) -> Option<&TaskViewRow> {
        self.rows.get(self.selected)
    }

    /// Compute the new lane order after moving `work_order_id` by
    /// `delta` within `ordered_ids`. Returns `None` when the move is
    /// not applicable (unknown id, pinned running head, or out of
    /// range). Pure: the caller still performs the CAS reorder with the
    /// expected lane revision, and a conflict refreshes the lane with
    /// "queue changed; retry" instead of applying speculative order.
    pub fn moved_lane_order(
        ordered_ids: &[String],
        pinned_head: Option<&str>,
        work_order_id: &str,
        delta: isize,
    ) -> Option<Vec<String>> {
        let pos = ordered_ids.iter().position(|id| id == work_order_id)?;
        // The pinned running/claimed predecessor never moves.
        if Some(work_order_id) == pinned_head {
            return None;
        }
        let target = pos as isize + delta;
        if target < 0 || target >= ordered_ids.len() as isize {
            return None;
        }
        let target = target as usize;
        let mut next = ordered_ids.to_vec();
        let removed = next.remove(pos);
        // A movable task never jumps ahead of the pinned head.
        if let Some(head) = pinned_head {
            if ordered_ids.first().map(String::as_str) == Some(head) && target == 0 {
                return None;
            }
        }
        next.insert(target, removed);
        Some(next)
    }
}

/// Task-model choice resolved for the composer: the last valid Task
/// model when available, else the normal current/default selection.
/// Convenience default only; WorkOrder creation snapshots it and an
/// already-created WorkOrder never silently changes model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskModelChoice {
    pub connection: Option<String>,
    pub model: Option<String>,
    pub revision: u64,
    pub from_preference: bool,
}

// ── Scheduling-sheet field focus ─────────────────────────────────────

/// Focusable sheet field for keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskSheetField {
    #[default]
    Delay,
    NotBefore,
    Repeat,
    Sequential,
    GateJoin,
    Model,
    Confirm,
}

impl TaskSheetField {
    pub const ORDER: &'static [Self] = &[
        Self::Delay,
        Self::NotBefore,
        Self::Repeat,
        Self::Sequential,
        Self::GateJoin,
        Self::Model,
        Self::Confirm,
    ];

    pub fn advance(self) -> Self {
        let pos = Self::ORDER.iter().position(|&f| f == self).unwrap_or(0);
        Self::ORDER[(pos + 1) % Self::ORDER.len()]
    }

    pub fn retreat(self) -> Self {
        let pos = Self::ORDER.iter().position(|&f| f == self).unwrap_or(0);
        Self::ORDER[(pos + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_with(delay: &str, not_before: &str, repeat: &str) -> TaskScheduleDraft {
        TaskScheduleDraft {
            delay_text: delay.to_string(),
            not_before_text: not_before.to_string(),
            repeat_text: repeat.to_string(),
            ..TaskScheduleDraft::default()
        }
    }

    #[test]
    fn composer_mode_toggles_and_labels() {
        assert_eq!(ComposerMode::default(), ComposerMode::Session);
        assert_eq!(ComposerMode::Session.toggle(), ComposerMode::Task);
        assert_eq!(ComposerMode::Task.toggle(), ComposerMode::Session);
        assert_eq!(ComposerMode::Task.label(), "task");
        assert!(ComposerMode::Task.is_task());
        assert!(!ComposerMode::Session.is_task());
    }

    #[test]
    fn delay_parser_accepts_bounded_forms() {
        assert_eq!(parse_delay_duration(""), Ok(0));
        assert_eq!(parse_delay_duration("0"), Ok(0));
        assert_eq!(parse_delay_duration("90"), Ok(90));
        assert_eq!(parse_delay_duration("20m"), Ok(1200));
        assert_eq!(parse_delay_duration("1h30m"), Ok(5400));
        assert_eq!(parse_delay_duration("2d"), Ok(172800));
        assert_eq!(parse_delay_duration(" 1H "), Ok(3600));
    }

    #[test]
    fn delay_parser_rejects_ambiguous_or_unbounded() {
        assert!(parse_delay_duration("soon").is_err());
        assert!(parse_delay_duration("-5m").is_err());
        assert!(parse_delay_duration("1.5h").is_err());
        assert!(parse_delay_duration("10x").is_err());
        assert!(parse_delay_duration("5").is_ok());
        assert!(parse_delay_duration("31d").is_err());
        assert!(parse_delay_duration("20").is_ok());
        // Trailing number without a unit must not guess.
        assert!(parse_delay_duration("1h30").is_err());
    }

    #[test]
    fn not_before_requires_explicit_timezone() {
        assert_eq!(parse_not_before_ms(""), Ok(None));
        assert_eq!(parse_not_before_ms("   "), Ok(None));
        let zoned = parse_not_before_ms("2026-09-18T10:00:00Z")
            .unwrap()
            .unwrap();
        assert!(zoned > 0);
        let offset = parse_not_before_ms("2026-09-18T10:00:00+02:00")
            .unwrap()
            .unwrap();
        assert_eq!(offset, zoned - 2 * 3600 * 1000);
        // Naive datetimes are ambiguous: fail, don't guess.
        assert!(parse_not_before_ms("2026-09-18 10:00:00").is_err());
        assert!(parse_not_before_ms("tomorrow").is_err());
    }

    #[test]
    fn repeat_is_finite_and_bounded() {
        assert_eq!(parse_repeat_count(""), Ok(1));
        assert_eq!(parse_repeat_count("3"), Ok(3));
        assert!(parse_repeat_count("0").is_err());
        assert!(parse_repeat_count("101").is_err());
        assert!(parse_repeat_count("many").is_err());
    }

    #[test]
    fn default_draft_is_zero_delay_single_occurrence() {
        let draft = TaskScheduleDraft::default();
        let validated = validate_draft(&draft).unwrap();
        assert_eq!(
            validated,
            ValidatedSchedule {
                delay_secs: 0,
                not_before_ms: None,
                repeat_count: 1,
            }
        );
        let request = build_work_order_create(
            "project-1",
            "do the thing",
            None,
            &draft,
            &validated,
            Some("key-1"),
        );
        assert_eq!(request.project_id, "project-1");
        assert_eq!(request.gates.len(), 1);
        assert_eq!(request.gates[0].kind, "immediate");
        assert_eq!(request.repeat_count, Some(1));
        assert_eq!(request.gate_join, None);
        assert_eq!(request.idempotency_key.as_deref(), Some("key-1"));
        assert!(request.sequence_lane_id.is_none());
    }

    #[test]
    fn multi_gate_draft_uses_join_and_lane() {
        let mut draft = draft_with("20m", "", "3");
        draft.sequential = true;
        draft.lane_id = Some("lane-1".to_string());
        draft.gate_join = GateJoin::Any;
        let validated = validate_draft(&draft).unwrap();
        let request = build_work_order_create(
            "project-1",
            "do the thing",
            Some(" Title "),
            &draft,
            &validated,
            None,
        );
        assert_eq!(request.gates.len(), 2);
        assert_eq!(request.gate_join.as_deref(), Some("any"));
        assert_eq!(request.sequence_lane_id.as_deref(), Some("lane-1"));
        assert_eq!(request.title.as_deref(), Some("Title"));
        let summary = describe_schedule(&validated, Some("abc123"), Some("foo/bar"));
        assert_eq!(
            summary,
            "Run after task abc123 AND wait 20m · repeat 3x · model foo/bar"
        );
    }

    #[test]
    fn unsupported_gates_fail_visibly() {
        let draft = TaskScheduleDraft {
            external_trigger: true,
            ..TaskScheduleDraft::default()
        };
        assert!(validate_draft(&draft).is_err());
        let draft = TaskScheduleDraft {
            sequential: true,
            ..TaskScheduleDraft::default()
        };
        assert!(validate_draft(&draft).is_err());
    }

    #[test]
    fn task_rows_group_into_bounded_sections() {
        fn row(state: &str, occurrence_state: Option<&str>, updated: i64) -> TaskViewRow {
            TaskViewRow {
                work_order: WorkOrderDto {
                    work_order_id: format!("wo-{state}-{updated}"),
                    revision: 1,
                    project_id: "p".to_string(),
                    creator_principal: "owner".to_string(),
                    parent_session_id: None,
                    parent_turn_id: None,
                    parent_work_order_id: None,
                    title: None,
                    prompt: "prompt".to_string(),
                    requested_model: None,
                    requested_approval: None,
                    requested_sandbox: None,
                    workspace_policy: None,
                    gates: Vec::new(),
                    gate_join: None,
                    repeat_count: 1,
                    sequence_lane_id: None,
                    state: state.to_string(),
                    created_at_ms: 0,
                    updated_at_ms: updated,
                    cancelled_at_ms: None,
                },
                occurrence: occurrence_state.map(|s| WorkOrderOccurrenceDto {
                    occurrence_id: "occ".to_string(),
                    work_order_id: "wo".to_string(),
                    project_id: "p".to_string(),
                    occurrence_index: 0,
                    state: s.to_string(),
                    gate_latches: Vec::new(),
                    not_before_ms: None,
                    next_check_at_ms: None,
                    session_id: if s == "running" {
                        Some("sess-1".to_string())
                    } else {
                        None
                    },
                    job_id: None,
                    workspace_id: None,
                    worktree_id: None,
                    attention_code: if s == "needs_attention" {
                        Some("model_unavailable".to_string())
                    } else {
                        None
                    },
                    diagnostic: None,
                    created_at_ms: 0,
                    updated_at_ms: updated,
                    claimed_at_ms: None,
                    started_at_ms: None,
                    terminal_at_ms: None,
                }),
            }
        }
        let rows = vec![
            row("active", Some("running"), 3),
            row("active", None, 2),
            row("active", Some("needs_attention"), 4),
            row("completed", None, 1),
        ];
        let groups = group_task_rows(&rows);
        let running = groups
            .iter()
            .find(|(s, _)| *s == TaskViewSection::Running)
            .unwrap();
        let waiting = groups
            .iter()
            .find(|(s, _)| *s == TaskViewSection::Waiting)
            .unwrap();
        let attention = groups
            .iter()
            .find(|(s, _)| *s == TaskViewSection::Attention)
            .unwrap();
        let recent = groups
            .iter()
            .find(|(s, _)| *s == TaskViewSection::Recent)
            .unwrap();
        assert_eq!(running.1.len(), 1);
        assert_eq!(waiting.1.len(), 1);
        assert_eq!(attention.1.len(), 1);
        assert_eq!(recent.1.len(), 1);
        assert_eq!(
            rows[attention.1[0]].attention_reason().as_deref(),
            Some("model unavailable")
        );
        assert_eq!(rows[running.1[0]].materialized_session_id(), Some("sess-1"));
    }

    #[test]
    fn lane_reorder_respects_pinned_head() {
        let order = vec!["wo-run".to_string(), "wo-a".to_string(), "wo-b".to_string()];
        // Pinned running head never moves.
        assert!(TaskViewState::moved_lane_order(&order, Some("wo-run"), "wo-run", 1).is_none());
        // Movable tasks never jump ahead of the pinned head.
        assert!(TaskViewState::moved_lane_order(&order, Some("wo-run"), "wo-a", -1).is_none());
        // Ordinary move swaps neighbors.
        assert_eq!(
            TaskViewState::moved_lane_order(&order, Some("wo-run"), "wo-a", 1),
            Some(vec![
                "wo-run".to_string(),
                "wo-b".to_string(),
                "wo-a".to_string()
            ])
        );
        // Unknown ids and out-of-range moves are not applicable.
        assert!(TaskViewState::moved_lane_order(&order, None, "wo-x", 1).is_none());
        assert!(TaskViewState::moved_lane_order(&order, None, "wo-b", 1).is_none());
    }

    #[test]
    fn sheet_fields_cycle_without_colliding_with_vim_navigation() {
        // Tab order is explicit and closed; bare j/k stay navigation.
        assert_eq!(TaskSheetField::Delay.advance(), TaskSheetField::NotBefore);
        assert_eq!(TaskSheetField::Confirm.advance(), TaskSheetField::Delay);
        assert_eq!(TaskSheetField::Delay.retreat(), TaskSheetField::Confirm);
    }
}
