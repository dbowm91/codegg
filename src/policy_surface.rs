//! Frontend-neutral approval/sandbox user-surface logic (M007).
//!
//! This module owns the *presentation* half of the M007 milestone: warning
//! and confirmation mapping for every approval-mode × sandbox-profile
//! combination, effective-state rendering (requested preference versus
//! daemon-resolved effective state), startup restore summaries, and CLI
//! flag resolution. It is deliberately free of frontend, daemon, scheduler,
//! plugin, and auth authority:
//!
//! - it never reads or writes [`codegg_core::approval::RuntimePreferenceStore`];
//! - it never captures an [`codegg_core::approval::ExecutionPolicySnapshot`];
//! - it never touches the network, the filesystem, or credentials.
//!
//! Callers supply plain values (already daemon-resolved where authority
//! matters) and receive text plus confirmation requirements. Enforcement
//! truth comes from the daemon via `ExecutionPolicyGet`; this module only
//! formats it. See `scripts/check_policy_surface.py`.

use codegg_core::approval::{ApprovalMode, SandboxProfile};

/// Severity of a mode/profile combination warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningLevel {
    /// Ordinary selection; no elevated risk beyond the default posture.
    Info,
    /// Prompts are skipped for some escalations; the user should read the
    /// explanation once. Sandbox containment still applies.
    Caution,
    /// Host-wide containment loss (`FullHost`) or fully autonomous host
    /// access (`Yolo` + `FullHost`). Requires explicit confirmation.
    Strong,
}

impl WarningLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Caution => "caution",
            Self::Strong => "strong",
        }
    }
}

/// Warning and confirmation contract for one approval × sandbox selection.
///
/// `confirmations_required` is the number of *separate* explicit
/// confirmations the frontend must collect before issuing the daemon
/// policy update: `0` renders the explanation inline with no blocking
/// prompt, `1` requires one confirmation dialog, `2` (only
/// `Yolo` + `FullHost`) requires a second explicit confirmation after
/// the first. Once the selection lands, frontends must not re-prompt
/// per action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyWarning {
    pub level: WarningLevel,
    pub title: String,
    pub body: Vec<String>,
    pub confirmations_required: u8,
}

/// Warning/confirmation mapping for each approval × sandbox combination.
///
/// The matrix keeps the two dimensions visible: approval prompting and
/// filesystem containment are orthogonal, and `Automatic`/`Yolo` never
/// imply `FullHost`.
pub fn warning_for(mode: ApprovalMode, profile: SandboxProfile) -> PolicyWarning {
    match (mode, profile) {
        (ApprovalMode::Interactive, SandboxProfile::ReadOnly) => PolicyWarning {
            level: WarningLevel::Info,
            title: "Interactive approval with read-only containment".to_string(),
            body: vec![
                "Escalations are sent to you for approval.".to_string(),
                "Filesystem writes are denied by containment; network has no OS isolation."
                    .to_string(),
            ],
            confirmations_required: 0,
        },
        (ApprovalMode::Interactive, SandboxProfile::WorkspaceWrite) => PolicyWarning {
            level: WarningLevel::Info,
            title: "Interactive approval with workspace containment".to_string(),
            body: vec![
                "Escalations are sent to you for approval.".to_string(),
                "Writes are confined to the workspace roots on supported hosts; network has no OS isolation."
                    .to_string(),
            ],
            confirmations_required: 0,
        },
        (ApprovalMode::Automatic, SandboxProfile::ReadOnly)
        | (ApprovalMode::Automatic, SandboxProfile::WorkspaceWrite) => PolicyWarning {
            level: WarningLevel::Caution,
            title: "Automatic approval inside containment".to_string(),
            body: vec![
                "Escalations go to the bounded read-only reviewer instead of prompting you."
                    .to_string(),
                "The reviewer can only allow actions inside the current authority and sandbox ceiling; explicit denies still deny."
                    .to_string(),
                "If no reviewer model is configured, Automatic defers to you (it never silently behaves as Yolo)."
                    .to_string(),
            ],
            confirmations_required: 0,
        },
        (ApprovalMode::Yolo, SandboxProfile::ReadOnly)
        | (ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite) => PolicyWarning {
            level: WarningLevel::Caution,
            title: "Yolo approval inside containment".to_string(),
            body: vec![
                "Approval prompts are skipped for escalations, but explicit denies and the configured sandbox remain enforced."
                    .to_string(),
                "The agent may modify or delete workspace files and run commands without asking."
                    .to_string(),
                "Filesystem containment still applies on supported hosts; network has no OS isolation."
                    .to_string(),
            ],
            confirmations_required: 1,
        },
        (ApprovalMode::Interactive, SandboxProfile::FullHost)
        | (ApprovalMode::Automatic, SandboxProfile::FullHost) => PolicyWarning {
            level: WarningLevel::Strong,
            title: "Full host access: CodeGG filesystem containment disabled".to_string(),
            body: vec![
                "The agent process has your OS user's host authority outside the workspace."
                    .to_string(),
                "Network access is unrestricted (there is no OS network isolation).".to_string(),
                "Deterministic hard denies still deny, but containment will not stop a permitted action."
                    .to_string(),
            ],
            confirmations_required: 1,
        },
        (ApprovalMode::Yolo, SandboxProfile::FullHost) => PolicyWarning {
            level: WarningLevel::Strong,
            title: "Yolo with full host access: strongest risk".to_string(),
            body: vec![
                "No approval prompts AND no CodeGG filesystem containment: the agent acts with your OS user's host authority without asking."
                    .to_string(),
                "Network access is unrestricted (there is no OS network isolation).".to_string(),
                "Only explicit denies and authority ceilings still apply. Use only in a trusted disposable environment."
                    .to_string(),
            ],
            confirmations_required: 2,
        },
    }
}

