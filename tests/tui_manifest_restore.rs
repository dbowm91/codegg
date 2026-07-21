//! Integration tests for the persistent manifest restore pipeline
//! (Multi-Project TUI Milestone 004).
//!
//! These tests pin down the contract that:
//!
//! * `ManifestPersistence` round-trips, coalesces, dedups, disables,
//!   resets, and writes with restrictive permissions.
//! * `DaemonLookupSnapshot::build_restore_plan` classifies persisted
//!   tabs into `RestoreEntryStatus` variants and selects the active
//!   tab correctly.
//! * `apply_restore_plan` materializes `ProjectTabs` and selects the
//!   active tab.
//! * `validate_manifest` deduplicates, caps, and rejects empty entries.
//! * `load_manifest_from` rejects oversized, symlink, invalid-JSON,
//!   and unsupported-major manifests.
//! * `snapshot_from_tabs` captures the active project and session.
//! * `ManifestDiagnostic::short_message()` is bounded.
//! * `ManifestPreferences` round-trips through serialization.

use std::collections::HashMap;
use std::time::Duration;

use codegg::tui::app::state::manifest::{
    validate_manifest, ManifestDiagnostic, ManifestLoadOutcome, PersistedProjectTab,
    TuiWorkspaceManifest, MANIFEST_SCHEMA_VERSION, MAX_MANIFEST_BYTES, MAX_PERSISTED_LABEL_LEN,
    MAX_PERSISTED_TABS,
};
use codegg::tui::app::state::persistence::{
    load_manifest_from, ManifestPersistence, PersistedSnapshot,
};
use codegg::tui::app::state::project_tabs::{ProjectTabState, ProjectTabs};
use codegg::tui::app::state::restore::{
    apply_restore_plan, CatalogEntry, DaemonLookupSnapshot, ProjectDetailSnapshot, SessionBinding,
};
use codegg::tui::app::state::snapshot::snapshot_from_tabs;
use codegg::tui::app::state::ProjectTabId;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn empty_snapshot() -> PersistedSnapshot {
    PersistedSnapshot {
        manifest: TuiWorkspaceManifest::default(),
    }
}

fn tab_with_project(pid: &str) -> PersistedProjectTab {
    PersistedProjectTab {
        project_id: Some(pid.into()),
        workspace_id: None,
        session_id: None,
        label_hint: None,
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    }
}

fn tab_with_session(pid: &str, sid: &str) -> PersistedProjectTab {
    PersistedProjectTab {
        project_id: Some(pid.into()),
        workspace_id: None,
        session_id: Some(sid.into()),
        label_hint: None,
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    }
}

fn make_live_tab(label: &str, project_id: Option<&str>) -> ProjectTabState {
    let mut t = ProjectTabState::empty(ProjectTabId::new(), label.into());
    t.project_id = project_id.map(str::to_string);
    t
}

// ---------------------------------------------------------------------------
// 1. manifest_persistence_round_trips_empty_manifest
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_round_trips_empty_manifest() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.schedule_force_save(empty_snapshot());
    svc.flush().unwrap();
    let outcome = svc.load_manifest();
    match outcome {
        ManifestLoadOutcome::Loaded(m) => {
            assert_eq!(m, TuiWorkspaceManifest::default());
        }
        other => panic!("expected Loaded, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. manifest_persistence_coalesces_rapid_writes
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_coalesces_rapid_writes() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    for _ in 0..10 {
        svc.schedule_force_save(empty_snapshot());
    }
    let wrote = svc.flush().unwrap();
    assert!(wrote);
    let m = svc.metrics();
    assert_eq!(m.saves_completed, 1);
}

// ---------------------------------------------------------------------------
// 3. manifest_persistence_dedups_identical_snapshots
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_dedups_identical_snapshots() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.schedule_force_save(empty_snapshot());
    svc.schedule_force_save(empty_snapshot());
    assert!(
        svc.metrics().saves_deduped >= 1,
        "saves_deduped should be >= 1"
    );
    let wrote = svc.flush().unwrap();
    assert!(wrote);
    let m = svc.metrics();
    assert_eq!(m.saves_completed, 1);
}

