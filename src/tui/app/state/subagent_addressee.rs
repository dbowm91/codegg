//! C007 / ADR-0041 Option A: who the composer's next submission is for.
//!
//! An armed addressee is **composer data, not a focus mode**. `InputTarget`
//! stays `{Build, Chat}` (ADR-0041, *Neutral or deferred*) because this state
//! does not change where keystrokes go; it changes what the *same* keystroke
//! means on submit. Arming therefore never moves the keyboard off the prompt
//! and never makes a side panel the destination.
//!
//! The captured identity is what the user was *looking at* when they armed it.
//! Nothing here re-derives a name, an agent type, or a description: the pool
//! that mints display names lives in `src/agent/subagent_names/`, and the TUI
//! deliberately has no pool (C005's I1 boundary). Copying the facts off the
//! row is the only thing this type may do, which is also why it stores them
//! rather than holding a row: the row moves under the user, the captured
//! addressee must not silently retarget to a re-opened tab showing a different
//! run.

use super::subagent_tabs::SubagentTabState;
use super::subagents::SubagentRow;

/// Longest delegated-task description carried into the addressee.
///
/// The description is reproduced in the header so the controlling agent can
/// identify the run without a name registry (the parent model never sees the
/// pool, so `Sally` alone is not resolvable). Bounded because it is
/// user-authored free text that would otherwise let one long task description
/// crowd the reply out of the steer payload.
pub const MAX_ADDRESSED_DESCRIPTION_CHARS: usize = 160;

/// A composer armed to address one running subagent.
///
/// Identity is the display name as shown, plus the `task_id` it was captured
/// from. The `task_id` is load-bearing and is **not** the daemon's turn
/// identity: it is the join key for invalidation only. The steer itself is
/// addressed to the *controlling parent turn* — see
/// [`addressed_text`](Self::addressed_text) and ADR-0041's *Addressing*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentAddressee {
    /// The name shown on the surface the user armed from, e.g. `Sally`.
    pub display_name: String,
    /// The projected task this addressee was captured from.
    ///
    /// Present so a snapshot that stops reporting this task can clear the
    /// addressee instead of leaving it aimed at a run that is no longer there.
    pub task_id: String,
    /// The subagent's agent type, e.g. `explore`.
    pub agent: String,
    /// What the run was asked to do, bounded.
    pub description: String,
}

impl SubagentAddressee {
    /// Capture the focused tab's run, if it is still live.
    ///
    /// Returns `None` for a detached tab or a finished run. The liveness check
    /// is the point: a detached tab's captured row still reads `running`
    /// because that is the last thing the projection said, so a sender that
    /// read `row.is_running()` alone would happily arm a reply to a run that
    /// has already left the tree.
    pub fn from_tab(tab: &SubagentTabState) -> Option<Self> {
        if !tab.is_running() {
            return None;
        }
        Self::from_row(&tab.row)
    }

    /// Capture one row's identity.
    ///
    /// Separate from [`from_tab`](Self::from_tab) because the sidebar row and
    /// the tab are different surfaces over the same run: arming from either
    /// must produce an identical addressee, or the same user would get two
    /// different addressee labels for one subagent.
    pub fn from_row(row: &SubagentRow) -> Option<Self> {
        if !row.is_running() {
            return None;
        }
        let display_name = row.label().trim();
        if display_name.is_empty() {
            return None;
        }
        let description = row.description.trim();
        let description = if description.chars().count() > MAX_ADDRESSED_DESCRIPTION_CHARS {
            let mut bounded: String = description
                .chars()
                .take(MAX_ADDRESSED_DESCRIPTION_CHARS)
                .collect();
            bounded.push('…');
            bounded
        } else {
            description.to_string()
        };
        Some(Self {
            display_name: display_name.to_string(),
            task_id: row.task_id.clone(),
            agent: row.agent.clone(),
            description,
        })
    }

    /// The name to show on the composer badge.
    pub fn label(&self) -> &str {
        &self.display_name
    }

    /// Whether `task_id` is the run this addressee was captured from.
    pub fn matches(&self, task_id: &str) -> bool {
        self.task_id == task_id
    }

    /// The payload handed to the controlling parent turn.
    ///
    /// The parent agent is the recipient, always — ADR-0041 *Indirect*: the
    /// frontend steers the turn that owns the subagent, and the parent decides
    /// whether to relay. So this names the subagent as an **addressee inside
    /// the parent's context** and never presents the subagent as the party the
    /// message was delivered to.
    ///
    /// The header carries the agent type and the task description as well as
    /// the name because the parent model has no name pool and no `task_id`
    /// join: `Sally` alone is a string it cannot resolve, whereas
    /// `Sally (explore), asked to find the retry path` identifies the run from
    /// facts the parent already knows how to act on.
    pub fn addressed_text(&self, body: &str) -> String {
        let mut header = format!(
            "[addressed to subagent {} ({})",
            self.display_name, self.agent
        );
        if !self.description.is_empty() {
            header.push_str(": ");
            header.push_str(&self.description);
        }
        header.push_str("]\n");
        let mut out = String::with_capacity(header.len() + body.len());
        out.push_str(&header);
        out.push_str(body);
        out
    }
}

