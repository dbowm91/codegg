use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use tokio::sync::{mpsc, watch, RwLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSessionStatus {
    Idle,
    Running,
    Paused,
}

pub struct SessionRuntime {
    pub session_id: String,
    pub project_id: String,
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

    pub fn get_or_create(
        &self,
        session_id: &str,
        project_id: &str,
        directory: PathBuf,
    ) -> std::sync::Arc<SessionRuntime> {
        self.sessions
            .entry(session_id.to_string())
            .or_insert_with(|| {
                std::sync::Arc::new(SessionRuntime {
                    session_id: session_id.to_string(),
                    project_id: project_id.to_string(),
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
                })
            })
            .clone()
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

    #[test]
    fn registry_creates_and_gets() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create("s1", "p1", PathBuf::from("/tmp"));
        assert_eq!(rt.session_id, "s1");
        assert_eq!(rt.project_id, "p1");

        let got = reg.get("s1");
        assert!(got.is_some());
        assert_eq!(got.unwrap().session_id, "s1");
    }

    #[test]
    fn registry_remove() {
        let reg = SessionRuntimeRegistry::new();
        reg.get_or_create("s1", "p1", PathBuf::from("/tmp"));
        assert!(reg.get("s1").is_some());

        reg.remove("s1");
        assert!(reg.get("s1").is_none());
    }

    #[test]
    fn runtime_starts_idle() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create("s1", "p1", PathBuf::from("/tmp"));
        assert_eq!(*rt.status.blocking_read(), RuntimeSessionStatus::Idle);
        assert!(rt.active_turn.blocking_read().is_none());
    }

    #[tokio::test]
    async fn pending_permissions_tracked() {
        let reg = SessionRuntimeRegistry::new();
        let rt = reg.get_or_create("s1", "p1", PathBuf::from("/tmp"));
        rt.pending_permissions.insert("perm-1".to_string());
        rt.pending_permissions.insert("perm-2".to_string());
        assert_eq!(rt.pending_permissions.len(), 2);
        rt.pending_permissions.remove("perm-1");
        assert_eq!(rt.pending_permissions.len(), 1);
    }
}
