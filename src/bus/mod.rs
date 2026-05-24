pub mod events;
pub mod global;

use crate::permission::PermissionChoice;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::time::{Duration, Instant};

static PERMISSION_REGISTRY: Lazy<PermissionRegistry> = Lazy::new(PermissionRegistry::new);

pub struct PermissionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<PermissionChoice>, Instant)>,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionChoice>) {
        Self::cleanup();
        PERMISSION_REGISTRY
            .senders
            .insert(perm_id, (tx, Instant::now()));
    }

    pub fn respond(perm_id: String, choice: PermissionChoice) -> bool {
        if let Some((_, (tx, _))) = PERMISSION_REGISTRY.senders.remove(&perm_id) {
            if let Err(_) = tx.send(choice) {
                tracing::warn!("failed to send permission response for {}", perm_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    pub fn unregister(perm_id: &str) {
        PERMISSION_REGISTRY.senders.remove(perm_id);
    }

    pub fn is_registered(perm_id: &str) -> bool {
        PERMISSION_REGISTRY.senders.contains_key(perm_id)
    }

    pub fn pending_permission_ids() -> Vec<String> {
        Self::cleanup();
        PERMISSION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn cleanup() {
        let ttl = Duration::from_secs(300);
        PERMISSION_REGISTRY
            .senders
            .retain(|_, (_, created)| created.elapsed() < ttl);
    }

    pub fn cleanup_now() {
        Self::cleanup();
    }
}

impl Default for PermissionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static QUESTION_REGISTRY: Lazy<QuestionRegistry> = Lazy::new(QuestionRegistry::new);

pub struct QuestionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<String>, Instant)>,
}

impl QuestionRegistry {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    pub fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>) {
        Self::cleanup();
        QUESTION_REGISTRY
            .senders
            .insert(question_id, (tx, Instant::now()));
    }

    pub fn answer_question(question_id: String, answers: String) -> bool {
        if let Some((_, (tx, _))) = QUESTION_REGISTRY.senders.remove(&question_id) {
            if let Err(_) = tx.send(answers) {
                tracing::warn!("failed to send question answer for {}", question_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    pub fn unregister(question_id: &str) {
        QUESTION_REGISTRY.senders.remove(question_id);
    }

    pub fn is_registered(question_id: &str) -> bool {
        QUESTION_REGISTRY.senders.contains_key(question_id)
    }

    pub fn pending_question_ids() -> Vec<String> {
        Self::cleanup();
        QUESTION_REGISTRY
            .senders
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn cleanup() {
        let ttl = Duration::from_secs(300);
        QUESTION_REGISTRY
            .senders
            .retain(|_, (_, created)| created.elapsed() < ttl);
    }

    pub fn cleanup_now() {
        Self::cleanup();
    }
}

impl Default for QuestionRegistry {
    fn default() -> Self {
        Self::new()
    }
}