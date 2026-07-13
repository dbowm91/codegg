use crate::error::ParseError;
use crate::operation::{GitOperation, ResetMode};
use crate::path::{Pathspec, RepoPath, RepoRoot};
use crate::ref_name::{BranchName, RemoteName, RevisionExpr};
use crate::risk::{GitRiskClass, RiskSet};

/// Parse a `git` argv slice into a typed [`GitOperation`].
///
/// Input is already tokenized argv — no shell splitting needed.
/// The parser never executes commands.
pub fn parse_git_argv(argv: &[String]) -> Result<GitOperation, ParseError> {
    if argv.is_empty() {
        return Err(ParseError::MalformedArgv {
            reason: "empty argv".into(),
        });
    }

    if argv[0] != "git" {
        return Err(ParseError::MalformedArgv {
            reason: format!("expected 'git' executable, got '{}'", argv[0]),
        });
    }

    let mut ctx = ParseCtx::new(argv);

    // Consume global options that appear before the subcommand.
    ctx.consume_global_options()?;

    let sub = ctx.next_arg().ok_or_else(|| ParseError::MalformedArgv {
        reason: "missing subcommand".into(),
    })?;

    let result = match sub.as_str() {
        "status" => parse_status(&mut ctx),
        "diff" => parse_diff(&mut ctx),
        "show" => parse_show(&mut ctx),
        "log" => parse_log(&mut ctx),
        "blame" => parse_blame(&mut ctx),
        "branch" => parse_branch(&mut ctx),
        "tag" => parse_tag(&mut ctx),
        "remote" => parse_remote(&mut ctx),
        "stash" => parse_stash(&mut ctx),
        "checkout" => parse_checkout(&mut ctx),
        "switch" => parse_switch(&mut ctx),
        "restore" => parse_restore(&mut ctx),
        "commit" => parse_commit(&mut ctx),
        "add" => parse_add(&mut ctx),
        "reset" => parse_reset(&mut ctx),
        "clean" => parse_clean(&mut ctx),
        "merge" => parse_merge(&mut ctx),
        "rebase" => parse_rebase(&mut ctx),
        "cherry-pick" => parse_cherry_pick(&mut ctx),
        "revert" => parse_revert(&mut ctx),
        "fetch" => parse_fetch(&mut ctx),
        "pull" => parse_pull(&mut ctx),
        "push" => parse_push(&mut ctx),
        "config" => parse_config(&mut ctx),
        "worktree" => parse_worktree(&mut ctx),
        _ => managed_fallback(ctx.full_argv),
    }?;

    // If -C was seen, wrap in ManagedGitArgv with OutsideProject risk
    if ctx.has_dash_c {
        return managed_outside_project(ctx.full_argv);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Internal parsing context
// ---------------------------------------------------------------------------

struct ParseCtx<'a> {
    full_argv: &'a [String],
    pos: usize,
    /// Whether -C <path> was seen (marks operation as OutsideProject).
    has_dash_c: bool,
}

impl<'a> ParseCtx<'a> {
    fn new(argv: &'a [String]) -> Self {
        Self {
            full_argv: argv,
            pos: 1, // skip "git"
            has_dash_c: false,
        }
    }

    fn next_arg(&mut self) -> Option<String> {
        if self.pos < self.full_argv.len() {
            let s = self.full_argv[self.pos].clone();
            self.pos += 1;
            Some(s)
        } else {
            None
        }
    }

    fn peek_arg(&self) -> Option<&str> {
        self.full_argv.get(self.pos).map(|s| s.as_str())
    }

    fn has_more(&self) -> bool {
        self.pos < self.full_argv.len()
    }

    /// Consume recognized global options before the subcommand.
    fn consume_global_options(&mut self) -> Result<(), ParseError> {
        while self.has_more() {
            let arg = self.peek_arg().unwrap();
            match arg {
                "-C" => {
                    self.pos += 1;
                    // consume the path argument
                    self.next_arg()
                        .ok_or_else(|| ParseError::MissingRequiredArgument {
                            argument: "-C <path>".into(),
                        })?;
                    self.has_dash_c = true;
                }
                "--git-dir" => {
                    self.pos += 1;
                    self.next_arg()
                        .ok_or_else(|| ParseError::MissingRequiredArgument {
                            argument: "--git-dir <path>".into(),
                        })?;
                }
                "--work-tree" => {
                    self.pos += 1;
                    self.next_arg()
                        .ok_or_else(|| ParseError::MissingRequiredArgument {
                            argument: "--work-tree <path>".into(),
                        })?;
                }
                _ => break,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a ManagedGitArgv with conservative worktree + history risk.
fn managed_fallback(argv: &[String]) -> Result<GitOperation, ParseError> {
    Ok(GitOperation::ManagedGitArgv {
        argv: argv.to_vec(),
        risk: RiskSet::new(vec![
            GitRiskClass::WorktreeMutation,
            GitRiskClass::HistoryIntegration,
        ]),
    })
}

/// Create a ManagedGitArgv with OutsideProject risk (for -C usage).
fn managed_outside_project(argv: &[String]) -> Result<GitOperation, ParseError> {
    Ok(GitOperation::ManagedGitArgv {
        argv: argv.to_vec(),
        risk: RiskSet::new(vec![
            GitRiskClass::WorktreeMutation,
            GitRiskClass::HistoryIntegration,
            GitRiskClass::OutsideProject,
        ]),
    })
}

/// Try to parse a string as a `u32`, returning `None` on failure.
fn parse_u32(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// Parse an optional trailing pathspec string into a Pathspec.
fn to_pathspec(s: &str) -> Result<Pathspec, ParseError> {
    Pathspec::new(s).map_err(|e| ParseError::UnsafePath {
        path: s.to_owned(),
        reason: e.to_string(),
    })
}

/// Parse an optional trailing path string into a RepoPath.
/// Uses a dummy RepoRoot at "/" since we don't resolve the real root in the parser.
fn to_repo_path(s: &str) -> Result<RepoPath, ParseError> {
    let root = RepoRoot::new("/").expect("root is valid");
    RepoPath::new(&root, s).map_err(|e| ParseError::UnsafePath {
        path: s.to_owned(),
        reason: e.to_string(),
    })
}

/// Consume all remaining non-flag arguments as paths after `--`.
fn collect_paths_after_double_dash(ctx: &mut ParseCtx) -> Result<Vec<RepoPath>, ParseError> {
    let mut paths = Vec::new();
    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        paths.push(to_repo_path(&arg)?);
    }
    Ok(paths)
}

/// Consume all remaining non-flag arguments as pathspecs after `--`.
fn collect_pathspecs_after_double_dash(ctx: &mut ParseCtx) -> Result<Vec<Pathspec>, ParseError> {
    let mut specs = Vec::new();
    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        specs.push(to_pathspec(&arg)?);
    }
    Ok(specs)
}

/// Check if `s` is a short flag (starts with `-`, not `--`).
fn is_short_flag(s: &str, flag: char) -> bool {
    // e.g. "-s" or combined like "-df"
    s.len() >= 2 && s.starts_with('-') && !s.starts_with("--") && s.contains(flag)
}

/// Check if `s` is a long flag.
fn is_long_flag(s: &str, flag: &str) -> bool {
    s == flag
}

/// Check if an argument looks like it could be a flag (starts with `-`).
fn is_flag(s: &str) -> bool {
    s.starts_with('-') && s.len() > 1
}

/// Check if argument is `--` (double dash pathspec boundary).
fn is_double_dash(s: &str) -> bool {
    s == "--"
}

// ---------------------------------------------------------------------------
// Subcommand parsers
// ---------------------------------------------------------------------------

fn parse_status(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut short = false;
    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--short") || is_long_flag(&arg, "-s") {
            short = true;
        } else if is_long_flag(&arg, "--porcelain")
            || is_long_flag(&arg, "-b")
            || is_long_flag(&arg, "--long")
            || is_long_flag(&arg, "--branch")
            || is_long_flag(&arg, "--ignored")
            || is_long_flag(&arg, "--untracked-files")
            || is_long_flag(&arg, "-u")
            || is_long_flag(&arg, "--show-stash")
            || is_long_flag(&arg, "--verbose")
            || is_long_flag(&arg, "-v")
        {
            // recognized status flags — ignore for now
        } else if is_flag(&arg) {
            // unknown flag — still treat as status
        } else {
            // path argument — still status
        }
    }
    Ok(GitOperation::Status { short })
}

fn parse_diff(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut staged = false;
    let mut stat = false;
    let mut name_only = false;
    let mut base_ref: Option<RevisionExpr> = None;
    let mut paths: Vec<RepoPath> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--staged") || is_long_flag(&arg, "--cached") {
            staged = true;
        } else if is_long_flag(&arg, "--stat") {
            stat = true;
        } else if is_long_flag(&arg, "--name-only") {
            name_only = true;
        } else if is_double_dash(&arg) {
            paths = collect_paths_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            // Could be a base ref or a path
            if base_ref.is_none() {
                base_ref =
                    Some(
                        RevisionExpr::new(&arg).map_err(|e| ParseError::MalformedArgv {
                            reason: e.to_string(),
                        })?,
                    );
            } else {
                paths.push(to_repo_path(&arg)?);
            }
        }
    }

    if staged {
        Ok(GitOperation::DiffStaged {
            stat,
            name_only,
            paths,
        })
    } else {
        Ok(GitOperation::Diff {
            staged: false,
            stat,
            name_only,
            base_ref,
            paths,
        })
    }
}

fn parse_show(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let rev = ctx
        .next_arg()
        .ok_or_else(|| ParseError::MissingRequiredArgument {
            argument: "revision".into(),
        })?;
    Ok(GitOperation::Show {
        rev: RevisionExpr::new(&rev).map_err(|e| ParseError::MalformedArgv {
            reason: e.to_string(),
        })?,
    })
}

fn parse_log(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut oneline = false;
    let mut max_count: Option<u32> = None;
    let mut paths: Vec<RepoPath> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--oneline") {
            oneline = true;
        } else if is_long_flag(&arg, "-n") || is_long_flag(&arg, "--max-count") {
            let val = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "-n <count>".into(),
                })?;
            max_count = parse_u32(&val);
        } else if is_double_dash(&arg) {
            paths = collect_paths_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            // Could be a pathspec
            paths.push(to_repo_path(&arg)?);
        }
    }

    Ok(GitOperation::Log {
        oneline,
        max_count,
        paths,
    })
}

fn parse_blame(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let path = ctx
        .next_arg()
        .ok_or_else(|| ParseError::MissingRequiredArgument {
            argument: "path".into(),
        })?;
    Ok(GitOperation::Blame {
        path: to_repo_path(&path)?,
    })
}