// ---------------------------------------------------------------------------
// 4. manifest_persistence_disable_drops_pending
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_disable_drops_pending() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.schedule_force_save(empty_snapshot());
    svc.disable();
    assert!(!svc.has_pending());
    assert!(svc.metrics().disabled);
    svc.schedule_save(empty_snapshot());
    let wrote = svc.flush().unwrap();
    assert!(!wrote);
}

// ---------------------------------------------------------------------------
// 5. manifest_persistence_reset_deletes_file
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_reset_deletes_file() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.schedule_force_save(empty_snapshot());
    svc.flush().unwrap();
    assert!(svc.manifest_path().exists());
    svc.reset().unwrap();
    assert!(!svc.manifest_path().exists());
}

// ---------------------------------------------------------------------------
// 6. restore_coordinator_marks_missing_projects
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_marks_missing_projects() {
    let snap = DaemonLookupSnapshot::default();
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_project("ghost"));
    let plan = snap.build_restore_plan(&m);
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].status,
        codegg::tui::app::state::restore::RestoreEntryStatus::Missing
    );
    assert!(!plan.entries[0].opens_tab());
}

// ---------------------------------------------------------------------------
// 7. restore_coordinator_marks_archived_projects
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_marks_archived_projects() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: true,
    });
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_project("p1"));
    let plan = snap.build_restore_plan(&m);
    assert_eq!(
        plan.entries[0].status,
        codegg::tui::app::state::restore::RestoreEntryStatus::Archived
    );
}

// ---------------------------------------------------------------------------
// 8. restore_coordinator_validates_workspace_membership
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_validates_workspace_membership() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    let mut details = HashMap::new();
    details.insert(
        "p1".to_string(),
        ProjectDetailSnapshot {
            project_id: "p1".into(),
            archived: false,
            workspaces: vec!["ws-known".into()],
            sessions: vec![],
        },
    );
    snap.project_details = details;
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("p1".into()),
        workspace_id: Some("ws-other".into()),
        session_id: None,
        label_hint: None,
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    });
    let plan = snap.build_restore_plan(&m);
    assert!(plan.entries[0].opens_tab());
    assert_eq!(plan.entries[0].resolved_workspace_id, None);
}

// ---------------------------------------------------------------------------
// 9. restore_coordinator_drops_rebound_session
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_drops_rebound_session() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    let mut details = HashMap::new();
    details.insert(
        "p1".to_string(),
        ProjectDetailSnapshot {
            project_id: "p1".into(),
            archived: false,
            workspaces: vec![],
            sessions: vec![SessionBinding {
                session_id: "s1".into(),
                canonical_project_id: "p-other".into(),
            }],
        },
    );
    snap.project_details = details;
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_session("p1", "s1"));
    let plan = snap.build_restore_plan(&m);
    assert!(plan.entries[0].resolved_session_id.is_none());
    assert!(plan.entries[0].opens_tab());
}

// ---------------------------------------------------------------------------
// 10. restore_coordinator_chooses_first_open_when_active_missing
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_chooses_first_open_when_active_missing() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    snap.catalog.push(CatalogEntry {
        project_id: "p2".into(),
        archived: false,
    });
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_project("p1"));
    m.ordered_tabs.push(tab_with_project("p2"));
    m.active_project_id = Some("ghost".into());
    let plan = snap.build_restore_plan(&m);
    assert!(plan.active_tab_id.is_some());
    assert_eq!(
        plan.entries
            .iter()
            .position(|e| Some(&e.tab_id) == plan.active_tab_id.as_ref()),
        Some(0)
    );
}

// ---------------------------------------------------------------------------
// 11. restore_coordinator_caps_at_max_persisted_tabs
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_caps_at_max_persisted_tabs() {
    let mut snap = DaemonLookupSnapshot::default();
    for i in 0..(MAX_PERSISTED_TABS + 5) {
        snap.catalog.push(CatalogEntry {
            project_id: format!("p{i}"),
            archived: false,
        });
    }
    let mut m = TuiWorkspaceManifest::default();
    for i in 0..(MAX_PERSISTED_TABS + 5) {
        m.ordered_tabs.push(tab_with_project(&format!("p{i}")));
    }
    let plan = snap.build_restore_plan(&m);
    assert_eq!(plan.entries.len(), MAX_PERSISTED_TABS);
}

