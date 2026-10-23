//! Destructive shell command classification.
//!
//! **This classification is an advisory risk signal, not a security
//! boundary.** A match annotates a command as risky so the approval path can
//! escalate it; it never grants authority, never guarantees that the effect
//! is impossible, and never rejects an invocation on its own. Command syntax
//! is advisory under ADR-0045; the authority to run a command comes from the
//! daemon-authorized principal, the workspace, the approval mode, the
//! requested ceilings and the obtained executor capabilities.
//!
//! Short, focused list of catastrophic shell operations that should always
//! require user approval, even in permissive modes. Everything not matched
//! here is considered non-destructive and may be auto-allowed.
//!
//! This list is intentionally small (a dozen patterns) so that common
//! workflows like `rm a_file.txt`, `mv`, `cp`, `npm install`, `cargo build`
//! are NOT flagged. The goal is to catch only true catastrophes:
//! - Filesystem wipes (rm -rf /) — owned by [`classify_root_delete`]
//! - Disk/filesystem destruction (mkfs, dd from /dev/zero)
//! - Fork bombs
//! - Permission bombs on system dirs (chmod 777 /)
//! - System shutdown/reboot
//! - Direct writes to raw block devices
//! - Pipe-from-internet-to-shell (curl|sh)

use once_cell::sync::Lazy;
use regex::Regex;

/// Returns the display name of the first matching destructive pattern, or None.
///
/// Pattern entries are `(human_readable_name, regex)`. The first match wins
/// so we can surface the most specific reason to the user.
///
/// The filesystem-root delete is **not** in this list. It is classified by
/// [`classify_root_delete`], which recognises the effective spellings by
/// their target position rather than by a flag-anchored regex.
pub fn destructive_bash_patterns() -> &'static [(&'static str, &'static str)] {
    DESTRUCTIVE_BASH_PATTERNS
}

const DESTRUCTIVE_BASH_PATTERNS: &[(&str, &str)] = &[
    // Disk / filesystem destruction
    ("mkfs on /dev", r"mkfs(\.\w+)?\s+/dev/"),
    (
        "dd from /dev",
        r"\bdd\s+[^|;&]*\bif=/dev/(zero|urandom|random)",
    ),
    // Fork bombs
    (
        "fork bomb (:(){:...|:})",
        r":\s*\(\s*\)\s*\{[^}]*\|[^}]*\}\s*;\s*:",
    ),
    // System shutdown / reboot
    (
        "system shutdown or reboot",
        r"\b(shutdown|reboot|halt|poweroff|telinit\s+0|init\s+0|systemctl\s+(poweroff|reboot|halt))\b",
    ),
    // Direct writes to raw block devices
    (
        "write to /dev/sd or /dev/nvme",
        r">\s*/dev/(sd|nvme|hd|vd)[a-z]",
    ),
    // Partition tools (mutate disk layout)
    (
        "partition tool (fdisk/parted/mklabel)",
        r"\b(fdisk|parted|sfdisk|sgdisk|mklabel)\b",
    ),
    // Pipe from internet to shell
    (
        "curl ... | sh or wget ... | sh",
        r"\b(curl|wget)\b[^|]*\|\s*(sh|bash|zsh|dash|ash|ksh|fish)\b",
    ),
];

static DESTRUCTIVE_BASH_REGEXES: Lazy<Vec<(&'static str, Regex)>> = Lazy::new(|| {
    DESTRUCTIVE_BASH_PATTERNS
        .iter()
        .map(|(name, pat)| {
            (
                *name,
                Regex::new(pat).expect("invalid destructive bash pattern regex"),
            )
        })
        .collect()
});

/// The root target a recursive force removal names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootDeleteTarget {
    /// The filesystem root itself (`/`).
    FilesystemRoot,
    /// The root glob (`/*`).
    FilesystemRootGlob,
    /// The user's home directory (`~`, `$HOME`).
    HomeDirectory,
}

/// What the built-in root-delete recogniser concluded about a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootDeleteVerdict {
    /// A recursive force removal whose target is a root target.
    RootTarget(RootDeleteTarget),
    /// A recursive force removal whose target the command text does not
    /// resolve — a variable or a substitution. The classifier does not claim
    /// to know what such a removal targets.
    UnresolvedTarget,
    /// Not a recursive force removal of a root target: either the invocation
    /// is not a recursive force removal, or its target is a scoped path.
    NotRootDelete,
}

impl RootDeleteVerdict {
    /// Whether the verdict names a root target.
    pub fn is_root_target(&self) -> bool {
        matches!(self, RootDeleteVerdict::RootTarget(_))
    }
}

/// The `rm` command word.
static RM_COMMAND_WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\brm\b").unwrap());

