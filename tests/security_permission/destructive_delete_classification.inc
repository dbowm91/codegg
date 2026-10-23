//! Destructive-delete classification coverage (C002).
//!
//! The built-in destructive-command classification is an **advisory risk
//! signal** under ADR-0045: it tells the truth about risk so the approval
//! path can escalate, and it never guarantees that the effect cannot happen.
//! This member pins that classification against the fixture set the C002 plan
//! names, and it pins the human-shell block in the other direction: a scoped
//! absolute path must **not** be hard-blocked as a root delete.
//!
//! Nothing here spawns a process. Every command below is inert classifier
//! data passed to pure functions; `scripts/check_shell_policy_test_safety.py`
//! audits this file and fails it if a destructive literal is ever combined
//! with a dispatch call.

use codegg::config::schema::{Config, PermissionConfig};
use codegg::permission::{PermissionChecker, PermissionResult};
use codegg::shell::{evaluate_command, HumanShellPolicyDecision};
use codegg::tool::bash::policy::find_ask_pattern;
use codegg::tool::destructive::{classify_root_delete, RootDeleteTarget, RootDeleteVerdict};

/// The spellings whose target resolves to a root target.
///
/// `rm -rf --no-preserve-root /` and `rm -rf / --no-preserve-root` are the
/// forms GNU coreutils actually executes against the filesystem root; the
/// bare `rm -rf /` is the form coreutils *refuses*. A flag-anchored matcher
/// sees only the last of the three.
const ROOT_DELETE_SPELLINGS: &[(&str, RootDeleteTarget)] = &[
    ("rm -rf /", RootDeleteTarget::FilesystemRoot),
    (
        "rm -rf --no-preserve-root /",
        RootDeleteTarget::FilesystemRoot,
    ),
    (
        "rm -rf / --no-preserve-root",
        RootDeleteTarget::FilesystemRoot,
    ),
    ("rm -fr /", RootDeleteTarget::FilesystemRoot),
    ("rm -rf /*", RootDeleteTarget::FilesystemRootGlob),
    ("rm -rf ~", RootDeleteTarget::HomeDirectory),
    ("rm -rf $HOME", RootDeleteTarget::HomeDirectory),
];

/// The subset of the root-delete spellings that names the filesystem root.
///
/// The home directory is deliberately excluded: the human shell keeps its
/// pre-existing *warning* for a home removal, it does not refuse it.
const FILESYSTEM_ROOT_SPELLINGS: &[&str] = &[
    "rm -rf /",
    "rm -rf --no-preserve-root /",
    "rm -rf / --no-preserve-root",
    "rm -fr /",
    "rm -rf /*",
];

/// The scoped removals the classifier must not report as a root delete.
const SCOPED_REMOVALS: &[&str] = &[
    "rm -rf /tmp/build",
    "rm -rf ./build",
    "rm -rf /home/user/project/cache",
];

#[test]
fn root_delete_spellings_are_classified_as_root_targets() {
    for (command, expected) in ROOT_DELETE_SPELLINGS {
        assert_eq!(
            classify_root_delete(command),
            RootDeleteVerdict::RootTarget(*expected),
            "the effective root-delete spelling {command:?} must be classified as a root target"
        );
    }
}

#[test]
fn scoped_removals_are_not_classified_as_root_targets() {
    for command in SCOPED_REMOVALS {
        assert_eq!(
            classify_root_delete(command),
            RootDeleteVerdict::NotRootDelete,
            "the scoped removal {command:?} must not be classified as a root target"
        );
    }
}

#[test]
fn a_variable_target_is_reported_as_unresolved() {
    // The classifier must not pretend to resolve a variable target: it says
    // "not recognised" rather than claiming the removal is scoped or safe.
    assert_eq!(
        classify_root_delete("rm -rf \"$TARGET\""),
        RootDeleteVerdict::UnresolvedTarget
    );
}

#[test]
fn effective_root_delete_spellings_raise_the_advisory_risk_signal() {
    for (command, _) in ROOT_DELETE_SPELLINGS {
        assert!(
            find_ask_pattern(command).is_some(),
            "the effective root-delete spelling {command:?} must raise the advisory risk signal"
        );
    }
}

#[test]
fn scoped_removals_do_not_raise_the_advisory_risk_signal() {
    for command in SCOPED_REMOVALS {
        assert!(
            find_ask_pattern(command).is_none(),
            "the scoped removal {command:?} must not raise the advisory risk signal"
        );
    }
}

#[test]
fn human_shell_does_not_block_a_scoped_absolute_path() {
    for command in SCOPED_REMOVALS {
        let decision = evaluate_command(command);
        assert!(
            !matches!(decision, HumanShellPolicyDecision::Block { .. }),
            "the scoped removal {command:?} must not be hard-blocked as a root delete, got: {decision:?}"
        );
    }
}

#[test]
fn human_shell_blocks_the_filesystem_root_spellings() {
    for command in FILESYSTEM_ROOT_SPELLINGS {
        let decision = evaluate_command(command);
        assert!(
            matches!(decision, HumanShellPolicyDecision::Block { .. }),
            "the filesystem-root spelling {command:?} must still be refused by the human-shell block, got: {decision:?}"
        );
    }
}

#[test]
fn human_shell_root_delete_reason_makes_no_guarantee_claim() {
    let decision = evaluate_command("rm -rf --no-preserve-root /");
    let HumanShellPolicyDecision::Block { reason } = decision else {
        panic!("expected the human-shell block to refuse the root delete");
    };
    assert!(
        reason.contains("not a security boundary"),
        "the refusal must label itself as a best-effort text rule, got: {reason}"
    );
    assert!(
        !reason.contains("catastrophic"),
        "the refusal must not describe the command's effect as a guaranteed catastrophe, got: {reason}"
    );
}

#[tokio::test]
async fn an_effective_root_delete_reaches_the_approval_path() {
    let checker = PermissionChecker::new(None, None);
    let result = checker
        .check_with_args("bash", None, Some("rm -rf --no-preserve-root /"), None)
        .await;
    assert!(
        matches!(result, PermissionResult::Ask(_)),
        "the effective root-delete spelling must escalate to the approval path, got: {result:?}"
    );
}

#[tokio::test]
async fn a_configured_deny_still_denies_before_the_risk_signal() {
    // The migration must not weaken a user-configured rule: an explicit deny
    // is terminal and is evaluated before the advisory classifier.
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            bash_deny_patterns: Some(vec!["rm -rf /tmp/build".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    let result = checker
        .check_with_args("bash", None, Some("rm -rf /tmp/build"), None)
        .await;
    assert!(
        matches!(result, PermissionResult::Deny),
        "a configured deny must stay terminal, got: {result:?}"
    );
}

#[test]
fn a_benign_prefix_does_not_suppress_the_root_delete_signal() {
    // A harmless leading command must not hide the removal that follows it.
    assert_eq!(
        classify_root_delete("echo ok && rm -rf --no-preserve-root /"),
        RootDeleteVerdict::RootTarget(RootDeleteTarget::FilesystemRoot)
    );
}
