use crate::path::{Pathspec, RepoPath};
use crate::ref_name::{BranchName, RemoteName, RevisionExpr};
use crate::risk::{GitRiskClass, RiskSet};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A fully parsed Git operation with typed arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitOperation {
    // ── Read-only status/inspection ──
    Status {
        short: bool,
    },
    Diff {
        staged: bool,
        stat: bool,
        name_only: bool,
        base_ref: Option<RevisionExpr>,
        paths: Vec<RepoPath>,
    },
    DiffStaged {
        stat: bool,
        name_only: bool,
        paths: Vec<RepoPath>,
    },
    Show {
        rev: RevisionExpr,
    },
    Log {
        oneline: bool,
        max_count: Option<u32>,
        paths: Vec<RepoPath>,
    },
    Blame {
        path: RepoPath,
    },
    ChangedFiles {
        base_ref: Option<RevisionExpr>,
    },

    // ── Branch/remote/tag/worktree listing ──
    BranchList {
        remotes: bool,
        all: bool,
    },
    RemoteList,
    RemoteGetUrl {
        remote: RemoteName,
    },
    TagList,
    WorktreeList,

    // ── Staging ──
    Add {
        paths: Vec<RepoPath>,
    },
    Reset {
        mode: ResetMode,
        paths: Option<Vec<RepoPath>>,
        rev: Option<RevisionExpr>,
    },

    // ── Commit ──
    Commit {
        message: String,
        amend: bool,
        allow_empty: bool,
    },

    // ── Stash ──
    StashList,
    StashShow {
        stash: Option<RevisionExpr>,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
        paths: Vec<Pathspec>,
    },
    StashApply {
        stash: Option<RevisionExpr>,
        index: bool,
    },
    StashPop {
        stash: Option<RevisionExpr>,
        index: bool,
    },
    StashDrop {
        stash: RevisionExpr,
    },

    // ── Checkout/Switch/Restore ──
    Checkout {
        /// The target: a branch name, tag, or revision.
        target: Option<String>,
        /// Paths to checkout (checkout -- <paths>).
        paths: Option<Vec<RepoPath>>,
        create: bool, // -b flag
        force: bool,
    },
    Switch {
        branch: BranchName,
        create: bool, // -c flag
        force: bool,
        detach: bool, // --detach
    },
    Restore {
        staged: bool, // --staged / --source
        paths: Vec<RepoPath>,
        source: Option<String>,
        worktree: bool, // --worktree
    },

    // ── Branch/Tag create/delete ──
    BranchCreate {
        name: BranchName,
        start_point: Option<String>,
        force: bool,
    },
    BranchDelete {
        name: BranchName,
        force: bool, // -D vs -d
    },
    BranchRename {
        old: BranchName,
        new: BranchName,
        force: bool,
    },
    TagCreate {
        name: String,
        rev: Option<String>,
        message: Option<String>,
        annotated: bool,
    },
    TagDelete {
        name: String,
    },
    TagForceDelete {
        name: String,
    },

    // ── Merge/Rebase/Cherry-pick/Revert ──
    Merge {
        revisions: Vec<String>,
        no_ff: bool,
        strategy: Option<String>,
        abort: bool,
    },
    Rebase {
        upstream: Option<String>,
        onto: Option<String>,
        interactive: bool,
        abort: bool,
        continue_op: bool,
        skip: bool,
    },
    CherryPick {
        revisions: Vec<String>,
        continue_op: bool,
        abort: bool,
        skip: bool,
    },
    Revert {
        revisions: Vec<String>,
        no_edit: bool,
        continue_op: bool,
        abort: bool,
        skip: bool,
    },

    // ── Network ──
    Fetch {
        remote: Option<RemoteName>,
        refspecs: Vec<String>,
        all: bool,
    },
    Pull {
        remote: Option<RemoteName>,
        branch: Option<String>,
        rebase: bool,
        ff_only: bool,
    },
    Push {
        remote: Option<RemoteName>,
        branch: Option<String>,
        set_upstream: bool, // -u
        force: bool,        // --force
        force_with_lease: bool,
        tags: bool,   // --tags
        delete: bool, // --delete
    },

    // ── Reset ──
    ResetHard {
        rev: Option<String>,
    },
    ResetMixed {
        rev: Option<String>,
    },
    ResetSoft {
        rev: Option<String>,
    },
    ResetMerge {
        rev: Option<String>,
    },
    ResetKeep {
        rev: Option<String>,
    },

    // ── Clean ──
    Clean {
        force: bool,   // -f
        dry_run: bool, // -n
        dirs: bool,    // -d
        ignored: bool, // -x
        paths: Vec<Pathspec>,
    },

    // ── Remote ──
    RemoteAdd {
        name: RemoteName,
        url: String,
    },
    RemoteRemove {
        name: RemoteName,
    },
    RemoteSetUrl {
        name: RemoteName,
        url: String,
        append: bool,
    },

    // ── Config ──
    ConfigGet {
        key: String,
        global: bool,
        local: bool,
    },
    ConfigSet {
        key: String,
        value: String,
        global: bool,
        local: bool,
    },
    ConfigUnset {
        key: String,
        global: bool,
        local: bool,
    },

    // ── In-progress operation control ──
    Abort,    // e.g., merge --abort, rebase --abort
    Continue, // merge --continue, rebase --continue
    Skip,     // rebase --skip, cherry-pick --skip

    // ── Fallback ──
    /// An operation that couldn't be fully parsed into a typed variant.
    /// Preserves the original argv for safe execution.
    ManagedGitArgv {
        argv: Vec<String>,
        risk: RiskSet,
    },

    // ── Raw shell required ──
    /// The command requires shell semantics (pipes, redirects, etc.)
    /// and cannot be represented as a typed operation.
    RawShellRequired {
        argv: Vec<String>,
    },
}