// ---------------------------------------------------------------------------
// 12. restore_apply_materializes_tabs_and_active_selection
// ---------------------------------------------------------------------------
#[test]
fn restore_apply_materializes_tabs_and_active_selection() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    snap.catalog.push(CatalogEntry {
        project_id: "p2".into(),
        archived: false,
    });
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_project("p1"));
    m.ordered_tabs.push(tab_with_project("p2"));
    m.active_project_id = Some("p2".into());
    let plan = snap.build_restore_plan(&m);
    let mut tabs = ProjectTabs::new();
    let heavy = apply_restore_plan(&mut tabs, &plan);
    assert_eq!(tabs.len(), 2);
    let active = tabs.active().expect("active tab");
    assert_eq!(active.project_id.as_deref(), Some("p2"));
    assert!(heavy.is_none());
}

// ---------------------------------------------------------------------------
// 13. manifest_load_rejects_oversized_file
// ---------------------------------------------------------------------------
#[test]
fn manifest_load_rejects_oversized_file() {
    let dir = tmpdir();
    let svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.ensure_root().unwrap();
    let path = svc.manifest_path();
    let huge = vec![b'x'; MAX_MANIFEST_BYTES + 1];
    std::fs::write(&path, &huge).unwrap();
    let outcome = load_manifest_from(&path);
    assert!(matches!(
        outcome,
        ManifestLoadOutcome::Rejected(ManifestDiagnostic::Oversized { .. })
    ));
}

// ---------------------------------------------------------------------------
// 14. manifest_load_rejects_symlink_at_target
// ---------------------------------------------------------------------------
#[cfg(unix)]
#[test]
fn manifest_load_rejects_symlink_at_target() {
    let dir = tmpdir();
    let path = dir.path().join("tab_manifest.json");
    let target = dir.path().join("elsewhere.json");
    std::fs::write(&target, b"{}").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let outcome = load_manifest_from(&path);
    assert!(matches!(
        outcome,
        ManifestLoadOutcome::Rejected(ManifestDiagnostic::ForbiddenIdentity { .. })
    ));
}

// ---------------------------------------------------------------------------
// 15. manifest_load_rejects_invalid_json
// ---------------------------------------------------------------------------
#[test]
fn manifest_load_rejects_invalid_json() {
    let dir = tmpdir();
    let path = dir.path().join("tab_manifest.json");
    std::fs::write(&path, b"not json at all {{{").unwrap();
    let outcome = load_manifest_from(&path);
    assert!(matches!(
        outcome,
        ManifestLoadOutcome::Rejected(ManifestDiagnostic::InvalidJson { .. })
    ));
}

// ---------------------------------------------------------------------------
// 16. manifest_load_rejects_unsupported_major_version
// ---------------------------------------------------------------------------
#[test]
fn manifest_load_rejects_unsupported_major_version() {
    let dir = tmpdir();
    let path = dir.path().join("tab_manifest.json");
    let manifest = serde_json::json!({
        "schema_version": 99,
        "ordered_tabs": [],
    });
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let outcome = load_manifest_from(&path);
    assert!(matches!(
        outcome,
        ManifestLoadOutcome::Rejected(ManifestDiagnostic::UnsupportedMajor { .. })
    ));
}

// ---------------------------------------------------------------------------
// 17. snapshot_from_tabs_includes_active_project_and_session
// ---------------------------------------------------------------------------
#[test]
fn snapshot_from_tabs_includes_active_project_and_session() {
    let mut tabs = ProjectTabs::new();
    let mut a = make_live_tab("alpha", Some("proj-a"));
    a.session_id = Some("sess-1".into());
    tabs.add_and_activate(a);
    let snap = snapshot_from_tabs(&tabs);
    assert_eq!(snap.manifest.active_project_id.as_deref(), Some("proj-a"));
    assert_eq!(snap.manifest.active_session_id.as_deref(), Some("sess-1"));
    assert_eq!(snap.manifest.ordered_tabs.len(), 1);
}

// ---------------------------------------------------------------------------
// 18. snapshot_from_tabs_handles_empty_container
// ---------------------------------------------------------------------------
#[test]
fn snapshot_from_tabs_handles_empty_container() {
    let tabs = ProjectTabs::new();
    let snap = snapshot_from_tabs(&tabs);
    assert!(snap.manifest.ordered_tabs.is_empty());
    assert!(snap.manifest.active_project_id.is_none());
    assert!(snap.manifest.active_session_id.is_none());
}