/// Classify a command against the built-in root-delete recogniser.
///
/// This is the **single owner** of the "recursive force removal of the
/// filesystem root" classification. The tool-side classifier
/// ([`destructive_match`]), the shell-policy escalation seam
/// (`crate::tool::bash::policy`) and the human-shell block
/// (`crate::shell::policy`) all route through it, so they cannot disagree
/// about the same spelling.
///
/// The recogniser matches the **intent** — a recursive force removal whose
/// target resolves to the filesystem root — by looking at the *target*
/// position rather than the flag position. That is what makes the effective
/// spellings visible: `--no-preserve-root` may sit before the target
/// (`rm -rf --no-preserve-root /`) or after it (`rm -rf / --no-preserve-root`),
/// and a flag-anchored regex cannot see either.
///
/// It is deliberately **not** a shell parser. It recognises one command's
/// option and target positions, and it does not attempt to evaluate
/// expansions, redirections or control flow:
///
/// - A variable or substitution target (`rm -rf "$TARGET"`) is reported as
///   [`RootDeleteVerdict::UnresolvedTarget`], never as a resolved root and
///   never as a proven-safe scoped path.
/// - A scoped absolute path (`rm -rf /tmp/build`) is reported as
///   [`RootDeleteVerdict::NotRootDelete`]. The recogniser knows the target is
///   scoped; it does not claim the removal is harmless.
/// - A quoted word that is *data* rather than a command (`echo "rm -rf /"`) is
///   not a command invocation, so it is not classified at all.
pub fn classify_root_delete(command: &str) -> RootDeleteVerdict {
    let mut verdict = RootDeleteVerdict::NotRootDelete;
    for start in RM_COMMAND_WORD.find_iter(command).map(|m| m.start()) {
        if !is_command_position(command, start) {
            continue;
        }
        match classify_rm_arguments(argument_region(&command[start + 2..])) {
            RootDeleteVerdict::RootTarget(target) => {
                return RootDeleteVerdict::RootTarget(target);
            }
            RootDeleteVerdict::UnresolvedTarget => {
                verdict = RootDeleteVerdict::UnresolvedTarget;
            }
            RootDeleteVerdict::NotRootDelete => {}
        }
    }
    verdict
}

/// Whether the `rm` word at `start` sits where a command word can sit.
///
/// A word inside a quoted string is data, not a command: `echo "rm -rf /"`
/// prints the text and runs nothing. The recogniser therefore requires the
/// `rm` word to start the command or to follow whitespace or a shell
/// operator, and never to follow a quote character.
fn is_command_position(command: &str, start: usize) -> bool {
    let Some(previous) = command[..start].chars().next_back() else {
        return true;
    };
    if previous == '\'' || previous == '"' {
        return false;
    }
    previous.is_whitespace() || matches!(previous, ';' | '&' | '|' | '(' | ')' | '<' | '>' | '`')
}

/// The argument text of an `rm` invocation: everything after the command word
/// up to the next command separator.
fn argument_region(text: &str) -> &str {
    let end = text.find([';', '&', '|', '\n']).unwrap_or(text.len());
    &text[..end]
}