/// Daemon-resolved effective policy rendered for frontends.
///
/// `requested_*` is the user's persisted preference; `effective_*` is what
/// the daemon resolved after project/admin ceilings (they are equal when
/// no ceiling narrows the request). `enforcement_summary` is the daemon's
/// truthful containment report (never invented by the frontend).
/// `reviewer_available` reports whether an `Automatic` escalation can
/// actually reach the reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicyView {
    pub requested_mode: ApprovalMode,
    pub effective_mode: ApprovalMode,
    pub requested_profile: SandboxProfile,
    pub effective_profile: SandboxProfile,
    pub enforcement_summary: String,
    pub reviewer_available: bool,
    pub reviewer_detail: String,
    pub revision: Option<u64>,
}

impl EffectivePolicyView {
    /// `true` when the visible effective state is stricter or otherwise
    /// different from the requested preference, or when `Automatic` cannot
    /// reach a reviewer and will defer to the human.
    pub fn is_degraded(&self) -> bool {
        self.requested_mode != self.effective_mode
            || self.requested_profile != self.effective_profile
            || (self.effective_mode == ApprovalMode::Automatic && !self.reviewer_available)
    }
}

/// One-line status rendering: `approval:<mode> sandbox:<profile>` plus
/// degradation markers. Bounded and secret-free.
pub fn format_policy_line(view: &EffectivePolicyView) -> String {
    let mut line = String::from("approval:");
    line.push_str(mode_label(view.requested_mode, view.effective_mode));
    line.push_str(" sandbox:");
    line.push_str(profile_label(
        view.requested_profile,
        view.effective_profile,
    ));
    if view.effective_profile == SandboxProfile::FullHost {
        line.push_str(" (no containment)");
    }
    if view.effective_mode == ApprovalMode::Automatic && !view.reviewer_available {
        line.push_str(" (reviewer unavailable: deferring to human)");
    }
    line
}

fn mode_label(requested: ApprovalMode, effective: ApprovalMode) -> &'static str {
    if requested != effective {
        // The ceiling narrowed the request; the detail view names both.
        // The one-line form keeps the effective value with a marker.
        return match effective {
            ApprovalMode::Interactive => "interactive (narrowed)",
            ApprovalMode::Automatic => "automatic (narrowed)",
            ApprovalMode::Yolo => "yolo (narrowed)",
        };
    }
    match effective {
        ApprovalMode::Interactive => "interactive",
        ApprovalMode::Automatic => "automatic",
        ApprovalMode::Yolo => "yolo",
    }
}

