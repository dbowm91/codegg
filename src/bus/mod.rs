pub mod events;
pub mod global;

use crate::permission::PermissionChoice;
use dashmap::DashMap;
use once_cell::sync::Lazy;

static PERMISSION_REGISTRY: Lazy<PermissionRegistry> = Lazy::new(PermissionRegistry::new);

pub struct PermissionRegistry {
    senders: DashMap<String, tokio::sync::oneshot::Sender<PermissionChoice>>,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    pub async fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionChoice>) {
        PERMISSION_REGISTRY.senders.insert(perm_id, tx);
    }

    pub async fn respond(perm_id: String, choice: PermissionChoice) -> bool {
        if let Some((_, tx)) = PERMISSION_REGISTRY.senders.remove(&perm_id) {
            let _ = tx.send(choice);
            true
        } else {
            false
        }
    }

    pub async fn unregister(perm_id: &str) {
        PERMISSION_REGISTRY.senders.remove(perm_id);
    }

    pub fn is_registered(perm_id: &str) -> bool {
        PERMISSION_REGISTRY.senders.contains_key(perm_id)
    }

    pub fn pending_permission_ids() -> Vec<String> {
        PERMISSION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl Default for PermissionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static QUESTION_REGISTRY: Lazy<QuestionRegistry> = Lazy::new(QuestionRegistry::new);

pub struct QuestionRegistry {
    senders: DashMap<String, tokio::sync::oneshot::Sender<String>>,
}

impl QuestionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    pub async fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>) {
        QUESTION_REGISTRY.senders.insert(question_id, tx);
    }

    pub async fn answer_question(question_id: String, answers: String) -> bool {
        if let Some((_, tx)) = QUESTION_REGISTRY.senders.remove(&question_id) {
            let _ = tx.send(answers);
            true
        } else {
            false
        }
    }

    pub async fn unregister(question_id: &str) {
        QUESTION_REGISTRY.senders.remove(question_id);
    }

    pub fn is_registered(question_id: &str) -> bool {
        QUESTION_REGISTRY.senders.contains_key(question_id)
    }

    pub fn pending_question_ids() -> Vec<String> {
        QUESTION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl Default for QuestionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