fn parse_branch(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut all = false;
    let mut remotes = false;
    let mut delete_flag = false;
    let mut force_delete = false;
    let mut rename_old: Option<String> = None;
    let mut rename_new: Option<String> = None;
    let mut create_name: Option<String> = None;
    let mut start_point: Option<String> = None;

    // For detecting -d/-D, -m, -b/-c patterns
    let mut args: Vec<String> = Vec::new();
    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        args.push(arg);
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if is_long_flag(arg, "--all") || is_short_flag(arg, 'a') {
            all = true;
        } else if is_long_flag(arg, "--remotes") || is_short_flag(arg, 'r') {
            remotes = true;
        } else if is_long_flag(arg, "--delete") || is_short_flag(arg, 'd') {
            delete_flag = true;
        } else if is_long_flag(arg, "--force") || is_short_flag(arg, 'D') {
            force_delete = true;
        } else if is_long_flag(arg, "--move") || is_short_flag(arg, 'm') {
            // -m old new
            i += 1;
            if i < args.len() {
                rename_old = Some(args[i].clone());
            }
            i += 1;
            if i < args.len() {
                rename_new = Some(args[i].clone());
            }
        } else if is_long_flag(arg, "--copy") || is_short_flag(arg, 'b') || is_short_flag(arg, 'c')
        {
            // -b/-c name start_point
            i += 1;
            if i < args.len() {
                create_name = Some(args[i].clone());
            }
            i += 1;
            if i < args.len() {
                start_point = Some(args[i].clone());
            }
        } else if !is_flag(arg) {
            if delete_flag && !force_delete {
                // git branch -d <name>
                return Ok(GitOperation::BranchDelete {
                    name: BranchName::new(arg).map_err(|e| ParseError::MalformedArgv {
                        reason: e.to_string(),
                    })?,
                    force: false,
                });
            } else if force_delete {
                // git branch -D <name>
                return Ok(GitOperation::BranchDelete {
                    name: BranchName::new(arg).map_err(|e| ParseError::MalformedArgv {
                        reason: e.to_string(),
                    })?,
                    force: true,
                });
            } else if create_name.is_some() {
                // -b/-c <name> already consumed
            } else if rename_old.is_none() {
                // First positional = branch name to create
                create_name = Some(arg.clone());
            }
        }
        i += 1;
    }

    // Branch rename
    if let (Some(old), Some(new)) = (rename_old, rename_new) {
        return Ok(GitOperation::BranchRename {
            old: BranchName::new(&old).map_err(|e| ParseError::MalformedArgv {
                reason: e.to_string(),
            })?,
            new: BranchName::new(&new).map_err(|e| ParseError::MalformedArgv {
                reason: e.to_string(),
            })?,
            force: false,
        });
    }

    // Branch create
    if let Some(name) = create_name {
        return Ok(GitOperation::BranchCreate {
            name: BranchName::new(&name).map_err(|e| ParseError::MalformedArgv {
                reason: e.to_string(),
            })?,
            start_point,
            force: force_delete, // -D in create context means force
        });
    }

    // Branch list
    Ok(GitOperation::BranchList { remotes, all })
}

fn parse_tag(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut list = false;
    let mut delete = false;
    let mut force_delete = false;
    let mut annotated = false;
    let mut name: Option<String> = None;
    let mut rev: Option<String> = None;
    let mut message: Option<String> = None;

    let mut args: Vec<String> = Vec::new();
    while ctx.has_more() {
        args.push(ctx.next_arg().unwrap());
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if is_long_flag(arg, "--list") || is_short_flag(arg, 'l') {
            list = true;
        } else if is_long_flag(arg, "--delete") || is_short_flag(arg, 'd') {
            delete = true;
        } else if is_long_flag(arg, "--force") || is_short_flag(arg, 'f') {
            force_delete = true;
        } else if is_long_flag(arg, "--annotate") || is_short_flag(arg, 'a') {
            annotated = true;
        } else if is_long_flag(arg, "--message") || is_short_flag(arg, 'm') {
            i += 1;
            if i < args.len() {
                message = Some(args[i].clone());
            }
        } else if !is_flag(arg) {
            if delete || force_delete {
                return Ok(if force_delete {
                    GitOperation::TagForceDelete { name: arg.clone() }
                } else {
                    GitOperation::TagDelete { name: arg.clone() }
                });
            } else if name.is_none() {
                name = Some(arg.clone());
            } else if rev.is_none() {
                rev = Some(arg.clone());
            }
        }
        i += 1;
    }

    if list || (name.is_none() && !delete && !force_delete) {
        return Ok(GitOperation::TagList);
    }

    if let Some(tag_name) = name {
        return Ok(GitOperation::TagCreate {
            name: tag_name,
            rev,
            message,
            annotated,
        });
    }

    Ok(GitOperation::TagList)
}

fn parse_remote(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let sub = match ctx.next_arg() {
        Some(s) => s,
        None => return Ok(GitOperation::RemoteList),
    };

    match sub.as_str() {
        "add" => {
            let name = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "remote name".into(),
                })?;
            let url = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "remote url".into(),
                })?;
            Ok(GitOperation::RemoteAdd {
                name: RemoteName::new(&name).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })?,
                url,
            })
        }
        "remove" | "rm" => {
            let name = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "remote name".into(),
                })?;
            Ok(GitOperation::RemoteRemove {
                name: RemoteName::new(&name).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })?,
            })
        }
        "set-url" => {
            let mut append = false;
            while ctx.has_more() {
                let arg = ctx.next_arg().unwrap();
                if is_long_flag(&arg, "--add") {
                    append = true;
                } else if !is_flag(&arg) {
                    let name = arg;
                    let url =
                        ctx.next_arg()
                            .ok_or_else(|| ParseError::MissingRequiredArgument {
                                argument: "remote url".into(),
                            })?;
                    return Ok(GitOperation::RemoteSetUrl {
                        name: RemoteName::new(&name).map_err(|e| ParseError::MalformedArgv {
                            reason: e.to_string(),
                        })?,
                        url,
                        append,
                    });
                }
            }
            Err(ParseError::MissingRequiredArgument {
                argument: "remote name and url".into(),
            })
        }
        "get-url" => {
            let name = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "remote name".into(),
                })?;
            Ok(GitOperation::RemoteGetUrl {
                remote: RemoteName::new(&name).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })?,
            })
        }
        "prune" | "rename" => managed_fallback(ctx.full_argv),
        _ => managed_fallback(ctx.full_argv),
    }
}

fn parse_stash(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    // Default to stash list if no sub-subcommand
    let sub = match ctx.next_arg() {
        Some(s) => s,
        None => return Ok(GitOperation::StashList),
    };

    match sub.as_str() {
        "list" => Ok(GitOperation::StashList),
        "show" => {
            let stash = if ctx.has_more() {
                let arg = ctx.next_arg().unwrap();
                if is_flag(&arg) {
                    // e.g. -p, --stat etc — ignore for typed model
                    None
                } else {
                    Some(
                        RevisionExpr::new(&arg).map_err(|e| ParseError::MalformedArgv {
                            reason: e.to_string(),
                        })?,
                    )
                }
            } else {
                None
            };
            Ok(GitOperation::StashShow { stash })
        }
        "push" => {
            let mut message: Option<String> = None;
            let mut include_untracked = false;
            let mut paths: Vec<Pathspec> = Vec::new();

            while ctx.has_more() {
                let arg = ctx.next_arg().unwrap();
                if is_long_flag(&arg, "--message") || is_short_flag(&arg, 'm') {
                    message = Some(ctx.next_arg().ok_or_else(|| {
                        ParseError::MissingRequiredArgument {
                            argument: "stash message".into(),
                        }
                    })?);
                } else if is_long_flag(&arg, "--include-untracked") || is_short_flag(&arg, 'u') {
                    include_untracked = true;
                } else if is_double_dash(&arg) {
                    paths = collect_pathspecs_after_double_dash(ctx)?;
                    break;
                } else if !is_flag(&arg) {
                    paths.push(to_pathspec(&arg)?);
                }
            }

            Ok(GitOperation::StashPush {
                message,
                include_untracked,
                paths,
            })
        }
        "apply" => {
            let stash = peek_non_flag(ctx).map(|s| {
                RevisionExpr::new(s).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })
            });
            let mut index = false;
            while ctx.has_more() {
                let arg = ctx.next_arg().unwrap();
                if is_long_flag(&arg, "--index") || is_short_flag(&arg, 'i') {
                    index = true;
                }
            }
            Ok(GitOperation::StashApply {
                stash: transpose_opt(stash)?,
                index,
            })
        }
        "pop" => {
            let stash = peek_non_flag(ctx).map(|s| {
                RevisionExpr::new(s).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })
            });
            let mut index = false;
            while ctx.has_more() {
                let arg = ctx.next_arg().unwrap();
                if is_long_flag(&arg, "--index") || is_short_flag(&arg, 'i') {
                    index = true;
                }
            }
            Ok(GitOperation::StashPop {
                stash: transpose_opt(stash)?,
                index,
            })
        }
        "drop" => {
            let stash = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "stash ref".into(),
                })?;
            Ok(GitOperation::StashDrop {
                stash: RevisionExpr::new(&stash).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })?,
            })
        }
        "branch" | "clear" | "create" | "store" => managed_fallback(ctx.full_argv),
        _ => managed_fallback(ctx.full_argv),
    }
}

/// Peek at the next non-flag argument without consuming it.
fn peek_non_flag<'a>(ctx: &'a mut ParseCtx<'_>) -> Option<&'a str> {
    let mut pos = ctx.pos;
    while pos < ctx.full_argv.len() {
        let s = &ctx.full_argv[pos];
        if !is_flag(s) {
            return Some(s.as_str());
        }
        pos += 1;
    }
    None
}

/// Transpose an Option<Result<T, E>> to Result<Option<T>, E>.
fn transpose_opt<T, E>(opt: Option<Result<T, E>>) -> Result<Option<T>, E> {
    match opt {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

fn parse_checkout(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut create = false;
    let mut force = false;
    let mut target: Option<String> = None;
    let mut paths: Option<Vec<RepoPath>> = None;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--force") || is_short_flag(&arg, 'f') {
            force = true;
        } else if is_long_flag(&arg, "--detach") {
            // detach is a managed fallback concern
        } else if is_long_flag(&arg, "--new-branch") || is_short_flag(&arg, 'b') {
            create = true;
            if let Some(name) = ctx.next_arg() {
                target = Some(name);
            }
        } else if is_double_dash(&arg) {
            paths = Some(collect_paths_after_double_dash(ctx)?);
            break;
        } else if !is_flag(&arg) {
            if target.is_none() && !create {
                target = Some(arg);
            } else {
                // Additional positional after target is a path
                let mut p = vec![to_repo_path(&arg)?];
                // Collect any more paths
                while ctx.has_more() {
                    let next = ctx.next_arg().unwrap();
                    if is_flag(&next) {
                        // Flags after paths are unusual but handle gracefully
                        if is_long_flag(&next, "--force") || is_short_flag(&next, 'f') {
                            force = true;
                        }
                    } else {
                        p.push(to_repo_path(&next)?);
                    }
                }
                paths = Some(p);
                break;
            }
        }
    }

    Ok(GitOperation::Checkout {
        target,
        paths,
        create,
        force,
    })
}

fn parse_switch(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut create = false;
    let mut force = false;
    let mut detach = false;
    let mut branch: Option<String> = None;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--force") || is_short_flag(&arg, 'f') {
            force = true;
        } else if is_long_flag(&arg, "--detach") {
            detach = true;
        } else if is_long_flag(&arg, "--create") || is_short_flag(&arg, 'c') {
            create = true;
            if let Some(name) = ctx.next_arg() {
                branch = Some(name);
            }
        } else if is_long_flag(&arg, "--orphan") || is_short_flag(&arg, 'C') {
            // -C (capital) in switch context is "force create"
            create = true;
            force = true;
            if let Some(name) = ctx.next_arg() {
                branch = Some(name);
            }
        } else if !is_flag(&arg) {
            branch = Some(arg);
        }
    }

    let branch_name = match branch {
        Some(b) => BranchName::new(&b).map_err(|e| ParseError::MalformedArgv {
            reason: e.to_string(),
        })?,
        None => {
            return Err(ParseError::MissingRequiredArgument {
                argument: "branch name".into(),
            })
        }
    };

    Ok(GitOperation::Switch {
        branch: branch_name,
        create,
        force,
        detach,
    })
}

