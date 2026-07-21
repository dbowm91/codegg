//! Targeted tests for the routing/lifecycle contracts introduced by
//! Multi-Project TUI Milestone 003 (project-correct event routing
//! and lifecycle).
//!
//! These tests verify:
//!
//! - Routing tokens reject stale tab/project/workspace/session ids
//!   and active-view epochs.
//! - The pure classifier routes session events to the active view,
//!   to an inactive summary, or to a diagnostic drop based on the
//!   session_index in the routing registry.
//! - The TuiTaskRegistry's `cancel_for_tab`,
//!   `cancel_for_session`, and `cancel_for_stale_epoch` helpers
//!   release exactly the tasks in the given scope.
//! - The ViewSwitchCoordinator exposes begin/commit/suspend/replace
//!   transitions that respect the active-view epoch.
//! - Inactive summaries are bounded (unread saturation, status
//!   truncation, health summary cap).

use codegg::tui::app::state::project_tabs::ProjectTabId;
use codegg::tui::app::state::routing::{
    apply_inactive_summary, classify_event, InactiveSummaryKind, RouteDecision, RoutingRegistry,
    UiRouteToken, MAX_TAB_LAST_ERROR_LEN,
};
use codegg::tui::app::state::view_switch::ViewSwitchCoordinator;
use codegg::tui::task_lifecycle::{TuiTaskKind, TuiTaskRegistry};
use codegg_core::bus::events::AppEvent;

fn make_session_event(session_id: &str) -> AppEvent {
    AppEvent::TextDelta {
        session_id: session_id.into(),
        delta: "hello".into(),
    }
}

#[test]
fn routing_token_rejects_stale_project_after_switch() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    registry.register_open_session(tid.clone(), "s1".into());

    let token = UiRouteToken::new(
        Some(tid.clone()),
        Some("p-old".into()),
        Some("w-old".into()),
        Some("s1".into()),
        1,
        0,
        1,
    );

    let check = registry.check_for(Some(&tid), Some("p-new"), Some("w-new"), Some("s1"), 1);
    assert!(!token.matches(&check), "stale project/workspace must fail");
}

#[test]
fn routing_token_rejects_pre_reconnect_completion() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    registry.register_open_session(tid.clone(), "s1".into());
    let token = UiRouteToken::new(
        Some(tid.clone()),
        Some("p".into()),
        Some("w".into()),
        Some("s1".into()),
        1,
        0,
        1,
    );
    // Bump reconnect epoch: token captured the old epoch.
    registry.bump_reconnect_epoch();
    let check = registry.check_for(Some(&tid), Some("p"), Some("w"), Some("s1"), 1);
    assert!(!token.matches(&check));
}

#[test]
fn routing_token_rejects_stale_active_view_epoch() {
    let registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    let token = UiRouteToken::new(
        Some(tid.clone()),
        Some("p".into()),
        Some("w".into()),
        Some("s".into()),
        5,
        0,
        1,
    );
    let check = registry.check_for(Some(&tid), Some("p"), Some("w"), Some("s"), 6);
    assert!(!token.matches(&check));
}

#[test]
fn classifier_routes_active_view_for_match() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    registry.register_open_session(tid.clone(), "s1".into());
    let decision = classify_event(
        &make_session_event("s1"),
        &registry,
        Some(&tid),
        7,
    );
    assert_eq!(
        decision,
        RouteDecision::ActiveView {
            tab_id: tid.clone()
        }
    );
}

#[test]
fn classifier_routes_inactive_summary() {
    let mut registry = RoutingRegistry::new();
    let active = ProjectTabId::new();
    let inactive = ProjectTabId::new();
    registry.register_open_session(inactive.clone(), "s_other".into());
    let decision = classify_event(
        &make_session_event("s_other"),
        &registry,
        Some(&active),
        1,
    );
    assert_eq!(
        decision,
        RouteDecision::InactiveSummary {
            tab_id: inactive.clone()
        }
    );
}

#[test]
fn classifier_drops_unknown_session() {
    let registry = RoutingRegistry::new();
    let active = ProjectTabId::new();
    let decision = classify_event(
        &make_session_event("ghost"),
        &registry,
        Some(&active),
        1,
    );
    match decision {
        RouteDecision::DropDiagnostic { .. } => {}
        other => panic!("expected drop, got {other:?}"),
    }
}

#[test]
fn classifier_routes_global_for_unscoped_events() {
    let registry = RoutingRegistry::new();
    let active = ProjectTabId::new();
    let decision = classify_event(&AppEvent::ConfigChanged, &registry, Some(&active), 1);
    assert_eq!(decision, RouteDecision::Global);
}

#[test]
fn inactive_summary_unread_saturates() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    for _ in 0..1000 {
        apply_inactive_summary(&mut registry, &tid, InactiveSummaryKind::UnreadActivity, None);
    }
    let summary = registry.activity(&tid).expect("summary exists");
    assert_eq!(summary.unread_count, 99);
}