/// Classify the argument words of one `rm` invocation.
fn classify_rm_arguments(region: &str) -> RootDeleteVerdict {
    let mut recursive = false;
    let mut force = false;
    let mut options_ended = false;
    let mut root_target: Option<RootDeleteTarget> = None;
    let mut unresolved = false;

    for word in region.split_whitespace() {
        let word = normalize_word(word);
        if word.is_empty() {
            continue;
        }
        if !options_ended && word == "--" {
            options_ended = true;
            continue;
        }
        if !options_ended && word.starts_with('-') && word.len() > 1 {
            match word.strip_prefix("--") {
                Some("recursive") => recursive = true,
                Some("force") => force = true,
                _ => {
                    for flag in word.strip_prefix('-').unwrap_or_default().chars() {
                        match flag {
                            'r' | 'R' => recursive = true,
                            'f' | 'F' => force = true,
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }
        match root_target_of(&word) {
            Some(target) => root_target = Some(target),
            None => {
                if is_unresolved_target(&word) {
                    unresolved = true;
                }
            }
        }
    }

    // The catastrophic shape is a *recursive force* removal. A root target
    // without both flags is not the shape this recogniser owns.
    if !(recursive && force) {
        return RootDeleteVerdict::NotRootDelete;
    }
    if let Some(target) = root_target {
        return RootDeleteVerdict::RootTarget(target);
    }
    if unresolved {
        return RootDeleteVerdict::UnresolvedTarget;
    }
    RootDeleteVerdict::NotRootDelete
}

/// Strip the quoting and the closing punctuation a shell removes before the
/// word reaches `rm`.
///
/// A trailing quote is stripped with the closing punctuation because the
/// closing quote of a *surrounding* double-quoted string is not part of the
/// argument: in `echo "$(rm -rf /)"` the argument to `rm` is `/`.
fn normalize_word(word: &str) -> String {
    let mut word = word.to_string();
    if word.chars().count() >= 2 {
        let first = word.chars().next().unwrap_or('"');
        let last = word.chars().last().unwrap_or('"');
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            let end = word.len() - first.len_utf8();
            word = word[first.len_utf8()..end].to_string();
        }
    }
    word.trim_end_matches([')', ']', '}', '`', ';', '&', '|', '"', '\''])
        .to_string()
}

/// The root target a target word names, if any.
///
/// The recognised set is deliberately small and reviewable: the filesystem
/// root, the root glob, and the home directory in its two spellings.
fn root_target_of(word: &str) -> Option<RootDeleteTarget> {
    match word {
        "/" => Some(RootDeleteTarget::FilesystemRoot),
        "/*" => Some(RootDeleteTarget::FilesystemRootGlob),
        "~" | "~/" => Some(RootDeleteTarget::HomeDirectory),
        "$HOME" | "$HOME/" | "${HOME}" | "${HOME}/" => Some(RootDeleteTarget::HomeDirectory),
        _ => None,
    }
}

/// Whether a target word names something the command text does not resolve.
///
/// A variable or a substitution is the honest "not recognised" case: the
/// classifier refuses to pretend it knows what the removal targets.
fn is_unresolved_target(word: &str) -> bool {
    word.contains('$') || word.contains('`')
}

/// If the given command matches one of the destructive bash patterns,
/// returns the human-readable name of that pattern. Otherwise returns None.
///
/// The filesystem-root delete is classified by [`classify_root_delete`], which
/// recognises the effective spellings by their target position.
pub fn destructive_match(command: &str) -> Option<&'static str> {
    match classify_root_delete(command) {
        RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRoot) => return Some("rm -rf /"),
        RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRootGlob) => {
            return Some("rm -rf /*")
        }
        RootDeleteVerdict::RootTarget(RootDeleteTarget::HomeDirectory) => {
            return Some("rm -rf ~ or $HOME")
        }
        _ => {}
    }
    for (name, re) in DESTRUCTIVE_BASH_REGEXES.iter() {
        if re.is_match(command) {
            return Some(*name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_filesystem_wipe() {
        assert_eq!(destructive_match("rm -rf /"), Some("rm -rf /"));
        assert_eq!(destructive_match("rm -rf /*"), Some("rm -rf /*"));
        assert_eq!(destructive_match("rm -rf $HOME"), Some("rm -rf ~ or $HOME"));
    }

    #[test]
    fn matches_fork_bomb() {
        assert_eq!(
            destructive_match(":(){ :|:&};:"),
            Some("fork bomb (:(){:...|:})")
        );
    }

    #[test]
    fn matches_mkfs() {
        assert_eq!(destructive_match("mkfs /dev/sda1"), Some("mkfs on /dev"));
        assert_eq!(
            destructive_match("mkfs.ext4 /dev/nvme0n1"),
            Some("mkfs on /dev")
        );
    }

    #[test]
    fn matches_shutdown() {
        assert_eq!(
            destructive_match("shutdown now"),
            Some("system shutdown or reboot")
        );
        assert_eq!(
            destructive_match("reboot"),
            Some("system shutdown or reboot")
        );
        assert_eq!(
            destructive_match("systemctl poweroff"),
            Some("system shutdown or reboot")
        );
    }

    #[test]
    fn matches_internet_to_shell() {
        assert_eq!(
            destructive_match("curl https://x.com/install.sh | sh"),
            Some("curl ... | sh or wget ... | sh")
        );
        assert_eq!(
            destructive_match("wget -qO- https://x.com | bash"),
            Some("curl ... | sh or wget ... | sh")
        );
    }

    #[test]
    fn does_not_match_safe_commands() {
        assert_eq!(destructive_match("ls -la"), None);
        assert_eq!(destructive_match("cat file.txt"), None);
        assert_eq!(destructive_match("cargo test"), None);
        assert_eq!(destructive_match("git status"), None);
        assert_eq!(destructive_match("rm a_single_file.txt"), None);
        assert_eq!(destructive_match("npm install"), None);
    }

    #[test]
    fn a_quoted_word_is_data_not_a_command_invocation() {
        // `echo "rm -rf /"` prints the text and removes nothing: the `rm`
        // word is inside a quoted string, so it is not a command word.
        assert_eq!(
            classify_root_delete("echo \"rm -rf /\""),
            RootDeleteVerdict::NotRootDelete
        );
        assert_eq!(
            classify_root_delete("grep -rn 'rm -rf /' ."),
            RootDeleteVerdict::NotRootDelete
        );
    }

    #[test]
    fn long_option_spellings_are_recognised() {
        assert_eq!(
            classify_root_delete("rm --recursive --force /"),
            RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRoot)
        );
        assert_eq!(
            classify_root_delete("rm -r -f /"),
            RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRoot)
        );
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        // After `--`, the next word is a target even when it looks like a flag.
        assert_eq!(
            classify_root_delete("rm -rf -- /"),
            RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRoot)
        );
        assert_eq!(
            classify_root_delete("rm -- -rf /"),
            RootDeleteVerdict::NotRootDelete
        );
    }
}