fn profile_label(requested: SandboxProfile, effective: SandboxProfile) -> &'static str {
    if requested != effective {
        return match effective {
            SandboxProfile::ReadOnly => "read-only (narrowed)",
            SandboxProfile::WorkspaceWrite => "workspace-write (narrowed)",
            SandboxProfile::FullHost => "full-host (narrowed)",
        };
    }
    match effective {
        SandboxProfile::ReadOnly => "read-only",
        SandboxProfile::WorkspaceWrite => "workspace-write",
        SandboxProfile::FullHost => "full-host",
    }
}

/// Multi-line detail rendering for `/policy`, `/status`, and help text.
/// Distinguishes requested preference from daemon-resolved effective
/// state; never presents the request as the enforcement truth.
pub fn format_policy_detail(view: &EffectivePolicyView) -> Vec<String> {
    let mut lines = Vec::with_capacity(8);
    lines.push(format!(
        "Requested: approval={} sandbox={}",
        view.requested_mode.as_str(),
        view.requested_profile.as_str(),
    ));
    lines.push(format!(
        "Effective: approval={} sandbox={}",
        view.effective_mode.as_str(),
        view.effective_profile.as_str(),
    ));
    if view.requested_mode != view.effective_mode
        || view.requested_profile != view.effective_profile
    {
        lines.push(
            "The effective state is stricter than the request: a project or administrator ceiling applies."
                .to_string(),
        );
    }
    let enforcement = view.enforcement_summary.trim();
    lines.push(if enforcement.is_empty() {
        "Enforcement: unknown (daemon did not report containment)".to_string()
    } else {
        format!("Enforcement: {enforcement}")
    });
    if view.effective_mode == ApprovalMode::Automatic {
        if view.reviewer_available {
            lines.push("Reviewer: available".to_string());
        } else {
            let detail = view.reviewer_detail.trim();
            lines.push(if detail.is_empty() {
                "Reviewer: unavailable — Automatic defers to you (never silent Yolo)"
                    .to_string()
            } else {
                format!("Reviewer: unavailable ({detail}) — Automatic defers to you (never silent Yolo)")
            });
        }
    }
    match view.revision {
        Some(revision) => lines.push(format!("Preference revision: {revision}")),
        None => lines.push("Preference revision: none stored (built-in defaults)".to_string()),
    }
    lines
}

/// Startup restore report: what the daemon restored and which fallbacks
/// applied because a remembered preference became invalid or unavailable.
///
/// `fallbacks` carries one short diagnostic per applied fallback (for
/// example a remembered model that is no longer in the catalog). An empty
/// list means the stored preference applied cleanly.
pub fn format_restore_summary(view: &EffectivePolicyView, fallbacks: &[String]) -> String {
    let mut out = format!(
        "Restored runtime policy: approval={} sandbox={}",
        view.effective_mode.as_str(),
        view.effective_profile.as_str(),
    );
    match view.revision {
        Some(revision) => out.push_str(&format!(" (revision {revision})")),
        None => out.push_str(" (built-in defaults; nothing stored yet)"),
    }
    out.push_str(&format!(". {}", view.enforcement_summary.trim()));
    if view.effective_mode == ApprovalMode::Automatic && !view.reviewer_available {
        out.push_str(" Automatic reviewer unavailable: escalations defer to you.");
    }
    for fallback in fallbacks {
        let trimmed = fallback.trim();
        if !trimmed.is_empty() {
            out.push_str(" Fallback: ");
            out.push_str(trimmed);
        }
    }
    out
}

/// CLI/headless policy override resolved from flags.
///
/// `None` means no flag was given and the daemon preference (or default)
/// applies untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliPolicyOverride {
    pub approval_mode: Option<ApprovalMode>,
    pub sandbox_profile: Option<SandboxProfile>,
}

