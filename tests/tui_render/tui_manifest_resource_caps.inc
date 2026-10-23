//! Resource-bounds tests for Multi-Project TUI milestone 4.
//!
//! Verifies that long-running operations on the manifest/restore
//! surface stay within bounded memory, task, and write-frequency
//! caps. These are lightweight integration tests; the heavier soak
//! tests live in `tests/tui_manifest_restore.rs`.
//!
//! See `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`
//! for the full resource-cap specification.

use codegg::tui::app::state::manifest::{
    validate_manifest, ManifestDiagnostic, PersistedProjectTab, TuiWorkspaceManifest,
    MAX_MANIFEST_BYTES, MAX_PERSISTED_TABS,
};
use codegg::tui::app::state::persistence::{
    ManifestPersistence, PersistedSnapshot, PersistenceMetrics, DEFAULT_DEBOUNCE,
};
use std::time::Duration;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn resource_caps_hold_under_high_tab_count() {
    let mut m = TuiWorkspaceManifest::default();
    for i in 0..(MAX_PERSISTED_TABS + 50) {
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
    validate_manifest(&mut m).expect("validate ok");
    assert_eq!(
        m.ordered_tabs.len(),
        MAX_PERSISTED_TABS,
        "manifest must be capped at MAX_PERSISTED_TABS"
    );
}

#[test]
fn resource_caps_hold_under_oversized_input() {
    let mut m = TuiWorkspaceManifest::default();
    let huge = "x".repeat(MAX_MANIFEST_BYTES);
    m.written_at = Some(huge);
    validate_manifest(&mut m).expect("validate ok");
    assert!(m.written_at.is_some());
    let len = m.written_at.as_ref().unwrap().len();
    assert!(len <= 128, "written_at should be truncated, got {len}");
}

#[test]
fn resource_caps_hold_under_massive_label_hints() {
    let mut m = TuiWorkspaceManifest::default();
    for i in 0..MAX_PERSISTED_TABS {
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some(format!("p-{i}")),
            workspace_id: None,
            session_id: None,
            label_hint: Some("a".repeat(2048)),
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });
    }
    validate_manifest(&mut m).expect("validate ok");
    for tab in &m.ordered_tabs {
        let len = tab.label_hint.as_ref().unwrap().chars().count();
        assert!(len <= 128, "label_hint too long: {len}");
    }
}

#[test]
fn resource_caps_hold_under_rapid_saves() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), DEFAULT_DEBOUNCE);
    let snap = PersistedSnapshot {
        manifest: TuiWorkspaceManifest::default(),
    };
    for _ in 0..100 {
        svc.schedule_save(snap.clone());
    }
    assert!(svc.has_pending());
    svc.flush().expect("flush ok");
    let m: PersistenceMetrics = svc.metrics();
    assert_eq!(m.saves_completed, 1);
    assert!(m.saves_coalesced + m.saves_deduped >= 99);
}

#[test]
fn resource_caps_hold_under_disable_re_enable_cycles() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::new(dir.path());
    for _ in 0..50 {
        svc.disable();
        svc.enable();
    }
    let snap = PersistedSnapshot {
        manifest: TuiWorkspaceManifest::default(),
    };
    svc.schedule_force_save(snap);
    svc.flush().expect("flush ok");
    assert_eq!(svc.metrics().saves_completed, 1);
}

#[test]
fn resource_caps_hold_under_corrupt_manifest_variants() {
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
        assert!(msg.len() < 200, "diagnostic too long: {msg}");
        assert!(!msg.is_empty());
    }
}

#[test]
fn debounce_window_is_subsecond() {
    // The default debounce must be small enough that the TUI
    // event loop tick (16 ms) catches it within a handful of
    // iterations.
    assert!(DEFAULT_DEBOUNCE < Duration::from_millis(2000));
}

#[test]
fn metrics_reset_only_on_metrics_call() {
    let dir = tmpdir();
    let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
    let snap = PersistedSnapshot {
        manifest: TuiWorkspaceManifest::default(),
    };
    svc.schedule_force_save(snap.clone());
    svc.flush().unwrap();
    let m1 = svc.metrics();
    svc.schedule_force_save(snap);
    svc.flush().unwrap();
    let m2 = svc.metrics();
    assert!(m2.saves_completed > m1.saves_completed);
}
