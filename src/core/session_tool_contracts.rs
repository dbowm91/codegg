//! The daemon-owned summary of each session's own tool contracts.
//!
//! ADR-0053: the daemon publishes the contracts of the registry the turn
//! runs against. The store holds that summary only — no registry and no
//! tool instance — and an unrecorded session has no entry.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::protocol::projection::dto::ToolContractProjection;

/// The contract summary of each session's own tool registry.
#[derive(Clone, Default)]
pub struct SessionToolContractStore {
    contracts: Arc<RwLock<HashMap<String, Vec<ToolContractProjection>>>>,
}

impl SessionToolContractStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the contracts the session's own registry holds.
    pub fn set(&self, session_id: &str, contracts: Vec<ToolContractProjection>) {
        self.contracts
            .write()
            .insert(session_id.to_string(), contracts);
    }

    /// The contracts the session's own registry holds; empty when the
    /// session has no record.
    pub fn get(&self, session_id: &str) -> Vec<ToolContractProjection> {
        self.contracts
            .read()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(name: &str) -> ToolContractProjection {
        ToolContractProjection {
            name: name.to_string(),
            caller_policy: "direct".to_string(),
            effect_class: "ReadOnly".to_string(),
            idempotency: "Idempotent".to_string(),
            implementation_id: format!("codegg/{name}"),
        }
    }

    #[test]
    fn the_store_is_send_sync_and_clone() {
        fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
        assert_send_sync_clone::<SessionToolContractStore>();

        let store = SessionToolContractStore::new();
        store.set("session-a", vec![contract("read")]);
        let clone = store.clone();
        clone.set("session-b", vec![contract("bash")]);

        assert_eq!(
            clone.get("session-a"),
            store.get("session-a"),
            "a cloned store must see the contracts recorded before the clone"
        );
        assert_eq!(clone.get("session-b").len(), 1);
    }

    #[test]
    fn set_then_get_returns_the_recorded_contracts() {
        let store = SessionToolContractStore::new();
        store.set("session-a", vec![contract("read"), contract("bash")]);

        let recorded = store.get("session-a");
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].name, "read");
        assert_eq!(recorded[1].name, "bash");
    }

    #[test]
    fn an_unrecorded_session_has_no_contracts() {
        let store = SessionToolContractStore::new();
        store.set("session-a", vec![contract("read")]);

        assert!(
            store.get("session-b").is_empty(),
            "a session with no record must not read as a fabricated default"
        );
    }
}
