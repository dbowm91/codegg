//! Active-view switch coordinator for the Multi-Project TUI (milestone 2).
//!
//! When switching between project tabs, the heavy session/render state
//! must be replaced only after an identity-matched async load succeeds.
//! This module tracks the switch transaction lifecycle.
//!
//! Invariants:
//! * Only one heavy active session state exists at any time.
//! * A switch is identified by `(tab_id, project_id, workspace_id,
//!   session_id, epoch)` — mismatches are stale and dropped.
//! * On failure, the incoming tab stays selected but no outgoing
//!   heavy state is restored under the new tab identity.

use crate::tui::app::state::project_tabs::ProjectTabId;

/// State of the active-view switch transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchState {
    /// No switch in progress.
    Idle,
    /// Switching from one tab to another. The outgoing tab's
    /// lightweight selection has been captured.
    Switching {
        from_tab: ProjectTabId,
        to_tab: ProjectTabId,
        epoch: u64,
    },
    /// Loading the incoming tab's session from the daemon.
    Loading {
        to_tab: ProjectTabId,
        session_id: String,
        project_id: String,
        workspace_id: String,
        epoch: u64,
    },
    /// The switch failed. The tab remains selected but the error
    /// is surfaced.
    Failed { to_tab: ProjectTabId, error: String },
}

/// Coordinator for the active-view switch transaction.
#[derive(Debug)]
pub struct ViewSwitchCoordinator {
    /// Monotonic counter incremented on every switch start.
    pub active_view_epoch: u64,
    /// Current switch state.
    pub switch_state: SwitchState,
    /// Tab pending incoming load, if any.
    pub pending_incoming_tab_id: Option<ProjectTabId>,
}

impl ViewSwitchCoordinator {
    /// Create a fresh coordinator in idle state.
    pub fn new() -> Self {
        Self {
            active_view_epoch: 0,
            switch_state: SwitchState::Idle,
            pending_incoming_tab_id: None,
        }
    }

    /// Begin a switch transaction. Returns the new epoch.
    pub fn begin_switch(&mut self, from_tab: ProjectTabId, to_tab: ProjectTabId) -> u64 {
        self.active_view_epoch += 1;
        self.switch_state = SwitchState::Switching {
            from_tab,
            to_tab: to_tab.clone(),
            epoch: self.active_view_epoch,
        };
        self.pending_incoming_tab_id = Some(to_tab);
        self.active_view_epoch
    }

    /// Transition from Switching to Loading when a session load starts.
    /// Only succeeds if the to_tab and epoch still match.
    pub fn begin_load(
        &mut self,
        to_tab: &ProjectTabId,
        session_id: String,
        project_id: String,
        workspace_id: String,
        epoch: u64,
    ) -> bool {
        if let SwitchState::Switching {
            to_tab: ref current_to,
            epoch: current_epoch,
            ..
        } = self.switch_state
        {
            if current_to == to_tab && current_epoch == epoch {
                self.switch_state = SwitchState::Loading {
                    to_tab: to_tab.clone(),
                    session_id,
                    project_id,
                    workspace_id,
                    epoch,
                };
                return true;
            }
        }
        false
    }

    /// Complete a load. Only succeeds if the identity tuple matches.
    pub fn complete_load_if_matches(
        &mut self,
        to_tab: &ProjectTabId,
        project_id: &str,
        workspace_id: &str,
        session_id: &str,
        epoch: u64,
    ) -> bool {
        if let SwitchState::Loading {
            to_tab: ref current_to,
            project_id: ref current_proj,
            workspace_id: ref current_ws,
            session_id: ref current_sess,
            epoch: current_epoch,
        } = self.switch_state
        {
            if current_to == to_tab
                && current_proj == project_id
                && current_ws == workspace_id
                && current_sess == session_id
                && current_epoch == epoch
            {
                self.switch_state = SwitchState::Idle;
                self.pending_incoming_tab_id = None;
                return true;
            }
        }
        false
    }

    /// Cancel the current switch and go back to idle.
    pub fn cancel(&mut self) {
        self.switch_state = SwitchState::Idle;
        self.pending_incoming_tab_id = None;
    }

