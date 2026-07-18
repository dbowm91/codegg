use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use tokio::sync::{mpsc, watch, RwLock};

use codegg_core::workspace::WorkspaceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSessionStatus {
    Idle,
    Running,
    Paused,
}

/// Per-session runtime state used by `CoreDaemon`.
///
/// **Phase 2**: `workspace_id` and `workspace_root` are the authoritative
/// identity for execution. `project_id` and `directory` remain as
/// compatibility projections for legacy session metadata and external
/// snapshot consumers (clients and tests that still expect the old
/// fields). When both are present, the workspace identity wins.
pub struct SessionRuntime {
    pub session_id: String,
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
    /// Compatibility projection of the session's stored `project_id`.
    /// Pre-Phase-2 clients and CLI output still read this. New code
    /// should prefer `workspace_id`.
    pub project_id: String,
    /// Compatibility projection of the session's stored `directory`. This
    /// is always equal to `workspace_root` for sessions created or
    /// rebound through `WorkspaceRegistry`. May differ for legacy
    /// sessions whose directory no longer exists on disk.
    pub directory: PathBuf,
    pub status: RwLock<RuntimeSessionStatus>,
    pub selected_model: RwLock<Option<String>>,
    pub selected_agent: RwLock<Option<String>>,
    pub active_turn: RwLock<Option<TurnHandle>>,
    pub attached_clients: DashMap<String, AttachMode>,
    pub pending_permissions: DashSet<String>,
    pub pending_questions: DashSet<String>,
    pub last_input_tokens: RwLock<Option<usize>>,
    pub last_output_tokens: RwLock<Option<usize>>,
    pub active_subagent_count: AtomicUsize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachMode {
    Observe,
    Control,
    ExclusiveControl,
}

pub struct TurnHandle {
    pub turn_id: String,
    pub cancel_tx: watch::Sender<bool>,
    pub steer_tx: Option<mpsc::UnboundedSender<String>>,
    pub started_at: DateTime<Utc>,
    /// Immutable runtime-asset identity captured when this turn started.
    pub asset_pin:
        Option<std::sync::Arc<std::sync::Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
}

pub struct SessionRuntimeRegistry {
    sessions: DashMap<String, std::sync::Arc<SessionRuntime>>,
}

impl Default for SessionRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    /// Construct a workspace-bound runtime. This is the canonical entry
    /// point used by the daemon once a session has been resolved to a
    /// `WorkspaceRecord`.
    ///
    /// `project_id` and `directory` are stored as compatibility
    /// projections; they MUST equal the workspace identity for new
    /// sessions.
    pub fn get_or_create(
        &self,
        session_id: &str,
        workspace_id: WorkspaceId,
        workspace_root: PathBuf,
        project_id: String,
        directory: PathBuf,
    ) -> std::sync::Arc<SessionRuntime> {
        let key = session_id.to_string();
        if let Some(existing) = self.sessions.get(&key) {
            return existing.clone();
        }
        let arc = std::sync::Arc::new(SessionRuntime {
            session_id: session_id.to_string(),
            workspace_id,
            workspace_root,
            project_id,
            directory,
            status: RwLock::new(RuntimeSessionStatus::Idle),
            selected_model: RwLock::new(None),
            selected_agent: RwLock::new(None),
            active_turn: RwLock::new(None),
            attached_clients: DashMap::new(),
            pending_permissions: DashSet::new(),
            pending_questions: DashSet::new(),
            last_input_tokens: RwLock::new(None),
            last_output_tokens: RwLock::new(None),
            active_subagent_count: AtomicUsize::new(0),
        });
        self.sessions.insert(key, arc.clone());
        arc
    }

    /// Test helper: build a minimal workspace-bound runtime without
    /// going through `WorkspaceRegistry`. Production callers must use
    /// `CoreDaemon::bind_runtime` (which resolves/registers the
    /// directory) or the full `get_or_create` constructor.
    #[cfg(test)]
    pub fn get_or_create_for_test(
        &self,
        session_id: &str,
        workspace_root: PathBuf,
    ) -> std::sync::Arc<SessionRuntime> {
        let workspace_id =
            WorkspaceId::new_unchecked(format!("test-ws-{}", uuid::Uuid::new_v4().simple()));
        let path_str = workspace_root.to_string_lossy().into_owned();
        self.get_or_create(
            session_id,
            workspace_id,
            workspace_root,
            path_str.clone(),
            PathBuf::from(path_str),
        )
    }

    pub fn get(&self, session_id: &str) -> Option<std::sync::Arc<SessionRuntime>> {
        self.sessions
            .get(session_id)
            .map(|r| std::sync::Arc::clone(&r))
    }

    #[allow(dead_code)]
    pub fn remove(&self, session_id: &str) -> Option<std::sync::Arc<SessionRuntime>> {
        self.sessions.remove(session_id).map(|(_, v)| v)
    }

    #[allow(dead_code)]
    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.iter().map(|r| r.key().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_id() -> WorkspaceId {
        WorkspaceId::new_unchecked("ws-test-1")
    }

    #[test]
    fn registry_creates_and_gets() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        assert_eq!(rt.session_id, "s1");
        assert_eq!(rt.workspace_id.as_str(), "ws-test-1");

        let got = reg.get("s1");
        assert!(got.is_some());
        assert_eq!(got.unwrap().session_id, "s1");
    }

    #[test]
    fn registry_dedupes_session_id() {
        let reg = SessionRuntimeRegistry::new();
        let a = reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        let b = reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn registry_remove() {
        let reg = SessionRuntimeRegistry::new();
        reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        assert!(reg.get("s1").is_some());

        reg.remove("s1");
        assert!(reg.get("s1").is_none());
    }

    #[test]
    fn runtime_starts_idle() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        assert_eq!(*rt.status.blocking_read(), RuntimeSessionStatus::Idle);
        assert!(rt.active_turn.blocking_read().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_permissions_tracked() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create(
            "s1",
            ws_id(),
            PathBuf::from("/tmp"),
            "p1".into(),
            PathBuf::from("/tmp"),
        );
        rt.pending_permissions.insert("perm-1".to_string());
        rt.pending_permissions.insert("perm-2".to_string());
        assert_eq!(rt.pending_permissions.len(), 2);
        rt.pending_permissions.remove("perm-1");
        assert_eq!(rt.pending_permissions.len(), 1);
    }
}