fn parse_restore(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut staged = false;
    let mut source: Option<String> = None;
    let mut worktree = false;
    let mut paths: Vec<RepoPath> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--staged")
            || is_long_flag(&arg, "--source")
            || is_long_flag(&arg, "--worktree")
        {
            if is_long_flag(&arg, "--staged") {
                staged = true;
            } else if is_long_flag(&arg, "--source") {
                source =
                    Some(
                        ctx.next_arg()
                            .ok_or_else(|| ParseError::MissingRequiredArgument {
                                argument: "--source <rev>".into(),
                            })?,
                    );
            } else if is_long_flag(&arg, "--worktree") {
                worktree = true;
            }
        } else if is_double_dash(&arg) {
            paths = collect_paths_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            paths.push(to_repo_path(&arg)?);
        }
    }

    Ok(GitOperation::Restore {
        staged,
        paths,
        source,
        worktree,
    })
}

fn parse_commit(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut message = String::new();
    let mut amend = false;
    let mut allow_empty = false;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--message") || is_short_flag(&arg, 'm') {
            message = ctx
                .next_arg()
                .ok_or_else(|| ParseError::MissingRequiredArgument {
                    argument: "--message <msg>".into(),
                })?;
        } else if is_long_flag(&arg, "--amend") {
            amend = true;
        } else if is_long_flag(&arg, "--allow-empty") {
            allow_empty = true;
        }
    }

    Ok(GitOperation::Commit {
        message,
        amend,
        allow_empty,
    })
}

fn parse_add(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut paths: Vec<RepoPath> = Vec::new();
    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_double_dash(&arg) {
            paths = collect_paths_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            paths.push(to_repo_path(&arg)?);
        }
    }
    Ok(GitOperation::Add { paths })
}

fn parse_reset(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut mode: Option<ResetMode> = None;
    let mut rev: Option<String> = None;
    let mut paths: Vec<RepoPath> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--soft") {
            mode = Some(ResetMode::Soft);
        } else if is_long_flag(&arg, "--mixed") {
            mode = Some(ResetMode::Mixed);
        } else if is_long_flag(&arg, "--hard") {
            mode = Some(ResetMode::Hard);
        } else if is_long_flag(&arg, "--merge") {
            mode = Some(ResetMode::Merge);
        } else if is_long_flag(&arg, "--keep") {
            mode = Some(ResetMode::Keep);
        } else if is_double_dash(&arg) {
            paths = collect_paths_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            if rev.is_none() {
                rev = Some(arg);
            } else {
                paths.push(to_repo_path(&arg)?);
            }
        }
    }

    let reset_mode = mode.unwrap_or(ResetMode::Mixed);

    // Map to the specific typed variants
    match reset_mode {
        ResetMode::Soft => Ok(GitOperation::ResetSoft { rev }),
        ResetMode::Mixed => {
            if paths.is_empty() {
                Ok(GitOperation::ResetMixed { rev })
            } else {
                Ok(GitOperation::Reset {
                    mode: ResetMode::Mixed,
                    paths: Some(paths),
                    rev: rev
                        .map(|r| {
                            RevisionExpr::new(&r).map_err(|e| ParseError::MalformedArgv {
                                reason: e.to_string(),
                            })
                        })
                        .transpose()?,
                })
            }
        }
        ResetMode::Hard => Ok(GitOperation::ResetHard { rev }),
        ResetMode::Merge => Ok(GitOperation::ResetMerge { rev }),
        ResetMode::Keep => Ok(GitOperation::ResetKeep { rev }),
    }
}

fn parse_clean(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut force = false;
    let mut dry_run = false;
    let mut dirs = false;
    let mut ignored = false;
    let mut paths: Vec<Pathspec> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_short_flag(&arg, 'f') || is_long_flag(&arg, "--force") {
            force = true;
        } else if is_short_flag(&arg, 'n') || is_long_flag(&arg, "--dry-run") {
            dry_run = true;
        } else if is_short_flag(&arg, 'd') || is_long_flag(&arg, "--dirs") {
            dirs = true;
        } else if is_short_flag(&arg, 'x') || is_long_flag(&arg, "--ignored") {
            ignored = true;
        } else if is_double_dash(&arg) {
            paths = collect_pathspecs_after_double_dash(ctx)?;
            break;
        } else if !is_flag(&arg) {
            paths.push(to_pathspec(&arg)?);
        }
    }

    Ok(GitOperation::Clean {
        force,
        dry_run,
        dirs,
        ignored,
        paths,
    })
}

fn parse_merge(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut no_ff = false;
    let mut strategy: Option<String> = None;
    let mut abort = false;
    let mut continue_op = false;
    let mut revisions: Vec<String> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--no-ff") {
            no_ff = true;
        } else if is_long_flag(&arg, "--strategy") || is_long_flag(&arg, "--strategy-option") {
            strategy = Some(
                ctx.next_arg()
                    .ok_or_else(|| ParseError::MissingRequiredArgument {
                        argument: "--strategy <name>".into(),
                    })?,
            );
        } else if is_long_flag(&arg, "--abort") {
            abort = true;
        } else if is_long_flag(&arg, "--continue") {
            continue_op = true;
        } else if is_long_flag(&arg, "--squash")
            || is_long_flag(&arg, "--no-commit")
            || is_long_flag(&arg, "--no-ff")
            || is_long_flag(&arg, "--ff-only")
            || is_long_flag(&arg, "--edit")
            || is_long_flag(&arg, "-e")
            || is_long_flag(&arg, "--no-edit")
            || is_long_flag(&arg, "--signoff")
            || is_long_flag(&arg, "-s")
            || is_long_flag(&arg, "--gpg-sign")
            || is_long_flag(&arg, "-S")
            || is_long_flag(&arg, "--stat")
            || is_long_flag(&arg, "--no-stat")
            || is_long_flag(&arg, "--summary")
            || is_long_flag(&arg, "--no-summary")
            || is_long_flag(&arg, "--progress")
            || is_long_flag(&arg, "--no-progress")
            || is_long_flag(&arg, "--quiet")
            || is_short_flag(&arg, 'q')
            || is_long_flag(&arg, "--verbose")
            || is_short_flag(&arg, 'v')
        {
            // recognized merge flags — consume without special action
        } else if is_long_flag(&arg, "--") {
            // consume remaining as revisions
            while ctx.has_more() {
                let r = ctx.next_arg().unwrap();
                revisions.push(r);
            }
            break;
        } else if !is_flag(&arg) {
            revisions.push(arg);
        }
    }

    if abort {
        return Ok(GitOperation::Abort);
    }
    if continue_op {
        return Ok(GitOperation::Continue);
    }

    Ok(GitOperation::Merge {
        revisions,
        no_ff,
        strategy,
        abort: false,
    })
}

fn parse_rebase(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut interactive = false;
    let mut abort = false;
    let mut continue_op = false;
    let mut skip = false;
    let mut onto: Option<String> = None;
    let mut upstream: Option<String> = None;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--interactive") || is_short_flag(&arg, 'i') {
            interactive = true;
        } else if is_long_flag(&arg, "--abort") {
            abort = true;
        } else if is_long_flag(&arg, "--continue") {
            continue_op = true;
        } else if is_long_flag(&arg, "--skip") {
            skip = true;
        } else if is_long_flag(&arg, "--onto") {
            onto = Some(
                ctx.next_arg()
                    .ok_or_else(|| ParseError::MissingRequiredArgument {
                        argument: "--onto <target>".into(),
                    })?,
            );
        } else if is_long_flag(&arg, "--autosquash")
            || is_long_flag(&arg, "--no-autosquash")
            || is_long_flag(&arg, "--autostash")
            || is_long_flag(&arg, "--no-autostash")
            || is_long_flag(&arg, "--keep-empty")
            || is_long_flag(&arg, "--no-keep-empty")
            || is_long_flag(&arg, "--rebase-merges")
            || is_long_flag(&arg, "--no-rebase-merges")
            || is_long_flag(&arg, "--exec")
            || is_short_flag(&arg, 'x')
            || is_long_flag(&arg, "--edit")
            || is_short_flag(&arg, 'e')
            || is_long_flag(&arg, "--no-edit")
            || is_long_flag(&arg, "--root")
            || is_long_flag(&arg, "--verify")
            || is_short_flag(&arg, 'v')
            || is_long_flag(&arg, "--quiet")
            || is_short_flag(&arg, 'q')
            || is_long_flag(&arg, "--verbose")
        {
            // recognized rebase flags
        } else if is_long_flag(&arg, "--") {
            while ctx.has_more() {
                let r = ctx.next_arg().unwrap();
                if upstream.is_none() {
                    upstream = Some(r);
                }
            }
            break;
        } else if !is_flag(&arg) && upstream.is_none() {
            upstream = Some(arg);
        }
    }

    if abort {
        return Ok(GitOperation::Abort);
    }
    if continue_op {
        return Ok(GitOperation::Continue);
    }
    if skip {
        return Ok(GitOperation::Skip);
    }

    Ok(GitOperation::Rebase {
        upstream,
        onto,
        interactive,
        abort: false,
        continue_op: false,
        skip: false,
    })
}

fn parse_cherry_pick(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut continue_op = false;
    let mut abort = false;
    let mut skip = false;
    let mut revisions: Vec<String> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--continue") {
            continue_op = true;
        } else if is_long_flag(&arg, "--abort") {
            abort = true;
        } else if is_long_flag(&arg, "--skip") {
            skip = true;
        } else if is_long_flag(&arg, "--no-commit")
            || is_long_flag(&arg, "-n")
            || is_long_flag(&arg, "--signoff")
            || is_short_flag(&arg, 's')
            || is_long_flag(&arg, "--edit")
            || is_short_flag(&arg, 'e')
            || is_long_flag(&arg, "--no-edit")
            || is_long_flag(&arg, "-x")
            || is_long_flag(&arg, "--mainline")
            || is_short_flag(&arg, 'm')
            || is_long_flag(&arg, "--strategy")
            || is_long_flag(&arg, "--strategy-option")
            || is_long_flag(&arg, "-X")
            || is_long_flag(&arg, "--gpg-sign")
            || is_short_flag(&arg, 'S')
            || is_long_flag(&arg, "--allow-empty")
            || is_long_flag(&arg, "--keep-redundant-commits")
            || is_long_flag(&arg, "--allow-empty-message")
            || is_long_flag(&arg, "--append-authors")
            || is_long_flag(&arg, "-a")
            || is_long_flag(&arg, "--committer-date-is-author-date")
            || is_long_flag(&arg, "--reset-author-date")
            || is_long_flag(&arg, "-D")
        {
            // recognized cherry-pick flags
        } else if !is_flag(&arg) {
            revisions.push(arg);
        }
    }

    if abort {
        return Ok(GitOperation::Abort);
    }
    if continue_op {
        return Ok(GitOperation::Continue);
    }
    if skip {
        return Ok(GitOperation::Skip);
    }

    Ok(GitOperation::CherryPick {
        revisions,
        continue_op: false,
        abort: false,
        skip: false,
    })
}