#[test]
fn inactive_summary_records_pending_counts() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    apply_inactive_summary(
        &mut registry,
        &tid,
        InactiveSummaryKind::PendingPermission,
        None,
    );
    apply_inactive_summary(
        &mut registry,
        &tid,
        InactiveSummaryKind::PendingQuestion,
        None,
    );
    let summary = registry.activity(&tid).expect("summary exists");
    assert_eq!(summary.pending_permission_count, 1);
    assert_eq!(summary.pending_question_count, 1);
}

#[test]
fn inactive_summary_bounds_status_message() {
    let mut registry = RoutingRegistry::new();
    let tid = ProjectTabId::new();
    let huge = "x".repeat(MAX_TAB_LAST_ERROR_LEN * 4);
    apply_inactive_summary(
        &mut registry,
        &tid,
        InactiveSummaryKind::StatusUpdate,
        Some(&huge),
    );
    let summary = registry.activity(&tid).expect("summary exists");
    let stored = summary.last_error.as_deref().unwrap();
    assert!(stored.ends_with('…'));
}

#[test]
fn routing_registry_drop_tab_clears_index() {
    let mut registry = RoutingRegistry::new();
    let t1 = ProjectTabId::new();
    let t2 = ProjectTabId::new();
    registry.register_open_session(t1.clone(), "s1".into());
    registry.register_open_session(t2.clone(), "s2".into());
    registry.drop_tab(&t1);
    assert_eq!(registry.tab_for_session("s1"), None);
    assert_eq!(registry.tab_for_session("s2"), Some(&t2));
}

#[tokio::test]
async fn task_registry_cancel_for_tab_only_targets_tab() {
    let mut reg = TuiTaskRegistry::new();
    let _a = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "tab_a",
        Some("tab-a".into()),
        None,
        None,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let _b = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "tab_b",
        Some("tab-b".into()),
        None,
        None,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let _global = reg.spawn_with_scope(
        TuiTaskKind::Other,
        "global",
        None,
        None,
        None,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    assert_eq!(reg.active_count(), 3);
    let n = reg.cancel_for_tab("tab-a");
    assert_eq!(n, 1);
    reg.reap_finished();
    assert_eq!(reg.active_count(), 2);
}

#[tokio::test]
async fn task_registry_cancel_for_session_only_targets_session() {
    let mut reg = TuiTaskRegistry::new();
    let _a = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "s1",
        None,
        Some("s-1".into()),
        None,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let _b = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "s2",
        None,
        Some("s-2".into()),
        None,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let n = reg.cancel_for_session("s-1");
    assert_eq!(n, 1);
}

#[tokio::test]
async fn task_registry_cancel_for_stale_epoch_only_targets_older() {
    let mut reg = TuiTaskRegistry::new();
    let _a = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "old",
        None,
        None,
        Some(1),
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let _b = reg.spawn_with_scope(
        TuiTaskKind::Command,
        "new",
        None,
        None,
        Some(5),
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        },
    );
    let n = reg.cancel_for_stale_epoch(5);
    assert_eq!(n, 1);
}

#[test]
fn view_switch_begin_loading_rejects_stale_epoch() {
    let mut coord = ViewSwitchCoordinator::new();
    let from = ProjectTabId::new();
    let to = ProjectTabId::new();
    let epoch = coord.begin_switch(from, to.clone());
    coord.bump_epoch();
    let ok = coord.begin_loading(to, "s".into(), "p".into(), "w".into(), epoch);
    assert!(!ok);
}

#[test]
fn view_switch_suspend_returns_prior_target_on_match() {
    let mut coord = ViewSwitchCoordinator::new();
    let from = ProjectTabId::new();
    let to = ProjectTabId::new();
    let epoch = coord.begin_switch(from, to.clone());
    let prior = coord.suspend_if_matches(epoch);
    assert_eq!(prior, Some(to));
}

#[test]
fn view_switch_replace_active_bumps_epoch() {
    let mut coord = ViewSwitchCoordinator::new();
    let from = ProjectTabId::new();
    let to = ProjectTabId::new();
    let e1 = coord.begin_switch(from, to);
    let next = coord.replace_active();
    assert!(next > e1);
}

#[test]
fn same_session_titles_across_tabs_do_not_collide_in_registry() {
    let mut registry = RoutingRegistry::new();
    let t1 = ProjectTabId::new();
    let t2 = ProjectTabId::new();
    registry.register_open_session(t1.clone(), "s1".into());
    registry.register_open_session(t2.clone(), "s1".into());
    // Re-registration must atomically move the binding to t2.
    assert_eq!(registry.tab_for_session("s1"), Some(&t2));
    assert_eq!(registry.activity(&t1).map(|s| s.activity_revision), None);
    assert!(registry.activity(&t2).is_some());
}