// ---------------------------------------------------------------------------
// 19. manifest_validate_dedups_duplicate_projects
// ---------------------------------------------------------------------------
#[test]
fn manifest_validate_dedups_duplicate_projects() {
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("proj-A".into()),
        workspace_id: None,
        session_id: None,
        label_hint: None,
        selected_model_id: None,
        selected_agent: None,
        order_key: Some("k1".into()),
    });
    m.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("proj-A".into()),
        workspace_id: None,
        session_id: None,
        label_hint: None,
        selected_model_id: None,
        selected_agent: None,
        order_key: Some("k2".into()),
    });
    validate_manifest(&mut m).unwrap();
    let pids: Vec<&str> = m
        .ordered_tabs
        .iter()
        .filter_map(|t| t.project_id.as_deref())
        .collect();
    assert_eq!(pids, vec!["proj-A"]);
}

// ---------------------------------------------------------------------------
// 20. manifest_validate_caps_persisted_tabs
// ---------------------------------------------------------------------------
#[test]
fn manifest_validate_caps_persisted_tabs() {
    let mut m = TuiWorkspaceManifest::default();
    for i in 0..(MAX_PERSISTED_TABS + 5) {
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some(format!("proj-{i}")),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });
    }
    validate_manifest(&mut m).unwrap();
    assert_eq!(m.ordered_tabs.len(), MAX_PERSISTED_TABS);
}

// ---------------------------------------------------------------------------
// 21. manifest_validate_rejects_empty_entries
// ---------------------------------------------------------------------------
#[test]
fn manifest_validate_rejects_empty_entries() {
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(PersistedProjectTab {
        project_id: None,
        workspace_id: None,
        session_id: None,
        label_hint: Some("orphan".into()),
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    });
    validate_manifest(&mut m).unwrap();
    assert!(m.ordered_tabs.is_empty());
}

// ---------------------------------------------------------------------------
// 22. manifest_serialize_uses_deterministic_field_order
// ---------------------------------------------------------------------------
#[test]
fn manifest_serialize_uses_deterministic_field_order() {
    let mut m1 = TuiWorkspaceManifest::default();
    m1.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("p1".into()),
        workspace_id: Some("ws-1".into()),
        session_id: Some("s1".into()),
        label_hint: Some("My Project".into()),
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    });
    let mut m2 = TuiWorkspaceManifest::default();
    m2.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("p1".into()),
        workspace_id: Some("ws-1".into()),
        session_id: Some("s1".into()),
        label_hint: Some("My Project".into()),
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    });
    let bytes1 = serde_json::to_vec(&m1).unwrap();
    let bytes2 = serde_json::to_vec(&m2).unwrap();
    assert_eq!(bytes1, bytes2);
}

// ---------------------------------------------------------------------------
// 23. restore_coordinator_returns_no_heavy_load_when_session_missing
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_returns_no_heavy_load_when_session_missing() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    let mut details = HashMap::new();
    details.insert(
        "p1".to_string(),
        ProjectDetailSnapshot {
            project_id: "p1".into(),
            archived: false,
            workspaces: vec![],
            sessions: vec![],
        },
    );
    snap.project_details = details;
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_session("p1", "s1"));
    m.active_project_id = Some("p1".into());
    let plan = snap.build_restore_plan(&m);
    assert_eq!(plan.pending_heavy_load, None);
}

// ---------------------------------------------------------------------------
// 24. restore_coordinator_keeps_active_when_active_tab_open
// ---------------------------------------------------------------------------
#[test]
fn restore_coordinator_keeps_active_when_active_tab_open() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    snap.catalog.push(CatalogEntry {
        project_id: "p2".into(),
        archived: false,
    });
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_project("p1"));
    m.ordered_tabs.push(tab_with_project("p2"));
    m.active_project_id = Some("p2".into());
    let plan = snap.build_restore_plan(&m);
    assert!(plan.active_tab_id.is_some());
    let active_entry = plan
        .entries
        .iter()
        .find(|e| Some(&e.tab_id) == plan.active_tab_id.as_ref())
        .expect("active entry");
    assert_eq!(active_entry.resolved_project_id.as_deref(), Some("p2"));
}