/// Resolve `--approval-mode`, `--sandbox`, and `--yolo` into one
/// override. `--yolo` is an exact alias for `--approval-mode yolo`;
/// combining it with a different explicit mode is rejected rather than
/// silently preferring one spelling.
pub fn resolve_cli_policy(
    approval_mode: Option<&str>,
    sandbox: Option<&str>,
    yolo: bool,
) -> Result<Option<CliPolicyOverride>, String> {
    let mut mode = match approval_mode {
        Some(raw) => Some(parse_approval_mode(raw)?),
        None => None,
    };
    if yolo {
        match mode {
            Some(ApprovalMode::Yolo) | None => mode = Some(ApprovalMode::Yolo),
            Some(other) => {
                return Err(format!(
                    "--yolo conflicts with --approval-mode {} (use one spelling)",
                    other.as_str()
                ));
            }
        }
    }
    let profile = match sandbox {
        Some(raw) => Some(parse_sandbox_profile(raw)?),
        None => None,
    };
    if mode.is_none() && profile.is_none() {
        return Ok(None);
    }
    Ok(Some(CliPolicyOverride {
        approval_mode: mode,
        sandbox_profile: profile,
    }))
}

/// Parse an approval-mode flag value. Fails closed with the valid set.
pub fn parse_approval_mode(raw: &str) -> Result<ApprovalMode, String> {
    ApprovalMode::parse(raw).ok_or_else(|| {
        "unknown approval mode (expected interactive, automatic, or yolo)".to_string()
    })
}