    /// Force back to idle (e.g., on tab close).
    pub fn idle(&mut self) {
        self.switch_state = SwitchState::Idle;
        self.pending_incoming_tab_id = None;
    }

    /// Record a failure.
    pub fn fail(&mut self, to_tab: ProjectTabId, error: String) {
        self.switch_state = SwitchState::Failed { to_tab, error };
        self.pending_incoming_tab_id = None;
    }

    /// Whether a switch or load is in progress.
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self.switch_state,
            SwitchState::Switching { .. } | SwitchState::Loading { .. }
        )
    }

    /// Whether a specific tab is the target of an in-flight switch.
    pub fn is_switching_to(&self, tab_id: &ProjectTabId) -> bool {
        match &self.switch_state {
            SwitchState::Switching { to_tab, .. } => to_tab == tab_id,
            SwitchState::Loading { to_tab, .. } => to_tab == tab_id,
            _ => false,
        }
    }

    /// Increment the epoch (e.g., on tab close to invalidate pending loads).
    /// Also invalidates any in-flight Switching or Loading state by
    /// moving the coordinator to Idle; the next `begin_switch` call
    /// must start a fresh transaction.
    pub fn bump_epoch(&mut self) -> u64 {
        self.active_view_epoch += 1;
        // Any pending switch is now stale.
        self.switch_state = SwitchState::Idle;
        self.pending_incoming_tab_id = None;
        self.active_view_epoch
    }

    /// Begin a fully-scoped loading transaction with explicit epoch
    /// capture. Records the canonical `(tab_id, project_id,
    /// workspace_id, session_id)` tuple plus the active-view epoch at
    /// the moment the load starts. The matching [`Self::commit_if_matches`]
    /// (or [`Self::suspend_if_matches`]) must validate every populated
    /// field before mutating heavy view state.
    pub fn begin_loading(
        &mut self,
        to_tab: ProjectTabId,
        session_id: String,
        project_id: String,
        workspace_id: String,
        epoch: u64,
    ) -> bool {
        let current_epoch = match &self.switch_state {
            SwitchState::Switching { epoch, .. } => *epoch,
            SwitchState::Loading { epoch, .. } => *epoch,
            SwitchState::Idle => self.active_view_epoch,
            SwitchState::Failed { .. } => self.active_view_epoch,
        };
        if current_epoch != epoch {
            return false;
        }
        self.switch_state = SwitchState::Loading {
            to_tab,
            session_id,
            project_id,
            workspace_id,
            epoch,
        };
        true
    }

    /// Commit a previously-started load only if the identity tuple and
    /// epoch still match. Records the active view as ready.
    pub fn commit_if_matches(
        &mut self,
        to_tab: &ProjectTabId,
        project_id: &str,
        workspace_id: &str,
        session_id: &str,
        epoch: u64,
    ) -> bool {
        self.complete_load_if_matches(to_tab, project_id, workspace_id, session_id, epoch)
    }

    /// Suspend the in-progress switch without committing. Used when
    /// the active tab changes underneath the load or the load fails
    /// non-fatally. Returns the prior target, if any.
    pub fn suspend_if_matches(&mut self, epoch: u64) -> Option<ProjectTabId> {
        match &self.switch_state {
            SwitchState::Switching { to_tab, epoch: e, .. }
            | SwitchState::Loading { to_tab, epoch: e, .. }
                if *e == epoch =>
            {
                let tab = to_tab.clone();
                self.switch_state = SwitchState::Idle;
                self.pending_incoming_tab_id = None;
                Some(tab)
            }
            _ => None,
        }
    }

    /// Mark a previously-started load as replaced by a new transaction.
    /// Bumps the active-view epoch. Returns the new epoch.
    pub fn replace_active(&mut self) -> u64 {
        self.bump_epoch()
    }
}