fn parse_revert(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut no_edit = false;
    let mut continue_op = false;
    let mut abort = false;
    let mut skip = false;
    let mut revisions: Vec<String> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--no-edit") {
            no_edit = true;
        } else if is_long_flag(&arg, "--continue") {
            continue_op = true;
        } else if is_long_flag(&arg, "--abort") {
            abort = true;
        } else if is_long_flag(&arg, "--skip") {
            skip = true;
        } else if is_long_flag(&arg, "--edit")
            || is_short_flag(&arg, 'e')
            || is_long_flag(&arg, "--no-commit")
            || is_short_flag(&arg, 'n')
            || is_long_flag(&arg, "--signoff")
            || is_short_flag(&arg, 's')
            || is_long_flag(&arg, "--strategy")
            || is_long_flag(&arg, "--strategy-option")
            || is_long_flag(&arg, "-X")
            || is_long_flag(&arg, "--mainline")
            || is_short_flag(&arg, 'm')
            || is_long_flag(&arg, "--gpg-sign")
            || is_short_flag(&arg, 'S')
            || is_long_flag(&arg, "--rerere-autoupdate")
            || is_long_flag(&arg, "--no-rerere-autoupdate")
            || is_long_flag(&arg, "--no-commit")
            || is_long_flag(&arg, "-F")
            || is_long_flag(&arg, "--file")
            || is_short_flag(&arg, 'f')
        {
            // recognized revert flags
        } else if !is_flag(&arg) {
            revisions.push(arg);
        }
    }

    if abort {
        return Ok(GitOperation::Abort);
    }
    if continue_op {
        return Ok(GitOperation::Continue);
    }
    if skip {
        return Ok(GitOperation::Skip);
    }

    Ok(GitOperation::Revert {
        revisions,
        no_edit,
        continue_op: false,
        abort: false,
        skip: false,
    })
}

fn parse_fetch(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut all = false;
    let mut remote: Option<String> = None;
    let mut refspecs: Vec<String> = Vec::new();

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--all") || is_short_flag(&arg, 'a') {
            all = true;
        } else if is_long_flag(&arg, "--tags")
            || is_long_flag(&arg, "--no-tags")
            || is_long_flag(&arg, "--prune")
            || is_long_flag(&arg, "--no-prune")
            || is_long_flag(&arg, "--depth")
            || is_long_flag(&arg, "--shallow-since")
            || is_long_flag(&arg, "--shallow-exclude")
            || is_long_flag(&arg, "--unshallow")
            || is_long_flag(&arg, "--update-head-ok")
            || is_long_flag(&arg, "--recurse-submodules")
            || is_long_flag(&arg, "--no-recurse-submodules")
            || is_long_flag(&arg, "--jobs")
            || is_short_flag(&arg, 'j')
            || is_long_flag(&arg, "--quiet")
            || is_short_flag(&arg, 'q')
            || is_long_flag(&arg, "--verbose")
            || is_short_flag(&arg, 'v')
            || is_long_flag(&arg, "--dry-run")
            || is_long_flag(&arg, "--force")
            || is_short_flag(&arg, 'f')
            || is_long_flag(&arg, "--multiple")
            || is_long_flag(&arg, "--atomic")
            || is_long_flag(&arg, "--set-upstream")
            || is_short_flag(&arg, 'u')
            || is_long_flag(&arg, "--show-forced-updates")
            || is_long_flag(&arg, "--auto-maintenance")
            || is_long_flag(&arg, "--no-auto-maintenance")
            || is_long_flag(&arg, "--auto-gc")
            || is_long_flag(&arg, "--no-auto-gc")
            || is_long_flag(&arg, "--write-fetch-head")
            || is_long_flag(&arg, "--no-write-fetch-head")
            || is_long_flag(&arg, "--ipv4")
            || is_short_flag(&arg, '4')
            || is_long_flag(&arg, "--ipv6")
            || is_short_flag(&arg, '6')
        {
            // recognized fetch flags
        } else if !is_flag(&arg) {
            if remote.is_none() {
                remote = Some(arg);
            } else {
                refspecs.push(arg);
            }
        }
    }

    Ok(GitOperation::Fetch {
        remote: remote
            .map(|r| {
                RemoteName::new(&r).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })
            })
            .transpose()?,
        refspecs,
        all,
    })
}

fn parse_pull(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut rebase = false;
    let mut ff_only = false;
    let mut remote: Option<String> = None;
    let mut branch: Option<String> = None;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--rebase") || is_short_flag(&arg, 'r') {
            rebase = true;
        } else if is_long_flag(&arg, "--ff-only") {
            ff_only = true;
        } else if is_long_flag(&arg, "--no-ff")
            || is_long_flag(&arg, "--squash")
            || is_long_flag(&arg, "--no-squash")
            || is_long_flag(&arg, "--no-commit")
            || is_long_flag(&arg, "--commit")
            || is_long_flag(&arg, "--edit")
            || is_short_flag(&arg, 'e')
            || is_long_flag(&arg, "--no-edit")
            || is_long_flag(&arg, "--no-stat")
            || is_long_flag(&arg, "--stat")
            || is_long_flag(&arg, "--quiet")
            || is_short_flag(&arg, 'q')
            || is_long_flag(&arg, "--verbose")
            || is_short_flag(&arg, 'v')
            || is_long_flag(&arg, "--depth")
            || is_long_flag(&arg, "--unshallow")
            || is_long_flag(&arg, "--tags")
            || is_long_flag(&arg, "--no-tags")
            || is_long_flag(&arg, "--recurse-submodules")
            || is_long_flag(&arg, "--no-recurse-submodules")
            || is_long_flag(&arg, "--all")
            || is_short_flag(&arg, 'a')
            || is_long_flag(&arg, "--dry-run")
            || is_short_flag(&arg, 'n')
            || is_long_flag(&arg, "--force")
            || is_short_flag(&arg, 'f')
            || is_long_flag(&arg, "--set-upstream")
            || is_short_flag(&arg, 'u')
            || is_long_flag(&arg, "--progress")
            || is_long_flag(&arg, "--no-progress")
            || is_long_flag(&arg, "--autostash")
            || is_long_flag(&arg, "--no-autostash")
            || is_long_flag(&arg, "--rebase-merges")
            || is_long_flag(&arg, "--no-rebase-merges")
            || is_long_flag(&arg, "--autosquash")
            || is_long_flag(&arg, "--no-autosquash")
            || is_long_flag(&arg, "--verify")
            || is_long_flag(&arg, "--strategy-option")
            || is_short_flag(&arg, 'X')
            || is_long_flag(&arg, "--gpg-sign")
            || is_short_flag(&arg, 'S')
            || is_long_flag(&arg, "--verify-signatures")
            || is_long_flag(&arg, "--no-verify-signatures")
            || is_long_flag(&arg, "--allow-unrelated-histories")
            || is_long_flag(&arg, "--no-rebase")
            || is_long_flag(&arg, "--autofetch")
            || is_long_flag(&arg, "--no-autofetch")
            || is_long_flag(&arg, "--show-forced-updates")
            || is_long_flag(&arg, "--no-show-forced-updates")
            || is_long_flag(&arg, "--upload-pack")
            || is_short_flag(&arg, 'p')
            || is_long_flag(&arg, "--force-with-lease")
            || is_long_flag(&arg, "--ipv4")
            || is_short_flag(&arg, '4')
            || is_long_flag(&arg, "--ipv6")
            || is_short_flag(&arg, '6')
        {
            // recognized pull flags
        } else if !is_flag(&arg) {
            if remote.is_none() {
                remote = Some(arg);
            } else if branch.is_none() {
                branch = Some(arg);
            }
        }
    }

    Ok(GitOperation::Pull {
        remote: remote
            .map(|r| {
                RemoteName::new(&r).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })
            })
            .transpose()?,
        branch,
        rebase,
        ff_only,
    })
}

fn parse_push(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut set_upstream = false;
    let mut force = false;
    let mut force_with_lease = false;
    let mut tags = false;
    let mut delete = false;
    let mut remote: Option<String> = None;
    let mut branch: Option<String> = None;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--set-upstream") || is_short_flag(&arg, 'u') {
            set_upstream = true;
        } else if is_long_flag(&arg, "--force")
            || is_long_flag(&arg, "--force-with-lease")
            || is_long_flag(&arg, "-f")
            || is_long_flag(&arg, "--delete")
            || is_long_flag(&arg, "--tags")
            || is_long_flag(&arg, "--prune")
            || is_long_flag(&arg, "--no-prune")
            || is_long_flag(&arg, "--follow-tags")
            || is_long_flag(&arg, "--signed")
            || is_long_flag(&arg, "--no-signed")
            || is_long_flag(&arg, "--dry-run")
            || is_short_flag(&arg, 'n')
            || is_long_flag(&arg, "--quiet")
            || is_short_flag(&arg, 'q')
            || is_long_flag(&arg, "--verbose")
            || is_short_flag(&arg, 'v')
            || is_long_flag(&arg, "--receive-pack")
            || is_long_flag(&arg, "-r")
            || is_long_flag(&arg, "--force-if-includes")
            || is_long_flag(&arg, "--no-force-if-includes")
            || is_long_flag(&arg, "--thin")
            || is_long_flag(&arg, "--no-thin")
            || is_long_flag(&arg, "--progress")
            || is_long_flag(&arg, "--no-progress")
            || is_long_flag(&arg, "--recurse-submodules")
            || is_long_flag(&arg, "--no-recurse-submodules")
            || is_long_flag(&arg, "--verify")
            || is_short_flag(&arg, 'v')
            || is_long_flag(&arg, "--ipv4")
            || is_short_flag(&arg, '4')
            || is_long_flag(&arg, "--ipv6")
            || is_short_flag(&arg, '6')
        {
            // Handle the specific ones we care about
            if is_long_flag(&arg, "--force") || is_short_flag(&arg, 'f') {
                force = true;
            }
            if is_long_flag(&arg, "--force-with-lease") {
                force_with_lease = true;
            }
            if is_long_flag(&arg, "--delete") {
                delete = true;
            }
            if is_long_flag(&arg, "--tags") {
                tags = true;
            }
        } else if !is_flag(&arg) {
            if remote.is_none() {
                remote = Some(arg);
            } else if branch.is_none() {
                branch = Some(arg);
            }
        }
    }

    Ok(GitOperation::Push {
        remote: remote
            .map(|r| {
                RemoteName::new(&r).map_err(|e| ParseError::MalformedArgv {
                    reason: e.to_string(),
                })
            })
            .transpose()?,
        branch,
        set_upstream,
        force,
        force_with_lease,
        tags,
        delete,
    })
}

