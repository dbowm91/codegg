# Windows Native Sandbox C049 — Broad-read compatibility decision dossier

Status: implemented — design/evidence only; ADR-0030 remains proposed, not ratified
Class: invariant (policy boundary)
Baseline: main aa6cbcb1e154a2676325fb654c5b68dc068edfc3 (2026-10-14); refresh exact head at execution.
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0004, ADR-0016, accepted bounded-test-only ADR-0029, **proposed/unapproved** ADR-0030, C042 guarantee.rs.
Dependencies: C042 hard (closed); no new ADR needed to prepare this decision pack.
Closure target: plans/closure/windows-native-sandbox/049-status.md.

## Purpose and current evidence

Codex documents a WRITE_RESTRICTED compatibility path where broad host reads are possible even when writes require restricting-SID authorization. Anthropic's standalone srt-win uses an account plus restrictive token and additive original-path ACLs; its Windows alpha backend is not equivalent to the built-in Claude Code Windows behavior. Published MiniMax MCode TUI currently registers only srt-macos despite cross-platform policy types. Existing Codegg strict ReadOnly/WorkspaceWrite mean true filesystem-read restriction; C042 refuses broad-read candidates. C011/C013 show direct/junction/hardlink leakage and unsafe original-object ACL leases, C014 lacks Git minimum, C044 child never entered. C045/C046/C048 remain blocked. The proposal must not authorize a behavior change by implication.

## Work packages

A. Produce an explicit threat-model/attestation table distinguishing: confidential host reads, workspace/scratch write scope, Delete/Rename/ADS/reparse writes, host security-descriptor mutation, child/descendant containment, privileged IPC/handle/process memory, network (unrestricted/offline/mediated), interactive/human execution and daemon-trusted in-process edits. Include strongest honest policy for each candidate, including absence of guarantees. Distinguish broad host reads from universal access to every host path.

B. Draft a concrete ADR-0030 **ratification packet** with separately selectable profile name (candidate WorkspaceWriteBroadRead), text for operator-visible host-secret risk, upgrade/default/opt-in semantics, exact approval receipt, session-persisted profile and parent/subagent ceiling transition matrix. Do not mark ADR accepted or edit public enums/config/wire/TUI in this plan. A child under a strict parent MUST NOT request this weaker mode. Automatic retry from strict to broad or to FullHost is forbidden.

C. Compare candidate operating modes: same-user WRITE_RESTRICTED + Codegg-created disposable stage; original-worktree ACL grants (ineligible under current no-host-mutation invariant); dedicated-principal plus explicitly granted paths; BaseContainer/guest strict path. State explicit reasons for any excluded approach. Use research/windows-native-sandbox/2026-10-14-native-compatibility-successors.md and cited upstream revision/SDK pin rather than unverified third-party behavior.

D. Reconcile C042 internal RequestedGuarantees/AttestedCapabilities constraints with a hypothetical broader public profile. Identify required future versioning/serialization, audit metadata and refusal reason changes; implement none. Ensure network=Unrestricted unless an independent WFP/socket gate passes; WRITE_RESTRICTED cannot satisfy confidential host-read denial.

## Verification / acceptance

Deliver (1) proposed threat-model matrix, (2) ADR-0030 decision packet showing accept/reject alternatives and migration gates, (3) tests-to-add checklist for old profile serialization, strict-parent→broad-child denial, denial of silent downgrade and no-FullHost fallback, and (4) an explicit maintainer decision request. Run planning/ADR integrity checks and diff whitespace checks on doc changes. No Windows native run required for document-only closure; record as not-run. Close C049 as a *decision-ready artifact*, not as ADR acceptance or backend availability. Do not rewrite C011/C013/C014/C043/C044 closures.

## Non-goals and invariants

No public compatibility profile, production backend, original-user ACL/MIC change, installer, operator privilege, firewall rules, stage promotion or release. Preserve existing strict profile semantics and immutable approval ceilings, no shell-regex containment fiction, fail closed on Windows Required. No schema migration in this milestone. If ADR-0030 is rejected, C050/C052 test-only studies remain legitimate but the public compatibility track is blocked.