/// Drop an addressee whose run the projection no longer vouches for.
///
/// Returns `true` when one was cleared.
///
/// Two things invalidate an addressee, and both are read from the arriving
/// snapshot rather than guessed: the row disappeared (the run left the tree),
/// or the row is still there but no longer running (the run finished). The
/// second case matters as much as the first — a run that completed while the
/// user was typing still appears in the tree, so a check for presence alone
/// would keep the addressee armed and the next submit would steer a finished
/// task's worth of text into a live turn.
///
/// A `None` addressee returns `false` without touching anything, so this is
/// cheap to call on every snapshot.
pub fn reconcile_addressee(
    addressee: &mut Option<SubagentAddressee>,
    rows: &[SubagentRow],
) -> bool {
    let Some(current) = addressee.as_ref() else {
        return false;
    };
    let still_live = rows
        .iter()
        .any(|row| row.task_id == current.task_id && row.is_running());
    if still_live {
        return false;
    }
    *addressee = None;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_row() -> SubagentRow {
        SubagentRow {
            task_id: "task-7".to_string(),
            run_id: Some("run-7".to_string()),
            display_name: Some("Sally".to_string()),
            profile: Some("Generalist".to_string()),
            usefulness: Some("Has seen this shape".to_string()),
            agent: "explore".to_string(),
            description: "find the retry path".to_string(),
            status: "running".to_string(),
        }
    }

    #[test]
    fn captures_the_facts_the_ui_shown_without_deriving_them() {
        let addressee = SubagentAddressee::from_row(&running_row()).expect("running row arms");
        assert_eq!(addressee.label(), "Sally");
        assert_eq!(addressee.task_id, "task-7");
        // The description comes off the row; the pool is never consulted, so
        // no derived field appears here that the surface did not already show.
        assert_eq!(addressee.description, "find the retry path");
    }

    #[test]
    fn a_finished_run_never_arms() {
        let mut row = running_row();
        row.status = "completed".to_string();
        assert!(SubagentAddressee::from_row(&row).is_none());
    }

    #[test]
    fn a_detached_tab_never_arms_even_though_its_row_still_reads_running() {
        let row = running_row();
        assert!(
            row.is_running(),
            "precondition: the row itself still says running"
        );
        let mut detached = SubagentTabState::attached(
            super::super::subagent_tabs::SubagentTabId::new(row.task_id.clone()),
            row,
        );
        detached.liveness = super::super::subagent_tabs::SubagentTabLiveness::Detached;
        assert!(SubagentAddressee::from_tab(&detached).is_none());
    }

    #[test]
    fn an_attached_running_tab_arms() {
        let attached = SubagentTabState::attached(
            super::super::subagent_tabs::SubagentTabId::new("task-7"),
            running_row(),
        );
        let addressee = SubagentAddressee::from_tab(&attached).expect("attached running tab arms");
        assert_eq!(addressee.label(), "Sally");
    }

    #[test]
    fn the_payload_addresses_the_subagent_inside_the_parents_context() {
        let addressee = SubagentAddressee::from_row(&running_row()).expect("arms");
        let text = addressee.addressed_text("try the second retry first");
        assert!(
            text.starts_with("[addressed to subagent Sally (explore)"),
            "{text}"
        );
        assert!(text.ends_with("try the second retry first"), "{text}");
    }

    #[test]
    fn a_long_description_is_bounded_rather_than_crowding_out_the_reply() {
        let mut row = running_row();
        row.description = "x".repeat(MAX_ADDRESSED_DESCRIPTION_CHARS * 2);
        let addressee = SubagentAddressee::from_row(&row).expect("arms");
        assert_eq!(
            addressee.description.chars().count(),
            MAX_ADDRESSED_DESCRIPTION_CHARS + 1,
            "bounded plus the ellipsis"
        );
        assert!(addressee.addressed_text("hi").ends_with("hi"));
    }

    #[test]
    fn a_run_that_leaves_the_tree_clears_the_addressee() {
        let mut addressee = Some(SubagentAddressee::from_row(&running_row()).expect("arms"));
        assert!(reconcile_addressee(&mut addressee, &[]));
        assert!(addressee.is_none());
    }

    #[test]
    fn a_run_that_finishes_but_is_still_in_the_tree_also_clears_the_addressee() {
        let mut finished = running_row();
        finished.status = "completed".to_string();
        let mut addressee = Some(SubagentAddressee::from_row(&running_row()).expect("arms"));
        assert!(
            reconcile_addressee(&mut addressee, &[finished]),
            "presence alone is not liveness"
        );
        assert!(addressee.is_none());
    }

    #[test]
    fn a_live_run_keeps_the_addressee() {
        let mut addressee = Some(SubagentAddressee::from_row(&running_row()).expect("arms"));
        assert!(!reconcile_addressee(&mut addressee, &[running_row()]));
        assert!(addressee.is_some());
    }

    #[test]
    fn another_surviving_run_does_not_keep_this_addressee_alive() {
        let mut other = running_row();
        other.task_id = "task-8".to_string();
        let mut addressee = Some(SubagentAddressee::from_row(&running_row()).expect("arms"));
        assert!(
            reconcile_addressee(&mut addressee, &[other]),
            "the match is on task_id, not on the row merely existing"
        );
    }

    #[test]
    fn reconciling_an_unarmed_composer_is_a_no_op() {
        let mut addressee = None;
        assert!(!reconcile_addressee(&mut addressee, &[]));
        assert!(addressee.is_none());
    }
}
