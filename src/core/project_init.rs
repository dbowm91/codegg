//! Ephemeral daemon-owned state for reviewed project-initialization drafts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MAX_PENDING_DRAFTS: usize = 32;
const DEFAULT_DRAFT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub(crate) struct PendingProjectInitDraft {
    pub client_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub expected_digest: Option<String>,
    pub candidate_markdown: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsumeDraftError {
    MissingOrExpired,
    ScopeMismatch,
}

#[derive(Default)]
pub struct ProjectInitDraftRegistry {
    drafts: parking_lot::Mutex<HashMap<String, PendingProjectInitDraft>>,
}

impl ProjectInitDraftRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn issue(
        &self,
        client_id: String,
        project_id: String,
        workspace_id: String,
        workspace_root: PathBuf,
        expected_digest: Option<String>,
        candidate_markdown: String,
    ) -> Result<String, ()> {
        self.issue_with_ttl(
            client_id,
            project_id,
            workspace_id,
            workspace_root,
            expected_digest,
            candidate_markdown,
            DEFAULT_DRAFT_TTL,
        )
    }

    fn issue_with_ttl(
        &self,
        client_id: String,
        project_id: String,
        workspace_id: String,
        workspace_root: PathBuf,
        expected_digest: Option<String>,
        candidate_markdown: String,
        ttl: Duration,
    ) -> Result<String, ()> {
        let now = Instant::now();
        let mut drafts = self.drafts.lock();
        drafts.retain(|_, draft| draft.expires_at > now);
        if drafts.len() >= MAX_PENDING_DRAFTS {
            return Err(());
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        drafts.insert(
            token.clone(),
            PendingProjectInitDraft {
                client_id,
                project_id,
                workspace_id,
                workspace_root,
                expected_digest,
                candidate_markdown,
                expires_at: now + ttl,
            },
        );
        Ok(token)
    }

    pub(crate) fn consume(
        &self,
        token: &str,
        client_id: &str,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<PendingProjectInitDraft, ConsumeDraftError> {
        let Some(draft) = self.drafts.lock().remove(token) else {
            return Err(ConsumeDraftError::MissingOrExpired);
        };
        if draft.expires_at <= Instant::now() {
            return Err(ConsumeDraftError::MissingOrExpired);
        }
        if draft.client_id != client_id
            || draft.project_id != project_id
            || draft.workspace_id != workspace_id
        {
            return Err(ConsumeDraftError::ScopeMismatch);
        }
        Ok(draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> (ProjectInitDraftRegistry, String) {
        let registry = ProjectInitDraftRegistry::new();
        let token = registry
            .issue(
                "client-a".into(),
                "project-a".into(),
                "workspace-a".into(),
                PathBuf::from("/tmp/workspace-a"),
                None,
                "candidate".into(),
            )
            .unwrap();
        (registry, token)
    }

    #[test]
    fn tokens_are_scope_bound_and_single_use() {
        let (registry, token) = registry();
        let draft = registry
            .consume(&token, "client-a", "project-a", "workspace-a")
            .unwrap();
        assert_eq!(draft.candidate_markdown, "candidate");
        assert!(matches!(
            registry.consume(&token, "client-a", "project-a", "workspace-a"),
            Err(ConsumeDraftError::MissingOrExpired)
        ));
    }

    #[test]
    fn daemon_restart_discards_unpublished_drafts() {
        let (_, token) = registry();
        let restarted = ProjectInitDraftRegistry::new();
        assert!(matches!(
            restarted.consume(&token, "client-a", "project-a", "workspace-a"),
            Err(ConsumeDraftError::MissingOrExpired)
        ));
    }

    #[test]
    fn wrong_scope_consumes_token_without_revealing_candidate() {
        let (registry, token) = registry();
        assert!(matches!(
            registry.consume(&token, "client-b", "project-a", "workspace-a"),
            Err(ConsumeDraftError::ScopeMismatch)
        ));
        assert!(matches!(
            registry.consume(&token, "client-a", "project-a", "workspace-a"),
            Err(ConsumeDraftError::MissingOrExpired)
        ));
    }

    #[test]
    fn expired_tokens_cannot_be_consumed() {
        let registry = ProjectInitDraftRegistry::new();
        let token = registry
            .issue_with_ttl(
                "client-a".into(),
                "project-a".into(),
                "workspace-a".into(),
                PathBuf::from("/tmp/workspace-a"),
                None,
                "candidate".into(),
                Duration::ZERO,
            )
            .unwrap();
        assert!(matches!(
            registry.consume(&token, "client-a", "project-a", "workspace-a"),
            Err(ConsumeDraftError::MissingOrExpired)
        ));
    }
}