/// Reset mode for `git reset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Merge,
    Keep,
}

impl fmt::Display for ResetMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Soft => write!(f, "--soft"),
            Self::Mixed => write!(f, "--mixed"),
            Self::Hard => write!(f, "--hard"),
            Self::Merge => write!(f, "--merge"),
            Self::Keep => write!(f, "--keep"),
        }
    }
}

impl GitOperation {
    /// Derive the risk classes for this operation.
    pub fn risk_classes(&self) -> RiskSet {
        match self {
            Self::Status { .. }
            | Self::Diff { .. }
            | Self::DiffStaged { .. }
            | Self::Show { .. }
            | Self::Log { .. }
            | Self::Blame { .. }
            | Self::ChangedFiles { .. }
            | Self::BranchList { .. }
            | Self::RemoteList
            | Self::RemoteGetUrl { .. }
            | Self::TagList
            | Self::WorktreeList
            | Self::StashList
            | Self::StashShow { .. }
            | Self::ConfigGet { .. } => RiskSet::read_only(),

            Self::Add { .. } => RiskSet::new(vec![GitRiskClass::IndexMutation]),
            Self::Reset {
                mode: ResetMode::Soft,
                ..
            } => RiskSet::new(vec![GitRiskClass::IndexMutation]),
            Self::Reset {
                mode: ResetMode::Mixed,
                ..
            } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::Reset { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
                GitRiskClass::HistoryIntegration,
            ]),

            Self::Commit { .. } => RiskSet::new(vec![GitRiskClass::HistoryIntegration]),

            Self::StashPush { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::StashApply { .. } | Self::StashPop { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::StashDrop { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),

            Self::Checkout { paths: Some(_), .. } => {
                RiskSet::new(vec![GitRiskClass::WorktreeMutation])
            }
            Self::Checkout { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::WorktreeMutation,
                GitRiskClass::DestructiveWorktree,
            ]),
            Self::Checkout { .. } => RiskSet::new(vec![
                GitRiskClass::WorktreeMutation,
                GitRiskClass::RefMutation,
            ]),

            Self::Switch { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::RefMutation,
                GitRiskClass::WorktreeMutation,
                GitRiskClass::DestructiveWorktree,
            ]),
            Self::Switch { .. } => RiskSet::new(vec![
                GitRiskClass::RefMutation,
                GitRiskClass::WorktreeMutation,
            ]),

            Self::Restore { staged: true, .. } => RiskSet::new(vec![GitRiskClass::IndexMutation]),
            Self::Restore { .. } => RiskSet::new(vec![GitRiskClass::WorktreeMutation]),

            Self::BranchCreate { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::RefMutation,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::BranchCreate { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),
            Self::BranchDelete { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::RefMutation,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::BranchDelete { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),
            Self::BranchRename { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),

            Self::TagCreate { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),
            Self::TagDelete { .. } => RiskSet::new(vec![GitRiskClass::RefMutation]),
            Self::TagForceDelete { .. } => RiskSet::new(vec![
                GitRiskClass::RefMutation,
                GitRiskClass::DestructiveHistory,
            ]),

            Self::Merge { abort: true, .. } => RiskSet::read_only(),
            Self::Merge { .. } => RiskSet::new(vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::Rebase {
                interactive: true, ..
            } => RiskSet::new(vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::Rebase { abort: true, .. }
            | Self::Rebase {
                continue_op: true, ..
            } => RiskSet::read_only(),
            Self::Rebase { .. } => RiskSet::new(vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::CherryPick {
                continue_op: true, ..
            }
            | Self::CherryPick { abort: true, .. } => RiskSet::read_only(),
            Self::CherryPick { .. } => RiskSet::new(vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::Revert {
                continue_op: true, ..
            }
            | Self::Revert { abort: true, .. } => RiskSet::read_only(),
            Self::Revert { .. } => RiskSet::new(vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
            ]),

            Self::Fetch { all: true, .. } | Self::Fetch { .. } => {
                RiskSet::new(vec![GitRiskClass::NetworkRead])
            }
            Self::Pull { .. } => RiskSet::new(vec![
                GitRiskClass::NetworkRead,
                GitRiskClass::HistoryIntegration,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::Push {
                force_with_lease: true,
                ..
            } => RiskSet::new(vec![
                GitRiskClass::NetworkWrite,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::Push { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::NetworkWrite,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::Push { delete: true, .. } => RiskSet::new(vec![
                GitRiskClass::NetworkWrite,
                GitRiskClass::DestructiveHistory,
            ]),
            Self::Push { .. } => RiskSet::new(vec![GitRiskClass::NetworkWrite]),

            Self::ResetHard { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
                GitRiskClass::HistoryIntegration,
                GitRiskClass::DestructiveWorktree,
            ]),
            Self::ResetMixed { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::ResetSoft { .. } => RiskSet::new(vec![GitRiskClass::IndexMutation]),
            Self::ResetMerge { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
            ]),
            Self::ResetKeep { .. } => RiskSet::new(vec![
                GitRiskClass::IndexMutation,
                GitRiskClass::WorktreeMutation,
                GitRiskClass::DestructiveWorktree,
            ]),

            Self::Clean { force: true, .. } => RiskSet::new(vec![
                GitRiskClass::WorktreeMutation,
                GitRiskClass::DestructiveWorktree,
            ]),
            Self::Clean { .. } => RiskSet::new(vec![GitRiskClass::WorktreeMutation]),

            Self::RemoteAdd { .. } | Self::RemoteRemove { .. } | Self::RemoteSetUrl { .. } => {
                RiskSet::new(vec![GitRiskClass::RepositoryConfigMutation])
            }

            Self::ConfigSet { .. } | Self::ConfigUnset { .. } => {
                RiskSet::new(vec![GitRiskClass::RepositoryConfigMutation])
            }

            Self::Abort | Self::Continue | Self::Skip => RiskSet::read_only(),

            Self::ManagedGitArgv { risk, .. } => risk.clone(),
            Self::RawShellRequired { .. } => RiskSet::new(vec![
                GitRiskClass::WorktreeMutation,
                GitRiskClass::HistoryIntegration,
            ]),
        }
    }

    /// Return the subcommand name for display purposes.
    pub fn subcommand_name(&self) -> &'static str {
        match self {
            Self::Status { .. } => "status",
            Self::Diff { .. } | Self::DiffStaged { .. } => "diff",
            Self::Show { .. } => "show",
            Self::Log { .. } => "log",
            Self::Blame { .. } => "blame",
            Self::ChangedFiles { .. } => "diff",
            Self::BranchList { .. } => "branch",
            Self::RemoteList
            | Self::RemoteGetUrl { .. }
            | Self::RemoteAdd { .. }
            | Self::RemoteRemove { .. }
            | Self::RemoteSetUrl { .. } => "remote",
            Self::TagList
            | Self::TagCreate { .. }
            | Self::TagDelete { .. }
            | Self::TagForceDelete { .. } => "tag",
            Self::WorktreeList => "worktree",
            Self::Add { .. } => "add",
            Self::Reset { .. }
            | Self::ResetHard { .. }
            | Self::ResetMixed { .. }
            | Self::ResetSoft { .. }
            | Self::ResetMerge { .. }
            | Self::ResetKeep { .. } => "reset",
            Self::Commit { .. } => "commit",
            Self::StashList
            | Self::StashShow { .. }
            | Self::StashPush { .. }
            | Self::StashApply { .. }
            | Self::StashPop { .. }
            | Self::StashDrop { .. } => "stash",
            Self::Checkout { .. } => "checkout",
            Self::Switch { .. } => "switch",
            Self::Restore { .. } => "restore",
            Self::BranchCreate { .. } | Self::BranchDelete { .. } | Self::BranchRename { .. } => {
                "branch"
            }
            Self::Merge { .. } => "merge",
            Self::Rebase { .. } => "rebase",
            Self::CherryPick { .. } => "cherry-pick",
            Self::Revert { .. } => "revert",
            Self::Fetch { .. } => "fetch",
            Self::Pull { .. } => "pull",
            Self::Push { .. } => "push",
            Self::Clean { .. } => "clean",
            Self::ConfigGet { .. } | Self::ConfigSet { .. } | Self::ConfigUnset { .. } => "config",
            Self::Abort | Self::Continue | Self::Skip => "operation",
            Self::ManagedGitArgv { argv, .. } => argv
                .get(1)
                .map(|s| Box::leak(s.clone().into_boxed_str()) as &'static str)
                .unwrap_or("unknown"),
            Self::RawShellRequired { .. } => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_risk_is_read_only() {
        let op = GitOperation::Status { short: false };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::ReadOnly));
        assert!(!risk.is_destructive());
        assert_eq!(op.subcommand_name(), "status");
    }

    #[test]
    fn add_risk_is_index_mutation() {
        let root = crate::path::RepoRoot::new("/tmp").unwrap();
        let op = GitOperation::Add {
            paths: vec![crate::path::RepoPath::new(&root, "foo.rs").unwrap()],
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(!risk.is_destructive());
        assert_eq!(op.subcommand_name(), "add");
    }

    #[test]
    fn commit_risk_is_history_integration() {
        let op = GitOperation::Commit {
            message: "test".into(),
            amend: false,
            allow_empty: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(!risk.is_destructive());
        assert_eq!(op.subcommand_name(), "commit");
    }

    #[test]
    fn push_force_is_destructive() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: true,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkWrite));
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
        assert!(risk.is_destructive());
        assert!(risk.requires_network());
        assert_eq!(op.subcommand_name(), "push");
    }

    #[test]
    fn push_force_with_lease_is_destructive() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: false,
            force_with_lease: true,
            tags: false,
            delete: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkWrite));
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
        assert!(risk.is_destructive());
    }

    #[test]
    fn push_delete_is_destructive() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: false,
            force_with_lease: false,
            tags: false,
            delete: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkWrite));
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn push_normal_is_not_destructive() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: false,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkWrite));
        assert!(!risk.is_destructive());
        assert!(risk.requires_network());
    }

    #[test]
    fn clean_force_is_destructive() {
        let op = GitOperation::Clean {
            force: true,
            dry_run: false,
            dirs: false,
            ignored: false,
            paths: vec![],
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
        assert!(risk.is_destructive());
        assert_eq!(op.subcommand_name(), "clean");
    }

    #[test]
    fn clean_no_force_is_not_destructive() {
        let op = GitOperation::Clean {
            force: false,
            dry_run: false,
            dirs: false,
            ignored: false,
            paths: vec![],
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn reset_hard_is_destructive() {
        let op = GitOperation::ResetHard { rev: None };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
        assert!(risk.is_destructive());
        assert_eq!(op.subcommand_name(), "reset");
    }

    #[test]
    fn reset_soft_is_index_only() {
        let op = GitOperation::ResetSoft { rev: None };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(!risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn reset_keep_is_destructive() {
        let op = GitOperation::ResetKeep { rev: None };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    #[test]
    fn merge_is_history_integration() {
        let op = GitOperation::Merge {
            revisions: vec!["main".into()],
            no_ff: false,
            strategy: None,
            abort: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert_eq!(op.subcommand_name(), "merge");
    }

    #[test]
    fn merge_abort_is_read_only() {
        let op = GitOperation::Merge {
            revisions: vec![],
            no_ff: false,
            strategy: None,
            abort: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::ReadOnly));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn rebase_interactive_is_destructive() {
        let op = GitOperation::Rebase {
            upstream: Some("main".into()),
            onto: None,
            interactive: true,
            abort: false,
            continue_op: false,
            skip: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn rebase_continue_is_read_only() {
        let op = GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: false,
            continue_op: true,
            skip: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::ReadOnly));
    }

    #[test]
    fn branch_create_force_is_destructive() {
        let op = GitOperation::BranchCreate {
            name: BranchName::new("main").unwrap(),
            start_point: None,
            force: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RefMutation));
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
        assert_eq!(op.subcommand_name(), "branch");
    }

    #[test]
    fn branch_delete_normal_is_ref_mutation() {
        let op = GitOperation::BranchDelete {
            name: BranchName::new("feature").unwrap(),
            force: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RefMutation));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn branch_delete_force_is_destructive() {
        let op = GitOperation::BranchDelete {
            name: BranchName::new("feature").unwrap(),
            force: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn tag_force_delete_is_destructive() {
        let op = GitOperation::TagForceDelete {
            name: "v1.0".into(),
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
        assert_eq!(op.subcommand_name(), "tag");
    }

    #[test]
    fn fetch_is_network_read() {
        let op = GitOperation::Fetch {
            remote: None,
            refspecs: vec![],
            all: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkRead));
        assert!(risk.requires_network());
        assert_eq!(op.subcommand_name(), "fetch");
    }

    #[test]
    fn pull_is_network_and_history() {
        let op = GitOperation::Pull {
            remote: None,
            branch: None,
            rebase: false,
            ff_only: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkRead));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert_eq!(op.subcommand_name(), "pull");
    }

    #[test]
    fn remote_add_is_config_mutation() {
        let op = GitOperation::RemoteAdd {
            name: RemoteName::new("origin").unwrap(),
            url: "https://example.com".into(),
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RepositoryConfigMutation));
        assert!(!risk.is_destructive());
        assert_eq!(op.subcommand_name(), "remote");
    }

    #[test]
    fn config_set_is_config_mutation() {
        let op = GitOperation::ConfigSet {
            key: "user.name".into(),
            value: "test".into(),
            global: false,
            local: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RepositoryConfigMutation));
        assert_eq!(op.subcommand_name(), "config");
    }

    #[test]
    fn managed_git_argv_preserves_risk() {
        let risk = RiskSet::new(vec![
            GitRiskClass::NetworkWrite,
            GitRiskClass::DestructiveHistory,
        ]);
        let op = GitOperation::ManagedGitArgv {
            argv: vec!["git".into(), "push".into(), "--force".into()],
            risk: risk.clone(),
        };
        assert_eq!(op.risk_classes(), risk);
        assert_eq!(op.subcommand_name(), "push");
    }

    #[test]
    fn raw_shell_required_has_mutation_risk() {
        let op = GitOperation::RawShellRequired {
            argv: vec!["git".into(), "push".into()],
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert_eq!(op.subcommand_name(), "unknown");
    }

    #[test]
    fn abort_continue_skip_are_read_only() {
        for op in [
            GitOperation::Abort,
            GitOperation::Continue,
            GitOperation::Skip,
        ] {
            let risk = op.risk_classes();
            assert!(risk.contains(&GitRiskClass::ReadOnly));
            assert!(!risk.is_destructive());
            assert_eq!(op.subcommand_name(), "operation");
        }
    }

    #[test]
    fn checkout_paths_is_worktree_only() {
        let root = crate::path::RepoRoot::new("/tmp").unwrap();
        let op = GitOperation::Checkout {
            target: None,
            paths: Some(vec![crate::path::RepoPath::new(&root, "foo.rs").unwrap()]),
            create: false,
            force: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.contains(&GitRiskClass::RefMutation));
        assert_eq!(op.subcommand_name(), "checkout");
    }

    #[test]
    fn checkout_branch_is_worktree_and_ref() {
        let op = GitOperation::Checkout {
            target: Some("main".into()),
            paths: None,
            create: false,
            force: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::RefMutation));
    }

    #[test]
    fn checkout_force_is_destructive() {
        let op = GitOperation::Checkout {
            target: Some("main".into()),
            paths: None,
            create: false,
            force: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    #[test]
    fn restore_staged_is_index_mutation() {
        let root = crate::path::RepoRoot::new("/tmp").unwrap();
        let op = GitOperation::Restore {
            staged: true,
            paths: vec![crate::path::RepoPath::new(&root, "foo.rs").unwrap()],
            source: None,
            worktree: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(!risk.contains(&GitRiskClass::WorktreeMutation));
        assert_eq!(op.subcommand_name(), "restore");
    }

    #[test]
    fn restore_worktree_is_worktree_mutation() {
        let root = crate::path::RepoRoot::new("/tmp").unwrap();
        let op = GitOperation::Restore {
            staged: false,
            paths: vec![crate::path::RepoPath::new(&root, "foo.rs").unwrap()],
            source: None,
            worktree: true,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.contains(&GitRiskClass::IndexMutation));
    }

    #[test]
    fn switch_normal() {
        let op = GitOperation::Switch {
            branch: BranchName::new("feature").unwrap(),
            create: false,
            force: false,
            detach: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RefMutation));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.is_destructive());
        assert_eq!(op.subcommand_name(), "switch");
    }

    #[test]
    fn switch_force_is_destructive() {
        let op = GitOperation::Switch {
            branch: BranchName::new("feature").unwrap(),
            create: false,
            force: true,
            detach: false,
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    #[test]
    fn stash_push_includes_worktree() {
        let op = GitOperation::StashPush {
            message: None,
            include_untracked: false,
            paths: vec![],
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert_eq!(op.subcommand_name(), "stash");
    }

    #[test]
    fn stash_drop_is_ref_mutation() {
        let op = GitOperation::StashDrop {
            stash: RevisionExpr::new("stash@{0}").unwrap(),
        };
        let risk = op.risk_classes();
        assert!(risk.contains(&GitRiskClass::RefMutation));
        assert!(!risk.is_destructive());
    }
}