// ---------------------------------------------------------------------------
// 25. manifest_persistence_writes_under_restrictive_permissions
// ---------------------------------------------------------------------------
#[cfg(unix)]
#[test]
fn manifest_persistence_writes_under_restrictive_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    svc.write_atomic(&TuiWorkspaceManifest::default()).unwrap();
    let meta = std::fs::metadata(svc.manifest_path()).unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0o600 got {mode:o}");
}

// ---------------------------------------------------------------------------
// 26. manifest_persistence_pending_is_due_respects_force_flag
// ---------------------------------------------------------------------------
#[test]
fn manifest_persistence_pending_is_due_respects_force_flag() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(10_000));
    svc.schedule_save(empty_snapshot());
    assert!(!svc.pending_is_due(), "not yet due with long debounce");
    let mut snap = empty_snapshot();
    snap.manifest.active_project_id = Some("different".into());
    svc.schedule_force_save(snap);
    assert!(svc.pending_is_due());
}

// ---------------------------------------------------------------------------
// 27. restore_apply_restore_plan_with_pending_heavy_load
// ---------------------------------------------------------------------------
#[test]
fn restore_apply_restore_plan_with_pending_heavy_load() {
    let mut snap = DaemonLookupSnapshot::default();
    snap.catalog.push(CatalogEntry {
        project_id: "p1".into(),
        archived: false,
    });
    let mut details = HashMap::new();
    details.insert(
        "p1".to_string(),
        ProjectDetailSnapshot {
            project_id: "p1".into(),
            archived: false,
            workspaces: vec![],
            sessions: vec![SessionBinding {
                session_id: "s1".into(),
                canonical_project_id: "p1".into(),
            }],
        },
    );
    snap.project_details = details;
    let mut m = TuiWorkspaceManifest::default();
    m.ordered_tabs.push(tab_with_session("p1", "s1"));
    m.active_project_id = Some("p1".into());
    let plan = snap.build_restore_plan(&m);
    assert!(plan.pending_heavy_load.is_some());
    let mut tabs = ProjectTabs::new();
    let heavy = apply_restore_plan(&mut tabs, &plan);
    assert!(heavy.is_some());
}

// ---------------------------------------------------------------------------
// 28. manifest_validate_truncates_long_label_hints
// ---------------------------------------------------------------------------
#[test]
fn manifest_validate_truncates_long_label_hints() {
    let mut m = TuiWorkspaceManifest::default();
    let long_label = "x".repeat(MAX_PERSISTED_LABEL_LEN + 50);
    m.ordered_tabs.push(PersistedProjectTab {
        project_id: Some("p1".into()),
        workspace_id: None,
        session_id: None,
        label_hint: Some(long_label),
        selected_model_id: None,
        selected_agent: None,
        order_key: None,
    });
    validate_manifest(&mut m).unwrap();
    let label = m.ordered_tabs[0].label_hint.as_deref().unwrap();
    assert!(
        label.chars().count() <= MAX_PERSISTED_LABEL_LEN,
        "label_hint should be truncated to {} chars, got {}",
        MAX_PERSISTED_LABEL_LEN,
        label.chars().count()
    );
}

// ---------------------------------------------------------------------------
// 29. manifest_diagnostic_short_messages_are_bounded
// ---------------------------------------------------------------------------
#[test]
fn manifest_diagnostic_short_messages_are_bounded() {
    for diag in [
        ManifestDiagnostic::Oversized { bytes: 100 },
        ManifestDiagnostic::Unreadable {
            reason: "perm denied".into(),
        },
        ManifestDiagnostic::UnsupportedMajor { on_disk: 99 },
        ManifestDiagnostic::InvalidJson {
            reason: "bad".into(),
        },
        ManifestDiagnostic::InvalidFields { reason: "x".into() },
        ManifestDiagnostic::ForbiddenIdentity { reason: "y".into() },
    ] {
        let msg = diag.short_message();
        assert!(msg.len() < 200, "diagnostic message too long: {msg}");
        assert!(!msg.is_empty());
    }
}

// ---------------------------------------------------------------------------
// 30. manifest_preferences_round_trip
// ---------------------------------------------------------------------------
#[test]
fn manifest_preferences_round_trip() {
    let mut m = TuiWorkspaceManifest::default();
    m.preferences.sidebar_visible = Some(true);
    let json = serde_json::to_string(&m).unwrap();
    let back: TuiWorkspaceManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.preferences.sidebar_visible, Some(true));
}