impl Default for ViewSwitchCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_idle() {
        let coord = ViewSwitchCoordinator::new();
        assert_eq!(coord.switch_state, SwitchState::Idle);
        assert_eq!(coord.active_view_epoch, 0);
        assert!(coord.pending_incoming_tab_id.is_none());
    }

    #[test]
    fn begin_switch_increments_epoch() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from.clone(), to.clone());
        assert_eq!(epoch, 1);
        assert!(coord.is_in_progress());
        assert_eq!(coord.pending_incoming_tab_id, Some(to));
    }

    #[test]
    fn begin_load_transitions_from_switching() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        let ok = coord.begin_load(
            &to,
            "sess".to_string(),
            "proj".to_string(),
            "ws".to_string(),
            epoch,
        );
        assert!(ok);
        assert!(matches!(coord.switch_state, SwitchState::Loading { .. }));
    }

    #[test]
    fn begin_load_rejects_stale_tab() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let stale_tab = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to);
        let ok = coord.begin_load(
            &stale_tab,
            "sess".to_string(),
            "proj".to_string(),
            "ws".to_string(),
            epoch,
        );
        assert!(!ok);
        assert!(matches!(coord.switch_state, SwitchState::Switching { .. }));
    }

    #[test]
    fn begin_load_rejects_stale_epoch() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        // Simulate a second switch bumping the epoch
        let _ = coord.bump_epoch();
        let ok = coord.begin_load(
            &to,
            "sess".to_string(),
            "proj".to_string(),
            "ws".to_string(),
            epoch, // stale epoch
        );
        assert!(!ok);
    }

    #[test]
    fn complete_load_if_matches_transitions_to_idle() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        coord.begin_load(
            &to,
            "sess".to_string(),
            "proj".to_string(),
            "ws".to_string(),
            epoch,
        );
        let ok = coord.complete_load_if_matches(&to, "proj", "ws", "sess", epoch);
        assert!(ok);
        assert_eq!(coord.switch_state, SwitchState::Idle);
        assert!(coord.pending_incoming_tab_id.is_none());
    }

    #[test]
    fn complete_load_rejects_stale_project() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        coord.begin_load(
            &to,
            "sess".to_string(),
            "proj".to_string(),
            "ws".to_string(),
            epoch,
        );
        let ok = coord.complete_load_if_matches(&to, "wrong-proj", "ws", "sess", epoch);
        assert!(!ok);
        assert!(matches!(coord.switch_state, SwitchState::Loading { .. }));
    }

    #[test]
    fn cancel_goes_to_idle() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        coord.begin_switch(from, to);
        coord.cancel();
        assert_eq!(coord.switch_state, SwitchState::Idle);
        assert!(coord.pending_incoming_tab_id.is_none());
    }

    #[test]
    fn fail_records_error() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        coord.begin_switch(from, to.clone());
        coord.fail(to.clone(), "boom".to_string());
        match &coord.switch_state {
            SwitchState::Failed { to_tab, error } => {
                assert_eq!(to_tab, &to);
                assert_eq!(error, "boom");
            }
            _ => panic!("expected Failed state"),
        }
    }

    #[test]
    fn is_switching_to_tracks_target() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        coord.begin_switch(from.clone(), to.clone());
        assert!(coord.is_switching_to(&to));
        assert!(!coord.is_switching_to(&from));
    }

    #[test]
    fn bump_epoch_increments() {
        let mut coord = ViewSwitchCoordinator::new();
        let e1 = coord.bump_epoch();
        let e2 = coord.bump_epoch();
        assert_eq!(e1, 1);
        assert_eq!(e2, 2);
    }

    #[test]
    fn begin_loading_rejects_stale_epoch() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        // Simulate a tab close that bumps the epoch.
        coord.bump_epoch();
        let ok = coord.begin_loading(to, "s".into(), "p".into(), "w".into(), epoch);
        assert!(!ok);
    }

    #[test]
    fn suspend_returns_prior_target_on_match() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        let prior = coord.suspend_if_matches(epoch);
        assert_eq!(prior, Some(to));
        assert_eq!(coord.switch_state, SwitchState::Idle);
    }

    #[test]
    fn suspend_rejects_stale_epoch() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let epoch = coord.begin_switch(from, to.clone());
        coord.bump_epoch();
        let prior = coord.suspend_if_matches(epoch);
        assert!(prior.is_none());
    }

    #[test]
    fn replace_active_bumps_epoch_to_next() {
        let mut coord = ViewSwitchCoordinator::new();
        let from = ProjectTabId::new();
        let to = ProjectTabId::new();
        let e1 = coord.begin_switch(from, to);
        let next = coord.replace_active();
        assert!(next > e1);
        assert_eq!(coord.switch_state, SwitchState::Idle);
    }
}