fn parse_config(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let mut global = false;
    let mut local = false;
    let mut key: Option<String> = None;
    let mut value: Option<String> = None;
    let mut unset = false;
    let mut get = false;

    while ctx.has_more() {
        let arg = ctx.next_arg().unwrap();
        if is_long_flag(&arg, "--global") || is_short_flag(&arg, 'g') {
            global = true;
        } else if is_long_flag(&arg, "--local") {
            local = true;
        } else if is_long_flag(&arg, "--unset") {
            unset = true;
        } else if is_long_flag(&arg, "--get") {
            get = true;
        } else if is_long_flag(&arg, "--list")
            || is_short_flag(&arg, 'l')
            || is_long_flag(&arg, "--edit")
            || is_short_flag(&arg, 'e')
            || is_long_flag(&arg, "--show-origin")
            || is_short_flag(&arg, 'o')
            || is_long_flag(&arg, "--show-scope")
            || is_long_flag(&arg, "--fixed-value")
            || is_long_flag(&arg, "--type")
            || is_short_flag(&arg, 't')
            || is_long_flag(&arg, "--int")
            || is_long_flag(&arg, "--bool")
            || is_long_flag(&arg, "--bool-or-int")
            || is_long_flag(&arg, "--path")
            || is_long_flag(&arg, "--expiry-date")
            || is_long_flag(&arg, "--no-type")
            || is_long_flag(&arg, "--null")
            || is_short_flag(&arg, 'z')
            || is_long_flag(&arg, "--name-only")
            || is_short_flag(&arg, 'n')
            || is_long_flag(&arg, "--includes")
            || is_long_flag(&arg, "--no-includes")
            || is_long_flag(&arg, "--default")
        {
            // recognized config flags
        } else if !is_flag(&arg) {
            if key.is_none() {
                key = Some(arg);
            } else if value.is_none() {
                value = Some(arg);
            }
        }
    }

    if unset {
        if let Some(k) = key {
            return Ok(GitOperation::ConfigUnset {
                key: k,
                global,
                local,
            });
        }
    } else if get || value.is_none() {
        if let Some(k) = key {
            return Ok(GitOperation::ConfigGet {
                key: k,
                global,
                local,
            });
        }
    }

    if let (Some(k), Some(v)) = (key, value) {
        return Ok(GitOperation::ConfigSet {
            key: k,
            value: v,
            global,
            local,
        });
    }

    // Fall through for --list or no key
    managed_fallback(ctx.full_argv)
}

