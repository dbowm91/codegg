use crate::operation::{GitOperation, ResetMode};

/// Render a [`GitOperation`] back into a `git` argv slice.
///
/// Every variant produces a complete argv beginning with `"git"` followed by the
/// subcommand. Paths are placed after a literal `"--"` separator when required
/// by git's grammar. No shell quoting is performed; the caller receives raw
/// string tokens suitable for `Command::args()`.
pub fn render_argv(op: &GitOperation) -> Vec<String> {
    match op {
        // ── Read-only status/inspection ──
        GitOperation::Status { short } => {
            let mut v = vec!["git".into(), "status".into()];
            if *short {
                v.push("-s".into());
            }
            v
        }

        GitOperation::Diff {
            staged,
            stat,
            name_only,
            base_ref,
            paths,
        } => render_diff(
            *staged,
            *stat,
            *name_only,
            base_ref.as_ref(),
            paths.as_slice(),
        ),

        GitOperation::DiffStaged {
            stat,
            name_only,
            paths,
        } => render_diff(true, *stat, *name_only, None, paths.as_slice()),

        GitOperation::Show { rev } => {
            vec!["git".into(), "show".into(), rev.as_str().into()]
        }

        GitOperation::Log {
            oneline,
            max_count,
            paths,
        } => {
            let mut v = vec!["git".into(), "log".into()];
            if *oneline {
                v.push("--oneline".into());
            }
            if let Some(n) = max_count {
                v.push(format!("-{n}"));
            }
            push_paths_after_dd(&mut v, paths);
            v
        }

        GitOperation::Blame { path } => {
            vec![
                "git".into(),
                "blame".into(),
                "--".into(),
                path.as_str().into(),
            ]
        }

        GitOperation::ChangedFiles { base_ref } => {
            let mut v = vec!["git".into(), "diff".into(), "--name-only".into()];
            if let Some(r) = base_ref {
                v.push(r.as_str().into());
            }
            v
        }

        // ── Branch/remote/tag/worktree listing ──
        GitOperation::BranchList { remotes, all } => {
            let mut v = vec!["git".into(), "branch".into()];
            if *all {
                v.push("-a".into());
            } else if *remotes {
                v.push("-r".into());
            }
            v
        }

        GitOperation::RemoteList => {
            vec!["git".into(), "remote".into()]
        }

        GitOperation::RemoteGetUrl { remote } => {
            vec![
                "git".into(),
                "remote".into(),
                "get-url".into(),
                remote.as_str().into(),
            ]
        }

        GitOperation::TagList => {
            vec!["git".into(), "tag".into()]
        }

        GitOperation::WorktreeList => {
            vec!["git".into(), "worktree".into(), "list".into()]
        }

        // ── Staging ──
        GitOperation::Add { paths } => {
            let mut v = vec!["git".into(), "add".into()];
            for p in paths {
                v.push(p.as_str().into());
            }
            v
        }

        GitOperation::Reset { mode, paths, rev } => {
            let mut v = vec!["git".into(), "reset".into()];
            // Render mode flag only for non-mixed (mixed is the default).
            match mode {
                ResetMode::Mixed => {}
                m => v.push(m.to_string()),
            }
            if let Some(r) = rev {
                v.push(r.as_str().into());
            }
            if let Some(ps) = paths {
                v.push("--".into());
                for p in ps {
                    v.push(p.as_str().into());
                }
            }
            v
        }

        // ── Commit ──
        GitOperation::Commit {
            message,
            amend,
            allow_empty,
        } => {
            let mut v = vec!["git".into(), "commit".into()];
            if *amend {
                v.push("--amend".into());
            }
            if *allow_empty {
                v.push("--allow-empty".into());
            }
            v.push("-m".into());
            v.push(message.clone());
            v
        }

        // ── Stash ──
        GitOperation::StashList => {
            vec!["git".into(), "stash".into(), "list".into()]
        }

        GitOperation::StashShow { stash } => {
            let mut v = vec!["git".into(), "stash".into(), "show".into()];
            if let Some(s) = stash {
                v.push(s.as_str().into());
            }
            v
        }

        GitOperation::StashPush {
            message,
            include_untracked,
            paths,
        } => {
            let mut v = vec!["git".into(), "stash".into(), "push".into()];
            if *include_untracked {
                v.push("-u".into());
            }
            if let Some(m) = message {
                v.push("-m".into());
                v.push(m.clone());
            }
            for p in paths {
                v.push("--".into());
                v.push(p.as_str().into());
            }
            v
        }

        GitOperation::StashApply { stash, index } => {
            let mut v = vec!["git".into(), "stash".into(), "apply".into()];
            if *index {
                v.push("--index".into());
            }
            if let Some(s) = stash {
                v.push(s.as_str().into());
            }
            v
        }

        GitOperation::StashPop { stash, index } => {
            let mut v = vec!["git".into(), "stash".into(), "pop".into()];
            if *index {
                v.push("--index".into());
            }
            if let Some(s) = stash {
                v.push(s.as_str().into());
            }
            v
        }

        GitOperation::StashDrop { stash } => {
            vec![
                "git".into(),
                "stash".into(),
                "drop".into(),
                stash.as_str().into(),
            ]
        }

        // ── Checkout/Switch/Restore ──
        GitOperation::Checkout {
            target,
            paths,
            create,
            force,
        } => {
            let mut v = vec!["git".into(), "checkout".into()];
            if *create {
                v.push("-b".into());
            }
            if *force {
                v.push("-f".into());
            }
            if let Some(t) = target {
                v.push(t.clone());
            }
            if let Some(ps) = paths {
                v.push("--".into());
                for p in ps {
                    v.push(p.as_str().into());
                }
            }
            v
        }

        GitOperation::Switch {
            branch,
            create,
            force,
            detach,
        } => {
            let mut v = vec!["git".into(), "switch".into()];
            if *create {
                v.push("-c".into());
            }
            if *force {
                v.push("-f".into());
            }
            if *detach {
                v.push("--detach".into());
            }
            v.push(branch.as_str().into());
            v
        }

        GitOperation::Restore {
            staged,
            paths,
            source,
            worktree,
        } => {
            let mut v = vec!["git".into(), "restore".into()];
            if *staged {
                v.push("--staged".into());
            }
            if *worktree {
                v.push("--worktree".into());
            }
            if let Some(s) = source {
                v.push("--source".into());
                v.push(s.clone());
            }
            v.push("--".into());
            for p in paths {
                v.push(p.as_str().into());
            }
            v
        }

        // ── Branch/Tag create/delete ──
        GitOperation::BranchCreate {
            name,
            start_point,
            force,
        } => {
            let mut v = vec!["git".into(), "branch".into()];
            if *force {
                v.push("-B".into());
            }
            v.push(name.as_str().into());
            if let Some(sp) = start_point {
                v.push(sp.clone());
            }
            v
        }

        GitOperation::BranchDelete { name, force } => {
            let mut v = vec!["git".into(), "branch".into()];
            if *force {
                v.push("-D".into());
            } else {
                v.push("-d".into());
            }
            v.push(name.as_str().into());
            v
        }

        GitOperation::BranchRename { old, new, force } => {
            let mut v = vec!["git".into(), "branch".into(), "-m".into()];
            if *force {
                v.push("-f".into());
            }
            v.push(old.as_str().into());
            v.push(new.as_str().into());
            v
        }

        GitOperation::TagCreate {
            name,
            rev,
            message,
            annotated,
        } => {
            let mut v = vec!["git".into(), "tag".into()];
            if *annotated || message.is_some() {
                v.push("-a".into());
            }
            v.push(name.clone());
            if let Some(r) = rev {
                v.push(r.clone());
            }
            if let Some(m) = message {
                v.push("-m".into());
                v.push(m.clone());
            }
            v
        }

        GitOperation::TagDelete { name } => {
            vec!["git".into(), "tag".into(), "-d".into(), name.clone()]
        }

        GitOperation::TagForceDelete { name } => {
            vec!["git".into(), "tag".into(), "-D".into(), name.clone()]
        }

        // ── Merge/Rebase/Cherry-pick/Revert ──
        GitOperation::Merge {
            revisions,
            no_ff,
            strategy,
            abort,
        } => {
            let mut v = vec!["git".into(), "merge".into()];
            if *abort {
                v.push("--abort".into());
                return v;
            }
            if *no_ff {
                v.push("--no-ff".into());
            }
            if let Some(s) = strategy {
                v.push(format!("--strategy={s}"));
            }
            for r in revisions {
                v.push(r.clone());
            }
            v
        }

        GitOperation::Rebase {
            upstream,
            onto,
            interactive,
            abort,
            continue_op,
            skip,
        } => {
            let mut v = vec!["git".into(), "rebase".into()];
            if *abort {
                v.push("--abort".into());
                return v;
            }
            if *continue_op {
                v.push("--continue".into());
                return v;
            }
            if *skip {
                v.push("--skip".into());
                return v;
            }
            if *interactive {
                v.push("-i".into());
            }
            if let Some(o) = onto {
                v.push("--onto".into());
                v.push(o.clone());
            }
            if let Some(u) = upstream {
                v.push(u.clone());
            }
            v
        }

        GitOperation::CherryPick {
            revisions,
            continue_op,
            abort,
            skip,
        } => {
            let mut v = vec!["git".into(), "cherry-pick".into()];
            if *abort {
                v.push("--abort".into());
                return v;
            }
            if *continue_op {
                v.push("--continue".into());
                return v;
            }
            if *skip {
                v.push("--skip".into());
                return v;
            }
            for r in revisions {
                v.push(r.clone());
            }
            v
        }

        GitOperation::Revert {
            revisions,
            no_edit,
            continue_op,
            abort,
            skip,
        } => {
            let mut v = vec!["git".into(), "revert".into()];
            if *abort {
                v.push("--abort".into());
                return v;
            }
            if *continue_op {
                v.push("--continue".into());
                return v;
            }
            if *skip {
                v.push("--skip".into());
                return v;
            }
            if *no_edit {
                v.push("--no-edit".into());
            }
            for r in revisions {
                v.push(r.clone());
            }
            v
        }

        // ── Network ──
        GitOperation::Fetch {
            remote,
            refspecs,
            all,
        } => {
            let mut v = vec!["git".into(), "fetch".into()];
            if *all {
                v.push("--all".into());
            }
            if let Some(r) = remote {
                v.push(r.as_str().into());
            }
            for rs in refspecs {
                v.push(rs.clone());
            }
            v
        }

        GitOperation::Pull {
            remote,
            branch,
            rebase,
            ff_only,
        } => {
            let mut v = vec!["git".into(), "pull".into()];
            if *rebase {
                v.push("--rebase".into());
            }
            if *ff_only {
                v.push("--ff-only".into());
            }
            if let Some(r) = remote {
                v.push(r.as_str().into());
            }
            if let Some(b) = branch {
                v.push(b.clone());
            }
            v
        }

        GitOperation::Push {
            remote,
            branch,
            set_upstream,
            force,
            force_with_lease,
            tags,
            delete,
        } => {
            let mut v = vec!["git".into(), "push".into()];
            if *set_upstream {
                v.push("-u".into());
            }
            if *force {
                v.push("--force".into());
            }
            if *force_with_lease {
                v.push("--force-with-lease".into());
            }
            if *tags {
                v.push("--tags".into());
            }
            if *delete {
                v.push("--delete".into());
            }
            if let Some(r) = remote {
                v.push(r.as_str().into());
            }
            if let Some(b) = branch {
                v.push(b.clone());
            }
            v
        }

        // ── Convenience reset shortcuts ──
        GitOperation::ResetHard { rev } => {
            let mut v = vec!["git".into(), "reset".into(), "--hard".into()];
            if let Some(r) = rev {
                v.push(r.clone());
            }
            v
        }

        GitOperation::ResetMixed { rev } => {
            let mut v = vec!["git".into(), "reset".into()];
            if let Some(r) = rev {
                v.push(r.clone());
            }
            v
        }

        GitOperation::ResetSoft { rev } => {
            let mut v = vec!["git".into(), "reset".into(), "--soft".into()];
            if let Some(r) = rev {
                v.push(r.clone());
            }
            v
        }

        GitOperation::ResetMerge { rev } => {
            let mut v = vec!["git".into(), "reset".into(), "--merge".into()];
            if let Some(r) = rev {
                v.push(r.clone());
            }
            v
        }

        GitOperation::ResetKeep { rev } => {
            let mut v = vec!["git".into(), "reset".into(), "--keep".into()];
            if let Some(r) = rev {
                v.push(r.clone());
            }
            v
        }

        // ── Clean ──
        GitOperation::Clean {
            force,
            dry_run,
            dirs,
            ignored,
            paths,
        } => {
            let mut v = vec!["git".into(), "clean".into()];
            // Short options must be combined: -f, -n, -d, -x
            let mut flags = String::new();
            if *force {
                flags.push('f');
            }
            if *dry_run {
                flags.push('n');
            }
            if *dirs {
                flags.push('d');
            }
            if *ignored {
                flags.push('x');
            }
            if !flags.is_empty() {
                v.push(format!("-{flags}"));
            }
            for p in paths {
                v.push("--".into());
                v.push(p.as_str().into());
            }
            v
        }

        // ── Remote ──
        GitOperation::RemoteAdd { name, url } => {
            vec![
                "git".into(),
                "remote".into(),
                "add".into(),
                name.as_str().into(),
                url.expose_secret().to_string(),
            ]
        }

        GitOperation::RemoteRemove { name } => {
            vec![
                "git".into(),
                "remote".into(),
                "remove".into(),
                name.as_str().into(),
            ]
        }

        GitOperation::RemoteSetUrl { name, url, append } => {
            let mut v = vec!["git".into(), "remote".into(), "set-url".into()];
            if *append {
                v.push("--add".into());
            }
            v.push(name.as_str().into());
            v.push(url.expose_secret().to_string());
            v
        }

        // ── Config ──
        GitOperation::ConfigGet { key, global, local } => {
            let mut v = vec!["git".into(), "config".into()];
            if *global {
                v.push("--global".into());
            } else if *local {
                v.push("--local".into());
            }
            v.push(key.clone());
            v
        }

        GitOperation::ConfigSet {
            key,
            value,
            global,
            local,
        } => {
            let mut v = vec!["git".into(), "config".into()];
            if *global {
                v.push("--global".into());
            }
            if *local {
                v.push("--local".into());
            }
            v.push(key.clone());
            v.push(value.clone());
            v
        }

        GitOperation::ConfigUnset { key, global, local } => {
            let mut v = vec!["git".into(), "config".into(), "--unset".into()];
            if *global {
                v.push("--global".into());
            }
            if *local {
                v.push("--local".into());
            }
            v.push(key.clone());
            v
        }

        // ── In-progress operation control ──
        // Abort/Continue/Skip without context render the generic
        // `git operation` argv.  Callers with context should construct
        // the specific subcommand variant (Merge { abort: true, .. }, etc.)
        // and render that instead.
        GitOperation::Abort => {
            vec!["git".into(), "operation".into(), "--abort".into()]
        }
        GitOperation::Continue => {
            vec!["git".into(), "operation".into(), "--continue".into()]
        }
        GitOperation::Skip => {
            vec!["git".into(), "operation".into(), "--skip".into()]
        }

        // ── Fallback passthrough ──
        GitOperation::ManagedGitArgv { argv, .. } => argv.clone(),
        GitOperation::RawShellRequired { argv } => argv.clone(),
    }
}