/// Parse a sandbox-profile flag value. Fails closed with the valid set.
pub fn parse_sandbox_profile(raw: &str) -> Result<SandboxProfile, String> {
    SandboxProfile::parse(raw).ok_or_else(|| {
        "unknown sandbox profile (expected read-only, workspace-write, or full-host)".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_matrix_levels_and_confirmations() {
        use ApprovalMode::{Automatic, Interactive, Yolo};
        use SandboxProfile::{FullHost, ReadOnly, WorkspaceWrite};
        let cases = [
            (Interactive, ReadOnly, WarningLevel::Info, 0),
            (Interactive, WorkspaceWrite, WarningLevel::Info, 0),
            (Automatic, ReadOnly, WarningLevel::Caution, 0),
            (Automatic, WorkspaceWrite, WarningLevel::Caution, 0),
            (Yolo, ReadOnly, WarningLevel::Caution, 1),
            (Yolo, WorkspaceWrite, WarningLevel::Caution, 1),
            (Interactive, FullHost, WarningLevel::Strong, 1),
            (Automatic, FullHost, WarningLevel::Strong, 1),
            (Yolo, FullHost, WarningLevel::Strong, 2),
        ];
        for (mode, profile, level, confirmations) in cases {
            let warning = warning_for(mode, profile);
            assert_eq!(warning.level, level, "{mode:?} x {profile:?}");
            assert_eq!(
                warning.confirmations_required, confirmations,
                "{mode:?} x {profile:?}"
            );
            assert!(!warning.title.is_empty());
            assert!(!warning.body.is_empty());
        }
    }

    #[test]
    fn yolo_workspace_write_warns_without_host_containment_loss() {
        let warning = warning_for(ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite);
        let joined = warning.body.join(" ");
        assert!(joined.contains("prompts are skipped"));
        assert!(joined.contains("sandbox remain enforced"));
        assert!(!joined
            .to_lowercase()
            .contains("no codegg filesystem containment"));
    }

    #[test]
    fn full_host_warnings_name_host_authority_and_network() {
        for mode in [
            ApprovalMode::Interactive,
            ApprovalMode::Automatic,
            ApprovalMode::Yolo,
        ] {
            let warning = warning_for(mode, SandboxProfile::FullHost);
            assert_eq!(warning.level, WarningLevel::Strong);
            let joined = warning.body.join(" ").to_lowercase();
            assert!(joined.contains("os user"), "{mode:?}");
            assert!(joined.contains("unrestricted"), "{mode:?}");
        }
    }

    #[test]
    fn yolo_full_host_is_strongest_with_second_confirmation() {
        let warning = warning_for(ApprovalMode::Yolo, SandboxProfile::FullHost);
        assert_eq!(warning.confirmations_required, 2);
        let joined = warning.body.join(" ").to_lowercase();
        assert!(joined.contains("without asking"));
    }

    #[test]
    fn policy_line_shows_effective_state_and_degradation() {
        let plain = EffectivePolicyView {
            requested_mode: ApprovalMode::Yolo,
            effective_mode: ApprovalMode::Yolo,
            requested_profile: SandboxProfile::WorkspaceWrite,
            effective_profile: SandboxProfile::WorkspaceWrite,
            enforcement_summary: "workspace_write requested; filesystem enforced (landlock); network unrestricted (no OS isolation)".to_string(),
            reviewer_available: false,
            reviewer_detail: String::new(),
            revision: Some(3),
        };
        assert!(!plain.is_degraded());
        let line = format_policy_line(&plain);
        assert!(line.contains("approval:yolo"));
        assert!(line.contains("sandbox:workspace-write"));

        let narrowed = EffectivePolicyView {
            requested_mode: ApprovalMode::Yolo,
            effective_mode: ApprovalMode::Interactive,
            ..plain.clone()
        };
        assert!(narrowed.is_degraded());
        let line = format_policy_line(&narrowed);
        assert!(line.contains("narrowed"));

        let auto_degraded = EffectivePolicyView {
            requested_mode: ApprovalMode::Automatic,
            effective_mode: ApprovalMode::Automatic,
            requested_profile: SandboxProfile::WorkspaceWrite,
            effective_profile: SandboxProfile::WorkspaceWrite,
            enforcement_summary: String::new(),
            reviewer_available: false,
            reviewer_detail: "no reviewer model configured".to_string(),
            revision: Some(1),
        };
        assert!(auto_degraded.is_degraded());
        let line = format_policy_line(&auto_degraded);
        assert!(line.contains("reviewer unavailable"));
        let detail = format_policy_detail(&auto_degraded).join("\n");
        assert!(detail.contains("never silent Yolo"));
        assert!(detail.contains("no reviewer model configured"));
    }

    #[test]
    fn restore_summary_reports_fallbacks() {
        let view = EffectivePolicyView {
            requested_mode: ApprovalMode::Interactive,
            effective_mode: ApprovalMode::Interactive,
            requested_profile: SandboxProfile::WorkspaceWrite,
            effective_profile: SandboxProfile::WorkspaceWrite,
            enforcement_summary: "workspace_write requested; filesystem unavailable (no landlock); network unrestricted (no OS isolation)".to_string(),
            reviewer_available: false,
            reviewer_detail: String::new(),
            revision: None,
        };
        let clean = format_restore_summary(&view, &[]);
        assert!(clean.contains("built-in defaults"));
        let with_fallback = format_restore_summary(
            &view,
            &[String::from(
                "remembered model 'conn/old' is not in the current catalog; session left unselected",
            )],
        );
        assert!(with_fallback.contains("Fallback: remembered model"));
    }

    #[test]
    fn cli_policy_resolution_alias_and_conflicts() {
        assert_eq!(resolve_cli_policy(None, None, false), Ok(None));
        assert_eq!(
            resolve_cli_policy(Some("yolo"), None, true),
            Ok(Some(CliPolicyOverride {
                approval_mode: Some(ApprovalMode::Yolo),
                sandbox_profile: None,
            }))
        );
        assert_eq!(
            resolve_cli_policy(None, Some("full-host"), true),
            Ok(Some(CliPolicyOverride {
                approval_mode: Some(ApprovalMode::Yolo),
                sandbox_profile: Some(SandboxProfile::FullHost),
            }))
        );
        assert!(resolve_cli_policy(Some("interactive"), None, true).is_err());
        assert!(resolve_cli_policy(Some("bogus"), None, false).is_err());
        assert!(resolve_cli_policy(None, Some("bogus"), false).is_err());
    }
}