fn parse_worktree(ctx: &mut ParseCtx) -> Result<GitOperation, ParseError> {
    let sub = ctx.peek_arg();
    match sub {
        Some("list") => {
            ctx.next_arg();
            Ok(GitOperation::WorktreeList)
        }
        Some("add") | Some("remove") | Some("move") | Some("lock") | Some("unlock") => {
            managed_fallback(ctx.full_argv)
        }
        _ => Ok(GitOperation::WorktreeList),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::GitOperation;
    use crate::risk::GitRiskClass;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    // ── Basic validation ────────────────────────────────────────────

    #[test]
    fn empty_argv_rejected() {
        let result = parse_git_argv(&[]);
        assert!(matches!(result, Err(ParseError::MalformedArgv { .. })));
    }

    #[test]
    fn non_git_executable_rejected() {
        let result = parse_git_argv(&argv(&["hg", "status"]));
        assert!(matches!(result, Err(ParseError::MalformedArgv { .. })));
    }

    #[test]
    fn git_only_executable_rejected() {
        let result = parse_git_argv(&argv(&["git"]));
        assert!(matches!(
            result,
            Err(ParseError::MalformedArgv { reason: _ })
        ));
    }

    // ── Global options ──────────────────────────────────────────────

    #[test]
    fn git_c_path_marks_outside_project() {
        let result = parse_git_argv(&argv(&["git", "-C", "/tmp/other", "status"])).unwrap();
        assert!(
            matches!(result, GitOperation::ManagedGitArgv { risk, .. } if risk.contains(&GitRiskClass::OutsideProject))
        );
    }

    #[test]
    fn git_git_dir_option_accepted() {
        let result = parse_git_argv(&argv(&["git", "--git-dir", "/tmp/.git", "status"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: false });
    }

    #[test]
    fn git_work_tree_option_accepted() {
        let result = parse_git_argv(&argv(&["git", "--work-tree", "/tmp", "status"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: false });
    }

    #[test]
    fn git_c_missing_path_rejected() {
        let result = parse_git_argv(&argv(&["git", "-C"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── status ──────────────────────────────────────────────────────

    #[test]
    fn status_plain() {
        let result = parse_git_argv(&argv(&["git", "status"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: false });
    }

    #[test]
    fn status_short() {
        let result = parse_git_argv(&argv(&["git", "status", "--short"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: true });
    }

    #[test]
    fn status_short_alias_s() {
        let result = parse_git_argv(&argv(&["git", "status", "-s"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: true });
    }

    #[test]
    fn status_porcelain() {
        let result = parse_git_argv(&argv(&["git", "status", "--porcelain"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: false });
    }

    // ── diff ────────────────────────────────────────────────────────

    #[test]
    fn diff_plain() {
        let result = parse_git_argv(&argv(&["git", "diff"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Diff {
                staged: false,
                stat: false,
                name_only: false,
                base_ref: None,
                paths: vec![],
            }
        );
    }

    #[test]
    fn diff_staged() {
        let result = parse_git_argv(&argv(&["git", "diff", "--staged"])).unwrap();
        assert!(matches!(result, GitOperation::DiffStaged { .. }));
    }

    #[test]
    fn diff_cached() {
        let result = parse_git_argv(&argv(&["git", "diff", "--cached"])).unwrap();
        assert!(matches!(result, GitOperation::DiffStaged { .. }));
    }

    #[test]
    fn diff_with_stat() {
        let result = parse_git_argv(&argv(&["git", "diff", "--stat"])).unwrap();
        match result {
            GitOperation::Diff { stat, .. } => assert!(stat),
            _ => panic!("expected Diff"),
        }
    }

    #[test]
    fn diff_with_base_ref() {
        let result = parse_git_argv(&argv(&["git", "diff", "HEAD~3"])).unwrap();
        match result {
            GitOperation::Diff { base_ref, .. } => {
                assert!(base_ref.is_some());
                assert_eq!(base_ref.unwrap().as_str(), "HEAD~3");
            }
            _ => panic!("expected Diff"),
        }
    }

    #[test]
    fn diff_paths_after_double_dash() {
        let result =
            parse_git_argv(&argv(&["git", "diff", "--", "src/main.rs", "README.md"])).unwrap();
        match result {
            GitOperation::Diff { paths, .. } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0].as_str(), "src/main.rs");
                assert_eq!(paths[1].as_str(), "README.md");
            }
            _ => panic!("expected Diff"),
        }
    }

    #[test]
    fn diff_staged_with_stat_and_paths() {
        let result = parse_git_argv(&argv(&[
            "git", "diff", "--staged", "--stat", "--", "foo.rs",
        ]))
        .unwrap();
        match result {
            GitOperation::DiffStaged {
                stat,
                name_only,
                paths,
                ..
            } => {
                assert!(stat);
                assert!(!name_only);
                assert_eq!(paths.len(), 1);
            }
            _ => panic!("expected DiffStaged"),
        }
    }

    // ── show ────────────────────────────────────────────────────────

    #[test]
    fn show_revision() {
        let result = parse_git_argv(&argv(&["git", "show", "HEAD~3"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Show {
                rev: RevisionExpr::new("HEAD~3").unwrap()
            }
        );
    }

    #[test]
    fn show_missing_rev() {
        let result = parse_git_argv(&argv(&["git", "show"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── log ─────────────────────────────────────────────────────────

    #[test]
    fn log_plain() {
        let result = parse_git_argv(&argv(&["git", "log"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Log {
                oneline: false,
                max_count: None,
                paths: vec![],
            }
        );
    }

    #[test]
    fn log_oneline() {
        let result = parse_git_argv(&argv(&["git", "log", "--oneline"])).unwrap();
        match result {
            GitOperation::Log { oneline, .. } => assert!(oneline),
            _ => panic!("expected Log"),
        }
    }

    #[test]
    fn log_max_count() {
        let result = parse_git_argv(&argv(&["git", "log", "-n", "10"])).unwrap();
        match result {
            GitOperation::Log {
                max_count: Some(n), ..
            } => assert_eq!(n, 10),
            _ => panic!("expected Log with max_count"),
        }
    }

    #[test]
    fn log_max_count_long() {
        let result = parse_git_argv(&argv(&["git", "log", "--max-count", "5"])).unwrap();
        match result {
            GitOperation::Log {
                max_count: Some(n), ..
            } => assert_eq!(n, 5),
            _ => panic!("expected Log with max_count"),
        }
    }

    #[test]
    fn log_paths() {
        let result = parse_git_argv(&argv(&["git", "log", "--", "src/"])).unwrap();
        match result {
            GitOperation::Log { paths, .. } => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0].as_str(), "src/");
            }
            _ => panic!("expected Log"),
        }
    }

    // ── blame ───────────────────────────────────────────────────────

    #[test]
    fn blame_path() {
        let result = parse_git_argv(&argv(&["git", "blame", "README.md"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Blame {
                path: to_repo_path("README.md").unwrap()
            }
        );
    }

    #[test]
    fn blame_missing_path() {
        let result = parse_git_argv(&argv(&["git", "blame"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── branch ──────────────────────────────────────────────────────

    #[test]
    fn branch_list() {
        let result = parse_git_argv(&argv(&["git", "branch"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchList {
                remotes: false,
                all: false,
            }
        );
    }

    #[test]
    fn branch_list_all() {
        let result = parse_git_argv(&argv(&["git", "branch", "-a"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchList {
                remotes: false,
                all: true,
            }
        );
    }

    #[test]
    fn branch_list_remotes() {
        let result = parse_git_argv(&argv(&["git", "branch", "-r"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchList {
                remotes: true,
                all: false,
            }
        );
    }

    #[test]
    fn branch_delete() {
        let result = parse_git_argv(&argv(&["git", "branch", "-d", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchDelete {
                name: BranchName::new("feature").unwrap(),
                force: false,
            }
        );
    }

    #[test]
    fn branch_force_delete() {
        let result = parse_git_argv(&argv(&["git", "branch", "-D", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchDelete {
                name: BranchName::new("feature").unwrap(),
                force: true,
            }
        );
    }

    #[test]
    fn branch_create() {
        let result = parse_git_argv(&argv(&["git", "branch", "my-branch"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchCreate {
                name: BranchName::new("my-branch").unwrap(),
                start_point: None,
                force: false,
            }
        );
    }

    #[test]
    fn branch_create_with_start_point() {
        let result = parse_git_argv(&argv(&["git", "branch", "-b", "new-branch", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchCreate {
                name: BranchName::new("new-branch").unwrap(),
                start_point: Some("main".into()),
                force: false,
            }
        );
    }

    #[test]
    fn branch_rename() {
        let result =
            parse_git_argv(&argv(&["git", "branch", "-m", "old-name", "new-name"])).unwrap();
        assert_eq!(
            result,
            GitOperation::BranchRename {
                old: BranchName::new("old-name").unwrap(),
                new: BranchName::new("new-name").unwrap(),
                force: false,
            }
        );
    }

    #[test]
    fn branch_delete_force_risk() {
        let result = parse_git_argv(&argv(&["git", "branch", "-D", "feature"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    // ── tag ─────────────────────────────────────────────────────────

    #[test]
    fn tag_list() {
        let result = parse_git_argv(&argv(&["git", "tag"])).unwrap();
        assert_eq!(result, GitOperation::TagList);
    }

    #[test]
    fn tag_list_l() {
        let result = parse_git_argv(&argv(&["git", "tag", "-l"])).unwrap();
        assert_eq!(result, GitOperation::TagList);
    }

    #[test]
    fn tag_delete() {
        let result = parse_git_argv(&argv(&["git", "tag", "-d", "v1.0"])).unwrap();
        assert_eq!(
            result,
            GitOperation::TagDelete {
                name: "v1.0".into(),
            }
        );
    }

    #[test]
    fn tag_create_annotated() {
        let result = parse_git_argv(&argv(&[
            "git",
            "tag",
            "-a",
            "v2.0",
            "HEAD",
            "-m",
            "Release 2.0",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::TagCreate {
                name: "v2.0".into(),
                rev: Some("HEAD".into()),
                message: Some("Release 2.0".into()),
                annotated: true,
            }
        );
    }

    #[test]
    fn tag_force_delete_risk() {
        let result = parse_git_argv(&argv(&["git", "tag", "-f", "v1.0"])).unwrap();
        match result {
            GitOperation::TagForceDelete { name } => assert_eq!(name, "v1.0"),
            _ => panic!("expected TagForceDelete"),
        }
    }

    // ── remote ──────────────────────────────────────────────────────

    #[test]
    fn remote_list() {
        let result = parse_git_argv(&argv(&["git", "remote"])).unwrap();
        assert_eq!(result, GitOperation::RemoteList);
    }

    #[test]
    fn remote_add() {
        let result = parse_git_argv(&argv(&[
            "git",
            "remote",
            "add",
            "origin",
            "https://example.com",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::RemoteAdd {
                name: RemoteName::new("origin").unwrap(),
                url: "https://example.com".into(),
            }
        );
    }

    #[test]
    fn remote_remove() {
        let result = parse_git_argv(&argv(&["git", "remote", "remove", "origin"])).unwrap();
        assert_eq!(
            result,
            GitOperation::RemoteRemove {
                name: RemoteName::new("origin").unwrap(),
            }
        );
    }

    #[test]
    fn remote_set_url() {
        let result = parse_git_argv(&argv(&[
            "git",
            "remote",
            "set-url",
            "origin",
            "https://new-url.com",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::RemoteSetUrl {
                name: RemoteName::new("origin").unwrap(),
                url: "https://new-url.com".into(),
                append: false,
            }
        );
    }

    #[test]
    fn remote_set_url_add() {
        let result = parse_git_argv(&argv(&[
            "git",
            "remote",
            "set-url",
            "--add",
            "origin",
            "https://extra.com",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::RemoteSetUrl {
                name: RemoteName::new("origin").unwrap(),
                url: "https://extra.com".into(),
                append: true,
            }
        );
    }

    #[test]
    fn remote_get_url() {
        let result = parse_git_argv(&argv(&["git", "remote", "get-url", "origin"])).unwrap();
        assert_eq!(
            result,
            GitOperation::RemoteGetUrl {
                remote: RemoteName::new("origin").unwrap(),
            }
        );
    }

    #[test]
    fn remote_prune_is_managed() {
        let result = parse_git_argv(&argv(&["git", "remote", "prune", "origin"])).unwrap();
        assert!(matches!(result, GitOperation::ManagedGitArgv { .. }));
    }

    // ── stash ───────────────────────────────────────────────────────

    #[test]
    fn stash_list() {
        let result = parse_git_argv(&argv(&["git", "stash", "list"])).unwrap();
        assert_eq!(result, GitOperation::StashList);
    }

    #[test]
    fn stash_no_subcommand() {
        let result = parse_git_argv(&argv(&["git", "stash"])).unwrap();
        assert_eq!(result, GitOperation::StashList);
    }

    #[test]
    fn stash_push_basic() {
        let result = parse_git_argv(&argv(&["git", "stash", "push"])).unwrap();
        assert_eq!(
            result,
            GitOperation::StashPush {
                message: None,
                include_untracked: false,
                paths: vec![],
            }
        );
    }

    #[test]
    fn stash_push_message_and_untracked() {
        let result = parse_git_argv(&argv(&[
            "git",
            "stash",
            "push",
            "-m",
            "work in progress",
            "-u",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::StashPush {
                message: Some("work in progress".into()),
                include_untracked: true,
                paths: vec![],
            }
        );
    }

    #[test]
    fn stash_push_with_paths() {
        let result = parse_git_argv(&argv(&["git", "stash", "push", "--", "src/main.rs"])).unwrap();
        match result {
            GitOperation::StashPush { paths, .. } => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0].as_str(), "src/main.rs");
            }
            _ => panic!("expected StashPush"),
        }
    }

    #[test]
    fn stash_apply() {
        let result = parse_git_argv(&argv(&["git", "stash", "apply"])).unwrap();
        assert_eq!(
            result,
            GitOperation::StashApply {
                stash: None,
                index: false,
            }
        );
    }

    #[test]
    fn stash_apply_with_ref() {
        let result = parse_git_argv(&argv(&["git", "stash", "apply", "stash@{1}"])).unwrap();
        assert_eq!(
            result,
            GitOperation::StashApply {
                stash: Some(RevisionExpr::new("stash@{1}").unwrap()),
                index: false,
            }
        );
    }

    #[test]
    fn stash_pop() {
        let result = parse_git_argv(&argv(&["git", "stash", "pop"])).unwrap();
        assert_eq!(
            result,
            GitOperation::StashPop {
                stash: None,
                index: false,
            }
        );
    }

    #[test]
    fn stash_drop() {
        let result = parse_git_argv(&argv(&["git", "stash", "drop", "stash@{0}"])).unwrap();
        assert_eq!(
            result,
            GitOperation::StashDrop {
                stash: RevisionExpr::new("stash@{0}").unwrap(),
            }
        );
    }

    #[test]
    fn stash_drop_missing_ref() {
        let result = parse_git_argv(&argv(&["git", "stash", "drop"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── checkout ────────────────────────────────────────────────────

    #[test]
    fn checkout_branch() {
        let result = parse_git_argv(&argv(&["git", "checkout", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: Some("main".into()),
                paths: None,
                create: false,
                force: false,
            }
        );
    }

    #[test]
    fn checkout_create_branch() {
        let result = parse_git_argv(&argv(&["git", "checkout", "-b", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: Some("feature".into()),
                paths: None,
                create: true,
                force: false,
            }
        );
    }

    #[test]
    fn checkout_force() {
        let result = parse_git_argv(&argv(&["git", "checkout", "--force", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: Some("main".into()),
                paths: None,
                create: false,
                force: true,
            }
        );
    }

    #[test]
    fn checkout_short_force() {
        let result = parse_git_argv(&argv(&["git", "checkout", "-f", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: Some("main".into()),
                paths: None,
                create: false,
                force: true,
            }
        );
    }

    #[test]
    fn checkout_paths() {
        let result = parse_git_argv(&argv(&["git", "checkout", "--", "src/main.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: None,
                paths: Some(vec![to_repo_path("src/main.rs").unwrap()]),
                create: false,
                force: false,
            }
        );
    }

    #[test]
    fn checkout_force_is_destructive() {
        let result = parse_git_argv(&argv(&["git", "checkout", "--force", "main"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    // ── switch ──────────────────────────────────────────────────────

    #[test]
    fn switch_branch() {
        let result = parse_git_argv(&argv(&["git", "switch", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("feature").unwrap(),
                create: false,
                force: false,
                detach: false,
            }
        );
    }

    #[test]
    fn switch_create() {
        let result = parse_git_argv(&argv(&["git", "switch", "-c", "new-branch"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("new-branch").unwrap(),
                create: true,
                force: false,
                detach: false,
            }
        );
    }

    #[test]
    fn switch_force_create() {
        let result = parse_git_argv(&argv(&["git", "switch", "-C", "new-branch"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("new-branch").unwrap(),
                create: true,
                force: true,
                detach: false,
            }
        );
    }

    #[test]
    fn switch_detach() {
        let result = parse_git_argv(&argv(&["git", "switch", "--detach", "HEAD"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("HEAD").unwrap(),
                create: false,
                force: false,
                detach: true,
            }
        );
    }

    #[test]
    fn switch_force() {
        let result = parse_git_argv(&argv(&["git", "switch", "--force", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("main").unwrap(),
                create: false,
                force: true,
                detach: false,
            }
        );
    }

    #[test]
    fn switch_missing_branch() {
        let result = parse_git_argv(&argv(&["git", "switch"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── restore ─────────────────────────────────────────────────────

    #[test]
    fn restore_paths() {
        let result = parse_git_argv(&argv(&["git", "restore", "--", "foo.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Restore {
                staged: false,
                paths: vec![to_repo_path("foo.rs").unwrap()],
                source: None,
                worktree: false,
            }
        );
    }

    #[test]
    fn restore_staged() {
        let result =
            parse_git_argv(&argv(&["git", "restore", "--staged", "--", "foo.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Restore {
                staged: true,
                paths: vec![to_repo_path("foo.rs").unwrap()],
                source: None,
                worktree: false,
            }
        );
    }

    #[test]
    fn restore_with_source() {
        let result = parse_git_argv(&argv(&[
            "git", "restore", "--source", "HEAD~1", "--", "foo.rs",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::Restore {
                staged: false,
                paths: vec![to_repo_path("foo.rs").unwrap()],
                source: Some("HEAD~1".into()),
                worktree: false,
            }
        );
    }

    #[test]
    fn restore_worktree() {
        let result =
            parse_git_argv(&argv(&["git", "restore", "--worktree", "--", "foo.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Restore {
                staged: false,
                paths: vec![to_repo_path("foo.rs").unwrap()],
                source: None,
                worktree: true,
            }
        );
    }

    // ── commit ──────────────────────────────────────────────────────

    #[test]
    fn commit_with_message() {
        let result = parse_git_argv(&argv(&["git", "commit", "-m", "initial commit"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Commit {
                message: "initial commit".into(),
                amend: false,
                allow_empty: false,
            }
        );
    }

    #[test]
    fn commit_amend() {
        let result = parse_git_argv(&argv(&["git", "commit", "--amend", "-m", "fix"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Commit {
                message: "fix".into(),
                amend: true,
                allow_empty: false,
            }
        );
    }

    #[test]
    fn commit_allow_empty() {
        let result = parse_git_argv(&argv(&[
            "git",
            "commit",
            "--allow-empty",
            "-m",
            "trigger ci",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::Commit {
                message: "trigger ci".into(),
                amend: false,
                allow_empty: true,
            }
        );
    }

    #[test]
    fn commit_missing_message() {
        let result = parse_git_argv(&argv(&["git", "commit", "-m"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── add ─────────────────────────────────────────────────────────

    #[test]
    fn add_single_path() {
        let result = parse_git_argv(&argv(&["git", "add", "foo.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Add {
                paths: vec![to_repo_path("foo.rs").unwrap()],
            }
        );
    }

    #[test]
    fn add_multiple_paths() {
        let result = parse_git_argv(&argv(&["git", "add", "a.rs", "b.rs", "c.rs"])).unwrap();
        match result {
            GitOperation::Add { paths } => assert_eq!(paths.len(), 3),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn add_after_double_dash() {
        let result = parse_git_argv(&argv(&["git", "add", "--", "foo.rs"])).unwrap();
        match result {
            GitOperation::Add { paths } => assert_eq!(paths.len(), 1),
            _ => panic!("expected Add"),
        }
    }

    // ── reset ───────────────────────────────────────────────────────

    #[test]
    fn reset_soft() {
        let result = parse_git_argv(&argv(&["git", "reset", "--soft", "HEAD~1"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ResetSoft {
                rev: Some("HEAD~1".into()),
            }
        );
    }

    #[test]
    fn reset_mixed_default() {
        let result = parse_git_argv(&argv(&["git", "reset", "HEAD~1"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ResetMixed {
                rev: Some("HEAD~1".into()),
            }
        );
    }

    #[test]
    fn reset_hard() {
        let result = parse_git_argv(&argv(&["git", "reset", "--hard", "HEAD~1"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ResetHard {
                rev: Some("HEAD~1".into()),
            }
        );
    }

    #[test]
    fn reset_keep() {
        let result = parse_git_argv(&argv(&["git", "reset", "--keep"])).unwrap();
        assert_eq!(result, GitOperation::ResetKeep { rev: None });
    }

    #[test]
    fn reset_merge() {
        let result = parse_git_argv(&argv(&["git", "reset", "--merge"])).unwrap();
        assert_eq!(result, GitOperation::ResetMerge { rev: None });
    }

    #[test]
    fn reset_paths() {
        let result =
            parse_git_argv(&argv(&["git", "reset", "--mixed", "HEAD", "--", "foo.rs"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Reset {
                mode: ResetMode::Mixed,
                paths: Some(vec![to_repo_path("foo.rs").unwrap()]),
                rev: Some(RevisionExpr::new("HEAD").unwrap()),
            }
        );
    }

    #[test]
    fn reset_hard_risk() {
        let result = parse_git_argv(&argv(&["git", "reset", "--hard"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    #[test]
    fn reset_keep_risk() {
        let result = parse_git_argv(&argv(&["git", "reset", "--keep"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    // ── clean ───────────────────────────────────────────────────────

    #[test]
    fn clean_force() {
        let result = parse_git_argv(&argv(&["git", "clean", "-f"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Clean {
                force: true,
                dry_run: false,
                dirs: false,
                ignored: false,
                paths: vec![],
            }
        );
    }

    #[test]
    fn clean_dry_run() {
        let result = parse_git_argv(&argv(&["git", "clean", "-n"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Clean {
                force: false,
                dry_run: true,
                dirs: false,
                ignored: false,
                paths: vec![],
            }
        );
    }

    #[test]
    fn clean_all_flags() {
        let result = parse_git_argv(&argv(&["git", "clean", "-f", "-d", "-x", "-n"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Clean {
                force: true,
                dry_run: true,
                dirs: true,
                ignored: true,
                paths: vec![],
            }
        );
    }

    #[test]
    fn clean_with_paths() {
        let result = parse_git_argv(&argv(&["git", "clean", "-f", "--", "build/"])).unwrap();
        match result {
            GitOperation::Clean { paths, .. } => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0].as_str(), "build/");
            }
            _ => panic!("expected Clean"),
        }
    }

    #[test]
    fn clean_force_is_destructive() {
        let result = parse_git_argv(&argv(&["git", "clean", "-f"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    // ── merge ───────────────────────────────────────────────────────

    #[test]
    fn merge_revision() {
        let result = parse_git_argv(&argv(&["git", "merge", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Merge {
                revisions: vec!["feature".into()],
                no_ff: false,
                strategy: None,
                abort: false,
            }
        );
    }

    #[test]
    fn merge_no_ff() {
        let result = parse_git_argv(&argv(&["git", "merge", "--no-ff", "feature"])).unwrap();
        match result {
            GitOperation::Merge { no_ff, .. } => assert!(no_ff),
            _ => panic!("expected Merge"),
        }
    }

    #[test]
    fn merge_strategy() {
        let result =
            parse_git_argv(&argv(&["git", "merge", "--strategy", "ours", "feature"])).unwrap();
        match result {
            GitOperation::Merge { strategy, .. } => {
                assert_eq!(strategy, Some("ours".into()))
            }
            _ => panic!("expected Merge"),
        }
    }

    #[test]
    fn merge_abort() {
        let result = parse_git_argv(&argv(&["git", "merge", "--abort"])).unwrap();
        assert_eq!(result, GitOperation::Abort);
    }

    #[test]
    fn merge_continue() {
        let result = parse_git_argv(&argv(&["git", "merge", "--continue"])).unwrap();
        assert_eq!(result, GitOperation::Continue);
    }

    // ── rebase ──────────────────────────────────────────────────────

    #[test]
    fn rebase_upstream() {
        let result = parse_git_argv(&argv(&["git", "rebase", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Rebase {
                upstream: Some("main".into()),
                onto: None,
                interactive: false,
                abort: false,
                continue_op: false,
                skip: false,
            }
        );
    }

    #[test]
    fn rebase_interactive() {
        let result = parse_git_argv(&argv(&["git", "rebase", "-i", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Rebase {
                upstream: Some("main".into()),
                onto: None,
                interactive: true,
                abort: false,
                continue_op: false,
                skip: false,
            }
        );
    }

    #[test]
    fn rebase_onto() {
        let result =
            parse_git_argv(&argv(&["git", "rebase", "--onto", "main", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Rebase {
                upstream: Some("feature".into()),
                onto: Some("main".into()),
                interactive: false,
                abort: false,
                continue_op: false,
                skip: false,
            }
        );
    }

    #[test]
    fn rebase_abort() {
        let result = parse_git_argv(&argv(&["git", "rebase", "--abort"])).unwrap();
        assert_eq!(result, GitOperation::Abort);
    }

    #[test]
    fn rebase_continue() {
        let result = parse_git_argv(&argv(&["git", "rebase", "--continue"])).unwrap();
        assert_eq!(result, GitOperation::Continue);
    }

    #[test]
    fn rebase_skip() {
        let result = parse_git_argv(&argv(&["git", "rebase", "--skip"])).unwrap();
        assert_eq!(result, GitOperation::Skip);
    }

    // ── cherry-pick ─────────────────────────────────────────────────

    #[test]
    fn cherry_pick_revision() {
        let result = parse_git_argv(&argv(&["git", "cherry-pick", "abc123"])).unwrap();
        assert_eq!(
            result,
            GitOperation::CherryPick {
                revisions: vec!["abc123".into()],
                continue_op: false,
                abort: false,
                skip: false,
            }
        );
    }

    #[test]
    fn cherry_pick_multiple() {
        let result = parse_git_argv(&argv(&["git", "cherry-pick", "abc123", "def456"])).unwrap();
        match result {
            GitOperation::CherryPick { revisions, .. } => {
                assert_eq!(revisions, vec!["abc123", "def456"]);
            }
            _ => panic!("expected CherryPick"),
        }
    }

    #[test]
    fn cherry_pick_continue() {
        let result = parse_git_argv(&argv(&["git", "cherry-pick", "--continue"])).unwrap();
        assert_eq!(result, GitOperation::Continue);
    }

    #[test]
    fn cherry_pick_abort() {
        let result = parse_git_argv(&argv(&["git", "cherry-pick", "--abort"])).unwrap();
        assert_eq!(result, GitOperation::Abort);
    }

    #[test]
    fn cherry_pick_skip() {
        let result = parse_git_argv(&argv(&["git", "cherry-pick", "--skip"])).unwrap();
        assert_eq!(result, GitOperation::Skip);
    }

    // ── revert ──────────────────────────────────────────────────────

    #[test]
    fn revert_revision() {
        let result = parse_git_argv(&argv(&["git", "revert", "abc123"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Revert {
                revisions: vec!["abc123".into()],
                no_edit: false,
                continue_op: false,
                abort: false,
                skip: false,
            }
        );
    }

    #[test]
    fn revert_no_edit() {
        let result = parse_git_argv(&argv(&["git", "revert", "--no-edit", "abc123"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Revert {
                revisions: vec!["abc123".into()],
                no_edit: true,
                continue_op: false,
                abort: false,
                skip: false,
            }
        );
    }

    #[test]
    fn revert_continue() {
        let result = parse_git_argv(&argv(&["git", "revert", "--continue"])).unwrap();
        assert_eq!(result, GitOperation::Continue);
    }

    #[test]
    fn revert_abort() {
        let result = parse_git_argv(&argv(&["git", "revert", "--abort"])).unwrap();
        assert_eq!(result, GitOperation::Abort);
    }

    #[test]
    fn revert_skip() {
        let result = parse_git_argv(&argv(&["git", "revert", "--skip"])).unwrap();
        assert_eq!(result, GitOperation::Skip);
    }

    // ── fetch ───────────────────────────────────────────────────────

    #[test]
    fn fetch_plain() {
        let result = parse_git_argv(&argv(&["git", "fetch"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Fetch {
                remote: None,
                refspecs: vec![],
                all: false,
            }
        );
    }

    #[test]
    fn fetch_all() {
        let result = parse_git_argv(&argv(&["git", "fetch", "--all"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Fetch {
                remote: None,
                refspecs: vec![],
                all: true,
            }
        );
    }

    #[test]
    fn fetch_remote_and_refspec() {
        let result = parse_git_argv(&argv(&[
            "git",
            "fetch",
            "origin",
            "refs/heads/main:refs/remotes/origin/main",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::Fetch {
                remote: Some(RemoteName::new("origin").unwrap()),
                refspecs: vec!["refs/heads/main:refs/remotes/origin/main".into()],
                all: false,
            }
        );
    }

    // ── pull ────────────────────────────────────────────────────────

    #[test]
    fn pull_plain() {
        let result = parse_git_argv(&argv(&["git", "pull"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Pull {
                remote: None,
                branch: None,
                rebase: false,
                ff_only: false,
            }
        );
    }

    #[test]
    fn pull_rebase() {
        let result = parse_git_argv(&argv(&["git", "pull", "--rebase"])).unwrap();
        match result {
            GitOperation::Pull { rebase, .. } => assert!(rebase),
            _ => panic!("expected Pull"),
        }
    }

    #[test]
    fn pull_remote_branch() {
        let result = parse_git_argv(&argv(&["git", "pull", "origin", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Pull {
                remote: Some(RemoteName::new("origin").unwrap()),
                branch: Some("main".into()),
                rebase: false,
                ff_only: false,
            }
        );
    }

    // ── push ────────────────────────────────────────────────────────

    #[test]
    fn push_plain() {
        let result = parse_git_argv(&argv(&["git", "push"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Push {
                remote: None,
                branch: None,
                set_upstream: false,
                force: false,
                force_with_lease: false,
                tags: false,
                delete: false,
            }
        );
    }

    #[test]
    fn push_set_upstream() {
        let result = parse_git_argv(&argv(&["git", "push", "-u", "origin", "main"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Push {
                remote: Some(RemoteName::new("origin").unwrap()),
                branch: Some("main".into()),
                set_upstream: true,
                force: false,
                force_with_lease: false,
                tags: false,
                delete: false,
            }
        );
    }

    #[test]
    fn push_force() {
        let result = parse_git_argv(&argv(&["git", "push", "--force", "origin", "main"])).unwrap();
        match result {
            GitOperation::Push { force, .. } => assert!(force),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn push_force_with_lease() {
        let result = parse_git_argv(&argv(&[
            "git",
            "push",
            "--force-with-lease",
            "origin",
            "main",
        ]))
        .unwrap();
        match result {
            GitOperation::Push {
                force_with_lease, ..
            } => assert!(force_with_lease),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn push_delete() {
        let result =
            parse_git_argv(&argv(&["git", "push", "--delete", "origin", "feature"])).unwrap();
        match result {
            GitOperation::Push { delete, .. } => assert!(delete),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn push_tags() {
        let result = parse_git_argv(&argv(&["git", "push", "--tags", "origin"])).unwrap();
        match result {
            GitOperation::Push { tags, .. } => assert!(tags),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn push_force_is_destructive() {
        let result = parse_git_argv(&argv(&["git", "push", "--force", "origin", "main"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn push_force_with_lease_is_destructive() {
        let result =
            parse_git_argv(&argv(&["git", "push", "--force-with-lease", "origin"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn push_delete_is_destructive() {
        let result =
            parse_git_argv(&argv(&["git", "push", "--delete", "origin", "feature"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    // ── config ──────────────────────────────────────────────────────

    #[test]
    fn config_get() {
        let result = parse_git_argv(&argv(&["git", "config", "user.name"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ConfigGet {
                key: "user.name".into(),
                global: false,
                local: false,
            }
        );
    }

    #[test]
    fn config_set() {
        let result = parse_git_argv(&argv(&["git", "config", "user.name", "Test User"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ConfigSet {
                key: "user.name".into(),
                value: "Test User".into(),
                global: false,
                local: false,
            }
        );
    }

    #[test]
    fn config_global_set() {
        let result =
            parse_git_argv(&argv(&["git", "config", "--global", "user.name", "Test"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ConfigSet {
                key: "user.name".into(),
                value: "Test".into(),
                global: true,
                local: false,
            }
        );
    }

    #[test]
    fn config_unset() {
        let result = parse_git_argv(&argv(&["git", "config", "--unset", "user.name"])).unwrap();
        assert_eq!(
            result,
            GitOperation::ConfigUnset {
                key: "user.name".into(),
                global: false,
                local: false,
            }
        );
    }

    // ── worktree ────────────────────────────────────────────────────

    #[test]
    fn worktree_list() {
        let result = parse_git_argv(&argv(&["git", "worktree", "list"])).unwrap();
        assert_eq!(result, GitOperation::WorktreeList);
    }

    #[test]
    fn worktree_no_subcommand() {
        let result = parse_git_argv(&argv(&["git", "worktree"])).unwrap();
        assert_eq!(result, GitOperation::WorktreeList);
    }

    #[test]
    fn worktree_add_is_managed() {
        let result = parse_git_argv(&argv(&["git", "worktree", "add", "../wt"])).unwrap();
        assert!(matches!(result, GitOperation::ManagedGitArgv { .. }));
    }

    // ── Fallback / unrecognized ─────────────────────────────────────

    #[test]
    fn unrecognized_subcommand_is_managed() {
        let result = parse_git_argv(&argv(&["git", "submodule", "update"])).unwrap();
        assert!(matches!(result, GitOperation::ManagedGitArgv { .. }));
    }

    #[test]
    fn init_is_managed() {
        let result = parse_git_argv(&argv(&["git", "init"])).unwrap();
        assert!(matches!(result, GitOperation::ManagedGitArgv { .. }));
    }

    #[test]
    fn managed_fallback_has_conservative_risk() {
        let result = parse_git_argv(&argv(&["git", "submodule", "init"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
    }

    // ── Risk classification from parser ─────────────────────────────

    #[test]
    fn add_risk() {
        let result = parse_git_argv(&argv(&["git", "add", "."])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn commit_risk() {
        let result = parse_git_argv(&argv(&["git", "commit", "-m", "msg"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn checkout_paths_risk() {
        let result = parse_git_argv(&argv(&["git", "checkout", "--", "foo.rs"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.contains(&GitRiskClass::RefMutation));
    }

    #[test]
    fn checkout_branch_risk() {
        let result = parse_git_argv(&argv(&["git", "checkout", "main"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::RefMutation));
    }

    #[test]
    fn restore_staged_risk() {
        let result = parse_git_argv(&argv(&["git", "restore", "--staged", "--", "f.rs"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(!risk.contains(&GitRiskClass::WorktreeMutation));
    }

    #[test]
    fn restore_worktree_risk() {
        let result =
            parse_git_argv(&argv(&["git", "restore", "--worktree", "--", "f.rs"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(!risk.contains(&GitRiskClass::IndexMutation));
    }

    #[test]
    fn merge_risk() {
        let result = parse_git_argv(&argv(&["git", "merge", "feature"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
    }

    #[test]
    fn rebase_interactive_risk() {
        let result = parse_git_argv(&argv(&["git", "rebase", "-i", "main"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn fetch_risk() {
        let result = parse_git_argv(&argv(&["git", "fetch"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkRead));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn pull_risk() {
        let result = parse_git_argv(&argv(&["git", "pull"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::NetworkRead));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
    }

    #[test]
    fn config_set_risk() {
        let result = parse_git_argv(&argv(&["git", "config", "key", "value"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::RepositoryConfigMutation));
    }

    #[test]
    fn remote_add_risk() {
        let result = parse_git_argv(&argv(&["git", "remote", "add", "origin", "url"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::RepositoryConfigMutation));
    }

    #[test]
    fn abort_risk() {
        let result = parse_git_argv(&argv(&["git", "merge", "--abort"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::ReadOnly));
        assert!(!risk.is_destructive());
    }

    #[test]
    fn reset_hard_risk_full() {
        let result = parse_git_argv(&argv(&["git", "reset", "--hard", "HEAD~1"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::IndexMutation));
        assert!(risk.contains(&GitRiskClass::WorktreeMutation));
        assert!(risk.contains(&GitRiskClass::HistoryIntegration));
        assert!(risk.contains(&GitRiskClass::DestructiveWorktree));
    }

    // ── Complex flag combinations ───────────────────────────────────

    #[test]
    fn branch_delete_force_independent_of_order() {
        let result = parse_git_argv(&argv(&["git", "branch", "-D", "feature"])).unwrap();
        let risk = result.risk_classes();
        assert!(risk.contains(&GitRiskClass::DestructiveHistory));
    }

    #[test]
    fn push_force_independent_of_order() {
        // --force after remote
        let result = parse_git_argv(&argv(&["git", "push", "origin", "main", "--force"])).unwrap();
        match result {
            GitOperation::Push { force, .. } => assert!(force),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn push_force_with_lease_independent_of_order() {
        let result = parse_git_argv(&argv(&[
            "git",
            "push",
            "origin",
            "--force-with-lease",
            "main",
        ]))
        .unwrap();
        match result {
            GitOperation::Push {
                force_with_lease, ..
            } => assert!(force_with_lease),
            _ => panic!("expected Push"),
        }
    }

    #[test]
    fn checkout_multiple_flags_and_target() {
        // -b consumes next arg as branch name; --force comes before -b
        let result =
            parse_git_argv(&argv(&["git", "checkout", "--force", "-b", "feature"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Checkout {
                target: Some("feature".into()),
                paths: None,
                create: true,
                force: true,
            }
        );
    }

    #[test]
    fn switch_create_with_detach() {
        // This is unusual but should parse
        let result = parse_git_argv(&argv(&[
            "git",
            "switch",
            "-c",
            "new-branch",
            "--detach",
            "HEAD",
        ]))
        .unwrap();
        assert_eq!(
            result,
            GitOperation::Switch {
                branch: BranchName::new("HEAD").unwrap(),
                create: true,
                force: false,
                detach: true,
            }
        );
    }

    // ── Pathspec edge cases ─────────────────────────────────────────

    #[test]
    fn add_no_paths() {
        let result = parse_git_argv(&argv(&["git", "add"])).unwrap();
        assert_eq!(result, GitOperation::Add { paths: vec![] });
    }

    #[test]
    fn clean_no_paths() {
        let result = parse_git_argv(&argv(&["git", "clean", "-f"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Clean {
                force: true,
                dry_run: false,
                dirs: false,
                ignored: false,
                paths: vec![],
            }
        );
    }

    // ── Status with various flags ───────────────────────────────────

    #[test]
    fn status_with_path() {
        let result = parse_git_argv(&argv(&["git", "status", "src/"])).unwrap();
        assert_eq!(result, GitOperation::Status { short: false });
    }

    #[test]
    fn diff_name_only() {
        let result = parse_git_argv(&argv(&["git", "diff", "--name-only"])).unwrap();
        match result {
            GitOperation::Diff { name_only, .. } => assert!(name_only),
            _ => panic!("expected Diff"),
        }
    }

    // ── Global option + subcommand combo ────────────────────────────

    #[test]
    fn git_dir_and_work_tree_with_status() {
        let result = parse_git_argv(&argv(&[
            "git",
            "--git-dir",
            "/tmp/.git",
            "--work-tree",
            "/tmp",
            "status",
            "--short",
        ]))
        .unwrap();
        assert_eq!(result, GitOperation::Status { short: true });
    }

    // ── Subcommand name display ─────────────────────────────────────

    #[test]
    fn subcommand_name_variants() {
        assert_eq!(
            parse_git_argv(&argv(&["git", "status"]))
                .unwrap()
                .subcommand_name(),
            "status"
        );
        assert_eq!(
            parse_git_argv(&argv(&["git", "diff"]))
                .unwrap()
                .subcommand_name(),
            "diff"
        );
        assert_eq!(
            parse_git_argv(&argv(&["git", "log"]))
                .unwrap()
                .subcommand_name(),
            "log"
        );
        assert_eq!(
            parse_git_argv(&argv(&["git", "commit", "-m", "x"]))
                .unwrap()
                .subcommand_name(),
            "commit"
        );
        assert_eq!(
            parse_git_argv(&argv(&["git", "push"]))
                .unwrap()
                .subcommand_name(),
            "push"
        );
    }

    // ── Revert with multiple revisions ──────────────────────────────

    #[test]
    fn revert_multiple() {
        let result =
            parse_git_argv(&argv(&["git", "revert", "abc123", "def456", "ghi789"])).unwrap();
        match result {
            GitOperation::Revert { revisions, .. } => {
                assert_eq!(revisions.len(), 3);
            }
            _ => panic!("expected Revert"),
        }
    }

    // ── Merge with multiple revisions ───────────────────────────────

    #[test]
    fn merge_multiple_revisions() {
        let result = parse_git_argv(&argv(&["git", "merge", "branch-a", "branch-b"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Merge {
                revisions: vec!["branch-a".into(), "branch-b".into()],
                no_ff: false,
                strategy: None,
                abort: false,
            }
        );
    }

    // ── Rebase no upstream ──────────────────────────────────────────

    #[test]
    fn rebase_no_upstream() {
        let result = parse_git_argv(&argv(&["git", "rebase"])).unwrap();
        assert_eq!(
            result,
            GitOperation::Rebase {
                upstream: None,
                onto: None,
                interactive: false,
                abort: false,
                continue_op: false,
                skip: false,
            }
        );
    }

    // ── Remote set-url missing args ─────────────────────────────────

    #[test]
    fn remote_set_url_missing_args() {
        let result = parse_git_argv(&argv(&["git", "remote", "set-url"]));
        assert!(matches!(
            result,
            Err(ParseError::MissingRequiredArgument { .. })
        ));
    }

    // ── Unrecognized subcommand preserves argv ──────────────────────

    #[test]
    fn unrecognized_preserves_full_argv() {
        let expected = argv(&["git", "bisect", "start"]);
        let result = parse_git_argv(&expected).unwrap();
        match result {
            GitOperation::ManagedGitArgv { argv: got, .. } => {
                assert_eq!(got, expected);
            }
            _ => panic!("expected ManagedGitArgv"),
        }
    }
}