// ── private helpers ──

fn render_diff(
    staged: bool,
    stat: bool,
    name_only: bool,
    base_ref: Option<&crate::ref_name::RevisionExpr>,
    paths: &[crate::path::RepoPath],
) -> Vec<String> {
    let mut v = vec!["git".into(), "diff".into()];
    if staged {
        v.push("--staged".into());
    }
    if stat {
        v.push("--stat".into());
    }
    if name_only {
        v.push("--name-only".into());
    }
    if let Some(r) = base_ref {
        v.push(r.as_str().into());
    }
    push_paths_after_dd(&mut v, paths);
    v
}

fn push_paths_after_dd(v: &mut Vec<String>, paths: &[crate::path::RepoPath]) {
    if !paths.is_empty() {
        v.push("--".into());
        for p in paths {
            v.push(p.as_str().into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::GitOperation;
    use crate::path::{Pathspec, RepoPath, RepoRoot};
    use crate::ref_name::{BranchName, RemoteName, RevisionExpr};
    use crate::sensitive::RedactedUrl;

    fn root() -> RepoRoot {
        RepoRoot::new("/tmp").unwrap()
    }

    fn rp(name: &str) -> RepoPath {
        RepoPath::new(&root(), name).unwrap()
    }

    fn bn(name: &str) -> BranchName {
        BranchName::new(name).unwrap()
    }

    fn rn(name: &str) -> RemoteName {
        RemoteName::new(name).unwrap()
    }

    fn rev(expr: &str) -> RevisionExpr {
        RevisionExpr::new(expr).unwrap()
    }

    fn ps(spec: &str) -> Pathspec {
        Pathspec::new(spec).unwrap()
    }

    // ── Status ──

    #[test]
    fn status_normal() {
        let op = GitOperation::Status { short: false };
        assert_eq!(render_argv(&op), vec!["git", "status"]);
    }

    #[test]
    fn status_short() {
        let op = GitOperation::Status { short: true };
        assert_eq!(render_argv(&op), vec!["git", "status", "-s"]);
    }

    // ── Diff ──

    #[test]
    fn diff_staged_stat() {
        let op = GitOperation::Diff {
            staged: true,
            stat: true,
            name_only: false,
            base_ref: None,
            paths: vec![],
        };
        assert_eq!(render_argv(&op), vec!["git", "diff", "--staged", "--stat"]);
    }

    #[test]
    fn diff_with_base_ref_and_paths() {
        let op = GitOperation::Diff {
            staged: false,
            stat: false,
            name_only: true,
            base_ref: Some(rev("main")),
            paths: vec![rp("src/main.rs")],
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "diff", "--name-only", "main", "--", "src/main.rs"]
        );
    }

    #[test]
    fn diff_staged_variant() {
        let op = GitOperation::DiffStaged {
            stat: true,
            name_only: false,
            paths: vec![rp("foo.rs")],
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "diff", "--staged", "--stat", "--", "foo.rs"]
        );
    }

    // ── Show ──

    #[test]
    fn show_rev() {
        let op = GitOperation::Show { rev: rev("HEAD~3") };
        assert_eq!(render_argv(&op), vec!["git", "show", "HEAD~3"]);
    }

    // ── Log ──

    #[test]
    fn log_oneline_max_count_with_paths() {
        let op = GitOperation::Log {
            oneline: true,
            max_count: Some(5),
            paths: vec![rp("src")],
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "log", "--oneline", "-5", "--", "src"]
        );
    }

    // ── Blame ──

    #[test]
    fn blame_path() {
        let op = GitOperation::Blame { path: rp("lib.rs") };
        assert_eq!(render_argv(&op), vec!["git", "blame", "--", "lib.rs"]);
    }

    // ── ChangedFiles ──

    #[test]
    fn changed_files_no_ref() {
        let op = GitOperation::ChangedFiles { base_ref: None };
        assert_eq!(render_argv(&op), vec!["git", "diff", "--name-only"]);
    }

    #[test]
    fn changed_files_with_ref() {
        let op = GitOperation::ChangedFiles {
            base_ref: Some(rev("origin/dev")),
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "diff", "--name-only", "origin/dev"]
        );
    }

    // ── BranchList ──

    #[test]
    fn branch_list_default() {
        let op = GitOperation::BranchList {
            remotes: false,
            all: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch"]);
    }

    #[test]
    fn branch_list_all_overrides_remotes() {
        let op = GitOperation::BranchList {
            remotes: true,
            all: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "-a"]);
    }

    #[test]
    fn branch_list_remotes() {
        let op = GitOperation::BranchList {
            remotes: true,
            all: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "-r"]);
    }

    // ── Remote ──

    #[test]
    fn remote_list() {
        let op = GitOperation::RemoteList;
        assert_eq!(render_argv(&op), vec!["git", "remote"]);
    }

    #[test]
    fn remote_get_url() {
        let op = GitOperation::RemoteGetUrl {
            remote: rn("origin"),
        };
        assert_eq!(render_argv(&op), vec!["git", "remote", "get-url", "origin"]);
    }

    #[test]
    fn remote_add() {
        let op = GitOperation::RemoteAdd {
            name: rn("upstream"),
            url: RedactedUrl::new("https://example.com/repo.git"),
        };
        assert_eq!(
            render_argv(&op),
            vec![
                "git",
                "remote",
                "add",
                "upstream",
                "https://example.com/repo.git"
            ]
        );
    }

    #[test]
    fn remote_add_with_credentials_passes_raw_to_argv() {
        // Execution argv must carry the raw URL even after redaction;
        // Debug/Serialize must never expose it.
        let op = GitOperation::RemoteAdd {
            name: rn("upstream"),
            url: RedactedUrl::new("https://user:secret@host.example/r.git"),
        };
        let argv = render_argv(&op);
        assert!(
            argv.iter()
                .any(|tok| tok == "https://user:secret@host.example/r.git"),
            "execution argv lost the raw credential-bearing URL: {argv:?}"
        );
        assert!(
            argv.iter().any(|tok| tok.contains("secret")),
            "redaction should not erase credential before arg construction"
        );
        // Debug output must not show the raw credential.
        let dbg = format!("{:?}", op);
        assert!(!dbg.contains("secret"), "Debug leaked raw secret: {dbg}");
        let json = serde_json::to_string(&op).unwrap();
        assert!(
            !json.contains("secret"),
            "Serialize leaked raw secret: {json}"
        );
    }

    #[test]
    fn remote_remove() {
        let op = GitOperation::RemoteRemove {
            name: rn("upstream"),
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "remote", "remove", "upstream"]
        );
    }

    #[test]
    fn remote_set_url() {
        let op = GitOperation::RemoteSetUrl {
            name: rn("origin"),
            url: RedactedUrl::new("git@new.git"),
            append: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "remote", "set-url", "origin", "git@new.git"]
        );
    }

    #[test]
    fn remote_set_url_with_credentials_passes_raw_to_argv() {
        let op = GitOperation::RemoteSetUrl {
            name: rn("origin"),
            url: RedactedUrl::new("https://user:secret@host.example/r.git"),
            append: false,
        };
        let argv = render_argv(&op);
        // Execution argv keeps the raw URL.
        assert!(
            argv.iter()
                .any(|tok| tok == "https://user:secret@host.example/r.git"),
            "execution argv lost raw URL: {argv:?}"
        );
        // Debug and Serialize stay redacted.
        let dbg = format!("{:?}", op);
        assert!(!dbg.contains("secret"), "Debug leaked raw secret: {dbg}");
        let json = serde_json::to_string(&op).unwrap();
        assert!(
            !json.contains("secret"),
            "Serialize leaked raw secret: {json}"
        );
    }

    #[test]
    fn remote_set_url_append() {
        let op = GitOperation::RemoteSetUrl {
            name: rn("origin"),
            url: RedactedUrl::new("git@mirror.git"),
            append: true,
        };
        assert_eq!(
            render_argv(&op),
            vec![
                "git",
                "remote",
                "set-url",
                "--add",
                "origin",
                "git@mirror.git"
            ]
        );
    }

    // ── TagList ──

    #[test]
    fn tag_list() {
        let op = GitOperation::TagList;
        assert_eq!(render_argv(&op), vec!["git", "tag"]);
    }

    // ── WorktreeList ──

    #[test]
    fn worktree_list() {
        let op = GitOperation::WorktreeList;
        assert_eq!(render_argv(&op), vec!["git", "worktree", "list"]);
    }

    // ── Add ──

    #[test]
    fn add_single_path() {
        let op = GitOperation::Add {
            paths: vec![rp("foo.rs")],
        };
        assert_eq!(render_argv(&op), vec!["git", "add", "foo.rs"]);
    }

    #[test]
    fn add_multiple_paths() {
        let op = GitOperation::Add {
            paths: vec![rp("a.rs"), rp("b.rs")],
        };
        assert_eq!(render_argv(&op), vec!["git", "add", "a.rs", "b.rs"]);
    }

    // ── Reset ──

    #[test]
    fn reset_mixed_default_omits_flag() {
        let op = GitOperation::Reset {
            mode: ResetMode::Mixed,
            paths: None,
            rev: Some(rev("HEAD")),
        };
        assert_eq!(render_argv(&op), vec!["git", "reset", "HEAD"]);
    }

    #[test]
    fn reset_soft_with_rev() {
        let op = GitOperation::Reset {
            mode: ResetMode::Soft,
            rev: Some(rev("HEAD~1")),
            paths: None,
        };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--soft", "HEAD~1"]);
    }

    #[test]
    fn reset_hard_with_paths() {
        let op = GitOperation::Reset {
            mode: ResetMode::Hard,
            rev: None,
            paths: Some(vec![rp("foo.rs"), rp("bar.rs")]),
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "reset", "--hard", "--", "foo.rs", "bar.rs"]
        );
    }

    // ── Commit ──

    #[test]
    fn commit_simple() {
        let op = GitOperation::Commit {
            message: "fix".into(),
            amend: false,
            allow_empty: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "commit", "-m", "fix"]);
    }

    #[test]
    fn commit_amend_allow_empty() {
        let op = GitOperation::Commit {
            message: "update".into(),
            amend: true,
            allow_empty: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "commit", "--amend", "--allow-empty", "-m", "update"]
        );
    }

    // ── Stash ──

    #[test]
    fn stash_list() {
        let op = GitOperation::StashList;
        assert_eq!(render_argv(&op), vec!["git", "stash", "list"]);
    }

    #[test]
    fn stash_push_with_message_and_untracked() {
        let op = GitOperation::StashPush {
            message: Some("wip".into()),
            include_untracked: true,
            paths: vec![ps("src/**")],
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "stash", "push", "-u", "-m", "wip", "--", "src/**"]
        );
    }

    #[test]
    fn stash_push_no_message_no_paths() {
        let op = GitOperation::StashPush {
            message: None,
            include_untracked: false,
            paths: vec![],
        };
        assert_eq!(render_argv(&op), vec!["git", "stash", "push"]);
    }

    #[test]
    fn stash_apply_with_index() {
        let op = GitOperation::StashApply {
            stash: Some(rev("stash@{1}")),
            index: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "stash", "apply", "--index", "stash@{1}"]
        );
    }

    #[test]
    fn stash_pop_default() {
        let op = GitOperation::StashPop {
            stash: None,
            index: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "stash", "pop"]);
    }

    #[test]
    fn stash_drop() {
        let op = GitOperation::StashDrop {
            stash: rev("stash@{0}"),
        };
        assert_eq!(render_argv(&op), vec!["git", "stash", "drop", "stash@{0}"]);
    }

    // ── Checkout ──

    #[test]
    fn checkout_branch_create() {
        let op = GitOperation::Checkout {
            target: Some("feat".into()),
            paths: None,
            create: true,
            force: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "checkout", "-b", "feat"]);
    }

    #[test]
    fn checkout_paths_only() {
        let op = GitOperation::Checkout {
            target: None,
            paths: Some(vec![rp("foo.rs")]),
            create: false,
            force: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "checkout", "--", "foo.rs"]);
    }

    #[test]
    fn checkout_force_branch() {
        let op = GitOperation::Checkout {
            target: Some("main".into()),
            paths: None,
            create: false,
            force: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "checkout", "-f", "main"]);
    }

    // ── Switch ──

    #[test]
    fn switch_simple() {
        let op = GitOperation::Switch {
            branch: bn("feature"),
            create: false,
            force: false,
            detach: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "switch", "feature"]);
    }

    #[test]
    fn switch_create_detach() {
        let op = GitOperation::Switch {
            branch: bn("abc123"),
            create: true,
            force: false,
            detach: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "switch", "-c", "--detach", "abc123"]
        );
    }

    // ── Restore ──

    #[test]
    fn restore_staged() {
        let op = GitOperation::Restore {
            staged: true,
            paths: vec![rp("foo.rs")],
            source: None,
            worktree: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "restore", "--staged", "--", "foo.rs"]
        );
    }

    #[test]
    fn restore_worktree_with_source() {
        let op = GitOperation::Restore {
            staged: false,
            paths: vec![rp("bar.rs")],
            source: Some("HEAD".into()),
            worktree: true,
        };
        assert_eq!(
            render_argv(&op),
            vec![
                "git",
                "restore",
                "--worktree",
                "--source",
                "HEAD",
                "--",
                "bar.rs"
            ]
        );
    }

    // ── BranchCreate ──

    #[test]
    fn branch_create_simple() {
        let op = GitOperation::BranchCreate {
            name: bn("feat"),
            start_point: None,
            force: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "feat"]);
    }

    #[test]
    fn branch_create_force_with_start() {
        let op = GitOperation::BranchCreate {
            name: bn("feat"),
            start_point: Some("main".into()),
            force: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "branch", "-B", "feat", "main"]
        );
    }

    // ── BranchDelete ──

    #[test]
    fn branch_delete_normal() {
        let op = GitOperation::BranchDelete {
            name: bn("feat"),
            force: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "-d", "feat"]);
    }

    #[test]
    fn branch_delete_force() {
        let op = GitOperation::BranchDelete {
            name: bn("feat"),
            force: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "-D", "feat"]);
    }

    // ── BranchRename ──

    #[test]
    fn branch_rename() {
        let op = GitOperation::BranchRename {
            old: bn("old"),
            new: bn("new"),
            force: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "branch", "-m", "old", "new"]);
    }

    #[test]
    fn branch_rename_force() {
        let op = GitOperation::BranchRename {
            old: bn("old"),
            new: bn("new"),
            force: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "branch", "-m", "-f", "old", "new"]
        );
    }

    // ── TagCreate ──

    #[test]
    fn tag_create_annotated() {
        let op = GitOperation::TagCreate {
            name: "v1.0".into(),
            rev: Some("abc123".into()),
            message: Some("Release 1.0".into()),
            annotated: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "tag", "-a", "v1.0", "abc123", "-m", "Release 1.0"]
        );
    }

    #[test]
    fn tag_create_lightweight() {
        let op = GitOperation::TagCreate {
            name: "v2.0".into(),
            rev: None,
            message: None,
            annotated: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "tag", "v2.0"]);
    }

    #[test]
    fn tag_create_message_implies_annotated() {
        let op = GitOperation::TagCreate {
            name: "v3.0".into(),
            rev: None,
            message: Some("msg".into()),
            annotated: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "tag", "-a", "v3.0", "-m", "msg"]
        );
    }

    // ── TagDelete ──

    #[test]
    fn tag_delete() {
        let op = GitOperation::TagDelete {
            name: "v1.0".into(),
        };
        assert_eq!(render_argv(&op), vec!["git", "tag", "-d", "v1.0"]);
    }

    #[test]
    fn tag_force_delete() {
        let op = GitOperation::TagForceDelete {
            name: "v1.0".into(),
        };
        assert_eq!(render_argv(&op), vec!["git", "tag", "-D", "v1.0"]);
    }

    // ── Merge ──

    #[test]
    fn merge_simple() {
        let op = GitOperation::Merge {
            revisions: vec!["main".into()],
            no_ff: false,
            strategy: None,
            abort: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "merge", "main"]);
    }

    #[test]
    fn merge_no_ff_strategy() {
        let op = GitOperation::Merge {
            revisions: vec!["feature".into()],
            no_ff: true,
            strategy: Some("ours".into()),
            abort: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "merge", "--no-ff", "--strategy=ours", "feature"]
        );
    }

    #[test]
    fn merge_abort() {
        let op = GitOperation::Merge {
            revisions: vec![],
            no_ff: false,
            strategy: None,
            abort: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "merge", "--abort"]);
    }

    // ── Rebase ──

    #[test]
    fn rebase_simple() {
        let op = GitOperation::Rebase {
            upstream: Some("main".into()),
            onto: None,
            interactive: false,
            abort: false,
            continue_op: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "rebase", "main"]);
    }

    #[test]
    fn rebase_interactive_onto() {
        let op = GitOperation::Rebase {
            upstream: Some("main".into()),
            onto: Some("origin/dev".into()),
            interactive: true,
            abort: false,
            continue_op: false,
            skip: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "rebase", "-i", "--onto", "origin/dev", "main"]
        );
    }

    #[test]
    fn rebase_abort() {
        let op = GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: true,
            continue_op: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "rebase", "--abort"]);
    }

    #[test]
    fn rebase_continue() {
        let op = GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: false,
            continue_op: true,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "rebase", "--continue"]);
    }

    #[test]
    fn rebase_skip() {
        let op = GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: false,
            continue_op: false,
            skip: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "rebase", "--skip"]);
    }

    // ── CherryPick ──

    #[test]
    fn cherry_pick_simple() {
        let op = GitOperation::CherryPick {
            revisions: vec!["abc123".into()],
            continue_op: false,
            abort: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "cherry-pick", "abc123"]);
    }

    #[test]
    fn cherry_pick_multiple() {
        let op = GitOperation::CherryPick {
            revisions: vec!["a".into(), "b".into(), "c".into()],
            continue_op: false,
            abort: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "cherry-pick", "a", "b", "c"]);
    }

    #[test]
    fn cherry_pick_abort() {
        let op = GitOperation::CherryPick {
            revisions: vec![],
            continue_op: false,
            abort: true,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "cherry-pick", "--abort"]);
    }

    // ── Revert ──

    #[test]
    fn revert_simple() {
        let op = GitOperation::Revert {
            revisions: vec!["def456".into()],
            no_edit: false,
            continue_op: false,
            abort: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "revert", "def456"]);
    }

    #[test]
    fn revert_no_edit() {
        let op = GitOperation::Revert {
            revisions: vec!["x".into()],
            no_edit: true,
            continue_op: false,
            abort: false,
            skip: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "revert", "--no-edit", "x"]);
    }

    // ── Fetch ──

    #[test]
    fn fetch_default() {
        let op = GitOperation::Fetch {
            remote: None,
            refspecs: vec![],
            all: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "fetch"]);
    }

    #[test]
    fn fetch_all() {
        let op = GitOperation::Fetch {
            remote: None,
            refspecs: vec![],
            all: true,
        };
        assert_eq!(render_argv(&op), vec!["git", "fetch", "--all"]);
    }

    #[test]
    fn fetch_remote_refspecs() {
        let op = GitOperation::Fetch {
            remote: Some(rn("origin")),
            refspecs: vec!["refs/heads/main:refs/remotes/origin/main".into()],
            all: false,
        };
        assert_eq!(
            render_argv(&op),
            vec![
                "git",
                "fetch",
                "origin",
                "refs/heads/main:refs/remotes/origin/main"
            ]
        );
    }

    // ── Pull ──

    #[test]
    fn pull_default() {
        let op = GitOperation::Pull {
            remote: None,
            branch: None,
            rebase: false,
            ff_only: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "pull"]);
    }

    #[test]
    fn pull_rebase_ff() {
        let op = GitOperation::Pull {
            remote: Some(rn("origin")),
            branch: Some("main".into()),
            rebase: true,
            ff_only: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "pull", "--rebase", "--ff-only", "origin", "main"]
        );
    }

    // ── Push ──

    #[test]
    fn push_default() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: false,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "push"]);
    }

    #[test]
    fn push_upstream() {
        let op = GitOperation::Push {
            remote: Some(rn("origin")),
            branch: Some("main".into()),
            set_upstream: true,
            force: false,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "push", "-u", "origin", "main"]
        );
    }

    #[test]
    fn push_force_with_lease() {
        let op = GitOperation::Push {
            remote: Some(rn("origin")),
            branch: Some("main".into()),
            set_upstream: false,
            force: false,
            force_with_lease: true,
            tags: false,
            delete: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "push", "--force-with-lease", "origin", "main"]
        );
    }

    #[test]
    fn push_force() {
        let op = GitOperation::Push {
            remote: None,
            branch: None,
            set_upstream: false,
            force: true,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        assert_eq!(render_argv(&op), vec!["git", "push", "--force"]);
    }

    #[test]
    fn push_tags_delete() {
        let op = GitOperation::Push {
            remote: Some(rn("origin")),
            branch: None,
            set_upstream: false,
            force: false,
            force_with_lease: false,
            tags: true,
            delete: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "push", "--tags", "--delete", "origin"]
        );
    }

    // ── Convenience Reset ──

    #[test]
    fn reset_hard_with_rev() {
        let op = GitOperation::ResetHard {
            rev: Some("HEAD~1".into()),
        };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--hard", "HEAD~1"]);
    }

    #[test]
    fn reset_hard_no_rev() {
        let op = GitOperation::ResetHard { rev: None };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--hard"]);
    }

    #[test]
    fn reset_soft_no_rev() {
        let op = GitOperation::ResetSoft { rev: None };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--soft"]);
    }

    #[test]
    fn reset_merge_with_rev() {
        let op = GitOperation::ResetMerge {
            rev: Some("abc".into()),
        };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--merge", "abc"]);
    }

    #[test]
    fn reset_keep_no_rev() {
        let op = GitOperation::ResetKeep { rev: None };
        assert_eq!(render_argv(&op), vec!["git", "reset", "--keep"]);
    }

    // ── Clean ──

    #[test]
    fn clean_force_dirs() {
        let op = GitOperation::Clean {
            force: true,
            dry_run: false,
            dirs: true,
            ignored: false,
            paths: vec![],
        };
        assert_eq!(render_argv(&op), vec!["git", "clean", "-fd"]);
    }

    #[test]
    fn clean_dry_run() {
        let op = GitOperation::Clean {
            force: false,
            dry_run: true,
            dirs: false,
            ignored: false,
            paths: vec![],
        };
        assert_eq!(render_argv(&op), vec!["git", "clean", "-n"]);
    }

    #[test]
    fn clean_all_flags() {
        let op = GitOperation::Clean {
            force: true,
            dry_run: true,
            dirs: true,
            ignored: true,
            paths: vec![ps("target/")],
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "clean", "-fndx", "--", "target/"]
        );
    }

    // ── Config ──

    #[test]
    fn config_get_local() {
        let op = GitOperation::ConfigGet {
            key: "user.name".into(),
            global: false,
            local: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "config", "--local", "user.name"]
        );
    }

    #[test]
    fn config_get_global() {
        let op = GitOperation::ConfigGet {
            key: "user.email".into(),
            global: true,
            local: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "config", "--global", "user.email"]
        );
    }

    #[test]
    fn config_set() {
        let op = GitOperation::ConfigSet {
            key: "user.name".into(),
            value: "Test".into(),
            global: false,
            local: true,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "config", "--local", "user.name", "Test"]
        );
    }

    #[test]
    fn config_unset() {
        let op = GitOperation::ConfigUnset {
            key: "user.name".into(),
            global: false,
            local: false,
        };
        assert_eq!(
            render_argv(&op),
            vec!["git", "config", "--unset", "user.name"]
        );
    }

    // ── Abort/Continue/Skip ──

    #[test]
    fn abort_generic() {
        let op = GitOperation::Abort;
        assert_eq!(render_argv(&op), vec!["git", "operation", "--abort"]);
    }

    #[test]
    fn continue_generic() {
        let op = GitOperation::Continue;
        assert_eq!(render_argv(&op), vec!["git", "operation", "--continue"]);
    }

    #[test]
    fn skip_generic() {
        let op = GitOperation::Skip;
        assert_eq!(render_argv(&op), vec!["git", "operation", "--skip"]);
    }

    // ── Fallback ──

    #[test]
    fn managed_git_argv_passthrough() {
        let op = GitOperation::ManagedGitArgv {
            argv: vec!["git".into(), "init".into()],
            risk: crate::risk::RiskSet::read_only(),
        };
        assert_eq!(render_argv(&op), vec!["git", "init"]);
    }

    #[test]
    fn raw_shell_required_passthrough() {
        let op = GitOperation::RawShellRequired {
            argv: vec!["git".into(), "push".into(), "--force".into()],
        };
        assert_eq!(render_argv(&op), vec!["git", "push", "--force"]);
    }

    // ── Round-trip: parse then render preserves argv shape ──
    // These tests verify the invariant that render(parse(argv)) == argv
    // once the parser supports all subcommands. They are currently #[ignore]
    // because the parser is still a stub.

    #[test]
    #[ignore = "parser stub does not support status yet"]
    fn roundtrip_status_short() {
        let argv = vec!["git".into(), "status".into(), "-s".into()];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support commit yet"]
    fn roundtrip_commit() {
        let argv = vec!["git".into(), "commit".into(), "-m".into(), "fix bug".into()];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support branch yet"]
    fn roundtrip_branch_delete_force() {
        let argv = vec!["git".into(), "branch".into(), "-D".into(), "feat".into()];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support push yet"]
    fn roundtrip_push_upstream() {
        let argv = vec![
            "git".into(),
            "push".into(),
            "-u".into(),
            "origin".into(),
            "main".into(),
        ];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support reset yet"]
    fn roundtrip_reset_hard() {
        let argv = vec![
            "git".into(),
            "reset".into(),
            "--hard".into(),
            "HEAD~1".into(),
        ];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support diff yet"]
    fn roundtrip_diff_staged_stat() {
        let argv = vec![
            "git".into(),
            "diff".into(),
            "--staged".into(),
            "--stat".into(),
        ];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }

    #[test]
    #[ignore = "parser stub does not support clean yet"]
    fn roundtrip_clean_all_flags() {
        let argv = vec!["git".into(), "clean".into(), "-fndx".into()];
        let op = crate::parser::parse_git_argv(&argv).unwrap();
        let rendered = render_argv(&op);
        assert_eq!(rendered, argv);
    }
}
