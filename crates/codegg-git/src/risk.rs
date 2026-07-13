use serde::{Deserialize, Serialize};
use std::fmt;

/// Risk classification for a Git operation. An operation may carry
/// multiple risk classes (e.g., a force-push is both NetworkWrite
/// and DestructiveHistory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitRiskClass {
    /// Read-only operation; no side effects.
    ReadOnly,
    /// Modifies the staging index (add, reset paths, stash create).
    IndexMutation,
    /// Modifies working tree files (checkout paths, restore, clean, stash apply/pop).
    WorktreeMutation,
    /// Creates, renames, or deletes refs (branch, tag).
    RefMutation,
    /// Integrates commit history (merge, rebase, cherry-pick, revert, reset --hard).
    HistoryIntegration,
    /// Reads from a remote (fetch).
    NetworkRead,
    /// Writes to a remote (push).
    NetworkWrite,
    /// Modifies repository configuration (git config).
    RepositoryConfigMutation,
    /// Destructive worktree changes (clean -f, checkout --force, reset --hard).
    DestructiveWorktree,
    /// Destructive history changes (force push, reset --hard, branch -D).
    DestructiveHistory,
    /// References paths outside the project root (-C with external path).
    OutsideProject,
}

impl fmt::Display for GitRiskClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "read-only"),
            Self::IndexMutation => write!(f, "index-mutation"),
            Self::WorktreeMutation => write!(f, "worktree-mutation"),
            Self::RefMutation => write!(f, "ref-mutation"),
            Self::HistoryIntegration => write!(f, "history-integration"),
            Self::NetworkRead => write!(f, "network-read"),
            Self::NetworkWrite => write!(f, "network-write"),
            Self::RepositoryConfigMutation => write!(f, "repo-config-mutation"),
            Self::DestructiveWorktree => write!(f, "destructive-worktree"),
            Self::DestructiveHistory => write!(f, "destructive-history"),
            Self::OutsideProject => write!(f, "outside-project"),
        }
    }
}

impl GitRiskClass {
    /// Whether this risk class implies data loss potential.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::DestructiveWorktree | Self::DestructiveHistory)
    }

    /// Whether this risk class requires network access.
    pub fn requires_network(&self) -> bool {
        matches!(self, Self::NetworkRead | Self::NetworkWrite)
    }
}

/// Risk set for an operation — a collection of risk classes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RiskSet(Vec<GitRiskClass>);

impl RiskSet {
    pub fn new(classes: Vec<GitRiskClass>) -> Self {
        Self(classes)
    }

    pub fn read_only() -> Self {
        Self(vec![GitRiskClass::ReadOnly])
    }

    pub fn contains(&self, class: &GitRiskClass) -> bool {
        self.0.contains(class)
    }

    pub fn is_destructive(&self) -> bool {
        self.0.iter().any(|c| c.is_destructive())
    }

    pub fn requires_network(&self) -> bool {
        self.0.iter().any(|c| c.requires_network())
    }

    pub fn classes(&self) -> &[GitRiskClass] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_set() {
        let rs = RiskSet::read_only();
        assert!(rs.contains(&GitRiskClass::ReadOnly));
        assert!(!rs.is_destructive());
        assert!(!rs.requires_network());
        assert_eq!(rs.classes().len(), 1);
    }

    #[test]
    fn destructive_set() {
        let rs = RiskSet::new(vec![
            GitRiskClass::DestructiveWorktree,
            GitRiskClass::IndexMutation,
        ]);
        assert!(rs.is_destructive());
        assert!(!rs.requires_network());
    }

    #[test]
    fn network_set() {
        let rs = RiskSet::new(vec![
            GitRiskClass::NetworkWrite,
            GitRiskClass::DestructiveHistory,
        ]);
        assert!(rs.requires_network());
        assert!(rs.is_destructive());
    }

    #[test]
    fn risk_class_display() {
        assert_eq!(GitRiskClass::ReadOnly.to_string(), "read-only");
        assert_eq!(GitRiskClass::IndexMutation.to_string(), "index-mutation");
        assert_eq!(
            GitRiskClass::DestructiveWorktree.to_string(),
            "destructive-worktree"
        );
        assert_eq!(
            GitRiskClass::DestructiveHistory.to_string(),
            "destructive-history"
        );
        assert_eq!(GitRiskClass::OutsideProject.to_string(), "outside-project");
    }

    #[test]
    fn risk_class_destructive() {
        assert!(GitRiskClass::DestructiveWorktree.is_destructive());
        assert!(GitRiskClass::DestructiveHistory.is_destructive());
        assert!(!GitRiskClass::WorktreeMutation.is_destructive());
        assert!(!GitRiskClass::HistoryIntegration.is_destructive());
    }

    #[test]
    fn risk_class_network() {
        assert!(GitRiskClass::NetworkRead.requires_network());
        assert!(GitRiskClass::NetworkWrite.requires_network());
        assert!(!GitRiskClass::ReadOnly.requires_network());
        assert!(!GitRiskClass::IndexMutation.requires_network());
    }

    #[test]
    fn empty_risk_set() {
        let rs = RiskSet::new(vec![]);
        assert!(rs.is_empty());
        assert!(!rs.is_destructive());
        assert!(!rs.requires_network());
    }
}
